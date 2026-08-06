use ruyiseek_index::{scan, ScanOptions};
use ruyiseek_ipc::{
    decode_request, default_socket_path, encode_response, read_frame, write_frame, DaemonStatus,
    Request, Response, PROTOCOL_VERSION,
};
use ruyiseek_query::SearchEngine;
use std::error::Error;
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const CLIENT_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug)]
struct Config {
    roots: Vec<PathBuf>,
    socket: PathBuf,
    include_hidden: bool,
    max_entries: usize,
    once: bool,
}

fn main() -> Result<(), Box<dyn Error>> {
    let Some(config) = parse_args(std::env::args().skip(1))? else {
        return Ok(());
    };

    let report = scan(&ScanOptions {
        roots: config.roots.clone(),
        include_hidden: config.include_hidden,
        max_entries: config.max_entries,
    });
    eprintln!(
        "ruyiseekd: indexed {} items from {} root(s), skipped {}, truncated={}",
        report.items.len(),
        config.roots.len(),
        report.skipped_paths,
        report.truncated
    );

    let status = DaemonStatus {
        indexed_items: report.items.len(),
        skipped_paths: report.skipped_paths,
        truncated: report.truncated,
    };
    let engine = SearchEngine::new(report.items);
    let listener = bind_single_instance(&config.socket)?;
    let _socket_guard = SocketGuard(config.socket.clone());
    eprintln!("ruyiseekd: listening on {}", config.socket.display());

    for connection in listener.incoming() {
        match connection {
            Ok(mut stream) => {
                stream.set_read_timeout(Some(CLIENT_TIMEOUT))?;
                stream.set_write_timeout(Some(CLIENT_TIMEOUT))?;
                if let Err(error) = serve(&mut stream, &engine, &status) {
                    eprintln!("ruyiseekd: client request failed: {error}");
                }
                if config.once {
                    break;
                }
            }
            Err(error) => eprintln!("ruyiseekd: accept failed: {error}"),
        }
    }
    Ok(())
}

fn serve<Stream: Read + Write>(
    stream: &mut Stream,
    engine: &SearchEngine,
    status: &DaemonStatus,
) -> Result<(), Box<dyn Error>> {
    let payload = read_frame(stream)?;
    let response = match decode_request(&payload) {
        Ok(Request::Ping) => Response::Pong {
            protocol_version: PROTOCOL_VERSION,
        },
        Ok(Request::Status) => Response::Status(status.clone()),
        Ok(Request::Search { query, limit }) => {
            let started = Instant::now();
            let hits = engine.search(&query, limit);
            Response::Search {
                elapsed_us: u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
                hits,
            }
        }
        Err(error) => Response::Error(error.to_string()),
    };
    write_frame(stream, &encode_response(&response))?;
    Ok(())
}

fn bind_single_instance(socket: &Path) -> io::Result<UnixListener> {
    let parent = socket
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "socket path has no parent"))?;
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true).mode(0o700);
    builder.create(parent)?;

    if socket.exists() {
        match UnixStream::connect(socket) {
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    format!(
                        "another ruyiseekd is already listening at {}",
                        socket.display()
                    ),
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::ConnectionRefused => {
                fs::remove_file(socket)?;
            }
            Err(error) => return Err(error),
        }
    }

    let listener = UnixListener::bind(socket)?;
    fs::set_permissions(socket, fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

fn parse_args(args: impl Iterator<Item = String>) -> Result<Option<Config>, Box<dyn Error>> {
    let mut roots = Vec::new();
    let mut socket = default_socket_path();
    let mut include_hidden = false;
    let mut max_entries = 250_000;
    let mut once = false;
    let mut args = args.peekable();

    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--root" => roots.push(PathBuf::from(next_value(&mut args, "--root")?)),
            "--socket" => socket = PathBuf::from(next_value(&mut args, "--socket")?),
            "--max-entries" => max_entries = next_value(&mut args, "--max-entries")?.parse()?,
            "--include-hidden" => include_hidden = true,
            "--once" => once = true,
            "-h" | "--help" => {
                print_help();
                return Ok(None);
            }
            _ => return Err(format!("unknown argument: {argument}").into()),
        }
    }

    if roots.is_empty() {
        roots.push(std::env::current_dir()?);
    }
    if max_entries == 0 {
        return Err("--max-entries must be greater than zero".into());
    }

    Ok(Some(Config {
        roots,
        socket,
        include_hidden,
        max_entries,
        once,
    }))
}

