use ruyiseek_ipc::{default_socket_path, request_daemon, DaemonStatus, Request, Response};
use ruyiseek_platform::hotkey::{
    ControlKey, DoubleCtrlRecognizer, GestureContext, GestureDecision, Key, KeyEvent, KeyState,
};
use slint::{ComponentHandle, ModelRc, Timer, TimerMode, VecModel};
use std::cell::{Cell, RefCell};
use std::error::Error;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

#[allow(clippy::all, clippy::pedantic)]
mod generated_ui {
    slint::include_modules!();
}
use generated_ui::{LauncherResult, LauncherWindow};

const DAEMON_TIMEOUT: Duration = Duration::from_secs(5);
const SEARCH_LIMIT: usize = 9;
const UI_POLL_INTERVAL: Duration = Duration::from_millis(16);

fn main() -> Result<(), Box<dyn Error>> {
    let options = parse_args(std::env::args().skip(1))?;
    if options.help {
        print_help();
        return Ok(());
    }
    if options.demo_double_ctrl {
        return demo_double_ctrl();
    }

    run_launcher(options.background)
}

fn run_launcher(background: bool) -> Result<(), Box<dyn Error>> {
    let launcher = LauncherWindow::new()?;
    let visible = Rc::new(Cell::new(!background));
    let generation = Rc::new(Cell::new(0_u64));
    let result_paths = Rc::new(RefCell::new(Vec::<PathBuf>::new()));

    let (worker_sender, worker_receiver) = mpsc::channel();
    let (ui_sender, ui_receiver) = mpsc::channel();
    let _search_worker = spawn_search_worker(worker_receiver, ui_sender.clone());
    worker_sender.send(WorkerCommand::Status)?;

    install_ui_callbacks(
        &launcher,
        worker_sender,
        &generation,
        &result_paths,
        Rc::clone(&visible),
    );

    let _hotkey_thread = install_hotkey(&ui_sender);
    let poll_timer =
        install_ui_event_pump(&launcher, ui_receiver, generation, result_paths, visible);

    if !background {
        launcher.show()?;
        launcher.invoke_focus_query();
    }
    slint::run_event_loop_until_quit()?;
    drop(poll_timer);
    Ok(())
}

fn install_ui_callbacks(
    launcher: &LauncherWindow,
    worker_sender: Sender<WorkerCommand>,
    generation: &Rc<Cell<u64>>,
    result_paths: &Rc<RefCell<Vec<PathBuf>>>,
    visible: Rc<Cell<bool>>,
) {
    let weak_launcher = launcher.as_weak();
    let callback_generation = Rc::clone(generation);
    let callback_paths = Rc::clone(result_paths);
    launcher.on_query_edited(move |query| {
        let Some(launcher) = weak_launcher.upgrade() else {
            return;
        };
        let next = callback_generation.get().wrapping_add(1);
        callback_generation.set(next);
        launcher.set_selected_index(0);

        if query.trim().is_empty() {
            callback_paths.borrow_mut().clear();
            launcher.set_results(empty_model());
            launcher.set_status_text("输入关键词开始搜索".into());
        } else {
            launcher.set_status_text("正在搜索…".into());
            let _ = worker_sender.send(WorkerCommand::Search {
                generation: next,
                query: query.to_string(),
            });
        }
    });

    let weak_launcher = launcher.as_weak();
    let activate_paths = Rc::clone(result_paths);
    let activate_visible = Rc::clone(&visible);
    launcher.on_activate_result(move |index| {
        let Some(launcher) = weak_launcher.upgrade() else {
            return;
        };
        let Ok(index) = usize::try_from(index) else {
            return;
        };
        let Some(path) = activate_paths.borrow().get(index).cloned() else {
            return;
        };
        match open_path(&path) {
            Ok(()) => {
                if let Err(error) = launcher.hide() {
                    eprintln!("ruyiseek-ui: 隐藏启动器失败：{error}");
                } else {
                    activate_visible.set(false);
                }
            }
            Err(error) => launcher.set_status_text(format!("无法打开：{error}").into()),
        }
    });

    let weak_launcher = launcher.as_weak();
    launcher.on_dismiss(move || {
        let Some(launcher) = weak_launcher.upgrade() else {
            return;
        };
        if let Err(error) = launcher.hide() {
            eprintln!("ruyiseek-ui: 隐藏启动器失败：{error}");
        } else {
            visible.set(false);
        }
    });
}

