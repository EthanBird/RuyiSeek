# 如意寻 RuyiSeek

如意寻是面向统信 UOS / Deepin 的本地文件搜索与全局启动器。目标是把高速文件索引、双击 Ctrl 呼出、应用与命令搜索、文件对话框跳转、托盘和后台服务整合成一个原生 Linux 桌面工具。

完整产品与架构设计见 [开发设计文档](docs/如意寻_RuyiSeek_完整开发设计文档.md)。

## 当前进度

仓库目前完成了阶段 A1 的第六个纵向切片：

- Rust workspace 及 `ruyiseekd`、`ruyiseek-ui`、`ruyi` 三个进程入口；
- 可独立测试的双击 Ctrl 状态机，覆盖长按、组合键、自动重复和锁屏/全屏抑制；
- 安全的目录快照扫描器，不跟随符号链接；
- 文件/目录内存搜索、基础排序和 Top-K 截断；
- 有长度上限的 Unix Socket 帧协议；
- Slint 原生无边框启动器、异步查询、键盘选择、回车打开和错误状态；
- X11/XInput2 原始按键监听，把双击 Ctrl 接入真实窗口显示/隐藏；
- 通过 systemd-logind `LockedHint` 监听真实锁屏状态，锁屏时抑制全局唤醒；
- 基于 D-Bus 的 GUI 单实例控制，重复启动会唤醒已有窗口；
- StatusNotifierItem 系统托盘，支持显示、隐藏和完全退出；
- 原生设置模式与版本化 TOML 配置，支持损坏回退和原子保存；
- 登录后自动启动、双击 Ctrl、全屏抑制三个可配置选项，热键设置保存后立即生效；
- 明确区分退出 UI 与完全退出，完全退出通过本地 IPC 正常停止 daemon；
- daemon、CLI 与 GUI 之间可运行的端到端查询链路；
- `--background` 常驻模式，以及 systemd 用户服务、D-Bus 激活和 Deepin XDG Autostart 配置；
- 单文件 `.deb` 安装包（`dist/ruyiseek_<版本>_amd64.deb`），混合 musl/gnu 构建，`apt install ./xxx.deb` 即可；
- UI 自动 fork daemon：未检测到运行中的 `ruyiseekd` 时自行拉起，关 UI 不带走 daemon；
- 本地卷自动发现（v0.1.0-12）：daemon 无 `--root` 启动时读取 Linux mountinfo，除 `$HOME` 外自动索引新加本地卷；运行期间插入或卸载卷会在 2 秒内触发后台重建，完成后原子切换，重建时旧索引仍可查询；
- 方向键导航（v0.1.0-6）：在 XInput2 raw stream 上拦截 ↑↓←→ 并回灌 `selected-index`，解决 Slint 1.6 把方向键交给 LineEdit 内部 TextInput、导致父级 key-pressed 看不到的问题。
- 右键上下文菜单（v0.1.0-8）：每个结果行右键弹出「打开 / 打开所在文件夹 / 复制文件 / 复制路径」四项；Slint 1.6 不暴露 `PopupMenu` / `MenuItem` / `MouseArea`，菜单用手写 Rectangle + TouchArea 实现，位置由 `popup-index` 驱动。复制通过 `xclip` / `wl-copy` 完成，按 `XDG_SESSION_TYPE` 选择主用工具。
- UI 体验改进（v0.1.0-9）：启动器改为 960×600 透明无边框窗，焦点卡居中显示；点击空白处直接隐藏，点卡片 padding 不消失（只把焦点放回搜索框）；Esc 直接隐藏（不再"先清空再隐藏"两段式）；第二次双击 Ctrl 不再保留上一次结果（显式清空 results、selected-index、query 和 generation）；新 SVG 图标（青蓝渐变放大镜），同时生成 48/64/128/256 PNG，hicolor 主题自动识别；任务栏托盘位图与 SVG 配色一致，去掉旧版的白环；搜索条去掉"寻"图标和"Ctrl ×2"提示芯片，回归纯文本输入栏。
- 长列表与右键菜单修复（v0.1.0-14）：右侧显示可拖动滚动条；菜单按滚动后鼠标的实际位置弹出，并使用独立 X11 顶层窗口完整显示在搜索卡片之上、可突破卡片边界。
- 文件变化自动刷新（v0.1.0-15）：daemon 使用 Linux inotify 监听已扫描目录；新建、删除或重命名文件后会在后台重建并原子切换内存索引，`中文.txt` 等运行后新增的中文文件名无需重启即可搜索。

文件级持久化增量索引仍在后续迭代中；当前版本已经能跟随文件变化以及本地卷的挂载与卸载重建内存索引。Wayland 下不会绕过桌面安全模型伪造全局修饰键监听：窗口、搜索、单实例与托盘可用，双击 Ctrl 将等待 Portal 或 DDE 提供合规能力。

