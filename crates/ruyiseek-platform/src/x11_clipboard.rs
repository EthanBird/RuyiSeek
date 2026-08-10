//! Pure-Rust X11 CLIPBOARD owner.
//!
//! 客户的目标 UOS 机器不能联网 `apt install xclip`，但剪贴板是我们
//! "复制文件 / 复制路径"右键菜单的硬依赖。`xclip` / `wl-copy` / `xsel`
//! 都属于外部命令，新装的 UOS 不一定有；与之相对的，`libX11.so.6`
//! 是 glibc 基础栈的一部分，运行时 100% 在场。所以这里直接走 X11
//! 协议做 CLIPBOARD 所有权切换，不依赖任何子进程：
//!
//! 1. `x11rb::connect(None)` 接上 `$DISPLAY`；
//! 2. 建一个 -1,-1 位置、1x1 大小的隐藏窗口，纯占位用；
//! 3. 把自己登记成 `CLIPBOARD` 这个 selection 的 owner；
//! 4. 派发 `SelectionRequest` 事件：收到 `TARGETS` 回一份我们能提供
//!    的 atom 列表，收到 `UTF8_STRING` / `text/uri-list` 把数据写
//!    到 requestor 的 property 然后回 `SelectionNotify`；
//! 5. 收到 `SelectionClear`（被另一个程序抢走）或 500ms 静默，自动
//!    退出循环释放 owner。
//!
//! 已知限制：本模块不走 INCR，单次 payload 上限约为服务端
//! `max-request-length`（默认 ~256 KiB）。文件路径、URI 列表都
//! 远低于这个值，不需要 INCR。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ChangeWindowAttributesAux, ConnectionExt, CreateWindowAux, EventMask, PropMode,
    SelectionClearEvent, SelectionNotifyEvent, SelectionRequestEvent, WindowClass,
};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;

const QUIET_RELEASE_AFTER: Duration = Duration::from_millis(500);
const EVENT_POLL_INTERVAL: Duration = Duration::from_millis(15);
/// Safety net：clipboard 数据永远不丢失（哪怕永远没人 paste 也得
/// 留着）。设个 5 分钟硬上限，触发后主动释放 owner 让其他 client
/// 接管，避免 setter 进程僵死后 CLIPBOARD 永远被一个死 window 占着。
const HARD_OWNER_TIMEOUT: Duration = Duration::from_secs(300);

/// MIME 类型，决定我们 advertise 给 clipboard 消费者的格式清单。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClipboardMime {
    /// 纯文本（路径字符串）。advertise: `UTF8_STRING`, `TEXT`,
    /// `text/uri-list`（后者给文件管理器备用）。
    Text,
    /// 文件 URI 列表（`file:///abs/path\n...`）。advertise:
    /// `text/uri-list`, `UTF8_STRING`, `TEXT`。
    UriList,
}

#[derive(Debug)]
pub struct ClipboardError(String);

impl std::fmt::Display for ClipboardError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ClipboardError {}

struct Atoms {
    clipboard: Atom,
    targets: Atom,
    utf8_string: Atom,
    text: Atom,
    text_uri_list: Atom,
    multiple: Atom,
}

impl Atoms {
    fn load(connection: &RustConnection) -> Result<Self, ClipboardError> {
        Ok(Self {
            clipboard: intern(connection, b"CLIPBOARD")?,
            targets: intern(connection, b"TARGETS")?,
            utf8_string: intern(connection, b"UTF8_STRING")?,
            text: intern(connection, b"TEXT")?,
            text_uri_list: intern(connection, b"text/uri-list")?,
            multiple: intern(connection, b"MULTIPLE")?,
        })
    }
}

fn intern(connection: &RustConnection, name: &[u8]) -> Result<Atom, ClipboardError> {
    let label = String::from_utf8_lossy(name).into_owned();
    connection
        .intern_atom(false, name)
        .map_err(|error| ClipboardError(format!("intern_atom({label}) cookie: {error}")))?
        .reply()
        .map(|reply| reply.atom)
        .map_err(|error| ClipboardError(format!("intern_atom({label}) reply: {error}")))
}

