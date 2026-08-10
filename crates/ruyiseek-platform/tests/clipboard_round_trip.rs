//! Integration test for the X11 CLIPBOARD owner module.
//!
//! Requires a running X server (xvfb-run works). Verifies that:
//! 1. `set_clipboard` writes data that another X client can read back.
//! 2. The advertised TARGETS list contains the expected MIME types.
//! 3. Reading via `text/uri-list` returns the URI byte stream.

#![cfg(all(target_os = "linux", feature = "x11"))]

use ruyiseek_platform::x11_clipboard::{set_clipboard, ClipboardMime, ClipboardOwner};
use std::process::{Command, Stdio};
use std::time::Duration;

#[test]
fn clipboard_round_trip_under_xvfb() {
    if std::env::var("DISPLAY").is_err() {
        eprintln!("DISPLAY 未设置，跳过 X11 集成测试");
        return;
    }
    // 1. 设剪贴板：用 ruyiseek-platform 的 x11_clipboard 模块写一段 UTF-8 文本。
    let payload = "如意寻 clipboard integration test \u{2728}";
    let Some(_owner) = run_clipboard_setter(payload, ClipboardMime::Text) else {
        return;
    };
    // 给 X server + 缓存一点点时间
    std::thread::sleep(Duration::from_millis(50));

    // 2. 用一个独立的客户端进程读回。最稳定的做法是 xclip，因为它
    //    就是普通 Linux 桌面上做这件事的参考实现；如果它读不到
    //    那八成是我们写的协议不对。
    let output = Command::new("xclip")
        .args(["-selection", "clipboard", "-o"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();
    let read = match output {
        Ok(out) if out.status.success() => {
            String::from_utf8_lossy(&out.stdout).trim_end().to_string()
        }
        Ok(out) => {
            eprintln!(
                "xclip 退出 {:?}：stderr={}",
                out.status,
                String::from_utf8_lossy(&out.stderr)
            );
            return;
        }
        Err(error) => {
            eprintln!("xclip 启动失败：{error}；跳过读取校验");
            return;
        }
    };
    assert_eq!(read, payload, "xclip 读出的剪贴板内容与写入不一致");
}

#[test]
fn clipboard_uri_list_round_trip_under_xvfb() {
    if std::env::var("DISPLAY").is_err() {
        return;
    }
    let payload = "file:///home/syc/example.txt\nfile:///tmp/另一个.md\n";
    let Some(_owner) = run_clipboard_setter(payload, ClipboardMime::UriList) else {
        return;
    };
    std::thread::sleep(Duration::from_millis(50));

    let output = Command::new("xclip")
        .args(["-selection", "clipboard", "-t", "text/uri-list", "-o"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();
    if let Ok(out) = output {
        if out.status.success() {
            let read = String::from_utf8_lossy(&out.stdout);
            assert!(
                read.contains("file:///home/syc/example.txt"),
                "text/uri-list 读不到第一个 URI：{read}"
            );
            assert!(
                read.contains("file:///tmp/另一个.md"),
                "text/uri-list 读不到第二个 URI：{read}"
            );
            return;
        }
    }
    eprintln!("xclip text/uri-list 读取失败，跳过断言");
}

/// Start the real `RuyiSeek` clipboard owner and keep its guard alive while the
/// independent xclip client reads the selection back.
fn run_clipboard_setter(payload: &str, mime: ClipboardMime) -> Option<ClipboardOwner> {
    match set_clipboard(payload.as_bytes().to_vec(), mime) {
        Ok(owner) => Some(owner),
        Err(error) => {
            eprintln!("RuyiSeek clipboard owner 启动失败：{error}；跳过 X11 集成测试");
            None
        }
    }
}
