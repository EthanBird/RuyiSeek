use dbus::arg::PropMap;
use dbus::blocking::stdintf::org_freedesktop_dbus::{Properties, PropertiesPropertiesChanged};
use dbus::blocking::Connection;
use std::error::Error;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const LOGIN1_NAME: &str = "org.freedesktop.login1";
const MANAGER_PATH: &str = "/org/freedesktop/login1";
const MANAGER_INTERFACE: &str = "org.freedesktop.login1.Manager";
const SESSION_INTERFACE: &str = "org.freedesktop.login1.Session";
const USER_INTERFACE: &str = "org.freedesktop.login1.User";
const LOCKED_PROPERTY: &str = "LockedHint";
const DBUS_TIMEOUT: Duration = Duration::from_secs(2);
const PROCESS_TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) struct Monitor {
    state: Arc<AtomicBool>,
    _thread: thread::JoinHandle<()>,
}

impl Monitor {
    pub(crate) fn state(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.state)
    }
}

pub(crate) fn spawn() -> Result<Monitor, Box<dyn Error>> {
    let connection = Connection::new_system()?;
    let session_path = resolve_session_path(&connection)?;
    let session = connection.with_proxy(LOGIN1_NAME, session_path.clone(), DBUS_TIMEOUT);
    let initially_locked: bool = session.get(SESSION_INTERFACE, LOCKED_PROPERTY)?;
    let state = Arc::new(AtomicBool::new(initially_locked));

    let signal_state = Arc::clone(&state);
    let signal_path = session_path.clone();
    session.match_signal(
        move |change: PropertiesPropertiesChanged, connection: &Connection, _: &dbus::Message| {
            if change.interface_name != SESSION_INTERFACE {
                return true;
            }
            if let Some(locked) = locked_hint(&change.changed_properties) {
                signal_state.store(locked, Ordering::Release);
            } else if change
                .invalidated_properties
                .iter()
                .any(|property| property == LOCKED_PROPERTY)
            {
                let proxy = connection.with_proxy(LOGIN1_NAME, signal_path.clone(), DBUS_TIMEOUT);
                match proxy.get::<bool>(SESSION_INTERFACE, LOCKED_PROPERTY) {
                    Ok(locked) => signal_state.store(locked, Ordering::Release),
                    Err(error) => {
                        signal_state.store(true, Ordering::Release);
                        eprintln!("ruyiseek-ui: 无法刷新锁屏状态，已停用全局唤醒：{error}");
                    }
                }
            }
            true
        },
    )?;

    let failure_state = Arc::clone(&state);
    let monitor_thread = thread::Builder::new()
        .name("ruyiseek-session-lock".to_owned())
        .spawn(move || loop {
            if let Err(error) = connection.process(PROCESS_TIMEOUT) {
                failure_state.store(true, Ordering::Release);
                eprintln!("ruyiseek-ui: 锁屏状态监听已停止，已停用全局唤醒：{error}");
                return;
            }
        })?;

    Ok(Monitor {
        state,
        _thread: monitor_thread,
    })
}

fn resolve_session_path(connection: &Connection) -> Result<dbus::Path<'static>, Box<dyn Error>> {
    let manager = connection.with_proxy(LOGIN1_NAME, MANAGER_PATH, DBUS_TIMEOUT);
    if let Ok(session_id) = std::env::var("XDG_SESSION_ID") {
        let result: Result<(dbus::Path<'static>,), dbus::Error> =
            manager.method_call(MANAGER_INTERFACE, "GetSession", (session_id,));
        if let Ok((path,)) = result {
            return Ok(path);
        }
    }

    let result: Result<(dbus::Path<'static>,), dbus::Error> =
        manager.method_call(MANAGER_INTERFACE, "GetSessionByPID", (std::process::id(),));
    if let Ok((path,)) = result {
        return Ok(path);
    }

    let (user_path,): (dbus::Path<'static>,) =
        manager.method_call(MANAGER_INTERFACE, "GetUserByPID", (std::process::id(),))?;
    let user = connection.with_proxy(LOGIN1_NAME, user_path, DBUS_TIMEOUT);
    let (_session_id, display_path): (String, dbus::Path<'static>) =
        user.get(USER_INTERFACE, "Display")?;
    if display_path.to_string() == "/" {
        return Err("login1 没有为当前用户报告图形会话".into());
    }
    Ok(display_path)
}

fn locked_hint(properties: &PropMap) -> Option<bool> {
    properties
        .get(LOCKED_PROPERTY)
        .and_then(|value| value.0.as_i64())
        .map(|value| value != 0)
}

#[cfg(test)]
mod tests {
    use super::locked_hint;
    use dbus::arg::{PropMap, Variant};

    #[test]
    fn reads_locked_hint_from_changed_properties() {
        let mut properties = PropMap::new();
        properties.insert("LockedHint".to_owned(), Variant(Box::new(true)));
        assert_eq!(locked_hint(&properties), Some(true));

        properties.insert("LockedHint".to_owned(), Variant(Box::new(false)));
        assert_eq!(locked_hint(&properties), Some(false));
    }

    #[test]
    fn missing_locked_hint_is_ignored() {
        assert_eq!(locked_hint(&PropMap::new()), None);
    }
}
