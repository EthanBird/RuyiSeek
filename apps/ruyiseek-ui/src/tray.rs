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
            StandardItem {
                label: "设置…".to_owned(),
                icon_name: "preferences-system".to_owned(),
                activate: Box::new(|tray: &mut Self| tray.dispatch(DesktopAction::Settings)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "退出界面（保留后台索引）".to_owned(),
                activate: Box::new(|tray: &mut Self| tray.dispatch(DesktopAction::ExitUi)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "完全退出如意寻".to_owned(),
                icon_name: "application-exit".to_owned(),
                activate: Box::new(|tray: &mut Self| tray.dispatch(DesktopAction::QuitAll)),
                ..Default::default()
            }
            .into(),
        ]
    }
}

fn make_icon(size: i32) -> Icon {
    let width = usize::try_from(size).expect("tray icon size must be positive");
    let mut data = Vec::with_capacity(width * width * 4);
    let lens_center = size * 4 / 5;
    let lens_radius = size * 2 / 5;
    let lens_thickness = (size / 7).max(1);

    // 渐变起止色与 packaging/icons/io.github.ethanbird.RuyiSeek.svg 一致：
    //   左上  #2a8aa3 (rgb 42,138,163)
    //   右下  #176b87 (rgb 23,107,135)
    // 在 16/22/32 px 的托盘图标上也能看出来微妙过渡。
    const START_R: f32 = 42.0;
    const START_G: f32 = 138.0;
    const START_B: f32 = 163.0;
    const END_R: f32 = 23.0;
    const END_G: f32 = 107.0;
    const END_B: f32 = 135.0;

    for y in 0..size {
        for x in 0..size {
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

            // 渐变进度 t 用对角线坐标 (x + y) / (2 * (size-1))，从 0 到 1。
            let denom = (2.0 * (size - 1) as f32).max(1.0);
            let t = ((x + y) as f32 / denom).clamp(0.0, 1.0);

            let pixel = if ring || handle {
                let r = (START_R + (END_R - START_R) * t).round() as u8;
                let g = (START_G + (END_G - START_G) * t).round() as u8;
                let b = (START_B + (END_B - START_B) * t).round() as u8;
                [255, r, g, b]
            } else {
                [0, 0, 0, 0]
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

        // 必须存在：透明像素（环 + 手柄以外的区域）
        assert!(icon.data.chunks_exact(4).any(|pixel| pixel == [0, 0, 0, 0]));

        // 渐变端点验证。
        //   16x16 网格里，环出现在距离中心² ∈ [16, 64]，手柄出现在 x≥8,y≥8 且
        //   x+y≥19 且 |x-y|≤2。两个端点都是非整数 t，所以这里按"亮端近似 #2a8aa3"
        //   和"暗端近似 #176b87"两个色域各出现一次来验证。
        let pixels: Vec<&[u8]> = icon.data.chunks_exact(4).collect();
        let mut unique: std::collections::BTreeMap<[u8; 4], usize> =
            std::collections::BTreeMap::new();
        for p in &pixels {
            let key = [p[0], p[1], p[2], p[3]];
            *unique.entry(key).or_insert(0) += 1;
        }
        // 16x16 图标上 (0,0) 透明，所以不会到达 #2a8aa3 的严格端点。
        // 实测亮端约 #26829c，暗端约 #176b87 (因为手柄出现在右下方向)。
        // 注：ksni::Icon::data 字节序为 [A, R, G, B]。
        let bright_count = unique
            .iter()
            .filter(|(p, _)| p[0] == 255 && p[1] >= 36 && p[1] <= 42 && p[2] >= 128 && p[2] <= 138)
            .map(|(_, c)| *c)
            .sum::<usize>();
        let dark_count = unique
            .iter()
            .filter(|(p, _)| p[0] == 255 && p[1] <= 26 && p[2] <= 110)
            .map(|(_, c)| *c)
            .sum::<usize>();
        assert!(
            bright_count > 0,
            "icon should contain bright gradient pixels near #2a8aa3"
        );
        assert!(
            dark_count > 0,
            "icon should contain dark gradient pixels near #176b87"
        );

        // 不该再出现透明白色（旧版是白环）
        assert!(!pixels.iter().any(|p| *p == [255, 255, 255, 255]));
    }
}