fn next_value(
    args: &mut impl Iterator<Item = String>,
    option: &str,
) -> Result<String, Box<dyn Error>> {
    args.next()
        .ok_or_else(|| format!("{option} requires a value").into())
}

fn print_help() {
    println!(
        "ruyiseekd {version}\n\nUSAGE:\n    ruyiseekd [OPTIONS]\n\nOPTIONS:\n    --root PATH          Add a root to the bootstrap snapshot (repeatable)\n    --socket PATH        Override the Unix Socket path\n    --max-entries N      Stop after N indexed items [default: 250000]\n    --include-hidden     Include dot-files and dot-directories\n    --once               Serve one client and exit (integration testing)\n    -h, --help           Show this help",
        version = env!("CARGO_PKG_VERSION")
    );
}

struct SocketGuard(PathBuf);

impl Drop for SocketGuard {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_file(&self.0) {
            if error.kind() != io::ErrorKind::NotFound {
                eprintln!(
                    "ruyiseekd: could not remove socket {}: {error}",
                    self.0.display()
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ruyiseek_core::{ItemKind, SearchItem};
    use ruyiseek_ipc::{decode_response, encode_request};
    use std::io::Cursor;

    struct MemoryStream {
        input: Cursor<Vec<u8>>,
        output: Vec<u8>,
    }

    impl Read for MemoryStream {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            self.input.read(buffer)
        }
    }

    impl Write for MemoryStream {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.output.write(buffer)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn stream_for(request: &Request) -> MemoryStream {
        let mut input = Vec::new();
        write_frame(&mut input, &encode_request(request)).expect("encode framed request");
        MemoryStream {
            input: Cursor::new(input),
            output: Vec::new(),
        }
    }

    fn fixture_engine() -> SearchEngine {
        SearchEngine::new(vec![SearchItem {
            id: 1,
            name: "完整开发设计文档.md".to_owned(),
            path: PathBuf::from("/project/docs/完整开发设计文档.md"),
            kind: ItemKind::File,
            hidden: false,
        }])
    }

    #[test]
    fn serves_search_across_the_protocol_boundary() {
        let mut stream = stream_for(&Request::Search {
            query: "设计文档".to_owned(),
            limit: 10,
        });
        let status = DaemonStatus {
            indexed_items: 1,
            skipped_paths: 0,
            truncated: false,
        };

        serve(&mut stream, &fixture_engine(), &status).expect("serve search request");
        let response = decode_response(
            &read_frame(&mut Cursor::new(stream.output)).expect("read framed response"),
        )
        .expect("decode response");

        let Response::Search { hits, .. } = response else {
            panic!("expected search response");
        };
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].item.id, 1);
    }

    #[test]
    fn malformed_requests_receive_structured_errors() {
        let mut input = Vec::new();
        write_frame(&mut input, b"UNKNOWN").expect("encode malformed request");
        let mut stream = MemoryStream {
            input: Cursor::new(input),
            output: Vec::new(),
        };

        let status = DaemonStatus {
            indexed_items: 1,
            skipped_paths: 0,
            truncated: false,
        };
        serve(&mut stream, &fixture_engine(), &status).expect("serve malformed request");
        let response = decode_response(
            &read_frame(&mut Cursor::new(stream.output)).expect("read framed response"),
        )
        .expect("decode response");
        assert!(matches!(response, Response::Error(_)));
    }
}