## 构建

需要 Rust 1.75 或更高版本。

```bash
cargo build --workspace
cargo test --workspace
```

## 安装包（DEB）

单文件 `.deb` 已自带除系统 `libc6` 基础栈以外的运行时库（`libgcc1`、X11、字体等 GUI 栈）；UOS 20 本身已提供所需的 glibc 2.28，因此可直接安装：

```bash
sudo apt install ./dist/ruyiseek_0.1.0-15_amd64.deb
# 或
sudo dpkg -i dist/ruyiseek_0.1.0-15_amd64.deb
```

无需 `apt-get -f install`，也无需先编译。装完后登录桌面，托盘自动出现；双击 Ctrl 唤起启动器，回车打开搜索项。如需重新构建产物：

```bash
bash packaging/deb/build.sh
```

构建脚本对 `ruyiseekd` 与 `ruyi` 走 `x86_64-unknown-linux-musl`（纯静态），`ruyiseek-ui` 走 `x86_64-unknown-linux-gnu`（动态链接，因为 winit/x11-dl 在运行时 `dlopen` `libX11.so.6`，musl-static 的 `dlopen` 是桩函数）。

构建脚本不会调用 apt、rustup 或其他安装器；它只使用主机已有的 Rust target 与打包工具，缺少时会直接报告并退出。已有 Cargo 源码缓存的离线构建可使用：

```bash
CARGO_NET_OFFLINE=true bash packaging/deb/build.sh
```

生成的 `.deb` 会自动执行包结构、glibc 2.28 ABI、隔离共享库解析和无图形 UI 烟测，并在同目录生成 `.sha256`。完整 daemon/CLI 协议测试可执行：

```bash
bash packaging/deb/verify.sh dist/ruyiseek_0.1.0-15_amd64.deb
```

## 本地演示

先启动守护进程并为一个小目录建立临时内存快照：

```bash
cargo run -p ruyiseekd -- --root "$PWD"
```

正式运行时不传 `--root`：daemon 会索引 `$HOME` 和当前可读的本地数据卷，并持续检测卷的挂载/卸载。显式传入一个或多个 `--root` 会关闭自动卷发现，便于开发测试或限定范围。

另一个终端执行搜索：

```bash
cargo run -p ruyi-cli -- search 设计文档
```

启动真实搜索窗口（需要已运行的图形会话）：

```bash
cargo run -p ruyiseek-ui
```

以隐藏常驻模式启动，等待 X11 下双击 Ctrl：

```bash
cargo run -p ruyiseek-ui -- --background
```

再次执行 `ruyiseek-ui` 会通过会话 D-Bus 显示已经运行的窗口。也可以显式控制：

```bash
cargo run -p ruyiseek-ui -- --toggle
cargo run -p ruyiseek-ui -- --hide
cargo run -p ruyiseek-ui -- --settings
cargo run -p ruyiseek-ui -- --exit-ui
cargo run -p ruyiseek-ui -- --quit
```

托盘遵循 StatusNotifierItem/DBusMenu 协议，在 DDE 支持该协议的托盘区域中显示；左键显示窗口，菜单可打开设置、隐藏窗口、只退出界面或完全退出。只退出界面会保留索引服务，完全退出会等待 daemon 确认停止后退出 UI。

基础设置保存在 `$XDG_CONFIG_HOME/ruyiseek/config.toml`，未设置该变量时使用 `$HOME/.config/ruyiseek/config.toml`。程序保存时会保留 `config.toml.previous`，并在当前配置损坏时回退。自动启动开关写入用户级 XDG Autostart 文件，只修改带有如意寻管理标记的条目。

自动启动提供两种打包入口：systemd 用户服务和 `packaging/autostart` 下的 Deepin XDG Autostart 文件。安装器应按目标系统选择其中一种；即使用户误启用两种，D-Bus 单实例也会让后启动进程正常退出。锁屏状态无法确认时采取安全关闭策略：保留托盘和搜索，但停用全局双击 Ctrl。

不连接显示服务器，仅验证双击 Ctrl 状态机：

```bash
cargo run -p ruyiseek-ui -- --demo-double-ctrl
```

默认 Socket 位于 `$XDG_RUNTIME_DIR/ruyiseek/daemon.sock`；未设置 `XDG_RUNTIME_DIR` 时使用 `/tmp/ruyiseek-<用户标识>/daemon.sock`。

## 兼容目标

- 统信 UOS / Deepin，x86_64 与 aarch64；
- X11 上完整支持双击 Ctrl（含方向键选择），Wayland 遵循 Portal / DDE 能力边界；
- 不以 root 运行，不读取 `/dev/input`，不监听 TCP；
- 发布物在 Debian 10 / UOS 兼容 glibc 上验证，避免新 glibc 污染。
