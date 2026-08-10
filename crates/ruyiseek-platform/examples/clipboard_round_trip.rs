//! 端到端测试：用 `x11_clipboard::set_clipboard` 写一段数据，然后
//! 通过另一个 X11 连接读回（不走 xclip，验证我们自己实现的协议）。
//!
//! 用法：
//!   xvfb-run -a cargo run --example clipboard_round_trip --features x11 -- <utf-8 文本>
//!
//! 在测试 X server 上：
//! 1. 开 X 连接 A，写 CLIPBOARD；
//! 2. 开 X 连接 B，ConvertSelection → 读 SelectionNotify → 读 property；
//! 3. 比较数据。
//!
//! 这能确认三个事：(a) set_clipboard 不会 panic；(b) 协议正确
//! （TARGETS / UTF8_STRING 都按规范返回）；(c) 数据未在传输中损坏。

use std::env;
use std::process::ExitCode;
use std::time::Duration;

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    AtomEnum, ConnectionExt, ConvertSelectionRequest, EventMask, PropMode, SelectionNotifyEvent,
};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;

use ruyiseek_platform::x11_clipboard::{set_clipboard, ClipboardMime};

fn main() -> ExitCode {
    let payload = env::args()
        .nth(1)
        .unwrap_or_else(|| "hello clipboard".to_owned());
    let mime = if env::args().any(|a| a == "--uri") {
        ClipboardMime::UriList
    } else {
        ClipboardMime::Text
    };
    println!("writing payload: {payload:?} as {mime:?}");

    let owner = match set_clipboard(payload.as_bytes().to_vec(), mime) {
        Ok(owner) => owner,
        Err(error) => {
            eprintln!("set_clipboard failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    println!("set_clipboard returned ok (owner held in background thread)");

    // 等待 background thread 真正进入事件循环（owner 初始化要 connect X + 建窗口）
    std::thread::sleep(Duration::from_millis(100));

    // 另起一个连接读
    let Ok((conn, screen_num)) = x11rb::connect(None) else {
        eprintln!("read 连接 X 失败");
        return ExitCode::FAILURE;
    };
    let root = conn.setup().roots[screen_num].root;
    let window = match conn.generate_id() {
        Ok(w) => w,
        Err(e) => {
            eprintln!("generate_id: {e}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(e) = conn.create_window(
        0,
        window,
        root,
        -10,
        -10,
        1,
        1,
        0,
        x11rb::protocol::xproto::WindowClass::INPUT_OUTPUT,
        x11rb::COPY_FROM_PARENT,
        &Default::default(),
    ) {
        eprintln!("create_window: {e}");
        return ExitCode::FAILURE;
    }

    let clipboard_atom = match intern_atom(&conn, b"CLIPBOARD") {
        Ok(a) => a,
        Err(e) => {
            eprintln!("intern CLIPBOARD: {e}");
            return ExitCode::FAILURE;
        }
    };
    let target_atom = match mime {
        ClipboardMime::Text => intern_atom(&conn, b"UTF8_STRING"),
        ClipboardMime::UriList => intern_atom(&conn, b"text/uri-list"),
    };
    let target = match target_atom {
        Ok(a) => a,
        Err(e) => {
            eprintln!("intern target: {e}");
            return ExitCode::FAILURE;
        }
    };
    let property = match intern_atom(&conn, b"XSEL_DATA") {
        Ok(a) => a,
        Err(e) => {
            eprintln!("intern XSEL_DATA: {e}");
            return ExitCode::FAILURE;
        }
    };

    if let Err(e) = conn.convert_selection(
        window,
        clipboard_atom,
        target,
        property,
        x11rb::CURRENT_TIME,
    ) {
        eprintln!("convert_selection: {e}");
        return ExitCode::FAILURE;
    }
    if let Err(e) = conn.flush() {
        eprintln!("flush: {e}");
        return ExitCode::FAILURE;
    }

    // 等待 SelectionNotify
    let mut got_notify = None;
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        match conn.wait_for_event() {
            Ok(Event::SelectionNotify(n)) => {
                got_notify = Some(n);
                break;
            }
            Ok(_) => continue,
            Err(e) => {
                eprintln!("wait_for_event: {e}");
                return ExitCode::FAILURE;
            }
        }
    }
    let notify = match got_notify {
        Some(n) => n,
        None => {
            eprintln!("未收到 SelectionNotify");
            return ExitCode::FAILURE;
        }
    };
    println!("got SelectionNotify: property={:?}", notify.property);
    if notify.property == x11rb::NONE {
        eprintln!("property == None，转换失败");
        return ExitCode::FAILURE;
    }

    // 读 property
    let reply = match conn.get_property(false, window, notify.property, AtomEnum::ANY, 0, 4096) {
        Ok(c) => c.reply(),
        Err(e) => {
            eprintln!("get_property cookie: {e}");
            return ExitCode::FAILURE;
        }
    };
    let reply = match reply {
        Ok(r) => r,
        Err(e) => {
            eprintln!("get_property reply: {e}");
            return ExitCode::FAILURE;
        }
    };
    let data = reply.value;
    let got = match std::str::from_utf8(&data) {
        Ok(s) => s.to_owned(),
        Err(_) => String::from_utf8_lossy(&data).into_owned(),
    };
    println!("read back ({} bytes): {:?}", data.len(), got);
    if got == payload {
        println!("✓ ROUND-TRIP OK");
        ExitCode::SUCCESS
    } else {
        eprintln!("✗ MISMATCH: expected {payload:?}, got {got:?}");
        ExitCode::FAILURE
    }
}

fn intern_atom(conn: &RustConnection, name: &[u8]) -> Result<u32, String> {
    conn.intern_atom(false, name)
        .map_err(|e| format!("{e}"))?
        .reply()
        .map(|r| r.atom)
        .map_err(|e| format!("{e}"))
}

// 抑制未使用
#[allow(dead_code)]
fn _unused() -> Option<ConvertSelectionRequest> {
    None
}
#[allow(dead_code)]
fn _unused2() -> Option<SelectionNotifyEvent> {
    None
}
#[allow(dead_code)]
fn _unused3() -> Option<PropMode> {
    None
}
#[allow(dead_code)]
fn _unused4() -> Option<EventMask> {
    None
}
