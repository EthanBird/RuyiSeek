use ruyiseek_index::{discover_default_roots, scan, ScanOptions};
use ruyiseek_ipc::{
    decode_request, default_socket_path, encode_response, read_frame, write_frame, DaemonStatus,
    Request, Response, PROTOCOL_VERSION,
};
use ruyiseek_query::SearchEngine;
use rustix::fs::inotify;
use rustix::io::Errno;
use std::error::Error;
use std::fs;
use std::io::{self, Read, Write};
use std::mem::MaybeUninit;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, PoisonError, RwLock};
use std::thread;
use std::time::{Duration, Instant};

const CLIENT_TIMEOUT: Duration = Duration::from_secs(5);
const REFRESH_POLL_INTERVAL: Duration = Duration::from_secs(2);
const FALLBACK_RESCAN_INTERVAL: Duration = Duration::from_secs(5 * 60);
const AUTO_ENTRIES_PER_ROOT: usize = 250_000;
const AUTO_ENTRIES_MAX: usize = 2_000_000;

#[derive(Debug)]
struct Config {
    roots: Vec<PathBuf>,
    automatic_home: Option<PathBuf>,
    socket: PathBuf,
    include_hidden: bool,
    max_entries: Option<usize>,
    once: bool,
}

struct IndexState {
    engine: SearchEngine,
    status: DaemonStatus,
    roots: Vec<PathBuf>,
    scanned_directories: Vec<PathBuf>,
    max_entries: usize,
}