fn install_ui_event_pump(
    launcher: &LauncherWindow,
    ui_receiver: Receiver<UiEvent>,
    generation: Rc<Cell<u64>>,
    result_paths: Rc<RefCell<Vec<PathBuf>>>,
    visible: Rc<Cell<bool>>,
) -> Timer {
    let timer = Timer::default();
    let weak_launcher = launcher.as_weak();
    timer.start(TimerMode::Repeated, UI_POLL_INTERVAL, move || {
        let Some(launcher) = weak_launcher.upgrade() else {
            return;
        };
        for event in ui_receiver.try_iter() {
            match event {
                UiEvent::Status(result) => match result {
                    Ok(status) if launcher.get_query().trim().is_empty() => {
                        launcher.set_status_text(status_message(&status).into());
                    }
                    Ok(_) => {}
                    Err(error) => launcher.set_status_text(error.into()),
                },
                UiEvent::Search {
                    generation: event_generation,
                    result,
                } if event_generation == generation.get() => match result {
                    Ok(items) => apply_search_results(&launcher, &result_paths, items),
                    Err(error) => {
                        result_paths.borrow_mut().clear();
                        launcher.set_results(empty_model());
                        launcher.set_status_text(error.into());
                    }
                },
                UiEvent::ToggleLauncher => toggle_launcher(&launcher, &visible),
                UiEvent::HotkeyIssue(message) if launcher.get_query().trim().is_empty() => {
                    launcher.set_status_text(message.into());
                }
                UiEvent::Search { .. } | UiEvent::HotkeyIssue(_) => {}
            }
        }
    });
    timer
}

fn apply_search_results(
    launcher: &LauncherWindow,
    result_paths: &RefCell<Vec<PathBuf>>,
    items: Vec<SearchResult>,
) {
    let mut paths = result_paths.borrow_mut();
    paths.clear();
    paths.extend(items.iter().map(|item| item.path.clone()));

    let models = items
        .into_iter()
        .map(|item| LauncherResult {
            title: item.title.into(),
            path: item.path.to_string_lossy().into_owned().into(),
            kind: item.kind.into(),
            score: format!("{:.0}%", item.score * 100.0).into(),
        })
        .collect::<Vec<_>>();
    let count = models.len();
    launcher.set_results(ModelRc::from(Rc::new(VecModel::from(models))));
    launcher.set_selected_index(0);
    if count == 0 {
        launcher.set_status_text("没有找到匹配项".into());
    }
}

fn toggle_launcher(launcher: &LauncherWindow, visible: &Cell<bool>) {
    if visible.get() {
        match launcher.hide() {
            Ok(()) => visible.set(false),
            Err(error) => eprintln!("ruyiseek-ui: 隐藏启动器失败：{error}"),
        }
    } else {
        match launcher.show() {
            Ok(()) => {
                visible.set(true);
                launcher.invoke_focus_query();
            }
            Err(error) => eprintln!("ruyiseek-ui: 显示启动器失败：{error}"),
        }
    }
}

fn empty_model() -> ModelRc<LauncherResult> {
    ModelRc::from(Rc::new(VecModel::from(Vec::new())))
}

fn spawn_search_worker(
    receiver: Receiver<WorkerCommand>,
    ui_sender: Sender<UiEvent>,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("ruyiseek-search".to_owned())
        .spawn(move || {
            while let Ok(mut command) = receiver.recv() {
                if matches!(command, WorkerCommand::Search { .. }) {
                    while let Ok(next) = receiver.try_recv() {
                        command = next;
                    }
                }
                let event = run_worker_command(command);
                if ui_sender.send(event).is_err() {
                    return;
                }
            }
        })
        .expect("search worker thread must start")
}

fn run_worker_command(command: WorkerCommand) -> UiEvent {
    let socket = default_socket_path();
    match command {
        WorkerCommand::Status => UiEvent::Status(
            request_daemon(&socket, &Request::Status, DAEMON_TIMEOUT)
                .map_err(connection_message)
                .and_then(|response| match response {
                    Response::Status(status) => Ok(status),
                    Response::Error(message) => Err(message),
                    _ => Err("后台服务返回了意外的状态响应".to_owned()),
                }),
        ),
        WorkerCommand::Search { generation, query } => {
            let result = request_daemon(
                &socket,
                &Request::Search {
                    query,
                    limit: SEARCH_LIMIT,
                },
                DAEMON_TIMEOUT,
            )
            .map_err(connection_message)
            .and_then(|response| match response {
                Response::Search { hits, .. } => Ok(hits
                    .into_iter()
                    .map(|hit| SearchResult {
                        title: hit.item.name,
                        path: hit.item.path,
                        kind: match hit.item.kind.protocol_name() {
                            "directory" => "文件夹",
                            "application" => "应用",
                            "command" => "命令",
                            _ => "文件",
                        }
                        .to_owned(),
                        score: hit.score,
                    })
                    .collect()),
                Response::Error(message) => Err(message),
                _ => Err("后台服务返回了意外的搜索响应".to_owned()),
            });
            UiEvent::Search { generation, result }
        }
    }
}

