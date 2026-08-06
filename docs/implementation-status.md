# 实现状态

更新时间：2026-08-06

## 已完成：阶段 A0——可运行的进程与协议骨架

- `ruyiseek-core`：跨进程共享的结果类型。
- `ruyiseek-index`：有边界、不跟随符号链接的临时目录快照。
- `ruyiseek-query`：名称/路径召回、连续子序列匹配、基础精排和 Top-K。
- `ruyiseek-ipc`：1 MiB 上限的长度前缀帧、协议版本、查询与状态消息。
- `ruyiseek-platform`：不吞键的双击 Ctrl 识别状态机。
- `ruyiseekd`：单实例 Unix Socket 服务和查询执行。
- `ruyi`：`ping`、`status`、`search` 命令。
- `ruyiseek-ui`：验证后台连接与热键判定的开发壳。
- systemd 用户服务和 Desktop Entry 初稿。

这一步明确是纵向工程底座，不把命令行开发壳宣称为 GUI 成品。

## 下一步：阶段 A1——真正的桌面常驻体验

1. 接入 Slint 启动器窗口，预创建后隐藏，显示时恢复焦点。
2. 实现 XInput2 RawKeyPress / RawKeyRelease 适配器，把事件送入现有状态机。
3. 增加 D-Bus 单实例与 `ShowLauncher`、`ToggleLauncher` 方法。
4. 实现 StatusNotifierItem 托盘以及“退出界面/完全退出”语义。
5. 增加 XDG Autostart 与 systemd 安装冲突检测。
6. 在 Debian 10 / UOS 兼容容器内完成 x86_64 构建与实机验收。

## 当前限制

- 目录扫描结果仅驻留内存，尚无快照、journal 或 inotify 增量更新。
- 搜索召回仍为小规模开发实现；百万级数据将由倒排与原生索引替代。
- 尚未连接显示服务器，因此没有真实全局按键和可见搜索窗口。
- 尚未实现 StatusNotifierItem 和 D-Bus。
- 许可证待仓库所有者决定，当前未擅自添加。