fn main() -> Result<(), Box<dyn Error>> {
    let Some(config) = parse_args(std::env::args().skip(1))? else {
        return Ok(());
    };

    // Claim the single-instance socket before walking any search roots.  On
    // UOS, systemd starts the daemon and UI together; a large initial scan
    // used to leave a several-second window in which the UI could start a
    // second daemon.  Binding first makes every later starter fail fast.
    let listener = bind_single_instance(&config.socket)?;
    let _socket_guard = SocketGuard(config.socket.clone());

    let roots = match config.automatic_home.as_deref() {
        Some(home) => discover_roots_or_home(home),
        None => config.roots.clone(),
    };
    let max_entries = entry_limit(config.max_entries, roots.len());
    let initial_state = build_index_state(roots, config.include_hidden, max_entries);
    eprintln!(
        "ruyiseekd: indexed {} items from {} root(s), skipped {}, truncated={}, limit={}",
        initial_state.status.indexed_items,
        initial_state.roots.len(),
        initial_state.status.skipped_paths,
        initial_state.status.truncated,
        initial_state.max_entries
    );
    let state = Arc::new(RwLock::new(initial_state));
    let _refresh_monitor = if config.once {
        None
    } else {
        Some(spawn_refresh_monitor(
            config.automatic_home,
            config.include_hidden,
            config.max_entries,
            Arc::clone(&state),
        )?)
    };
    eprintln!("ruyiseekd: listening on {}", config.socket.display());

    for connection in listener.incoming() {
        match connection {
            Ok(mut stream) => {
                stream.set_read_timeout(Some(CLIENT_TIMEOUT))?;
                stream.set_write_timeout(Some(CLIENT_TIMEOUT))?;
                let directive = match serve(&mut stream, state.as_ref()) {
                    Ok(directive) => directive,
                    Err(error) => {
                        eprintln!("ruyiseekd: client request failed: {error}");
                        ServerDirective::Continue
                    }
                };
                if config.once || directive == ServerDirective::Shutdown {
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
    state: &RwLock<IndexState>,
) -> Result<ServerDirective, Box<dyn Error>> {
    let payload = read_frame(stream)?;
    let (response, directive) = match decode_request(&payload) {
        Ok(Request::Ping) => (
            Response::Pong {
                protocol_version: PROTOCOL_VERSION,
            },
            ServerDirective::Continue,
        ),
        Ok(Request::Status) => {
            let status = state
                .read()
                .unwrap_or_else(PoisonError::into_inner)
                .status
                .clone();
            (Response::Status(status), ServerDirective::Continue)
        }
        Ok(Request::Shutdown) => (Response::Acknowledged, ServerDirective::Shutdown),
        Ok(Request::Search { query, limit }) => {
            let started = Instant::now();
            let hits = state
                .read()
                .unwrap_or_else(PoisonError::into_inner)
                .engine
                .search(&query, limit);
            (
                Response::Search {
                    elapsed_us: u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
                    hits,
                },
                ServerDirective::Continue,
            )
        }
        Err(error) => (
            Response::Error(error.to_string()),
            ServerDirective::Continue,
        ),
    };
    write_frame(stream, &encode_response(&response))?;
    Ok(directive)
}

fn build_index_state(roots: Vec<PathBuf>, include_hidden: bool, max_entries: usize) -> IndexState {
    let report = scan(&ScanOptions {
        roots: roots.clone(),
        include_hidden,
        max_entries,
    });
    let status = DaemonStatus {
        indexed_items: report.items.len(),
        skipped_paths: report.skipped_paths,
        truncated: report.truncated,
    };
    IndexState {
        engine: SearchEngine::new(report.items),
        status,
        roots,
        scanned_directories: report.scanned_directories,
        max_entries,
    }
}

fn discover_roots_or_home(home: &Path) -> Vec<PathBuf> {
    match discover_default_roots(home) {
        Ok(roots) => roots,
        Err(error) => {
            eprintln!("ruyiseekd: could not read Linux mount table, indexing HOME only: {error}");
            vec![home.to_path_buf()]
        }
    }
}

fn entry_limit(configured: Option<usize>, root_count: usize) -> usize {
    configured.unwrap_or_else(|| {
        AUTO_ENTRIES_PER_ROOT
            .saturating_mul(root_count.max(1))
            .min(AUTO_ENTRIES_MAX)
    })
}

fn spawn_refresh_monitor(
    automatic_home: Option<PathBuf>,
    include_hidden: bool,
    configured_max_entries: Option<usize>,
    state: Arc<RwLock<IndexState>>,
) -> io::Result<RefreshMonitor> {
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = Arc::clone(&stop);
    let handle = thread::Builder::new()
        .name("ruyiseek-refresh-monitor".to_owned())
        .spawn(move || {
            let initial_directories = state
                .read()
                .unwrap_or_else(PoisonError::into_inner)
                .scanned_directories
                .clone();
            let mut watcher = create_file_watcher(&initial_directories);
            let mut last_refresh = Instant::now();
            let mut refresh_pending = false;

            while !worker_stop.load(Ordering::Acquire) {
                thread::park_timeout(REFRESH_POLL_INTERVAL);
                if worker_stop.load(Ordering::Acquire) {
                    break;
                }

                let file_changed = refresh_pending
                    || watcher.as_ref().is_some_and(|active| {
                        match active.has_changes() {
                            Ok(changed) => changed,
                            Err(error) => {
                                eprintln!("ruyiseekd: file watch failed: {error}");
                                true
                            }
                        }
                    });
                refresh_pending = false;

                let current_roots = state
                    .read()
                    .unwrap_or_else(PoisonError::into_inner)
                    .roots
                    .clone();
                let roots = match automatic_home.as_deref() {
                    Some(home) => match discover_default_roots(home) {
                        Ok(roots) => roots,
                        Err(error) => {
                            eprintln!("ruyiseekd: mount refresh failed: {error}");
                            current_roots.clone()
                        }
                    },
                    None => current_roots.clone(),
                };
                let mounts_changed = roots != current_roots;
                let fallback_due = last_refresh.elapsed() >= FALLBACK_RESCAN_INTERVAL;
                if !file_changed && !mounts_changed && !fallback_due {
                    continue;
                }

                if mounts_changed {
                    eprintln!(
                        "ruyiseekd: mount set changed ({} -> {} roots), rebuilding index",
                        current_roots.len(),
                        roots.len()
                    );
                } else if file_changed {
                    eprintln!("ruyiseekd: filesystem changed, rebuilding index");
                } else {
                    eprintln!("ruyiseekd: periodic fallback refresh, rebuilding index");
                }
                let max_entries = entry_limit(configured_max_entries, roots.len());
                let replacement = build_index_state(roots, include_hidden, max_entries);
                if worker_stop.load(Ordering::Acquire) {
                    break;
                }

                let replacement_watcher = create_file_watcher(&replacement.scanned_directories);
                // Keep the old watcher alive throughout the scan and creation of the new
                // watcher. If something changed while rebuilding, schedule one more pass so
                // an event racing with the directory walk cannot leave a stale snapshot.
                refresh_pending = watcher.as_ref().is_some_and(|active| {
                    active.has_changes().unwrap_or_else(|error| {
                        eprintln!("ruyiseekd: file watch follow-up failed: {error}");
                        true
                    })
                }) || replacement_watcher.as_ref().is_some_and(|active| {
                    active.has_changes().unwrap_or_else(|error| {
                        eprintln!("ruyiseekd: replacement file watch failed: {error}");
                        true
                    })
                });
                watcher = replacement_watcher;
                last_refresh = Instant::now();

                eprintln!(
                    "ruyiseekd: refreshed {} items from {} root(s), skipped {}, truncated={}, limit={}",
                    replacement.status.indexed_items,
                    replacement.roots.len(),
                    replacement.status.skipped_paths,
                    replacement.status.truncated,
                    replacement.max_entries
                );
                *state.write().unwrap_or_else(PoisonError::into_inner) = replacement;
            }
        })?;
    Ok(RefreshMonitor {
        stop,
        handle: Some(handle),
    })
}

fn create_file_watcher(directories: &[PathBuf]) -> Option<FileWatcher> {
    match FileWatcher::new(directories) {
        Ok(watcher) => {
            if watcher.skipped > 0 {
                eprintln!(
                    "ruyiseekd: watching {} directories; {} could not be watched",
                    watcher.watched, watcher.skipped
                );
            } else {
                eprintln!("ruyiseekd: watching {} directories", watcher.watched);
            }
            Some(watcher)
        }
        Err(error) => {
            eprintln!(
                "ruyiseekd: file watching unavailable ({error}); periodic refresh remains active"
            );
            None
        }
    }
}

struct FileWatcher {
    fd: std::os::fd::OwnedFd,
    watched: usize,
    skipped: usize,
}

impl FileWatcher {
    fn new(directories: &[PathBuf]) -> io::Result<Self> {
        let fd = inotify::init(inotify::CreateFlags::CLOEXEC | inotify::CreateFlags::NONBLOCK)
            .map_err(errno_to_io)?;
        let flags = inotify::WatchFlags::CREATE
            | inotify::WatchFlags::DELETE
            | inotify::WatchFlags::DELETE_SELF
            | inotify::WatchFlags::MOVED_FROM
            | inotify::WatchFlags::MOVED_TO
            | inotify::WatchFlags::MOVE_SELF
            | inotify::WatchFlags::DONT_FOLLOW
            | inotify::WatchFlags::EXCL_UNLINK
            | inotify::WatchFlags::ONLYDIR;
        let mut watched = 0;
        let mut skipped = 0;
        for directory in directories {
            match inotify::add_watch(&fd, directory, flags) {
                Ok(_) => watched += 1,
                Err(_) => skipped += 1,
            }
        }
        Ok(Self {
            fd,
            watched,
            skipped,
        })
    }

    fn has_changes(&self) -> io::Result<bool> {
        let mut buffer = [MaybeUninit::<u8>::uninit(); 64 * 1024];
        let mut reader = inotify::Reader::new(&self.fd, &mut buffer);
        match reader.next() {
            Ok(_) => Ok(true),
            Err(Errno::AGAIN) => Ok(false),
            Err(error) => Err(errno_to_io(error)),
        }
    }
}

fn errno_to_io(error: Errno) -> io::Error {
    io::Error::from_raw_os_error(error.raw_os_error())
}

struct RefreshMonitor {
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl Drop for RefreshMonitor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.handle.take() {
            handle.thread().unpark();
            if handle.join().is_err() {
                eprintln!("ruyiseekd: refresh monitor panicked during shutdown");
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ServerDirective {
    Continue,
    Shutdown,
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
    let mut max_entries = None;
    let mut once = false;
    let mut args = args.peekable();

    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--root" => roots.push(PathBuf::from(next_value(&mut args, "--root")?)),
            "--socket" => socket = PathBuf::from(next_value(&mut args, "--socket")?),
            "--max-entries" => max_entries = Some(next_value(&mut args, "--max-entries")?.parse()?),
            "--include-hidden" => include_hidden = true,
            "--once" => once = true,
            "-h" | "--help" => {
                print_help();
                return Ok(None);
            }
            _ => return Err(format!("unknown argument: {argument}").into()),
        }
    }

    let automatic_home = if roots.is_empty() {
        // 当 UI fork 出 daemon 时不会传任何参数，回落到 $HOME，避免无意义地
        // 索引 / 根目录（c++ runtime + 内核模块会让 Top-K 全部跑飞）。如果
        // $HOME 不存在（极端容器/POSIX-only 环境），最后再退回 current_dir。
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            if home.is_dir() {
                roots.push(home.clone());
                Some(home)
            } else {
                let current = std::env::current_dir()?;
                roots.push(current.clone());
                Some(current)
            }
        } else {
            let current = std::env::current_dir()?;
            roots.push(current.clone());
            Some(current)
        }
    } else {
        None
    };
    if max_entries == Some(0) {
        return Err("--max-entries must be greater than zero".into());
    }

    Ok(Some(Config {
        roots,
        automatic_home,
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
        "ruyiseekd {version}\n\nUSAGE:\n    ruyiseekd [OPTIONS]\n\nOPTIONS:\n    --root PATH          Index only this root (repeatable; disables volume discovery)\n    --socket PATH        Override the Unix Socket path\n    --max-entries N      Global item limit [default: 250000 per auto root, max 2000000]\n    --include-hidden     Include dot-files and dot-directories\n    --once               Serve one client and exit (integration testing)\n    -h, --help           Show this help",
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
    use std::time::{SystemTime, UNIX_EPOCH};

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

    fn fixture_state() -> RwLock<IndexState> {
        RwLock::new(IndexState {
            engine: SearchEngine::new(vec![SearchItem {
                id: 1,
                name: "完整开发设计文档.md".to_owned(),
                path: PathBuf::from("/project/docs/完整开发设计文档.md"),
                kind: ItemKind::File,
                hidden: false,
            }]),
            status: DaemonStatus {
                indexed_items: 1,
                skipped_paths: 0,
                truncated: false,
            },
            roots: vec![PathBuf::from("/project")],
            scanned_directories: vec![PathBuf::from("/project")],
            max_entries: 250_000,
        })
    }

    #[test]
    fn file_watcher_detects_a_new_chinese_filename() {
        let root = std::env::temp_dir().join(format!(
            "ruyiseekd-watch-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock must be after the Unix epoch")
                .as_nanos()
        ));
        fs::create_dir_all(&root).expect("create watcher fixture");
        let watcher = FileWatcher::new(std::slice::from_ref(&root)).expect("create file watcher");

        fs::write(root.join("中文.txt"), b"").expect("create Chinese-named file");
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut changed = false;
        while Instant::now() < deadline {
            changed = watcher.has_changes().expect("read file watch events");
            if changed {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }

        assert!(changed, "creating 中文.txt should emit an inotify event");
        fs::remove_dir_all(&root).expect("remove watcher fixture");
    }

    #[test]
    fn serves_search_across_the_protocol_boundary() {
        let mut stream = stream_for(&Request::Search {
            query: "设计文档".to_owned(),
            limit: 10,
        });
        assert_eq!(
            serve(&mut stream, &fixture_state()).expect("serve search request"),
            ServerDirective::Continue
        );
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

        assert_eq!(
            serve(&mut stream, &fixture_state()).expect("serve malformed request"),
            ServerDirective::Continue
        );
        let response = decode_response(
            &read_frame(&mut Cursor::new(stream.output)).expect("read framed response"),
        )
        .expect("decode response");
        assert!(matches!(response, Response::Error(_)));
    }

    #[test]
    fn shutdown_is_acknowledged_and_stops_the_accept_loop() {
        let mut stream = stream_for(&Request::Shutdown);
        assert_eq!(
            serve(&mut stream, &fixture_state()).expect("serve shutdown request"),
            ServerDirective::Shutdown
        );
        let response = decode_response(
            &read_frame(&mut Cursor::new(stream.output)).expect("read framed response"),
        )
        .expect("decode response");
        assert_eq!(response, Response::Acknowledged);
    }

    #[test]
    fn no_root_arg_falls_back_to_home_when_available() {
        // 当 UI fork 出 daemon 时不带任何参数，应当落到 $HOME 而非 current_dir，
        // 这样无论 UI 是从 autostart、托盘还是 shell 拉起，daemon 索引的都是
        // 用户的真实文件树，而不是 / 或者容器 cwd。
        let config = parse_args(std::iter::empty())
            .expect("parse_args")
            .expect("non-help result");
        assert_eq!(config.roots.len(), 1, "应至少注入一个 root");
        assert_eq!(config.automatic_home.as_ref(), config.roots.first());
        let expected = std::env::var_os("HOME").map(PathBuf::from);
        match expected {
            Some(home) if home.is_dir() => {
                assert_eq!(config.roots[0], home, "应该直接使用 $HOME");
            }
            _ => {
                // 容器或 CI 中 $HOME 可能未设置 / 不是目录，回落 current_dir 也合法。
                assert_eq!(
                    config.roots[0],
                    std::env::current_dir().expect("current_dir"),
                    "无 $HOME 时退回 current_dir"
                );
            }
        }
    }

    #[test]
    fn explicit_roots_disable_automatic_volume_discovery() {
        let config = parse_args(
            ["--root", "/srv/search", "--max-entries", "42"]
                .into_iter()
                .map(str::to_owned),
        )
        .expect("parse_args")
        .expect("non-help result");

        assert_eq!(config.roots, vec![PathBuf::from("/srv/search")]);
        assert_eq!(config.automatic_home, None);
        assert_eq!(config.max_entries, Some(42));
    }

    #[test]
    fn automatic_budget_scales_per_volume_and_is_capped() {
        assert_eq!(entry_limit(None, 0), 250_000);
        assert_eq!(entry_limit(None, 3), 750_000);
        assert_eq!(entry_limit(None, 100), 2_000_000);
        assert_eq!(entry_limit(Some(123), 100), 123);
    }
}
