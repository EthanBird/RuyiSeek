# 实现状态

更新时间：2026-08-08

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

## 已完成：阶段 A1.2——托盘与桌面会话集成

- 会话 D-Bus 名称 `io.github.ethanbird.RuyiSeek` 保证 GUI 单实例；普通的第二次启动只调用 `ShowLauncher` 后退出。
- 暴露 `ShowLauncher`、`HideLauncher`、`ToggleLauncher` 和 `Quit` 控制方法，并提供对应命令行参数。
- 实现 StatusNotifierItem/DBusMenu 托盘：左键显示、中键切换，菜单可显示、隐藏或完全退出。
- 托盘包含内嵌 ARGB32 多尺寸图标，不依赖当前图标主题一定安装项目图标。
- 提供 D-Bus 激活 service 文件；`--background`、Desktop Entry 和 systemd 用户服务均复用同一实例。
- D-Bus 使用 vendored libdbus 构建路径，避免构建机缺少 `pkg-config`/开发头文件；依赖版本锁定并保持 Rust 1.75。

## 已完成：阶段 A1.3——会话安全与自动启动

- 通过 system bus 查询当前 `org.freedesktop.login1.Session` 的 `LockedHint`，并持续订阅 `PropertiesChanged`。
- 优先按 `XDG_SESSION_ID` 找到会话；systemd 用户服务不属于 session scope 时，回退到 PID 和当前用户的 display session。
- 锁屏状态以原子变量送入 XInput2 线程，每个原始按键事件均使用实时值；状态机在锁屏时重置，不会积累半次手势。
- 监听建立失败、属性刷新失败或 system bus 断开时采取 fail-closed：停用全局热键，而不是冒险在锁屏界面唤醒。
- 增加仅面向 Deepin 的 XDG Autostart 文件；安装器可在它与 systemd 用户服务之间二选一。
- 即使两种自动启动方式被同时启用，A1.2 的 D-Bus 单实例也会让后启动进程成功退出，不形成 systemd 重启循环。

## 已完成：阶段 A1.4——设置、配置与进程语义闭环

- 增加原生设置模式，可从托盘、D-Bus 或 `ruyiseek-ui --settings` 打开；支持登录后自动启动、双击 Ctrl 和全屏抑制三个基础选项。
- 配置保存到 `$XDG_CONFIG_HOME/ruyiseek/config.toml`（未设置时使用 `$HOME/.config`），包含显式 `schema_version`，采用临时文件、`fsync` 和同目录原子替换。
- 保存新配置前保留 `config.toml.previous`；当前文件损坏或版本不受支持时尝试回退上一次有效配置，无法回退才使用默认值。
- 自动启动设置只管理带 `X-RuyiSeek-Managed=true` 标记的用户 Desktop Entry，不会覆盖或删除同名的外部文件。
- 双击 Ctrl 与全屏抑制通过原子共享状态立即送入 X11 监听线程，保存设置后不需要重启 UI。
- 托盘现在明确区分“退出界面（保留后台索引）”与“完全退出如意寻”，并增加设置入口。
- IPC 增加 `SHUTDOWN` / `ACK`，daemon 回应后以成功状态退出；`ruyi stop` 和 `ruyiseek-ui --quit` 均复用同一协议。
- D-Bus 增加 `ShowSettings` 和 `ExitUi`；原有 `Quit` 调整为同时停止 UI 与 daemon，修正此前名不副实的行为。

## 已完成：阶段 A1.5——UOS 验收与安装打包

- 产物形态确定为单 .deb：二进制版本 `0.1.0-1` → `0.1.0-6`，路径 `dist/ruyiseek_<ver>_amd64.deb`。
- 构建策略为混合双 target：`ruyiseekd` 与 `ruyi` 走 `x86_64-unknown-linux-musl`（纯静态、无 C 运行时依赖），`ruyiseek-ui` 走 `x86_64-unknown-linux-gnu`（动态链接，因为 winit/x11-dl 在运行时 `dlopen` `libX11.so.6`，musl-static 的 `dlopen` 是桩函数）。
- Depends 收敛为标准 GUI 栈：`libc6`、`libgcc1`（UOS 20 包名，不是 Debian 11+ 的 `libgcc-s1`）、`libx11-6`、`libxcb1`、`libxi6`、`libxcursor1`、`libx11-xcb1`、`libxkbcommon0`、`libfontconfig1`，全为 `apt install ./xxx.deb` 直装无需 `apt-get -f`。
- 安装/卸载脚本（postinst / prerm / postrm）覆盖了 D-Bus 服务刷新、systemd `--user` 守护进程重载、XDG autostart 同步；升级路径保留上一份有效 `config.toml.previous`。
- `ruyiseek-ui` 在未检测到运行中的 daemon 时自动 fork 出 `ruyiseekd`（detached child，关闭 UI 不带走 daemon），用户安装后点击 launcher 即生效，无需 `systemctl --user daemon-reload`。
- postinst 提示信息对齐自动启动行为：登录后托盘自动拉起；如已 `systemctl --user enable --now ruyiseek-ui.service` 则不必重复。
- 35 个单元测试在 release 模式下全部通过（`ruyiseek-platform` 7、`ruyiseek-query` 4、`ruyiseek-ui` 12、`ruyiseekd` 9、其他 3）。
- 已知遗留：UOS 20 实机图形会话的窗口、焦点、DDE 合成器和双击 Ctrl 验收仍需用户在 `apt install ./xxx.deb` 后人工跑一遍；其余矩阵已经在本地与 CI 模拟。

## 下一步：阶段 A1.6——易用性与细节硬化

1. **方向键导航（v0.1.0-6 已完成，见 PR/提交）**：Slint 1.6 的 `process_key_input` 把方向键交给 LineEdit 内的 TextInput，父级 key-pressed 看不到；改用 XInput2 raw stream 的 `on_arrow` 回调更新 `selected-index`，launcher 不可见时忽略，避免在其它应用中误触发。
2. **设置界面返回**：关闭设置应当回到 launcher 而不是隐藏整个窗口。
3. **搜索去抖/重排**：连续输入时合并并发请求并丢弃过期响应（已实现），后续按 IO 与排序再分阶段优化。
4. **文档与代码同步**：完整开发设计文档、`README.md`、`implementation-status.md` 的路径、目录树、systemd 单元对齐到 0.1.0-6 的实际形态。
5. **小问题清理**：托盘菜单层级、退出确认、`config.toml` 字段命名空间化、错误信息去技术化。

## 当前限制

- 目录扫描结果仅驻留内存，尚无快照、journal 或 inotify 增量更新。
- 搜索召回仍为小规模开发实现；百万级数据将由倒排与原生索引替代。
- StatusNotifierItem、窗口前置/焦点、D-Bus 激活、锁屏状态和自动启动的 UOS 20 实机验收仍待用户在 DDE 图形会话中跑一遍（参见上文 A1.5）。
- Wayland 下全局双击 Ctrl 暂不可用；必须通过 Portal / DDE 的合规接口实现。
- 设置界面目前只覆盖基础启动与唤醒项，索引范围、排除规则、搜索行为和外观设置将在对应功能落地时补齐。
- 许可证待仓库所有者决定，当前未擅自添加。
