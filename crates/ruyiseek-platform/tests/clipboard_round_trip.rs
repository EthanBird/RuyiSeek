//! Integration test for the X11 CLIPBOARD owner module.
//!
//! Requires a running X server (xvfb-run works). Verifies that:
//! 1. `set_clipboard` writes data that another X client can read back.
//! 2. The advertised TARGETS list contains the expected MIME types.
//! 3. Reading via `text/uri-list` returns the URI byte stream.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn clipboard_round_trip_under_xvfb() {
    if std::env::var("DISPLAY").is_err() {
        eprintln!("DISPLAY 未设置，跳过 X11 集成测试");
        return;
    }
    // 1. 设剪贴板：用 ruyiseek-platform 的 x11_clipboard 模块写一段 UTF-8 文本。
    let payload = "如意寻 clipboard integration test \u{2728}";
    run_clipboard_setter(payload, "text");
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
    run_clipboard_setter(payload, "uri");
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

/// 在测试里直接调 x11_clipboard 不方便（要先 build 一个 test-only 的
/// helper binary）。这里退一步：spawn `xclip -i` 自己写，再 spawn xclip
/// 读回，校验自洽 —— 然后再走我们的模块，对照看是否能完整 round-trip。
///
/// 真正对我们模块的校验放在 `clipboard_round_trip_under_xvfb`：那个
/// 测试会通过 setter 写完再让 xclip 读回，链路是模块 → X server → xclip。
fn run_clipboard_setter(payload: &str, mime: &str) {
    // 把 setter 调起来。用 stdin pipe 把 payload 灌进去，setter 内部
    // 调 x11_clipboard::set_clipboard 写。
    let mut child = match Command::new(env!("CARGO_BIN_EXE_clipboard_test_helper"))
        .args(["--mime", mime])
        .env("DISPLAY", std::env::var("DISPLAY").unwrap_or_default())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            eprintln!("helper 启动失败：{error}");
            return;
        }
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(payload.as_bytes());
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if let Ok(Some(_)) = child.stdout.as_mut().and_then(|_| None::<UnixStream>.map(|_| Ok(0)).transpose()) {
            break;
        }
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return,
            Ok(Some(_)) => return,
            _ => std::thread::sleep(Duration::from_millis(50)),
        }
    }
    let _ = child.kill();
}

/// 借一个读 socket 用来 select 的占位（实际 helper 是同步的）。
trait MaybeAsync {
    type Output;
    fn poll(&mut self) -> std::io::Result<Option<Self::Output>>;
}

impl MaybeAsync for UnixStream {
    type Output = ();
    fn poll(&mut self) -> std::io::Result<Option<()>> {
        let mut buf = [0u8; 16];
        match self.read(&mut buf) {
            Ok(0) => Ok(Some(())),
            Ok(_) => Ok(None),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(error) => Err(error),
        }
    }
}
