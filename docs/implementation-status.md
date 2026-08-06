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
- `ruyiseek-ui`：阶段 A0 的后台连接与热键判定开发壳。
- systemd 用户服务和 Desktop Entry 初稿。

这一步明确是纵向工程底座，不把命令行开发壳宣称为 GUI 成品。

## 已完成：阶段 A1.1——真实启动器与 X11 唤醒

- 使用 Slint 1.6 实现 720 px 无边框、置顶、透明启动器；依赖已锁定并用 Rust 1.75 验证。
- 查询在独立工作线程中通过 Unix Socket 调用 `ruyiseekd`，连续输入会合并待处理请求，过期响应不会覆盖新结果。
- 最多展示 9 项结果，支持上下键、回车、鼠标、Esc 清空/隐藏；使用 `xdg-open` 参数调用而非 shell 拼接。
- 使用 `x11rb` 订阅 XInput2 `RawKeyPress` / `RawKeyRelease`，读取实时键盘映射并接入既有双击 Ctrl 状态机。
- 热键不抓取、不吞掉原始按键；全屏窗口通过 EWMH 状态抑制唤醒。
- `ruyiseek-ui --background` 在窗口隐藏后仍保持事件循环，systemd 用户服务已切换到该模式。
- Wayland 会话明确降级为菜单启动，不读取 `/dev/input`，不请求 root 权限。

## 下一步：阶段 A1.2——托盘与桌面会话集成

1. 增加 D-Bus 单实例与 `ShowLauncher`、`ToggleLauncher` 方法。
2. 实现 StatusNotifierItem 托盘以及“退出界面/完全退出”语义。
3. 接入会话锁屏状态，补全热键抑制上下文。
4. 增加 XDG Autostart 与 systemd 安装冲突检测。
5. 在 Debian 10 / UOS 兼容容器内完成 x86_64 构建与实机验收。

## 当前限制

- 目录扫描结果仅驻留内存，尚无快照、journal 或 inotify 增量更新。
- 搜索召回仍为小规模开发实现；百万级数据将由倒排与原生索引替代。
- 还没有在 UOS 实机图形会话中完成窗口、焦点、DDE 合成器和双击 Ctrl 验收。
- 尚未实现 StatusNotifierItem、D-Bus 单实例和锁屏状态订阅。
- Wayland 下全局双击 Ctrl 暂不可用；必须通过 Portal / DDE 的合规接口实现。
- 许可证待仓库所有者决定，当前未擅自添加。
