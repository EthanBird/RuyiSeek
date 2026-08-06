use super::{DesktopAction, UiEvent};
use ksni::menu::StandardItem;
use ksni::{Category, Icon, MenuItem, ToolTip, Tray, TrayService};
use std::io;
use std::sync::mpsc::Sender;
use std::thread;

pub(crate) struct TrayGuard {
    handle: ksni::Handle<RuyiTray>,
}

impl Drop for TrayGuard {
    fn drop(&mut self) {
        self.handle.shutdown();
    }
}

pub(crate) fn spawn(sender: Sender<UiEvent>) -> Result<TrayGuard, io::Error> {
    let service = TrayService::new(RuyiTray { sender });
    let handle = service.handle();
    thread::Builder::new()
        .name("ruyiseek-tray".to_owned())
        .spawn(move || {
            if let Err(error) = service.run() {
                eprintln!("ruyiseek-ui: 系统托盘服务已停止：{error}");
            }
        })?;
    Ok(TrayGuard { handle })
}

pub(crate) struct RuyiTray {
    sender: Sender<UiEvent>,
}

impl RuyiTray {
    fn dispatch(&self, action: DesktopAction) {
        let _ = self.sender.send(UiEvent::Desktop(action));
    }
}

impl Tray for RuyiTray {
    fn activate(&mut self, _x: i32, _y: i32) {
        self.dispatch(DesktopAction::Show);
    }

    fn secondary_activate(&mut self, _x: i32, _y: i32) {
        self.dispatch(DesktopAction::Toggle);
    }

    fn category(&self) -> Category {
        Category::SystemServices
    }

    fn id(&self) -> String {
        "io.github.ethanbird.RuyiSeek".to_owned()
    }

    fn title(&self) -> String {
        "如意寻".to_owned()
    }

    fn icon_name(&self) -> String {
        "system-search".to_owned()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        [16, 22, 32].into_iter().map(make_icon).collect()
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip {
            icon_name: self.icon_name(),
            icon_pixmap: self.icon_pixmap(),
            title: self.title(),
            description: "双击 Ctrl，快速查找文件、应用与命令".to_owned(),
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: "显示如意寻".to_owned(),
                icon_name: "system-search".to_owned(),
                activate: Box::new(|tray: &mut Self| tray.dispatch(DesktopAction::Show)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "隐藏搜索窗口".to_owned(),
                activate: Box::new(|tray: &mut Self| tray.dispatch(DesktopAction::Hide)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "完全退出".to_owned(),
                icon_name: "application-exit".to_owned(),
                activate: Box::new(|tray: &mut Self| tray.dispatch(DesktopAction::Quit)),
                ..Default::default()
            }
            .into(),
        ]
    }
}

fn make_icon(size: i32) -> Icon {
    let width = usize::try_from(size).expect("tray icon size must be positive");
    let mut data = Vec::with_capacity(width * width * 4);
    let center = size - 1;
    let outer_radius = size - 2;
    let lens_center = size * 4 / 5;
    let lens_radius = size * 2 / 5;
    let lens_thickness = (size / 7).max(1);

    for y in 0..size {
        for x in 0..size {
            let outer_x = 2 * x - center;
            let outer_y = 2 * y - center;
            let inside = outer_x * outer_x + outer_y * outer_y <= outer_radius * outer_radius;

            let lens_x = 2 * x - lens_center;
            let lens_y = 2 * y - lens_center;
            let lens_distance = lens_x * lens_x + lens_y * lens_y;
            let lens_outer = lens_radius + lens_thickness;
            let lens_inner = (lens_radius - lens_thickness).max(0);
            let ring = lens_distance <= lens_outer * lens_outer
                && lens_distance >= lens_inner * lens_inner;
            let handle = x >= size / 2
                && y >= size / 2
                && x + y >= size * 6 / 5
                && (x - y).abs() <= lens_thickness;

            let pixel = if !inside {
                [0, 0, 0, 0]
            } else if ring || handle {
                [255, 255, 255, 255]
            } else {
                [255, 24, 126, 148]
            };
            data.extend_from_slice(&pixel);
        }
    }

    Icon {
        width: size,
        height: size,
        data,
    }
}

#[cfg(test)]
mod tests {
    use super::make_icon;

    #[test]
    fn generated_icon_has_argb_pixels_and_expected_colors() {
        let icon = make_icon(16);
        assert_eq!(icon.width, 16);
        assert_eq!(icon.height, 16);
        assert_eq!(icon.data.len(), 16 * 16 * 4);
        assert!(icon.data.chunks_exact(4).any(|pixel| pixel == [0, 0, 0, 0]));
        assert!(icon
            .data
            .chunks_exact(4)
            .any(|pixel| pixel == [255, 24, 126, 148]));
        assert!(icon
            .data
            .chunks_exact(4)
            .any(|pixel| pixel == [255, 255, 255, 255]));
    }
}
