use ruyiseek_ipc::{
    decode_response, default_socket_path, encode_request, read_frame, write_frame, Request,
    Response,
};
use ruyiseek_platform::hotkey::{
    ControlKey, DoubleCtrlRecognizer, GestureContext, GestureDecision, Key, KeyEvent, KeyState,
};
use std::error::Error;
use std::os::unix::net::UnixStream;
use std::time::Duration;

const DAEMON_TIMEOUT: Duration = Duration::from_secs(5);

fn main() -> Result<(), Box<dyn Error>> {
    if std::env::args().any(|argument| argument == "--help" || argument == "-h") {
        print_help();
        return Ok(());
    }
    if std::env::args().any(|argument| argument == "--demo-double-ctrl") {
        return demo_double_ctrl();
    }

    let socket = default_socket_path();
    let mut stream = UnixStream::connect(&socket).map_err(|error| {
        format!(
            "无法连接后台服务 {}：{error}。请先启动 ruyiseekd",
            socket.display()
        )
    })?;
    stream.set_read_timeout(Some(DAEMON_TIMEOUT))?;
    stream.set_write_timeout(Some(DAEMON_TIMEOUT))?;
    write_frame(&mut stream, &encode_request(&Request::Status))?;
    match decode_response(&read_frame(&mut stream)?)? {
        Response::Status(status) => {
            println!(
                "如意寻 UI 开发壳已连接：索引 {} 项，跳过 {} 路径，已截断={}。",
                status.indexed_items, status.skipped_paths, status.truncated
            );
            println!("下一迭代将在此进程接入 Slint 窗口、XInput2 与托盘。");
            Ok(())
        }
        Response::Error(message) => Err(message.into()),
        _ => Err("后台服务返回了意外响应".into()),
    }
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
        "ruyiseek-ui {version}\n\nUSAGE:\n    ruyiseek-ui\n    ruyiseek-ui --demo-double-ctrl\n\n当前版本是阶段 A 开发壳；Slint 与原生桌面适配将在下一迭代接入。",
        version = env!("CARGO_PKG_VERSION")
    );
}