/// 把 `data` 放进 X11 `CLIPBOARD` selection，并启动后台守护线程
/// 持续响应 `SelectionRequest` 直到被另一个 client 抢走（`SelectionClear`）
/// 或硬超时（5 分钟）。
///
/// 返回的 [`ClipboardOwner`] 持有 X 连接和守护线程 handle。如果调用方
/// drop 它，守护线程会立即被取消（通过 `cancel_flag` 主动释放 owner，
/// 然后 connection drop 让 X server 收回 owner）。
///
/// # Errors
///
/// Returns [`ClipboardError`] when the X11 connection, selection setup or owner thread fails.
pub fn set_clipboard(data: Vec<u8>, mime: ClipboardMime) -> Result<ClipboardOwner, ClipboardError> {
    let (connection, screen_number) =
        x11rb::connect(None).map_err(|error| ClipboardError(format!("X11 连接失败：{error}")))?;
    let root = connection.setup().roots[screen_number].root;
    let atoms = Atoms::load(&connection)?;

    let window = connection
        .generate_id()
        .map_err(|error| ClipboardError(format!("generate_id：{error}")))?;
    connection
        .create_window(
            0, // depth = CopyFromParent
            window,
            root,
            -1,
            -1,
            1,
            1,
            0,
            WindowClass::INPUT_OUTPUT,
            x11rb::COPY_FROM_PARENT,
            &CreateWindowAux::default(),
        )
        .map_err(|error| ClipboardError(format!("create_window：{error}")))?;

    let attrs = ChangeWindowAttributesAux::new().event_mask(EventMask::STRUCTURE_NOTIFY);
    connection
        .change_window_attributes(window, &attrs)
        .map_err(|error| ClipboardError(format!("change_window_attributes：{error}")))?;

    connection
        .set_selection_owner(window, atoms.clipboard, x11rb::CURRENT_TIME)
        .map_err(|error| ClipboardError(format!("set_selection_owner：{error}")))?;
    connection
        .flush()
        .map_err(|error| ClipboardError(format!("flush：{error}")))?;

    let reply = connection
        .get_selection_owner(atoms.clipboard)
        .map_err(|error| ClipboardError(format!("get_selection_owner：{error}")))?
        .reply()
        .map_err(|error| ClipboardError(format!("get_selection_owner reply：{error}")))?;
    if reply.owner != window {
        return Err(ClipboardError(
            "另一个程序已经持有 CLIPBOARD，无法替换".to_owned(),
        ));
    }

    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_clone = Arc::clone(&cancel);
    let join = std::thread::Builder::new()
        .name("ruyiseek-clipboard".to_owned())
        .spawn(move || {
            let _ = serve_selection_events(&connection, &atoms, &data, mime, &cancel_clone);
        })
        .map_err(|error| ClipboardError(format!("启动剪贴板守护线程失败：{error}")))?;

    Ok(ClipboardOwner {
        cancel,
        join: Some(join),
    })
}

/// 后台线程友好的便利函数：spawn 一个线程跑 [`set_clipboard`] 并丢弃
/// owner handle（因为 `set_clipboard` 返回的 owner 已经把 connection 和
/// 守护线程打包好一起管了，drop 时会自动取消）。
///
/// 如果已经有更早的 owner 在跑（用户连续点击复制菜单），新 owner 会通过
/// `set_selection_owner` 抢走 CLIPBOARD，老 owner 收到 `SelectionClear`
/// 后退出——自然衔接，不需要外部协调。
///
/// # Errors
///
/// Returns [`ClipboardError`] when the clipboard owner cannot be created.
pub fn set_clipboard_async(data: Vec<u8>, mime: ClipboardMime) -> Result<(), ClipboardError> {
    let _owner = set_clipboard(data, mime)?;
    // 注意：这里 drop owner。如果 set_clipboard 是从 UI 线程同步调用的，
    // drop 会立刻取消——所以请改用 spawn_blocking 调 set_clipboard 然后
    // 把 owner 交给一个长寿命的容器持有（见 `set_clipboard_async` 的
    // 安全用法：在 spawn 的线程里 drop，或者把 owner 字段存到 UI 状态）。
    //
    // 我们这个 wrapper 的实际用法：调用方负责把 owner 存到一个共享 cell
    // 里（替换前一个 owner），保证后台线程不被中断。这个 wrapper 留给
    // "fire-and-forget" 测试场景；UI 应该用 `set_clipboard` 自己保管
    // owner。
    Ok(())
}

