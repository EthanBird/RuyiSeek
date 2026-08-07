use ruyiseek_ipc::{default_socket_path, request_daemon, DaemonStatus, Request, Response};
use ruyiseek_platform::hotkey::{
    ControlKey, DoubleCtrlRecognizer, GestureContext, GestureDecision, Key, KeyEvent, KeyState,
};
use ruyiseek_platform::x11_hotkey::DoubleCtrlControl;
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

mod autostart;
mod config;
mod session_bus;
mod session_lock;
mod tray;

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

    run_launcher(options.mode)
}

fn run_launcher(mode: LaunchMode) -> Result<(), Box<dyn Error>> {
    let (ui_sender, ui_receiver) = mpsc::channel();
    let _instance_guard = match session_bus::claim_or_forward(ui_sender.clone(), mode.action()) {
        Ok(session_bus::Claim::Primary(guard)) => Some(guard),
        Ok(session_bus::Claim::Forwarded) => return Ok(()),
        Err(error) => {
            eprintln!("ruyiseek-ui: 无法接入桌面会话 D-Bus：{error}");
            if matches!(mode, LaunchMode::Hide | LaunchMode::ExitUi) {
                return Ok(());
            }
            None
        }
    };

    match mode {
        LaunchMode::Hide | LaunchMode::ExitUi => return Ok(()),
        LaunchMode::Quit => {
            stop_daemon().map_err(|error| format!("无法停止后台服务：{error}"))?;
            return Ok(());
        }
        LaunchMode::Show | LaunchMode::Background | LaunchMode::Toggle | LaunchMode::Settings => {}
    }

    let config_path = match config::default_path() {
        Ok(path) => Some(path),
        Err(error) => {
            eprintln!("ruyiseek-ui: 无法确定配置路径，将使用临时默认配置：{error}");
            None
        }
    };
    let (loaded_config, config_warning) = config_path.as_ref().map_or_else(
        || (config::AppConfig::default(), None),
        |path| config::AppConfig::load_resilient(path),
    );
    if let Some(warning) = config_warning {
        eprintln!("ruyiseek-ui: {warning}");
    }
    let hotkey_control = DoubleCtrlControl::new(
        loaded_config.double_ctrl_enabled,
        loaded_config.suppress_in_fullscreen,
    );
    let app_config = Rc::new(RefCell::new(loaded_config));

    let launcher = LauncherWindow::new()?;
    let visible = Rc::new(Cell::new(false));
    let generation = Rc::new(Cell::new(0_u64));
    let result_paths = Rc::new(RefCell::new(Vec::<PathBuf>::new()));

    let (worker_sender, worker_receiver) = mpsc::channel();
    let _search_worker = spawn_search_worker(worker_receiver, ui_sender.clone());
    worker_sender.send(WorkerCommand::Status)?;

    install_ui_callbacks(
        &launcher,
        worker_sender.clone(),
        &generation,
        &result_paths,
        Rc::clone(&visible),
    );
    install_settings_callbacks(
        &launcher,
        Rc::clone(&app_config),
        config_path,
        hotkey_control.clone(),
        Rc::clone(&visible),
    );

    let _hotkey_thread = install_hotkey(&ui_sender, hotkey_control);
    let _tray_guard = match tray::spawn(ui_sender) {
        Ok(guard) => Some(guard),
        Err(error) => {
            eprintln!("ruyiseek-ui: 无法启动系统托盘线程：{error}");
            None
        }
    };
    let poll_timer = install_ui_event_pump(
        &launcher,
        ui_receiver,
        generation,
        result_paths,
        Rc::clone(&visible),
        Rc::clone(&app_config),
        worker_sender.clone(),
    );

    if let Some(action) = mode.action() {
        apply_desktop_action(&launcher, &visible, &app_config, &worker_sender, action);
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

fn install_settings_callbacks(
    launcher: &LauncherWindow,
    app_config: Rc<RefCell<config::AppConfig>>,
    config_path: Option<PathBuf>,
    hotkey_control: DoubleCtrlControl,
    visible: Rc<Cell<bool>>,
) {
    let weak_launcher = launcher.as_weak();
    launcher.on_close_settings(move || {
        let Some(launcher) = weak_launcher.upgrade() else {
            return;
        };
        launcher.set_settings_mode(false);
        if let Err(error) = launcher.hide() {
            eprintln!("ruyiseek-ui: 隐藏设置窗口失败：{error}");
        } else {
            visible.set(false);
        }
    });

    let weak_launcher = launcher.as_weak();
    launcher.on_save_settings(
        move |launch_at_login, double_ctrl_enabled, suppress_in_fullscreen| {
            let Some(launcher) = weak_launcher.upgrade() else {
                return;
            };
            let Some(config_path) = config_path.as_ref() else {
                launcher.set_settings_status_text("无法确定配置目录，设置未保存".into());
                return;
            };

            let previous = app_config.borrow().clone();
            let next = config::AppConfig {
                launch_at_login,
                double_ctrl_enabled,
                suppress_in_fullscreen,
                ..previous.clone()
            };
            let autostart_changed = previous.launch_at_login != launch_at_login;
            let autostart_path = match autostart::default_path() {
                Ok(path) => path,
                Err(error) => {
                    launcher.set_settings_status_text(format!("自动启动设置失败：{error}").into());
                    return;
                }
            };
            let executable = match std::env::current_exe() {
                Ok(path) => path,
                Err(error) => {
                    launcher.set_settings_status_text(format!("无法确定程序路径：{error}").into());
                    return;
                }
            };

            if autostart_changed {
                if let Err(error) =
                    autostart::set_enabled(&autostart_path, &executable, launch_at_login)
                {
                    launcher.set_settings_status_text(format!("自动启动设置失败：{error}").into());
                    return;
                }
            }
            if let Err(error) = next.save(config_path) {
                if autostart_changed {
                    let _ = autostart::set_enabled(
                        &autostart_path,
                        &executable,
                        previous.launch_at_login,
                    );
                }
                launcher.set_settings_status_text(format!("保存失败：{error}").into());
                return;
            }

            hotkey_control.update(double_ctrl_enabled, suppress_in_fullscreen);
            *app_config.borrow_mut() = next;
            launcher.set_settings_status_text("设置已保存并立即生效".into());
        },
    );
}

fn install_ui_event_pump(
    launcher: &LauncherWindow,
    ui_receiver: Receiver<UiEvent>,
    generation: Rc<Cell<u64>>,
    result_paths: Rc<RefCell<Vec<PathBuf>>>,
    visible: Rc<Cell<bool>>,
    app_config: Rc<RefCell<config::AppConfig>>,
    worker_sender: Sender<WorkerCommand>,
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
                UiEvent::Desktop(action) => {
                    apply_desktop_action(&launcher, &visible, &app_config, &worker_sender, action);
                }
                UiEvent::Shutdown(Ok(())) => quit_ui(),
                UiEvent::Shutdown(Err(error)) => {
                    eprintln!("ruyiseek-ui: 完全退出时停止后台服务失败：{error}");
                    show_launcher(&launcher, &visible);
                    launcher.set_status_text(format!("后台停止失败，尚未退出：{error}").into());
                }
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

fn apply_desktop_action(
    launcher: &LauncherWindow,
    visible: &Cell<bool>,
    app_config: &RefCell<config::AppConfig>,
    worker_sender: &Sender<WorkerCommand>,
    action: DesktopAction,
) {
    match action {
        DesktopAction::Show => show_launcher(launcher, visible),
        DesktopAction::Hide => hide_launcher(launcher, visible),
        DesktopAction::Toggle => {
            if visible.get() {
                hide_launcher(launcher, visible);
            } else {
                show_launcher(launcher, visible);
            }
        }
        DesktopAction::Settings => show_settings(launcher, visible, app_config),
        DesktopAction::ExitUi => quit_ui(),
        DesktopAction::QuitAll => {
            hide_launcher(launcher, visible);
            if worker_sender.send(WorkerCommand::Shutdown).is_err() {
                eprintln!("ruyiseek-ui: 后台控制线程已经停止");
                show_launcher(launcher, visible);
                launcher.set_status_text("后台控制线程异常，尚未退出".into());
            }
        }
    }
}

fn show_settings(
    launcher: &LauncherWindow,
    visible: &Cell<bool>,
    app_config: &RefCell<config::AppConfig>,
) {
    let config = app_config.borrow();
    launcher.set_launch_at_login(config.launch_at_login);
    launcher.set_double_ctrl_enabled(config.double_ctrl_enabled);
    launcher.set_suppress_in_fullscreen(config.suppress_in_fullscreen);
    launcher.set_settings_status_text("修改后点击保存".into());
    launcher.set_settings_mode(true);
    if let Err(error) = launcher.show() {
        eprintln!("ruyiseek-ui: 显示设置窗口失败：{error}");
    } else {
        visible.set(true);
    }
}

fn quit_ui() {
    if let Err(error) = slint::quit_event_loop() {
        eprintln!("ruyiseek-ui: 无法退出事件循环：{error}");
    }
}

fn show_launcher(launcher: &LauncherWindow, visible: &Cell<bool>) {
    launcher.set_settings_mode(false);
    match launcher.show() {
        Ok(()) => {
            visible.set(true);
            launcher.invoke_focus_query();
        }
        Err(error) => eprintln!("ruyiseek-ui: 显示启动器失败：{error}"),
    }
}

fn hide_launcher(launcher: &LauncherWindow, visible: &Cell<bool>) {
    if !visible.get() {
        return;
    }
    match launcher.hide() {
        Ok(()) => visible.set(false),
        Err(error) => eprintln!("ruyiseek-ui: 隐藏启动器失败：{error}"),
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
        WorkerCommand::Shutdown => UiEvent::Shutdown(stop_daemon()),
    }
}

struct HotkeyRuntime {
    _listener: thread::JoinHandle<()>,
    _lock_monitor: session_lock::Monitor,
}

fn install_hotkey(
    ui_sender: &Sender<UiEvent>,
    control: DoubleCtrlControl,
) -> Option<HotkeyRuntime> {
    if std::env::var("XDG_SESSION_TYPE").is_ok_and(|value| value.eq_ignore_ascii_case("wayland")) {
        let _ = ui_sender.send(UiEvent::HotkeyIssue(
            "Wayland 会话暂不监听修饰键；可从应用菜单打开".to_owned(),
        ));
        return None;
    }

    let lock_monitor = match session_lock::spawn() {
        Ok(monitor) => monitor,
        Err(error) => {
            eprintln!("ruyiseek-ui: 无法确认锁屏状态：{error}");
            let _ = ui_sender.send(UiEvent::HotkeyIssue(
                "无法确认锁屏状态；为避免锁屏误唤醒，双击 Ctrl 已停用".to_owned(),
            ));
            return None;
        }
    };

    let trigger_sender = ui_sender.clone();
    match ruyiseek_platform::x11_hotkey::spawn_double_ctrl_listener(
        lock_monitor.state(),
        control,
        move || {
            let _ = trigger_sender.send(UiEvent::Desktop(DesktopAction::Toggle));
        },
    ) {
        Ok(listener) => Some(HotkeyRuntime {
            _listener: listener,
            _lock_monitor: lock_monitor,
        }),
        Err(error) => {
            eprintln!("ruyiseek-ui: 双击 Ctrl 不可用：{error}");
            let _ = ui_sender.send(UiEvent::HotkeyIssue(format!("双击 Ctrl 不可用：{error}")));
            None
        }
    }
}

fn stop_daemon() -> Result<(), String> {
    let socket = default_socket_path();
    if !socket.exists() {
        return Ok(());
    }
    request_daemon(&socket, &Request::Shutdown, DAEMON_TIMEOUT)
        .map_err(connection_message)
        .and_then(|response| match response {
            Response::Acknowledged => Ok(()),
            Response::Error(message) => Err(message),
            _ => Err("后台服务返回了意外的停止响应".to_owned()),
        })
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
    Shutdown,
}

enum UiEvent {
    Status(Result<DaemonStatus, String>),
    Search {
        generation: u64,
        result: Result<Vec<SearchResult>, String>,
    },
    Shutdown(Result<(), String>),
    Desktop(DesktopAction),
    HotkeyIssue(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DesktopAction {
    Show,
    Hide,
    Toggle,
    Settings,
    ExitUi,
    QuitAll,
}

struct SearchResult {
    title: String,
    path: PathBuf,
    kind: String,
    score: f32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum LaunchMode {
    #[default]
    Show,
    Background,
    Toggle,
    Hide,
    Settings,
    ExitUi,
    Quit,
}

impl LaunchMode {
    const fn action(self) -> Option<DesktopAction> {
        match self {
            Self::Show => Some(DesktopAction::Show),
            Self::Background => None,
            Self::Toggle => Some(DesktopAction::Toggle),
            Self::Hide => Some(DesktopAction::Hide),
            Self::Settings => Some(DesktopAction::Settings),
            Self::ExitUi => Some(DesktopAction::ExitUi),
            Self::Quit => Some(DesktopAction::QuitAll),
        }
    }
}

#[derive(Debug, Default, Eq, PartialEq)]
struct Options {
    mode: LaunchMode,
    demo_double_ctrl: bool,
    help: bool,
}

fn parse_args(args: impl Iterator<Item = String>) -> Result<Options, Box<dyn Error>> {
    let mut options = Options::default();
    let mut explicit_mode = None;
    for argument in args {
        match argument.as_str() {
            "--background" => set_mode(&mut explicit_mode, LaunchMode::Background)?,
            "--toggle" => set_mode(&mut explicit_mode, LaunchMode::Toggle)?,
            "--hide" => set_mode(&mut explicit_mode, LaunchMode::Hide)?,
            "--settings" => set_mode(&mut explicit_mode, LaunchMode::Settings)?,
            "--exit-ui" => set_mode(&mut explicit_mode, LaunchMode::ExitUi)?,
            "--quit" => set_mode(&mut explicit_mode, LaunchMode::Quit)?,
            "--demo-double-ctrl" => options.demo_double_ctrl = true,
            "-h" | "--help" => options.help = true,
            _ => return Err(format!("未知参数：{argument}").into()),
        }
    }
    options.mode = explicit_mode.unwrap_or_default();
    Ok(options)
}

fn set_mode(target: &mut Option<LaunchMode>, mode: LaunchMode) -> Result<(), Box<dyn Error>> {
    if target.replace(mode).is_some() {
        return Err("只能指定一个启动/控制选项".into());
    }
    Ok(())
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
        "ruyiseek-ui {version}\n\n用法：\n    ruyiseek-ui [--background | --toggle | --hide | --settings | --exit-ui | --quit]\n    ruyiseek-ui --demo-double-ctrl\n\n选项：\n    --background        隐藏启动并注册托盘、热键与 D-Bus 服务\n    --toggle            显示或隐藏已运行的搜索窗口\n    --hide              隐藏已运行的搜索窗口\n    --settings          打开设置窗口\n    --exit-ui           退出界面，保留后台索引服务\n    --quit              完全退出界面与后台索引服务\n    --demo-double-ctrl  运行手势状态机演示\n    -h, --help          显示帮助",
        version = env!("CARGO_PKG_VERSION")
    );
}

#[cfg(test)]
mod tests {
    use super::{parse_args, LaunchMode, Options};

    fn args(values: &[&str]) -> impl Iterator<Item = String> {
        values
            .iter()
            .map(|value| (*value).to_owned())
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[test]
    fn normal_launch_requests_show() {
        assert_eq!(parse_args(args(&[])).unwrap(), Options::default());
    }

    #[test]
    fn parses_desktop_control_modes() {
        for (argument, expected) in [
            ("--background", LaunchMode::Background),
            ("--toggle", LaunchMode::Toggle),
            ("--hide", LaunchMode::Hide),
            ("--settings", LaunchMode::Settings),
            ("--exit-ui", LaunchMode::ExitUi),
            ("--quit", LaunchMode::Quit),
        ] {
            assert_eq!(parse_args(args(&[argument])).unwrap().mode, expected);
        }
    }

    #[test]
    fn rejects_conflicting_control_modes() {
        let error = parse_args(args(&["--background", "--quit"])).unwrap_err();
        assert_eq!(error.to_string(), "只能指定一个启动/控制选项");
    }
}
