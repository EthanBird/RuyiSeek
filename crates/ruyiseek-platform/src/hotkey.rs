//! Double-Control gesture recognizer.
//!
//! `XInput2`, Portal and DDE adapters should translate native events into this state machine.
//! The recognizer never grabs or suppresses the original keyboard events.

use std::time::Duration;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlKey {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Key {
    Control(ControlKey),
    OtherModifier,
    NonModifier,
}

/// Directional arrow keys reported by the XInput2 listener.
///
/// Used by the launcher UI to advance the selected result. They are reported
/// as a *separate* signal from [`Key`] because Slint 1.6's focused `LineEdit`
/// swallows arrow events before any user-defined `key-pressed` callback can
/// see them, so the UI needs the raw X11 stream to move the selection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArrowKey {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyState {
    Pressed,
    Released,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KeyEvent {
    pub key: Key,
    pub state: KeyState,
    pub at: Duration,
    pub repeat: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GestureContext {
    pub session_locked: bool,
    pub fullscreen_blocked: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DoubleCtrlConfig {
    pub max_hold: Duration,
    pub max_gap: Duration,
    pub sides_equivalent: bool,
}

impl Default for DoubleCtrlConfig {
    fn default() -> Self {
        Self {
            max_hold: Duration::from_millis(220),
            max_gap: Duration::from_millis(320),
            sides_equivalent: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GestureDecision {
    None,
    Triggered,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TapNumber {
    First,
    Second,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum State {
    Idle,
    Pressed {
        key: ControlKey,
        pressed_at: Duration,
        tap: TapNumber,
        chorded: bool,
    },
    Armed {
        key: ControlKey,
        released_at: Duration,
    },
}

#[derive(Debug)]
pub struct DoubleCtrlRecognizer {
    config: DoubleCtrlConfig,
    state: State,
    last_event_at: Option<Duration>,
}

impl DoubleCtrlRecognizer {
    #[must_use]
    pub const fn new(config: DoubleCtrlConfig) -> Self {
        Self {
            config,
            state: State::Idle,
            last_event_at: None,
        }
    }

    pub fn reset(&mut self) {
        self.state = State::Idle;
        self.last_event_at = None;
    }

    pub fn expire(&mut self, now: Duration) {
        if let State::Armed { released_at, .. } = self.state {
            if now >= released_at && now - released_at > self.config.max_gap {
                self.state = State::Idle;
            }
        }
    }

    pub fn handle(&mut self, event: KeyEvent, context: GestureContext) -> GestureDecision {
        if context.session_locked || context.fullscreen_blocked {
            self.reset();
            return GestureDecision::None;
        }
        if self.last_event_at.is_some_and(|last| event.at < last) {
            self.reset();
            return GestureDecision::None;
        }
        self.last_event_at = Some(event.at);
        self.expire(event.at);

        if event.repeat {
            self.state = State::Idle;
            return GestureDecision::None;
        }

        match (event.key, event.state) {
            (Key::NonModifier, KeyState::Pressed) => {
                self.state = State::Idle;
            }
            (Key::OtherModifier, _) | (Key::NonModifier, KeyState::Released) => {}
            (Key::Control(key), KeyState::Pressed) => self.on_control_pressed(key, event.at),
            (Key::Control(key), KeyState::Released) => {
                return self.on_control_released(key, event.at);
            }
        }
        GestureDecision::None
    }

    fn on_control_pressed(&mut self, key: ControlKey, at: Duration) {
        self.state = match self.state {
            State::Armed {
                key: first_key,
                released_at,
            } if at >= released_at
                && at - released_at <= self.config.max_gap
                && self.keys_match(first_key, key) =>
            {
                State::Pressed {
                    key,
                    pressed_at: at,
                    tap: TapNumber::Second,
                    chorded: false,
                }
            }
            State::Pressed {
                key: pressed_key,
                pressed_at,
                tap,
                ..
            } => State::Pressed {
                key: pressed_key,
                pressed_at,
                tap,
                chorded: true,
            },
            _ => State::Pressed {
                key,
                pressed_at: at,
                tap: TapNumber::First,
                chorded: false,
            },
        };
    }

    fn on_control_released(&mut self, key: ControlKey, at: Duration) -> GestureDecision {
        let State::Pressed {
            key: pressed_key,
            pressed_at,
            tap,
            chorded,
        } = self.state
        else {
            self.state = State::Idle;
            return GestureDecision::None;
        };

        if !self.keys_match(pressed_key, key)
            || at < pressed_at
            || at - pressed_at > self.config.max_hold
            || chorded
        {
            self.state = State::Idle;
            return GestureDecision::None;
        }

        match tap {
            TapNumber::First => {
                self.state = State::Armed {
                    key,
                    released_at: at,
                };
                GestureDecision::None
            }
            TapNumber::Second => {
                self.state = State::Idle;
                GestureDecision::Triggered
            }
        }
    }

    const fn keys_match(&self, left: ControlKey, right: ControlKey) -> bool {
        self.config.sides_equivalent
            || matches!(
                (left, right),
                (ControlKey::Left, ControlKey::Left) | (ControlKey::Right, ControlKey::Right)
            )
    }
}

impl Default for DoubleCtrlRecognizer {
    fn default() -> Self {
        Self::new(DoubleCtrlConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPEN: GestureContext = GestureContext {
        session_locked: false,
        fullscreen_blocked: false,
    };

    fn event(ms: u64, key: Key, state: KeyState) -> KeyEvent {
        KeyEvent {
            key,
            state,
            at: Duration::from_millis(ms),
            repeat: false,
        }
    }

    fn tap(
        recognizer: &mut DoubleCtrlRecognizer,
        key: ControlKey,
        down: u64,
        up: u64,
    ) -> GestureDecision {
        assert_eq!(
            recognizer.handle(event(down, Key::Control(key), KeyState::Pressed), OPEN),
            GestureDecision::None
        );
        recognizer.handle(event(up, Key::Control(key), KeyState::Released), OPEN)
    }

    #[test]
    fn two_complete_taps_trigger_on_second_release() {
        let mut recognizer = DoubleCtrlRecognizer::default();
        assert_eq!(
            tap(&mut recognizer, ControlKey::Left, 0, 40),
            GestureDecision::None
        );
        assert_eq!(
            tap(&mut recognizer, ControlKey::Left, 200, 240),
            GestureDecision::Triggered
        );
    }

    #[test]
    fn long_hold_does_not_arm() {
        let mut recognizer = DoubleCtrlRecognizer::default();
        assert_eq!(
            tap(&mut recognizer, ControlKey::Left, 0, 1_000),
            GestureDecision::None
        );
        assert_eq!(
            tap(&mut recognizer, ControlKey::Left, 1_100, 1_150),
            GestureDecision::None
        );
    }

    #[test]
    fn ctrl_shortcut_cancels_the_sequence() {
        let mut recognizer = DoubleCtrlRecognizer::default();
        recognizer.handle(
            event(0, Key::Control(ControlKey::Left), KeyState::Pressed),
            OPEN,
        );
        recognizer.handle(event(10, Key::NonModifier, KeyState::Pressed), OPEN);
        recognizer.handle(
            event(20, Key::Control(ControlKey::Left), KeyState::Released),
            OPEN,
        );
        assert_eq!(
            tap(&mut recognizer, ControlKey::Left, 100, 130),
            GestureDecision::None
        );
    }

    #[test]
    fn automatic_repeat_cancels_the_sequence() {
        let mut recognizer = DoubleCtrlRecognizer::default();
        recognizer.handle(
            event(0, Key::Control(ControlKey::Left), KeyState::Pressed),
            OPEN,
        );
        let mut repeated = event(20, Key::Control(ControlKey::Left), KeyState::Pressed);
        repeated.repeat = true;
        recognizer.handle(repeated, OPEN);
        assert_eq!(
            tap(&mut recognizer, ControlKey::Left, 50, 80),
            GestureDecision::None
        );
    }

    #[test]
    fn opposite_sides_follow_configuration() {
        let mut equivalent = DoubleCtrlRecognizer::default();
        tap(&mut equivalent, ControlKey::Left, 0, 30);
        assert_eq!(
            tap(&mut equivalent, ControlKey::Right, 100, 130),
            GestureDecision::Triggered
        );

        let mut distinct = DoubleCtrlRecognizer::new(DoubleCtrlConfig {
            sides_equivalent: false,
            ..DoubleCtrlConfig::default()
        });
        tap(&mut distinct, ControlKey::Left, 0, 30);
        assert_eq!(
            tap(&mut distinct, ControlKey::Right, 100, 130),
            GestureDecision::None
        );
    }

    #[test]
    fn locked_or_fullscreen_context_never_triggers() {
        for context in [
            GestureContext {
                session_locked: true,
                fullscreen_blocked: false,
            },
            GestureContext {
                session_locked: false,
                fullscreen_blocked: true,
            },
        ] {
            let mut recognizer = DoubleCtrlRecognizer::default();
            recognizer.handle(
                event(0, Key::Control(ControlKey::Left), KeyState::Pressed),
                context,
            );
            recognizer.handle(
                event(20, Key::Control(ControlKey::Left), KeyState::Released),
                context,
            );
            recognizer.handle(
                event(100, Key::Control(ControlKey::Left), KeyState::Pressed),
                context,
            );
            assert_eq!(
                recognizer.handle(
                    event(120, Key::Control(ControlKey::Left), KeyState::Released),
                    context
                ),
                GestureDecision::None
            );
        }
    }

    #[test]
    fn gap_expiry_starts_a_new_sequence() {
        let mut recognizer = DoubleCtrlRecognizer::default();
        tap(&mut recognizer, ControlKey::Left, 0, 20);
        assert_eq!(
            tap(&mut recognizer, ControlKey::Left, 500, 520),
            GestureDecision::None
        );
    }
}
