//! `XInput2` adapter for the global double-Control gesture on X11.
//!
//! It observes raw keyboard events and never grabs or suppresses them.

use crate::hotkey::{
    ArrowKey, ControlKey, DoubleCtrlRecognizer, GestureContext, GestureDecision, Key, KeyEvent,
    KeyState,
};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Instant;
use x11rb::connection::Connection;
use x11rb::protocol::xinput::{
    ConnectionExt as XInputConnectionExt, Device, EventMask, KeyEventFlags, RawKeyPressEvent,
    XIEventMask,
};
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ClientMessageEvent, ConnectionExt as XProtoConnectionExt,
    EventMask as XEventMask, Window,
};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;

const CONTROL_L: u32 = 0xffe3;
const CONTROL_R: u32 = 0xffe4;
const UP_ARROW_KEYSYM: u32 = 0xff52;
const DOWN_ARROW_KEYSYM: u32 = 0xff54;
const LEFT_ARROW_KEYSYM: u32 = 0xff51;
const RIGHT_ARROW_KEYSYM: u32 = 0xff53;
const OTHER_MODIFIERS: &[u32] = &[
    0xffe1, 0xffe2, // Shift
    0xffe5, 0xffe6, // Caps Lock, Shift Lock
    0xffe7, 0xffe8, // Meta
    0xffe9, 0xffea, // Alt
    0xffeb, 0xffec, // Super
    0xffed, 0xffee, // Hyper
    0xfe03, // ISO Level 3 Shift
];

#[derive(Debug)]
pub struct HotkeyError(String);

impl fmt::Display for HotkeyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for HotkeyError {}

/// Return the physical pixel size of the default X11 root screen.
///
/// The UI uses this to size its transparent context-menu overlay. Keeping the
/// query in the X11 platform module avoids adding another native dependency to
/// the UI crate.
///
/// # Errors
///
/// Returns [`HotkeyError`] when the X11 connection fails or has no default
/// screen entry.
pub fn default_screen_size() -> Result<(u16, u16), HotkeyError> {
    let (connection, screen_number) =
        x11rb::connect(None).map_err(|error| HotkeyError(format!("连接 X11 失败：{error}")))?;
    let screen = connection
        .setup()
        .roots
        .get(screen_number)
        .ok_or_else(|| HotkeyError("X11 未返回默认屏幕".to_owned()))?;
    Ok((screen.width_in_pixels, screen.height_in_pixels))
}

/// Ask the EWMH-compatible X11 window manager to activate a top-level window.
///
/// This is used after the independent context-menu window closes. On UOS the
/// menu otherwise leaves keyboard focus on the window beneath the launcher.
///
/// # Errors
///
/// Returns [`HotkeyError`] if the X11 connection or EWMH property requests fail,
/// or when no client window has the requested UTF-8 title.
pub fn activate_window_named(title: &str) -> Result<(), HotkeyError> {
    let (connection, screen_number) =
        x11rb::connect(None).map_err(|error| HotkeyError(format!("连接 X11 失败：{error}")))?;
    let root = connection
        .setup()
        .roots
        .get(screen_number)
        .ok_or_else(|| HotkeyError("X11 未返回默认屏幕".to_owned()))?
        .root;
    let client_list = intern_atom(&connection, b"_NET_CLIENT_LIST")?;
    let window_name = intern_atom(&connection, b"_NET_WM_NAME")?;
    let utf8_string = intern_atom(&connection, b"UTF8_STRING")?;
    let active_window = intern_atom(&connection, b"_NET_ACTIVE_WINDOW")?;

    let clients = connection
        .get_property(false, root, client_list, AtomEnum::WINDOW, 0, u32::MAX)
        .map_err(display_error("查询 X11 客户端窗口失败"))?
        .reply()
        .map_err(display_error("读取 X11 客户端窗口失败"))?;
    let mut candidates: Vec<Window> = clients
        .value32()
        .into_iter()
        .flatten()
        .collect();
    // Deepin's window manager omits Slint's borderless launcher from
    // _NET_CLIENT_LIST, although it remains a direct child of the root window.
    candidates.extend(
        connection
            .query_tree(root)
            .map_err(display_error("查询 X11 顶层窗口失败"))?
            .reply()
            .map_err(display_error("读取 X11 顶层窗口失败"))?
            .children,
    );
    let target = find_window_named(
        &connection,
        candidates,
        window_name,
        utf8_string,
        title,
    )
        .ok_or_else(|| HotkeyError(format!("未找到 X11 窗口：{title}")))?;

    let event = ClientMessageEvent::new(
        32,
        target,
        active_window,
        [1, x11rb::CURRENT_TIME, 0, 0, 0],
    );
    connection
        .send_event(
            false,
            root,
            XEventMask::SUBSTRUCTURE_REDIRECT | XEventMask::SUBSTRUCTURE_NOTIFY,
            event,
        )
        .map_err(display_error("请求激活 X11 窗口失败"))?
        .check()
        .map_err(display_error("窗口管理器拒绝激活请求"))?;
    connection
        .flush()
        .map_err(display_error("刷新 X11 激活请求失败"))
}

