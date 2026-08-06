//! `XInput2` adapter for the global double-Control gesture on X11.
//!
//! It observes raw keyboard events and never grabs or suppresses them.

use crate::hotkey::{
    ControlKey, DoubleCtrlRecognizer, GestureContext, GestureDecision, Key, KeyEvent, KeyState,
};
use std::fmt;
use std::thread::{self, JoinHandle};
use std::time::Instant;
use x11rb::connection::Connection;
use x11rb::protocol::xinput::{
    ConnectionExt as XInputConnectionExt, Device, EventMask, KeyEventFlags, RawKeyPressEvent,
    XIEventMask,
};
use x11rb::protocol::xproto::{Atom, AtomEnum, ConnectionExt as XProtoConnectionExt, Window};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;

const CONTROL_L: u32 = 0xffe3;
const CONTROL_R: u32 = 0xffe4;
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

/// Start a worker that observes `XInput2` raw keyboard events.
///
/// # Errors
///
/// Returns [`HotkeyError`] if X11/XInput2 setup fails or the worker cannot be spawned.
pub fn spawn_double_ctrl_listener<Callback>(
    on_trigger: Callback,
) -> Result<JoinHandle<()>, HotkeyError>
where
    Callback: Fn() + Send + 'static,
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
        .spawn(move || run_event_loop(&connection, root, keymap, &atoms, &on_trigger))
        .map_err(|error| HotkeyError(format!("启动 X11 热键线程失败：{error}")))
}

fn run_event_loop<Callback>(
    connection: &RustConnection,
    root: Window,
    mut keymap: KeyMap,
    atoms: &FullscreenAtoms,
    on_trigger: &Callback,
) where
    Callback: Fn(),
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
                fullscreen_blocked = atoms.is_fullscreen(connection, root).unwrap_or(false);
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
                    session_locked: false,
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
        let entries = reply
            .keysyms
            .chunks(per_keycode)
            .map(classify_keysyms)
            .collect();
        Ok(Self {
            first_keycode,
            entries,
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
}
