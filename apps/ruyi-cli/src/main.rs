use ruyiseek_ipc::{default_socket_path, request_daemon, Request, Response};
use std::error::Error;
use std::path::PathBuf;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(5);

fn main() -> Result<(), Box<dyn Error>> {
    let Some((socket, request)) = parse_args(std::env::args().skip(1))? else {
        return Ok(());
    };
    let response = request_daemon(&socket, &request, TIMEOUT)
        .map_err(|error| format!("{error}. Start ruyiseekd first"))?;
    print_response(response)
}

fn print_response(response: Response) -> Result<(), Box<dyn Error>> {
    match response {
        Response::Acknowledged => println!("ruyiseekd acknowledged the request"),
        Response::Pong { protocol_version } => {
            println!("ruyiseekd online (protocol {protocol_version})");
        }
        Response::Status(status) => println!(
            "indexed={} skipped={} truncated={}",
            status.indexed_items, status.skipped_paths, status.truncated
        ),
        Response::Search { elapsed_us, hits } => {
            for hit in &hits {
                println!(
                    "{:.3}\t{}\t{}",
                    hit.score,
                    hit.item.kind.protocol_name(),
                    hit.item.path.display()
                );
            }
            eprintln!("{} result(s) in {elapsed_us} µs", hits.len());
        }
        Response::Error(message) => {
            return Err(format!("ruyiseekd rejected the request: {message}").into())
        }
    }
    Ok(())
}

fn parse_args(
    args: impl Iterator<Item = String>,
) -> Result<Option<(PathBuf, Request)>, Box<dyn Error>> {
    let mut socket = default_socket_path();
    let mut limit = 20;
    let mut positional = Vec::new();
    let mut args = args.peekable();

    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--socket" => socket = PathBuf::from(next_value(&mut args, "--socket")?),
            "--limit" => limit = next_value(&mut args, "--limit")?.parse()?,
            "-h" | "--help" => {
                print_help();
                return Ok(None);
            }
            _ if argument.starts_with('-') => {
                return Err(format!("unknown option: {argument}").into())
            }
            _ => positional.push(argument),
        }
    }

    let request = match positional.first().map(String::as_str) {
        Some("ping") if positional.len() == 1 => Request::Ping,
        Some("status") if positional.len() == 1 => Request::Status,
        Some("stop") if positional.len() == 1 => Request::Shutdown,
        Some("search") if positional.len() > 1 => Request::Search {
            query: positional[1..].join(" "),
            limit,
        },
        _ => {
            print_help();
            return Err("expected ping, status, stop, or search QUERY".into());
        }
    };
    Ok(Some((socket, request)))
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
        "ruyi {version}\n\nUSAGE:\n    ruyi [--socket PATH] ping\n    ruyi [--socket PATH] status\n    ruyi [--socket PATH] stop\n    ruyi [--socket PATH] [--limit N] search QUERY",
        version = env!("CARGO_PKG_VERSION")
    );
}