fn find_window_named(
    connection: &RustConnection,
    mut pending: Vec<Window>,
    window_name: Atom,
    utf8_string: Atom,
    title: &str,
) -> Option<Window> {
    // DDE may reparent a borderless client underneath a compositor frame and
    // may change that relationship while another top-level window is closing.
    // Walk a bounded tree instead of assuming the client is a root child.
    let mut inspected = 0_usize;
    while let Some(window) = pending.pop() {
        inspected += 1;
        if inspected > 512 {
            break;
        }
        if connection
            .get_property(false, window, window_name, utf8_string, 0, u32::MAX)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .is_some_and(|reply| reply.value == title.as_bytes())
        {
            return Some(window);
        }
        if let Some(children) = connection
            .query_tree(window)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
        {
            pending.extend(children.children);
        }
    }
    None
}

/// Preferences shared between the settings UI and the X11 listener.
#[derive(Clone, Debug)]
pub struct DoubleCtrlControl {
    enabled: Arc<AtomicBool>,
    suppress_in_fullscreen: Arc<AtomicBool>,
}

impl DoubleCtrlControl {
    #[must_use]
    pub fn new(enabled: bool, suppress_in_fullscreen: bool) -> Self {
        Self {
            enabled: Arc::new(AtomicBool::new(enabled)),
            suppress_in_fullscreen: Arc::new(AtomicBool::new(suppress_in_fullscreen)),
        }
    }

    pub fn update(&self, enabled: bool, suppress_in_fullscreen: bool) {
        self.suppress_in_fullscreen
            .store(suppress_in_fullscreen, Ordering::Release);
        self.enabled.store(enabled, Ordering::Release);
    }

    fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    fn suppress_in_fullscreen(&self) -> bool {
        self.suppress_in_fullscreen.load(Ordering::Acquire)
    }
}

/// Start a worker that observes `XInput2` raw keyboard events.
///
/// The `on_trigger` callback fires when the double-Control gesture is
/// recognized; `on_arrow` fires on every press of an arrow key (Up, Down,
/// Left, Right). The arrow callback is *unconditional* — the consumer is
/// responsible for filtering out events when the launcher is not focused,
/// because raw events are delivered to every subscriber regardless of which
/// X11 window currently has focus.
///
/// # Errors
///
/// Returns [`HotkeyError`] if X11/XInput2 setup fails or the worker cannot be spawned.
pub fn spawn_double_ctrl_listener<TriggerCallback, ArrowCallback>(
    session_locked: Arc<AtomicBool>,
    control: DoubleCtrlControl,
    on_trigger: TriggerCallback,
    on_arrow: ArrowCallback,
) -> Result<JoinHandle<()>, HotkeyError>
where
    TriggerCallback: Fn() + Send + 'static,
    ArrowCallback: Fn(ArrowKey) + Send + 'static,
{
    let (connection, screen_number) =
        x11rb::connect(None).map_err(|error| HotkeyError(format!("连接 X11 失败：{error}")))?;
    let screen = connection
        .setup()
        .roots
        .get(screen_number)
        .ok_or_else(|| HotkeyError("X11 未返回默认屏幕".to_owned()))?;
    let root = screen.root;

    let version = connection
        .xinput_xi_query_version(2, 0)
        .map_err(display_error("查询 XInput2 版本失败"))?
        .reply()
        .map_err(display_error("读取 XInput2 版本失败"))?;
    if version.major_version < 2 {
        return Err(HotkeyError(format!(
            "XInput2 版本过低：{}.{}",
            version.major_version, version.minor_version
        )));
    }

    let masks = [EventMask {
        deviceid: u16::from(Device::ALL_MASTER),
        mask: vec![XIEventMask::RAW_KEY_PRESS | XIEventMask::RAW_KEY_RELEASE],
    }];
    connection
        .xinput_xi_select_events(root, &masks)
        .map_err(display_error("订阅 XInput2 原始按键失败"))?
        .check()
        .map_err(display_error("XInput2 拒绝原始按键订阅"))?;
    connection
        .flush()
        .map_err(display_error("刷新 X11 请求失败"))?;

    let keymap = KeyMap::load(&connection)?;
    let atoms = FullscreenAtoms::load(&connection)?;
    thread::Builder::new()
        .name("ruyiseek-x11-hotkey".to_owned())
        .spawn(move || {
            run_event_loop(
                &connection,
                root,
                keymap,
                &atoms,
                &session_locked,
                &control,
                &on_trigger,
                &on_arrow,
            );
        })
        .map_err(|error| HotkeyError(format!("启动 X11 热键线程失败：{error}")))
}