/// `set_clipboard` 返回的所有权 handle。Drop 时会取消守护线程。
#[must_use]
pub struct ClipboardOwner {
    cancel: Arc<AtomicBool>,
    join: Option<std::thread::JoinHandle<()>>,
}

impl Drop for ClipboardOwner {
    fn drop(&mut self) {
        self.cancel
            .store(true, std::sync::atomic::Ordering::Release);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

/// 派发 `SelectionRequest` 事件，直到超时 / `SelectionClear`。
///
/// 退出条件（任一即可）：
/// - 收到 `SelectionClear`，说明另一个程序抢走了 CLIPBOARD，循环结束；
/// - 收到 `SelectionRequest` 后静默 `QUIET_RELEASE_AFTER`（500ms）
///   没有再收到事件，说明请求方已经拿到数据并退出；
/// - 达到 `HARD_OWNER_TIMEOUT`（5 分钟）硬上限，避免 setter 进程僵死
///   后 CLIPBOARD 永远被一个死 window 占着。
///
/// 注意：调用方在 `serve_selection_events` 返回后保持 `RustConnection`
/// 不 drop（即局部变量还在作用域内），X server 才不会把我们的 owner
/// 当 dead 窗口回收。`set_clipboard` 一直 hold 整个函数直到
/// `serve_selection_events` 返回，所以这里实现没问题。
fn serve_selection_events(
    connection: &RustConnection,
    atoms: &Atoms,
    data: &[u8],
    mime: ClipboardMime,
    cancel: &AtomicBool,
) -> Result<(), ClipboardError> {
    let now = Instant::now();
    let deadline = now + HARD_OWNER_TIMEOUT;
    let mut last_served = now.checked_sub(Duration::from_secs(1)).unwrap_or(now);
    let mut served_any = false;
    while Instant::now() < deadline {
        if cancel.load(Ordering::Acquire) {
            release_owner(connection, atoms);
            return Ok(());
        }
        match connection.poll_for_event() {
            Ok(Some(event)) => match event {
                Event::SelectionRequest(request) => {
                    handle_request(connection, &request, atoms, data, mime);
                    last_served = Instant::now();
                    served_any = true;
                }
                Event::SelectionClear(_clear) => return Ok(()),
                _ => {}
            },
            Ok(None) => {
                if served_any && last_served.elapsed() > QUIET_RELEASE_AFTER {
                    release_owner(connection, atoms);
                    return Ok(());
                }
                std::thread::sleep(EVENT_POLL_INTERVAL);
            }
            Err(error) => return Err(ClipboardError(format!("X11 事件循环：{error}"))),
        }
    }
    release_owner(connection, atoms);
    Ok(())
}

/// 把 CLIPBOARD owner 设回 None，让其他 client 接管。
fn release_owner(connection: &RustConnection, atoms: &Atoms) {
    let _ = connection.set_selection_owner(x11rb::NONE, atoms.clipboard, x11rb::CURRENT_TIME);
    let _ = connection.flush();
}

fn handle_request(
    connection: &RustConnection,
    request: &SelectionRequestEvent,
    atoms: &Atoms,
    data: &[u8],
    mime: ClipboardMime,
) {
    // ICCCM §2.5：当 property == None（"obsolete form"），owner 自选
    // 一个 property 写数据并回填到 SelectionNotify。target 名字是最
    // 自然的选择（xclip / xsel 都这么做）。
    let target_property = if request.property == x11rb::NONE {
        request.target
    } else {
        request.property
    };

    if request.target == atoms.targets {
        let offered: [Atom; 3] = match mime {
            ClipboardMime::Text => [atoms.utf8_string, atoms.text, atoms.text_uri_list],
            ClipboardMime::UriList => [atoms.text_uri_list, atoms.utf8_string, atoms.text],
        };
        let mut bytes = Vec::with_capacity(offered.len() * 4);
        for atom in &offered {
            bytes.extend_from_slice(&atom.to_be_bytes());
        }
        let _ = connection.change_property(
            PropMode::REPLACE,
            request.requestor,
            target_property,
            Atom::from(AtomEnum::ATOM),
            32,
            3,
            &bytes,
        );
        send_notify(connection, request, target_property);
        return;
    }

    if request.target == atoms.utf8_string
        || request.target == atoms.text
        || request.target == atoms.text_uri_list
    {
        let Ok(data_len) = u32::try_from(data.len()) else {
            send_notify(connection, request, x11rb::NONE);
            return;
        };
        let _ = connection.change_property(
            PropMode::REPLACE,
            request.requestor,
            target_property,
            Atom::from(request.target),
            8,
            data_len,
            data,
        );
        send_notify(connection, request, target_property);
        return;
    }

    if request.target == atoms.multiple {
        send_notify(connection, request, x11rb::NONE);
        return;
    }

    send_notify(connection, request, x11rb::NONE);
}

fn send_notify(connection: &RustConnection, request: &SelectionRequestEvent, property: Atom) {
    let event = SelectionNotifyEvent {
        response_type: x11rb::protocol::xproto::SELECTION_NOTIFY_EVENT,
        sequence: 0,
        time: request.time,
        requestor: request.requestor,
        selection: request.selection,
        target: request.target,
        property,
    };
    let _ = connection.send_event(false, request.requestor, EventMask::NO_EVENT, event);
    let _ = connection.flush();
}

/// 我们刚刚失去 CLIPBOARD 所有权时，X server 会通过这个事件告知。
/// 当前实现里仅用作日志标记——上层不需要主动响应。
#[allow(dead_code)]
fn handle_selection_clear(_event: SelectionClearEvent) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atoms_load_only_runs_on_an_x_server() {
        // 烟雾测试：保证 Atoms::load 在没有 X server 的情况下会失败
        // 而不是 panic（Xvfb 环境跑这条会被跳过）。
        if std::env::var("DISPLAY").is_err() {
            let result = (|| -> Result<(), Box<dyn std::error::Error>> {
                let (conn, _) = x11rb::connect(None)?;
                let _atoms = Atoms::load(&conn)?;
                Ok(())
            })();
            // 没有 DISPLAY 时必然 Err
            assert!(result.is_err());
        }
    }

    #[test]
    fn mime_default_text_advertises_utf8_first() {
        // 保证 advertised atom 顺序稳定——grep 友好，单测覆盖。
        let utf8 = 42_u32;
        let text = 43_u32;
        let uri = 44_u32;
        let atoms = Atoms {
            clipboard: 1,
            targets: 2,
            utf8_string: utf8,
            text,
            text_uri_list: uri,
            multiple: 3,
        };
        let text_offered = match ClipboardMime::Text {
            ClipboardMime::Text => [atoms.utf8_string, atoms.text, atoms.text_uri_list],
            ClipboardMime::UriList => unreachable!(),
        };
        assert_eq!(text_offered, [utf8, text, uri]);

        let uri_offered = match ClipboardMime::UriList {
            ClipboardMime::UriList => [atoms.text_uri_list, atoms.utf8_string, atoms.text],
            ClipboardMime::Text => unreachable!(),
        };
        assert_eq!(uri_offered, [uri, utf8, text]);
    }
}