fn install_hotkey(ui_sender: &Sender<UiEvent>) -> Option<thread::JoinHandle<()>> {
    if std::env::var("XDG_SESSION_TYPE").is_ok_and(|value| value.eq_ignore_ascii_case("wayland")) {
        let _ = ui_sender.send(UiEvent::HotkeyIssue(
            "Wayland 会话暂不监听修饰键；可从应用菜单打开".to_owned(),
        ));
        return None;
    }

    let trigger_sender = ui_sender.clone();
    match ruyiseek_platform::x11_hotkey::spawn_double_ctrl_listener(move || {
        let _ = trigger_sender.send(UiEvent::ToggleLauncher);
    }) {
        Ok(handle) => Some(handle),
        Err(error) => {
            eprintln!("ruyiseek-ui: 双击 Ctrl 不可用：{error}");
            let _ = ui_sender.send(UiEvent::HotkeyIssue(format!("双击 Ctrl 不可用：{error}")));
            None
        }
    }
}

fn open_path(path: &PathBuf) -> Result<(), Box<dyn Error>> {
    Command::new("xdg-open")
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(())
}

fn connection_message(error: impl std::fmt::Display) -> String {
    format!("无法连接后台服务：{error}。请先启动 ruyiseekd")
}

fn status_message(status: &DaemonStatus) -> String {
    let suffix = if status.truncated {
        "（索引已达上限）"
    } else {
        ""
    };
    format!(
        "已索引 {} 项，跳过 {} 个路径{}",
        status.indexed_items, status.skipped_paths, suffix
    )
}

enum WorkerCommand {
    Status,
    Search { generation: u64, query: String },
}

enum UiEvent {
    Status(Result<DaemonStatus, String>),
    Search {
        generation: u64,
        result: Result<Vec<SearchResult>, String>,
    },
    ToggleLauncher,
    HotkeyIssue(String),
}

struct SearchResult {
    title: String,
    path: PathBuf,
    kind: String,
    score: f32,
}

#[derive(Default)]
struct Options {
    background: bool,
    demo_double_ctrl: bool,
    help: bool,
}

fn parse_args(args: impl Iterator<Item = String>) -> Result<Options, Box<dyn Error>> {
    let mut options = Options::default();
    for argument in args {
        match argument.as_str() {
            "--background" => options.background = true,
            "--demo-double-ctrl" => options.demo_double_ctrl = true,
            "-h" | "--help" => options.help = true,
            _ => return Err(format!("未知参数：{argument}").into()),
        }
    }
    Ok(options)
}

fn demo_double_ctrl() -> Result<(), Box<dyn Error>> {
    let mut recognizer = DoubleCtrlRecognizer::default();
    let context = GestureContext::default();
    let events = [
        key_event(0, KeyState::Pressed),
        key_event(40, KeyState::Released),
        key_event(200, KeyState::Pressed),
        key_event(240, KeyState::Released),
    ];

    for event in events {
        if recognizer.handle(event, context) == GestureDecision::Triggered {
            println!("双击 Ctrl 已识别：应显示如意寻启动器。");
            return Ok(());
        }
    }
    Err("演示序列未触发双击 Ctrl".into())
}

const fn key_event(milliseconds: u64, state: KeyState) -> KeyEvent {
    KeyEvent {
        key: Key::Control(ControlKey::Left),
        state,
        at: Duration::from_millis(milliseconds),
        repeat: false,
    }
}

fn print_help() {
    println!(
        "ruyiseek-ui {version}\n\n用法：\n    ruyiseek-ui [--background]\n    ruyiseek-ui --demo-double-ctrl\n\n选项：\n    --background        以后台模式启动，等待双击 Ctrl 唤醒\n    --demo-double-ctrl  运行手势状态机演示\n    -h, --help          显示帮助",
        version = env!("CARGO_PKG_VERSION")
    );
}