#[allow(clippy::too_many_arguments)]
fn run_event_loop<TriggerCallback, ArrowCallback>(
    connection: &RustConnection,
    root: Window,
    mut keymap: KeyMap,
    atoms: &FullscreenAtoms,
    session_locked: &AtomicBool,
    control: &DoubleCtrlControl,
    on_trigger: &TriggerCallback,
    on_arrow: &ArrowCallback,
) where
    TriggerCallback: Fn(),
    ArrowCallback: Fn(ArrowKey),
{
    let started = Instant::now();
    let mut recognizer = DoubleCtrlRecognizer::default();
    let mut fullscreen_blocked = false;

    loop {
        let event = match connection.wait_for_event() {
            Ok(event) => event,
            Err(error) => {
                eprintln!("ruyiseek-ui: X11 热键监听已停止：{error}");
                return;
            }
        };

        let native_event = match event {
            Event::XinputRawKeyPress(event) => {
                fullscreen_blocked = control.suppress_in_fullscreen()
                    && atoms.is_fullscreen(connection, root).unwrap_or(false);
                if let Some(arrow) = keymap.classify_arrow(event.detail) {
                    on_arrow(arrow);
                }
                Some(to_key_event(&keymap, &event, KeyState::Pressed, started))
            }
            Event::XinputRawKeyRelease(event) => {
                Some(to_key_event(&keymap, &event, KeyState::Released, started))
            }
            Event::MappingNotify(_) => {
                if let Ok(updated) = KeyMap::load(connection) {
                    keymap = updated;
                    recognizer.reset();
                }
                None
            }
            _ => None,
        };

        if native_event.is_some_and(|event| {
            recognizer.handle(
                event,
                GestureContext {
                    session_locked: !control.enabled() || session_locked.load(Ordering::Acquire),
                    fullscreen_blocked,
                },
            ) == GestureDecision::Triggered
        }) {
            on_trigger();
        }
    }
}

fn to_key_event(
    keymap: &KeyMap,
    event: &RawKeyPressEvent,
    state: KeyState,
    started: Instant,
) -> KeyEvent {
    KeyEvent {
        key: keymap.classify(event.detail),
        state,
        at: started.elapsed(),
        repeat: u32::from(event.flags) & u32::from(KeyEventFlags::KEY_REPEAT) != 0,
    }
}

struct KeyMap {
    first_keycode: u8,
    entries: Vec<Key>,
    /// Raw keysyms (Vec per keycode) for arrow detection. The X server returns
    /// multiple keysyms per keycode because of shift levels; we keep them all
    /// so that the arrow detector can match regardless of which group the user
    /// is in.
    keysyms: Vec<Vec<u32>>,
}

impl KeyMap {
    fn load(connection: &RustConnection) -> Result<Self, HotkeyError> {
        let setup = connection.setup();
        let first_keycode = setup.min_keycode;
        let count = setup
            .max_keycode
            .checked_sub(first_keycode)
            .and_then(|value| value.checked_add(1))
            .ok_or_else(|| HotkeyError("X11 键码范围无效".to_owned()))?;
        let reply = connection
            .get_keyboard_mapping(first_keycode, count)
            .map_err(display_error("读取 X11 键盘映射失败"))?
            .reply()
            .map_err(display_error("X11 键盘映射响应失败"))?;
        let per_keycode = usize::from(reply.keysyms_per_keycode);
        if per_keycode == 0 {
            return Err(HotkeyError("X11 键盘映射为空".to_owned()));
        }
        let chunks: Vec<&[u32]> = reply.keysyms.chunks(per_keycode).collect();
        let entries = chunks.iter().map(|ks| classify_keysyms(ks)).collect();
        let keysyms = chunks.into_iter().map(<[u32]>::to_vec).collect();
        Ok(Self {
            first_keycode,
            entries,
            keysyms,
        })
    }

