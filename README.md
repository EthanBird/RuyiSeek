# 如意寻 RuyiSeek

如意寻是面向统信 UOS / Deepin 的本地文件搜索与全局启动器。目标是把高速文件索引、双击 Ctrl 呼出、应用与命令搜索、文件对话框跳转、托盘和后台服务整合成一个原生 Linux 桌面工具。

完整产品与架构设计见 [开发设计文档](docs/如意寻_RuyiSeek_完整开发设计文档.md)。

## 当前进度

仓库目前完成了阶段 A1 的第二个纵向切片：

- Rust workspace 及 `ruyiseekd`、`ruyiseek-ui`、`ruyi` 三个进程入口；
- 可独立测试的双击 Ctrl 状态机，覆盖长按、组合键、自动重复和锁屏/全屏抑制；
- 安全的目录快照扫描器，不跟随符号链接；
- 文件/目录内存搜索、基础排序和 Top-K 截断；
- 有长度上限的 Unix Socket 帧协议；
- Slint 原生无边框启动器、异步查询、键盘选择、回车打开和错误状态；
- X11/XInput2 原始按键监听，把双击 Ctrl 接入真实窗口显示/隐藏；
- daemon、CLI 与 GUI 之间可运行的端到端查询链路；
- `--background` 常驻模式及对应的 systemd 用户服务配置。

StatusNotifierItem 托盘、D-Bus 单实例/激活、锁屏状态接入和持久化增量索引仍在后续迭代中。Wayland 下不会绕过桌面安全模型伪造全局修饰键监听：窗口和搜索可用，双击 Ctrl 将等待 Portal 或 DDE 提供合规能力。

## 构建

需要 Rust 1.75 或更高版本。

```bash
cargo build --workspace
cargo test --workspace
```

## 本地演示

先启动守护进程并为一个小目录建立临时内存快照：

```bash
cargo run -p ruyiseekd -- --root "$PWD"
```

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

不连接显示服务器，仅验证双击 Ctrl 状态机：

```bash
cargo run -p ruyiseek-ui -- --demo-double-ctrl
```

默认 Socket 位于 `$XDG_RUNTIME_DIR/ruyiseek/daemon.sock`；未设置 `XDG_RUNTIME_DIR` 时使用 `/tmp/ruyiseek-<用户标识>/daemon.sock`。

## 兼容目标

- 统信 UOS / Deepin，x86_64 与 aarch64；
- X11 上完整支持双击 Ctrl，Wayland 遵循 Portal / DDE 能力边界；
- 不以 root 运行，不读取 `/dev/input`，不监听 TCP；
- Debian 10 / UOS 兼容构建将在专用容器中执行，避免新 glibc 污染发布物。
