use super::{DesktopAction, UiEvent};
use dbus::blocking::stdintf::org_freedesktop_dbus::RequestNameReply;
use dbus::blocking::Connection;
use dbus_crossroads::Crossroads;
use std::error::Error;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

const BUS_NAME: &str = "io.github.ethanbird.RuyiSeek";
const OBJECT_PATH: &str = "/io/github/ethanbird/RuyiSeek";
const INTERFACE: &str = "io.github.ethanbird.RuyiSeek.Launcher";
const CALL_TIMEOUT: Duration = Duration::from_secs(2);

pub(crate) enum Claim {
    Primary(InstanceGuard),
    Forwarded,
}

pub(crate) struct InstanceGuard {
    _service_thread: thread::JoinHandle<()>,
}

pub(crate) fn claim_or_forward(
    sender: Sender<UiEvent>,
    action: Option<DesktopAction>,
) -> Result<Claim, Box<dyn Error>> {
    let connection = Connection::new_session()?;
    let reply = connection.request_name(BUS_NAME, false, false, true)?;

    if matches!(
        reply,
        RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner
    ) {
        let service_thread = spawn_service(connection, sender)?;
        Ok(Claim::Primary(InstanceGuard {
            _service_thread: service_thread,
        }))
    } else {
        if let Some(action) = action {
            forward_action(&connection, action)?;
        }
        Ok(Claim::Forwarded)
    }
}

fn spawn_service(
    connection: Connection,
    sender: Sender<UiEvent>,
) -> Result<thread::JoinHandle<()>, std::io::Error> {
    thread::Builder::new()
        .name("ruyiseek-dbus".to_owned())
        .spawn(move || {
            let mut crossroads = Crossroads::new();
            let interface = crossroads.register(INTERFACE, move |builder| {
                register_action(builder, "ShowLauncher", sender.clone(), DesktopAction::Show);
                register_action(builder, "HideLauncher", sender.clone(), DesktopAction::Hide);
                register_action(
                    builder,
                    "ToggleLauncher",
                    sender.clone(),
                    DesktopAction::Toggle,
                );
                register_action(builder, "Quit", sender, DesktopAction::Quit);
            });
            crossroads.insert(OBJECT_PATH, &[interface], ());

            if let Err(error) = crossroads.serve(&connection) {
                eprintln!("ruyiseek-ui: D-Bus 控制服务已停止：{error}");
            }
        })
}

fn register_action(
    builder: &mut dbus_crossroads::IfaceBuilder<()>,
    method: &'static str,
    sender: Sender<UiEvent>,
    action: DesktopAction,
) {
    builder.method(method, (), (), move |_, (), ()| {
        sender
            .send(UiEvent::Desktop(action))
            .map_err(|_| dbus_crossroads::MethodErr::failed("RuyiSeek is shutting down"))?;
        Ok(())
    });
}

fn forward_action(connection: &Connection, action: DesktopAction) -> Result<(), dbus::Error> {
    let method = match action {
        DesktopAction::Show => "ShowLauncher",
        DesktopAction::Hide => "HideLauncher",
        DesktopAction::Toggle => "ToggleLauncher",
        DesktopAction::Quit => "Quit",
    };
    let proxy = connection.with_proxy(BUS_NAME, OBJECT_PATH, CALL_TIMEOUT);
    proxy.method_call::<(), _, _, _>(INTERFACE, method, ())
}