    fn classify(&self, detail: u32) -> Key {
        let Ok(keycode) = u8::try_from(detail) else {
            return Key::NonModifier;
        };
        let Some(index) = keycode.checked_sub(self.first_keycode) else {
            return Key::NonModifier;
        };
        self.entries
            .get(usize::from(index))
            .copied()
            .unwrap_or(Key::NonModifier)
    }

    fn classify_arrow(&self, detail: u32) -> Option<ArrowKey> {
        let keycode = u8::try_from(detail).ok()?;
        let index = usize::from(keycode.checked_sub(self.first_keycode)?);
        let syms = self.keysyms.get(index)?;
        for sym in syms {
            match *sym {
                UP_ARROW_KEYSYM => return Some(ArrowKey::Up),
                DOWN_ARROW_KEYSYM => return Some(ArrowKey::Down),
                LEFT_ARROW_KEYSYM => return Some(ArrowKey::Left),
                RIGHT_ARROW_KEYSYM => return Some(ArrowKey::Right),
                _ => {}
            }
        }
        None
    }
}

fn classify_keysyms(keysyms: &[u32]) -> Key {
    if keysyms.contains(&CONTROL_L) {
        Key::Control(ControlKey::Left)
    } else if keysyms.contains(&CONTROL_R) {
        Key::Control(ControlKey::Right)
    } else if keysyms
        .iter()
        .any(|keysym| OTHER_MODIFIERS.contains(keysym))
    {
        Key::OtherModifier
    } else {
        Key::NonModifier
    }
}

struct FullscreenAtoms {
    active_window: Atom,
    window_state: Atom,
    fullscreen: Atom,
}

impl FullscreenAtoms {
    fn load(connection: &RustConnection) -> Result<Self, HotkeyError> {
        Ok(Self {
            active_window: intern_atom(connection, b"_NET_ACTIVE_WINDOW")?,
            window_state: intern_atom(connection, b"_NET_WM_STATE")?,
            fullscreen: intern_atom(connection, b"_NET_WM_STATE_FULLSCREEN")?,
        })
    }

    fn is_fullscreen(
        &self,
        connection: &RustConnection,
        root: Window,
    ) -> Result<bool, HotkeyError> {
        let active = connection
            .get_property(false, root, self.active_window, AtomEnum::WINDOW, 0, 1)
            .map_err(display_error("查询活动窗口失败"))?
            .reply()
            .map_err(display_error("读取活动窗口失败"))?
            .value32()
            .and_then(|mut values| values.next())
            .unwrap_or_default();
        if active == 0 {
            return Ok(false);
        }
        let state = connection
            .get_property(
                false,
                active,
                self.window_state,
                AtomEnum::ATOM,
                0,
                u32::MAX,
            )
            .map_err(display_error("查询窗口状态失败"))?
            .reply()
            .map_err(display_error("读取窗口状态失败"))?;
        Ok(state
            .value32()
            .is_some_and(|mut values| values.any(|atom| atom == self.fullscreen)))
    }
}

fn intern_atom(connection: &RustConnection, name: &[u8]) -> Result<Atom, HotkeyError> {
    connection
        .intern_atom(false, name)
        .map_err(display_error("创建 X11 属性查询失败"))?
        .reply()
        .map(|reply| reply.atom)
        .map_err(display_error("读取 X11 属性标识失败"))
}

fn display_error<Error: fmt::Display>(context: &'static str) -> impl FnOnce(Error) -> HotkeyError {
    move |error| HotkeyError(format!("{context}：{error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keysyms_distinguish_control_modifier_and_regular_keys() {
        assert_eq!(
            classify_keysyms(&[0, CONTROL_L]),
            Key::Control(ControlKey::Left)
        );
        assert_eq!(
            classify_keysyms(&[CONTROL_R]),
            Key::Control(ControlKey::Right)
        );
        assert_eq!(classify_keysyms(&[0xffe9]), Key::OtherModifier);
        assert_eq!(classify_keysyms(&[0x61]), Key::NonModifier);
    }

    #[test]
    fn hotkey_control_updates_preferences_without_restarting_listener() {
        let control = DoubleCtrlControl::new(true, true);
        assert!(control.enabled());
        assert!(control.suppress_in_fullscreen());

        control.update(false, false);
        assert!(!control.enabled());
        assert!(!control.suppress_in_fullscreen());
    }
}
