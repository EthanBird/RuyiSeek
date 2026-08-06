# 如意寻（RuyiSeek）完整开发设计文档

> 一呼即现，所想即所得。

| 项目 | 定义 |
| --- | --- |
| 中文名称 | 如意寻 |
| 英文名称 | RuyiSeek |
| 命令名称 | ruyi |
| 后台服务 | ruyiseekd |
| Debian 包名 | ruyiseek |
| Desktop ID | io.github.ethanbird.RuyiSeek |
| D-Bus 服务名 | io.github.ethanbird.RuyiSeek |
| 首要平台 | 统信 UOS 桌面版、DDE |
| 首要架构 | x86_64、aarch64 |
| 后续平台 | Deepin、Debian、Ubuntu、银河麒麟及其他 Linux 桌面 |
| 文档版本 | 1.0 |
| 文档日期 | 2026-08-06 |

---

## 1. 文档目的

本文档定义一款面向统信 UOS 和 Linux 桌面的高性能文件搜索、应用启动与文件工作流工具。产品必须完整覆盖 Listary 6 Pro 的核心能力，而不是只实现一个文件搜索窗口。

如意寻由以下四个同等重要的产品表面构成：

1. 轻量启动器：双击 Ctrl 后瞬间出现，用于文件、文件夹、应用、命令和网页搜索。
2. 深度搜索窗口：适合大量结果、复杂筛选、多窗口比较、预览和批量操作。
3. 快速切换：在打开、保存、选择文件夹对话框中快速跳转到目标目录。
4. 桌面与文件管理器增强：直接输入搜索、收藏菜单、历史目录、动作菜单和上下文操作。

此外，产品必须具备完整的桌面右下角托盘、登录自启动、后台索引、增量更新、故障恢复、设置管理和诊断能力。

本文档既是产品需求规格，也是可以直接据此拆分代码、编写测试和验收发布版本的工程设计。

---

## 2. 产品定位

### 2.1 一句话定义

如意寻是统信 UOS 上常驻后台的全局文件与操作入口：用户双击 Ctrl，即可在任何界面查找文件、启动应用、切换目录、执行动作或发起网页搜索。

### 2.2 核心目标

- 双击 Ctrl 后，搜索框应当像输入法候选窗一样自然出现。
- 搜索过程必须是输入即出结果，不允许等待整盘遍历。
- 文件名、路径、拼音、拼音首字母和模糊缩写均可检索。
- 轻量启动器与深度搜索窗口共享同一搜索语义和排序结果。
- 文件选择对话框可以跳转到最近目录、收藏目录或搜索到的目录。
- 索引、托盘、热键和 UI 均在用户会话内稳定常驻。
- 不依赖浏览器或 WebView，不使用 Electron。
- 空闲时几乎不占用 CPU；索引构建时主动让出前台 I/O。
- 支持统信 UOS 常见的 DDE、X11 和 Wayland 环境。
- 所有核心能力离线可用。

### 2.3 非目标

以下内容不属于首个正式版的硬性范围：

- 替代完整文件管理器。
- 云盘账号、密码和文件同步服务。
- 互联网内容聚合搜索。
- 大模型语义搜索。
- OCR 和图片内容识别。
- 对加密目录绕过权限建立索引。
- 以 root 身份常驻或读取普通用户无权访问的路径。

全文检索、OCR、语义搜索可以作为后续可选插件，但不得拖慢基础文件名搜索。

---

## 3. 命名与品牌设计

### 3.1 名称

正式名称采用“如意寻”，英文名采用 RuyiSeek。

“如意”具有自然、吉祥、顺手的中文含义；“寻”直接表达搜索。名称不强调某一种文件系统，也不会限制未来扩展至应用、命令、网页和动作。

### 3.2 品牌口号

主口号：

> 一呼即现，所想即所得。

辅助文案：

- 双击 Ctrl，马上找到。
- 文件、应用、命令，一处直达。
- 不翻目录，只管输入。

### 3.3 图标方向

图标建议由“如意云头”和“放大镜”融合：

- 外轮廓为简化的如意云纹。
- 中央负形构成放大镜镜片。
- 主色使用青黛蓝或玉石青。
- 托盘小图标必须提供单色符号版本，保证 16×16 像素可辨识。
- 索引中、暂停和错误状态通过小圆点表达，不重新绘制主体轮廓。

---

## 4. 参考产品能力基线

### 4.1 Listary 6 Pro 官方能力摘要

根据 Listary 官方文档，Listary 6 的产品能力包括：

- 双击 Ctrl 呼出轻量搜索栏。
- 文件与应用的毫秒级搜索。
- 模糊匹配、使用习惯排序和中文拼音搜索。
- 独立的深度文件搜索窗口。
- 文件类型和修改日期筛选。
- 路径关键词搜索。
- 文件预览。
- 网络驱动器、共享目录和指定本地目录索引。
- 应用启动。
- 文件对话框 Quick Switch。
- 推荐目录、历史目录和收藏目录。
- 网页搜索及搜索建议。
- 自定义命令。
- 自定义过滤器。
- 高、普通、低、忽略四级优先级规则。
- 自定义动作以及结果右键菜单。
- 收藏菜单和子菜单。
- 文件管理器直接输入搜索。
- 文件管理器空白区域双击或中键菜单。
- 可配置热键、全屏免打扰。
- 文件管理器和第三方应用集成。
- 深色主题。
- 右下角托盘、设置入口、索引控制和后台运行。

Listary 官方功能资料：

- 产品与 Pro 能力：https://www.listary.com/pro
- 文件搜索：https://help.listary.com/search-file
- 快速切换：https://help.listary.com/quick-switch
- 应用启动：https://help.listary.com/launch-apps
- 网页搜索：https://help.listary.com/web-search
- 命令：https://help.listary.com/options-commands
- 过滤器：https://help.listary.com/options-filters
- 优先级：https://help.listary.com/options-priorities
- 索引：https://help.listary.com/options-index
- 菜单：https://help.listary.com/options-menu
- 动作：https://help.listary.com/options-actions
- 热键：https://help.listary.com/options-hotkey
- 集成：https://help.listary.com/options-integration
- 更新记录：https://help.listary.com/changelog

### 4.2 UOS 可复用基础

DDE 已有两项可复用基础设施：

1. deepin-anything：以 Windows Everything 为目标的高速文件名索引项目。
2. dde-grand-search：DDE 系统级搜索工具，依赖 deepin-anything-server。

相关资料：

- deepin-anything：https://github.com/linuxdeepin/deepin-anything
- deepin-anything 应用接口：https://github.com/linuxdeepin/deepin-anything/blob/master/docs/app_developer.md
- DDE Grand Search：https://github.com/linuxdeepin/dde-grand-search
- DDE 全局快捷键模块：https://github.com/linuxdeepin/dde-daemon/tree/master/keybinding1
- XDG Global Shortcuts：https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.GlobalShortcuts.html

如意寻第一阶段可以通过适配器使用系统已有的 deepin-anything-server，以缩短可用版本的落地时间；最终仍应提供自主 Rust 索引后端，避免产品能力和发行版兼容性完全依赖 DDE 组件。

---

## 5. 功能对等矩阵

下表中的 P0 表示首个正式版必须具备，P1 表示完整 Listary 6 Pro 对等版本必须具备，P2 表示如意寻增强能力。

| 编号 | Listary 6 Pro 能力 | 如意寻实现 | 优先级 |
| --- | --- | --- | --- |
| F-001 | 双击 Ctrl 呼出 | X11 原始按键监听；Wayland Portal 或 DDE 适配 | P0 |
| F-002 | 自定义呼出热键 | DDE keybinding、X11 Grab、Wayland Portal | P0 |
| F-003 | 轻量启动器 | 预创建隐藏窗口，呼出即聚焦 | P0 |
| F-004 | 文件名即时搜索 | 内存索引和 Top-K 排序 | P0 |
| F-005 | 文件夹搜索 | 与文件统一索引，单独类型权重 | P0 |
| F-006 | 应用启动 | 扫描 Desktop Entry，遵循 OnlyShowIn 等规则 | P0 |
| F-007 | 模糊匹配 | 候选召回加精排 | P0 |
| F-008 | 中文拼音搜索 | 全拼、首字母、混合中文拼音 | P0 |
| F-009 | 使用习惯排序 | 本地衰减使用分与上下文分 | P0 |
| F-010 | 路径关键词 | 支持 in、路径片段和斜杠语法 | P0 |
| F-011 | 内置过滤器 | 文件、文件夹、文档、图片、视频、音频 | P0 |
| F-012 | 自定义过滤器 | 扩展名、目录、附加查询组合 | P1 |
| F-013 | 深度搜索窗口 | 可调整尺寸、表格结果、侧栏筛选 | P0 |
| F-014 | 多搜索窗口 | 独立查询状态、共享索引 | P1 |
| F-015 | 修改日期筛选 | 今天、本周、本月、自定义范围 | P0 |
| F-016 | 文件大小筛选 | 范围和比较表达式 | P1 |
| F-017 | 预览面板 | 文本、图片、PDF、音视频元信息 | P1 |
| F-018 | 搜索历史 | 查询历史和打开历史 | P0 |
| F-019 | 文件收藏 | 文件、目录、应用收藏 | P0 |
| F-020 | 本地目录索引 | 多根目录、排除规则、重建 | P0 |
| F-021 | 网络目录索引 | SMB、NFS、GVfs 和手工挂载路径 | P1 |
| F-022 | 离线网络结果 | 可配置保留并标记离线 | P1 |
| F-023 | 优先级规则 | 高、普通、低、忽略 | P1 |
| F-024 | 正则优先级 | 文件名、路径正则规则 | P1 |
| F-025 | Quick Switch | DDE、GTK、Qt 和 Portal 对话框适配 | P1 |
| F-026 | 推荐目录 | 最近目录、当前文件管理器目录、上下文排序 | P1 |
| F-027 | Ctrl+G 快速跳转 | 可配置并支持候选第一项 | P1 |
| F-028 | 网页搜索 | 关键词、URL 模板和浏览器打开 | P0 |
| F-029 | 搜索建议 | 可配置建议提供商 | P1 |
| F-030 | 自定义命令 | 参数模板、工作目录、静默执行 | P0 |
| F-031 | 管理员命令 | Polkit 提权，不让主进程提权 | P1 |
| F-032 | 文件动作 | 打开目录、复制路径、复制、移动、删除 | P0 |
| F-033 | 自定义动作 | 参数模板、范围、热键 | P1 |
| F-034 | 系统上下文菜单 | MIME 应用与 DDE 文件操作桥接 | P1 |
| F-035 | 收藏菜单 | 文件夹、命令、分组、子菜单和分隔线 | P1 |
| F-036 | 空白双击菜单 | DDE 文件管理器集成 | P1 |
| F-037 | 中键菜单 | DDE 文件管理器集成 | P1 |
| F-038 | 直接输入搜索 | DDE 文件管理器适配器 | P1 |
| F-039 | 文件拖放 | 从结果拖向桌面、应用和对话框 | P1 |
| F-040 | 全屏免打扰 | 检测全屏活动窗口并暂时禁用呼出 | P0 |
| F-041 | 主题 | 跟随系统、浅色、深色、玉石毛玻璃 | P0 |
| F-042 | 多语言 | 简体中文为主，预留 gettext/Fluent | P1 |
| F-043 | 登录自启动 | systemd --user 与 XDG Autostart | P0 |
| F-044 | 右下角托盘 | StatusNotifierItem，状态和菜单 | P0 |
| F-045 | 后台索引 | 独立守护进程、增量更新、节流 | P0 |
| F-046 | 暂停索引 | 15 分钟、1 小时、直到手工恢复 | P0 |
| F-047 | 索引诊断 | 状态、缺失路径、溢出、权限和重建 | P1 |
| F-048 | 设置导入导出 | JSON/TOML 版本化备份 | P1 |
| F-049 | CLI 与 URI | ruyi 命令及 ruyiseek:// 协议 | P1 |
| F-050 | 内容搜索 | 独立可选插件，不影响文件名索引 | P2 |

完整对等验收的判断标准不是“界面上存在入口”，而是对应能力必须通过第 22 章的场景测试。

---

## 6. 总体系统架构

### 6.1 进程划分

如意寻采用“索引守护进程 + 常驻交互进程 + 可选集成宿主”的结构。

~~~mermaid
flowchart TD
    UI["ruyiseek-ui\n启动器、窗口、托盘"] --> IPC["本地 IPC\nUnix Socket + D-Bus"]
    IPC --> CORE["ruyiseekd\n查询、排序、状态"]
    CORE --> INDEX["索引存储\n快照、日志、内存映射"]
    CORE --> WATCH["文件监视\ninotify / DDE Adapter"]
    UI --> INTEG["集成层\n热键、对话框、文件管理器"]
    INTEG --> DESKTOP["DDE / X11 / Wayland / AT-SPI"]
~~~

#### ruyiseekd

- 只在当前用户会话中运行。
- 负责扫描、索引、查询、排序、历史、收藏、动作定义和状态持久化。
- 不创建窗口，不依赖显示服务器。
- 崩溃后由 systemd --user 自动恢复。
- 暴露受限的本地 IPC，不监听 TCP 端口。

#### ruyiseek-ui

- 负责右下角托盘、全局热键状态机和所有可见窗口。
- 登录时启动并常驻。
- 提前创建启动器窗口并隐藏，避免每次呼出重新加载。
- 断开守护进程时仍可显示错误状态、设置页和修复入口。

#### ruyiseek-integration-host

- 仅在需要文件管理器或文件对话框插件时加载。
- 隔离不同工具包适配器崩溃。
- 通过能力声明报告当前环境支持的功能。
- 可以按 DDE、GTK、Qt、Portal 分成独立动态组件。

### 6.2 Rust 工作区

建议仓库结构：

    ruyiseek/
    ├── Cargo.toml
    ├── crates/
    │   ├── ruyiseek-core/
    │   ├── ruyiseek-index/
    │   ├── ruyiseek-query/
    │   ├── ruyiseek-rank/
    │   ├── ruyiseek-storage/
    │   ├── ruyiseek-ipc/
    │   ├── ruyiseek-platform/
    │   ├── ruyiseek-integrations/
    │   ├── ruyiseek-actions/
    │   ├── ruyiseek-preview/
    │   └── ruyiseek-plugin-sdk/
    ├── apps/
    │   ├── ruyiseekd/
    │   ├── ruyiseek-ui/
    │   ├── ruyi-cli/
    │   └── integration-host/
    ├── ui/
    │   ├── launcher.slint
    │   ├── deep-search.slint
    │   ├── quick-switch.slint
    │   ├── settings.slint
    │   └── components/
    ├── packaging/
    │   ├── debian/
    │   ├── systemd-user/
    │   ├── dbus/
    │   ├── desktop/
    │   └── icons/
    ├── tests/
    │   ├── corpus/
    │   ├── integration/
    │   ├── performance/
    │   └── visual/
    └── docs/

### 6.3 UI 技术选型

主 UI 建议使用 Rust + Slint，而不是 WebView：

- 二进制体积和启动速度可控。
- 可以使用 GPU 或软件渲染。
- 声明式 UI 适合快速迭代和多主题。
- 核心、IPC 和 UI 共享 Rust 类型。
- 不依赖浏览器运行时。

DDE 特有的窗口模糊、托盘、激活、主题和快捷键能力放在 ruyiseek-platform 中，通过 X11、D-Bus、Portal 和桌面协议实现，不把平台逻辑散落到 Slint 组件。

如果某一 UOS 版本的 Slint 窗口行为无法满足要求，可以保留 Qt/DTK 前端实现，但不得改动核心 IPC。这样 UI 前端可以替换，而索引与功能不会重写。

---

## 7. 用户体验与界面规范

### 7.1 轻量启动器

#### 默认状态

- 默认宽度 720 px。
- 搜索框高度 64 px。
- 位于当前活动屏幕上方约 22% 处。
- 圆角 16 px。
- 无系统标题栏。
- 输入为空时只展示搜索框、提示和少量最近项目。
- 输入后向下展开，最多展示 9 个结果。
- 最大展开高度约 560 px。

#### 视觉层级

1. 搜索图标或当前模式图标。
2. 输入文字。
3. 当前过滤器、命令或网页搜索提供商标签。
4. 结果名称。
5. 父路径或说明。
6. 右侧动作提示和快捷键。

#### 行为

- 双击 Ctrl：显示或隐藏启动器。
- Enter：打开第一项或当前项。
- Ctrl+Enter：打开所在目录并选中文件。
- Ctrl+N / Ctrl+P 或方向键：移动选择。
- Tab：在文件、应用、命令、网页模式之间切换。
- Ctrl+O 或右方向键：打开动作菜单。
- Alt+P：显示或隐藏预览。
- Ctrl+Ctrl：启动器已打开时切换至深度搜索窗口。
- Escape：清空查询；再次按下关闭。该行为可配置为一次直接关闭。

#### 窗口生命周期

窗口在 UI 进程启动时创建，在首次显示前完成字体、主题和组件初始化。隐藏时不销毁。显示路径不得触发磁盘扫描、网络请求或设置文件读取。

### 7.2 深度搜索窗口

#### 布局

~~~mermaid
flowchart LR
    FILTER["左侧筛选\n类型、日期、根目录"] --> RESULT["中央结果\n名称、路径、大小、时间"]
    RESULT --> PREVIEW["右侧预览\n可折叠"]
~~~

- 默认尺寸 980×680 px。
- 可调整尺寸并记忆每个显示器的上次位置。
- 支持名称、路径、大小、类型、修改时间列。
- 支持列表和紧凑表格视图。
- 左侧筛选栏可折叠。
- 右侧预览栏可折叠。
- 支持打开多个独立窗口。
- 数百万结果使用虚拟列表，不创建等量 UI 节点。

#### 筛选器

- 对象：全部、文件、文件夹、应用。
- 类型：文档、图片、视频、音频、压缩包、代码、自定义。
- 修改时间：今天、昨天、过去 7 天、过去 30 天、自定义。
- 大小：空文件、小于 1 MB、1～100 MB、大于 100 MB、自定义。
- 根目录：本地磁盘、主目录、网络位置和自定义索引根。
- 优先级：常用结果、包含低优先级结果。
- 可见性：隐藏文件、系统路径、离线网络文件。

#### 批量操作

- Ctrl/Shift 多选。
- 打开、复制路径、复制到、移动到、放入回收站。
- 拖放到文件管理器、桌面或支持文件 URI 的应用。
- 多选时仅展示对所有选中对象都有效的动作。

### 7.3 Quick Switch 快速切换窗口

当打开或保存对话框获得焦点时，如意寻显示轻量辅助栏：

- 默认不抢占文件名输入框焦点。
- 展示当前目录、推荐目录、最近目录和收藏目录。
- 可直接输入目录关键词。
- Ctrl+G 跳转到第一推荐目录。
- 选择目录后更新原文件对话框，而不是额外打开文件管理器。
- 对话框关闭时辅助栏立即关闭。

推荐目录排序依据：

1. 最近活动的 DDE 文件管理器目录。
2. 最近通过如意寻打开的目录。
3. 当前应用上一次使用的目录。
4. 全局最近目录。
5. 收藏目录。

### 7.4 收藏菜单

收藏菜单支持：

- 文件夹。
- 文件。
- 应用。
- 自定义命令。
- 网页地址。
- 子菜单。
- 分隔线。
- 拖放排序。

显示方式：

- 启动器中的菜单按钮。
- 独立自定义热键。
- 文件管理器空白区域双击。
- 文件管理器空白区域中键。
- 托盘右键菜单的收藏子菜单。

### 7.5 设置窗口

设置按以下页面组织：

1. 常规。
2. 搜索与排序。
3. 索引范围。
4. 过滤器。
5. 优先级。
6. 快捷键。
7. 快速切换。
8. 收藏菜单。
9. 命令。
10. 网页搜索。
11. 动作。
12. 文件管理器与应用集成。
13. 外观。
14. 备份与恢复。
15. 诊断。
16. 关于。

设置必须即时校验。可能造成大量重建的设置要先显示影响范围，再由用户确认执行。

---

## 8. 双击 Ctrl 与全局快捷键

### 8.1 识别规则

双击 Ctrl 必须以完整的“按下—释放”作为一次轻触。默认参数：

| 参数 | 默认值 | 可配置范围 |
| --- | ---: | ---: |
| 单次最长按住时间 | 220 ms | 100～500 ms |
| 两次轻触最大间隔 | 320 ms | 180～600 ms |
| 左右 Ctrl 是否等价 | 是 | 是/否 |
| 第二次按下时触发 | 否 | 不建议开放 |
| 全屏应用中禁用 | 是 | 是/否 |

以下情况必须取消识别：

- 两次 Ctrl 之间按下其他非修饰键。
- Ctrl 按住超过阈值。
- 发生 Ctrl+C、Ctrl+V 等组合键。
- 键盘自动重复事件。
- 会话处于锁屏或切换用户状态。
- 当前全屏应用在免打扰列表。

### 8.2 X11 实现

- 使用 XInput2 RawKeyPress 和 RawKeyRelease。
- 监听而不主动抓取 Ctrl，保证前台应用正常收到按键。
- 使用 XKB 将 keycode 映射至 Control_L 和 Control_R。
- 使用单调时钟，不使用系统墙上时间。
- 活动窗口通过 EWMH 属性获得。
- 全屏状态通过 _NET_WM_STATE_FULLSCREEN 判断。

禁止使用：

- 轮询 xdotool。
- 读取所有 /dev/input/event 设备。
- 要求用户加入 input 组。
- 让 UI 主线程处理 X11 事件。

### 8.3 Wayland 实现

优先顺序：

1. XDG Global Shortcuts Portal。
2. DDE keybinding D-Bus 接口。
3. DDE compositor 插件。
4. 退化为用户设置的组合快捷键。

标准 Portal 并未定义“双击修饰键”这种快捷键。如果 DDE Portal 接受单独 Ctrl，则应用根据 Activated 和 Deactivated 信号自行完成双击状态机；如果拒绝，则必须使用 DDE 扩展，不能通过键盘记录或隐藏输入设备绕过 Wayland 安全模型。

### 8.4 自定义快捷键

至少支持：

- 显示启动器。
- 显示深度搜索。
- 显示收藏菜单。
- Quick Switch。
- 暂停/恢复索引。
- 每一个命令。
- 每一个网页搜索。
- 每一个动作。

设置快捷键时必须检测冲突，并显示冲突对象。Wayland 下以桌面 Portal 最终确认的快捷键为准。

---

## 9. 文件索引系统

### 9.1 后端抽象

定义统一接口 IndexBackend：

    trait IndexBackend {
        async fn capabilities(&self) -> Capabilities;
        async fn roots(&self) -> Vec<IndexRoot>;
        async fn search(&self, request: SearchRequest) -> SearchPage;
        async fn resolve(&self, id: EntryId) -> Option<Entry>;
        async fn rebuild(&self, root: RootId) -> JobId;
        async fn pause(&self, scope: PauseScope);
        async fn subscribe(&self) -> IndexEventStream;
    }

实现：

- DeepinAnythingBackend：调用 UOS 已有服务。
- NativeIndexBackend：自主 Rust 索引。
- LocateBackend：故障时的只读降级后端。
- CompositeBackend：合并本地、网络、应用和插件结果。

所有后端返回统一 Entry，不允许 UI 依赖后端私有结构。

### 9.2 原生索引记录

文件记录建议包含：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| entry_id | u64 | 如意寻内部稳定标识 |
| root_id | u32 | 索引根 |
| parent_id | u64 | 父目录记录 |
| device_id | u64 | 文件系统设备 |
| inode | u64 | inode |
| raw_name_offset | u64 | 原始名称字节偏移 |
| raw_name_length | u32 | 原始名称长度 |
| normalized_offset | u64 | 规范化名称偏移 |
| kind | u8 | 文件、目录、链接、应用等 |
| flags | u32 | 隐藏、离线、不可访问等 |
| extension_id | u32 | 扩展名词典编号 |
| size | u64 | 字节数 |
| mtime_ns | i64 | 修改时间 |
| ctime_ns | i64 | 元数据变更时间 |

Linux 文件名不保证是合法 UTF-8。索引必须保存原始字节；UI 使用可逆转义或损失显示，并保留原始字节用于打开文件。

### 9.3 索引文件

    ~/.local/share/ruyiseek/
    ├── index/
    │   ├── manifest.json
    │   ├── root-0001.current
    │   ├── root-0001.previous
    │   ├── root-0001.journal
    │   └── dictionaries/
    ├── state.db
    ├── previews/
    └── logs/

快照要求：

- 分区或索引根独立快照。
- 文件头包含版本、端序、根路径、记录数和校验和。
- 写入新快照后 fsync，再原子 rename。
- 保留上一份可用快照。
- journal 采用长度前缀、序列号和 CRC。
- 读取失败时自动回退 previous，而不是直接删除全部索引。

### 9.4 初始扫描

- 使用受控并行目录遍历。
- SSD 默认较高并发，机械硬盘和网络目录降低并发。
- 前台出现交互 I/O 时自动降速。
- 默认排除 /proc、/sys、/dev、/run 和临时挂载。
- 对每个挂载点单独建根，避免跨文件系统误扫。
- 符号链接默认不跟随。
- bind mount 和循环挂载通过 device/inode 集合检测。
- 进度以已扫描目录数、文件数和当前根展示。

### 9.5 增量更新

原生后端优先使用 inotify：

- 新建目录后立即添加 watch。
- rename 使用 cookie 配对。
- 队列溢出后标记根为 Dirty 并执行差异扫描。
- 删除 watch 不等同于删除根，需检查卸载和权限变化。
- suspend/resume 后进行快速一致性检查。

大规模目录可能触及 inotify watch 上限。如意寻不得静默修改系统 sysctl。诊断页应显示当前使用量和建议值，并允许用户复制管理员命令。

在 UOS 可用时，可以通过 DeepinAnythingBackend 接收 deepin-anything 的变更流。该后端不可成为唯一实现。

### 9.6 网络目录

支持：

- 已挂载 SMB/CIFS。
- 已挂载 NFS。
- GVfs 可解析路径。
- 用户手工指定的其他目录。

策略：

- 网络根默认使用低扫描并发。
- 支持 15 分钟、1 小时、每天和手工重新扫描。
- 断网后保留索引但标记离线，是否显示由设置决定。
- 网络恢复时依据根指纹、mtime 和抽样校验决定差异扫描或全量扫描。
- 不保存网络凭据，使用系统挂载和密钥环。

### 9.7 排除规则

支持：

- 精确路径。
- 路径前缀。
- glob。
- 文件名正则。
- 隐藏文件。
- 文件大小阈值。
- 文件系统类型。

默认建议排除：

- .git/objects
- node_modules/.cache
- 浏览器缓存
- 缩略图缓存
- 回收站

默认仅建议，不强制替用户排除项目文件。

---

## 10. 查询语言

### 10.1 基础语义

普通空格分隔的多个词默认执行 AND；词序不敏感，但连续匹配和原顺序匹配得分更高。

示例：

| 查询 | 含义 |
| --- | --- |
| 报告 | 名称包含或模糊匹配“报告” |
| 年度 报告 | 同时匹配“年度”和“报告” |
| azb | 匹配“安装包”等拼音首字母 |
| an zhuang bao | 匹配中文拼音 |
| photo doc/ | 名称匹配 photo，父路径匹配 doc |
| in:项目 报告 | 在“项目”路径中搜索报告 |
| folder: 项目 | 只搜索文件夹 |
| doc: 年度 | 只搜索文档 |

### 10.2 Listary 6 对等过滤器

- folder:
- file:
- doc:
- pic:
- video:
- audio:

过滤器可以出现在查询任意位置。中文全角冒号输入后应自动规范化。

### 10.3 如意寻扩展语法

| 语法 | 示例 | 含义 |
| --- | --- | --- |
| ext | ext:pdf,docx | 扩展名集合 |
| in | in:~/项目 | 限定路径 |
| root | root:工作盘 | 指定索引根 |
| after | after:2026-01-01 | 修改时间下界 |
| before | before:2026-08-01 | 修改时间上界 |
| size | size:>100MB | 文件大小 |
| exact | "季度报告" | 精确短语 |
| exclude | -临时 | 排除词 |
| OR | pdf \| docx | 逻辑或 |
| hidden | hidden:true | 包含隐藏文件 |
| offline | offline:true | 包含离线网络结果 |

解析器必须生成 AST，不允许通过字符串拼接实现过滤。无效语法显示局部错误，同时尽可能保留普通文本搜索。

### 10.4 模式前缀

| 前缀 | 模式 |
| --- | --- |
| > | 命令 |
| = | 计算器 |
| ? | 网页搜索选择 |
| / 或 ~ | 路径导航 |
| @ | 插件或提供商 |

前缀模式只是快速入口，普通关键词仍可以自动召回命令和应用。

---

## 11. 搜索与排序算法

### 11.1 两阶段搜索

第一阶段召回：

- 精确前缀表。
- token 词典。
- Unicode n-gram 倒排。
- 拼音全拼索引。
- 拼音首字母索引。
- 最近与收藏的内存小表。

第二阶段精排：

- 名称匹配质量。
- 路径匹配质量。
- 连续性。
- 词序。
- 类型。
- 使用历史。
- 最近使用。
- 手工优先级。
- 当前应用和当前目录上下文。
- 隐藏、离线和低优先级惩罚。

禁止对数百万条记录逐一执行编辑距离。

### 11.2 基础评分

对候选对象 i 和查询 q 定义：

$$
S(i,q)=
w_n N(i,q)+
w_p P(i,q)+
w_c C(i,q)+
w_o O(i,q)+
w_u U(i)+
w_r R(i)+
w_x X(i)-
\Pi(i)
$$

其中：

- $N(i,q)$：文件名匹配分。
- $P(i,q)$：父路径匹配分。
- $C(i,q)$：连续字符奖励。
- $O(i,q)$：查询词顺序奖励。
- $U(i)$：历史使用分。
- $R(i)$：手工优先级规则。
- $X(i)$：当前上下文分。
- $\Pi(i)$：隐藏、离线、低优先级等惩罚。

所有子分量归一化至 $[0,1]$ 或固定有限区间，防止某个来源因量纲不同压倒其他来源。

### 11.3 使用历史衰减

使用分不能只累计次数，否则早期常用文件会永久占据首位。对对象 i 的一次使用事件 j：

$$
U_i(t)=
\log(1+c_i)+
\alpha\sum_j a_j\exp\left(
-\frac{\ln 2}{h}(t-t_{ij})
\right)
$$

其中：

- $c_i$ 是累计使用次数。
- $a_j$ 是打开、定位、执行动作等事件权重。
- $h$ 是半衰期，默认 14 天。
- $t_{ij}$ 是事件时间。

实际实现对事件做日级聚合，避免无限增长。

### 11.4 拼音匹配

为每个包含汉字的名称生成：

- 原始中文。
- 无声调全拼。
- 每个汉字首字母。
- 中文与 ASCII 分段。

“安装包”可生成：

- anzhuangbao
- azb
- an zhuang bao

多音字默认保留常见读音，候选量受限；用户成功打开结果后，可以记录该查询与目标之间的别名，不修改全局词典。

### 11.5 稳定性

相同查询和相同状态必须稳定排序。最终 tie-break 顺序：

1. 总分。
2. 名称精确度。
3. 使用时间。
4. 路径长度。
5. 规范化路径字节序。
6. entry_id。

### 11.6 Top-K

启动器只需要前 9～50 项，使用固定容量最小堆。深度搜索返回游标分页，不一次传输全部结果。

分页游标包含：

- 查询版本。
- 快照版本。
- 最后一项排序键。
- 过滤器摘要。

索引更新导致游标失效时返回明确状态，UI 保持当前选择并重新查询。

---

## 12. 应用启动

### 12.1 Desktop Entry

扫描：

- /usr/share/applications
- /usr/local/share/applications
- ~/.local/share/applications
- XDG_DATA_DIRS 中的 applications

处理字段：

- Name、GenericName、Comment。
- Localized Name。
- Exec。
- Icon。
- Keywords。
- Categories。
- Hidden、NoDisplay。
- OnlyShowIn、NotShowIn。
- Terminal。
- TryExec。

Exec 必须按 Desktop Entry 规范解析字段码，不能直接交给 shell。

### 12.2 应用排序

- 精确名称。
- 中文本地化名称。
- 英文名称。
- Keywords。
- 使用频率。
- 当前工作区上下文。

同一应用的多个 Desktop Entry 应按 desktop file ID 去重。

### 12.3 启动隔离

启动应用时：

- 清理如意寻私有环境变量。
- 保留正常桌面会话变量。
- 不把守护进程文件描述符泄漏给子进程。
- Terminal=true 时通过用户配置的终端启动。
- 记录成功启动，不把创建进程即视为长期成功。

---

## 13. Quick Switch 与桌面集成

### 13.1 适配器接口

    trait FileDialogAdapter {
        fn probe(&self, window: &WindowContext) -> ProbeResult;
        async fn current_folder(&self) -> Option<PathBytes>;
        async fn navigate(&self, folder: &PathBytes) -> Result<()>;
        async fn set_filename(&self, name: &OsStr) -> Result<()>;
        async fn close_overlay(&self);
    }

每个适配器必须声明：

- 支持 X11、Wayland 或两者。
- 能否读取当前目录。
- 能否导航。
- 能否设置文件名。
- 能否获取宿主应用 ID。
- 是否需要辅助功能权限。

### 13.2 DDE、Qt 和 GTK

优先使用 AT-SPI2 的可访问性对象树：

- 识别 ROLE_FILE_CHOOSER、ROLE_DIALOG 和工具包特征。
- 查找位置输入框、面包屑和文件列表。
- 通过可访问性动作设置位置，而不是模拟鼠标坐标。
- 工具包升级导致节点变化时，通过语义角色而非固定层级匹配。

适配顺序：

1. DDE/DTK 文件对话框专用适配器。
2. GTK FileChooser 适配器。
3. Qt QFileDialog 适配器。
4. XDG Portal 文件对话框适配器。
5. 通用“打开目录并提示用户”降级模式。

### 13.3 X11 覆盖层

X11 下可以读取目标窗口位置，将 Quick Switch 辅助栏停靠在对话框下方。要求：

- 对话框移动和缩放时跟随。
- 多显示器 DPI 正确。
- 对话框最小化、关闭或失焦时隐藏。
- 辅助栏不得出现在锁屏和其他用户会话。

### 13.4 Wayland 限制与正式方案

Wayland 普通客户端不能任意读取或设置其他窗口坐标，也不能监听全部键盘事件。正式兼容方案分两层：

- 通用版本：通过 AT-SPI 控制对话框内容，辅助窗口在当前屏幕固定位置显示。
- UOS 深度集成版本：扩展 DDE Portal FileChooser 后端或安装 DDE 受支持的插件，使 Quick Switch 成为文件对话框的一部分。

不得宣传未实际支持的“全局贴附”。设置页按当前会话显示能力矩阵。

### 13.5 文件管理器直接输入搜索

在 DDE 文件管理器内，当焦点不在文本框、重命名框和终端时，直接输入字符可显示如意寻小搜索栏。

X11：

- 监听活动窗口和原始按键。
- 通过 AT-SPI 判断焦点角色。
- 仅将字符复制给如意寻 UI，不吞掉系统快捷键。

Wayland：

- 通过 DDE 文件管理器插件实现。
- 没有插件时关闭该能力，不使用输入设备绕过。

### 13.6 当前目录同步

集成层维护 ActiveFolderContext：

| 来源 | 可信度 |
| --- | ---: |
| DDE 文件管理器插件报告 | 1.00 |
| 文件对话框适配器读取 | 0.95 |
| 如意寻打开所在目录 | 0.90 |
| 终端 OSC 7 | 0.85 |
| AT-SPI 面包屑读取 | 0.75 |
| 窗口标题推断 | 0.30 |

低可信度上下文只能用于排序提示，不能直接驱动文件操作。

---

## 14. 命令系统

### 14.1 内置命令

Linux 对等命令建议：

| 关键词 | 功能 |
| --- | --- |
| opt | 打开设置 |
| .. | 上级目录 |
| mkdir | 在当前目录创建文件夹 |
| touch | 在当前目录创建文件 |
| term | 在当前目录打开终端 |
| terma | 通过 Polkit 以管理员方式打开任务 |
| shutdown | 关机，二次确认 |
| reboot | 重启，二次确认 |
| apps | 打开应用管理 |
| network | 打开网络设置 |
| hosts | 使用管理员流程编辑 hosts |
| index | 打开索引状态 |
| rebuild | 重建指定索引根 |
| pause | 暂停索引 |

破坏性或系统级命令必须明确确认，不能因模糊匹配后按 Enter 直接执行。

### 14.2 自定义命令字段

- ID。
- 启用状态。
- 关键词。
- 标题。
- 图标。
- 可执行文件。
- argv 参数模板。
- 工作目录模板。
- 环境变量白名单。
- 是否静默。
- 是否需要 Polkit。
- 适用范围。
- 可选热键。
- 超时。

模板变量：

- {query}
- {current_folder}
- {selected_path}
- {selected_parent}
- {clipboard_text}
- {home}

### 14.3 执行安全

- 默认使用 execve 风格参数数组。
- 默认禁止 shell 字符串。
- 用户明确启用 Shell 模式时显示高风险标识。
- 参数替换后不做二次 shell 展开。
- 提权由一次性 helper 和 Polkit 完成。
- 主进程永远不以 root 身份重启。
- 记录退出码、耗时和错误，不默认记录敏感参数内容。

---

## 15. 动作系统

### 15.1 内置动作

- 打开。
- 打开所在目录并选中。
- 使用指定应用打开。
- 复制路径。
- 复制文件 URI。
- 复制名称。
- 复制。
- 剪切。
- 移动到。
- 复制到。
- 重命名。
- 放入回收站。
- 永久删除，必须二次确认。
- 收藏或取消收藏。
- 在终端打开目录。
- 查看属性。
- 计算校验和。

### 15.2 动作范围

- 任何应用。
- 如意寻。
- DDE 文件管理器。
- 文件对话框。
- 仅文件。
- 仅目录。
- 指定 MIME。
- 单选或多选。

### 15.3 自定义动作

模板变量：

- {action_path}
- {action_parent}
- {action_name}
- {action_uri}
- {action_paths}
- {current_folder}

多选参数必须作为多个 argv 项传递，不拼接为一个可能被错误解析的字符串。

### 15.4 删除语义

普通删除默认调用 freedesktop Trash 语义。跨文件系统失败时不得回退为永久删除。永久删除必须：

1. 明确显示对象数量和总大小。
2. 显示不可恢复提示。
3. 用户再次确认。
4. 失败项逐项报告。

---

## 16. 网页搜索

### 16.1 使用方式

输入网页搜索关键词和空格进入提供商模式，例如：

- b UOS：百度搜索 UOS。
- g Rust Slint：Google 搜索。
- wiki 文件系统：维基百科搜索。

### 16.2 提供商模型

- 关键词。
- 名称。
- URL 模板。
- 图标。
- 建议提供商。
- 建议 URL。
- JSONPath 或解析器。
- 地区和语言。
- 是否启用。
- 可选热键。

URL 模板使用 {query} 占位并进行正确的百分号编码。

### 16.3 默认提供商

中国区默认建议：

- 百度。
- 必应。
- 搜狗。
- 知乎。
- 哔哩哔哩。
- Gitee。
- GitHub。
- Stack Overflow。
- 维基百科。

默认列表可以按安装语言和地区调整，但不得在后台自动发起建议请求。只有用户进入具体网页搜索模式后才请求建议。

---

## 17. 收藏、历史与优先级

### 17.1 历史类型

- 查询历史。
- 打开历史。
- 目录历史。
- 应用启动历史。
- 命令执行历史。
- 动作历史。
- Quick Switch 历史。

历史用于排序，但不同事件权重不同。

### 17.2 收藏

收藏对象使用稳定引用：

- 本地文件优先保存 device/inode 和最后路径。
- 网络文件保存根 ID 和相对路径。
- Desktop Entry 保存 desktop file ID。
- 命令和网页搜索保存配置对象 ID。

路径变化后可通过 inode 或索引 rename 事件修复收藏。

### 17.3 优先级

四级定义：

- High：固定排序提升。
- Normal：默认。
- Low：默认折叠到低优先级区域。
- Ignored：不出现在普通搜索。

规则支持目录和正则。冲突时：

1. 更具体路径优先。
2. 精确对象优先于正则。
3. 用户规则优先于默认规则。
4. 同级以最新显式规则优先。

Ignored 只影响如意寻结果，不修改文件权限。

---

## 18. 右下角托盘与后台运行

### 18.1 托盘协议

使用 StatusNotifierItem D-Bus 协议，适配 DDE 托盘。必要时提供 AppIndicator 兼容层。

状态图标：

- 正常：玉石青。
- 索引中：蓝色小点或轻微旋转。
- 暂停：黄色暂停点。
- 离线根：灰色提示点。
- 错误：红色提示点。

### 18.2 左键行为

默认左键打开或聚焦深度搜索窗口。设置可改为：

- 打开启动器。
- 打开收藏菜单。
- 不执行动作。

### 18.3 右键菜单

    打开如意寻
    打开深度搜索
    收藏
    最近打开
    ----------------
    索引状态：正常
    暂停索引 >
        15 分钟
        1 小时
        本次会话
    立即更新索引
    重建索引
    ----------------
    登录时自动启动  ✓
    设置
    诊断与日志
    关于
    ----------------
    退出界面
    完全退出

“退出界面”保留后台索引；“完全退出”同时停止当前用户的 UI 和守护进程。菜单文字必须明确，避免用户误以为关闭窗口等于退出。

### 18.4 systemd 用户服务

ruyiseekd.service：

    [Unit]
    Description=RuyiSeek Index and Search Service
    After=graphical-session.target

    [Service]
    Type=dbus
    BusName=io.github.ethanbird.RuyiSeek.Daemon
    ExecStart=/usr/libexec/ruyiseek/ruyiseekd
    Restart=on-failure
    RestartSec=2

    [Install]
    WantedBy=default.target

UI 使用独立服务或 DDE XDG Autostart。安装程序应检测当前 UOS 的用户级 systemd 支持情况，并选择单一启动方式，避免启动两份。

### 18.5 单实例

- 守护进程占有唯一 D-Bus 名称。
- UI 占有唯一 UI D-Bus 名称。
- 再次运行 ruyiseek 时向现有实例发送 ShowLauncher 或 ShowSearch。
- 托盘点击永远复用现有窗口。

### 18.6 空闲策略

后台空闲时：

- 无文件事件则不轮询目录。
- 不刷新不可见 UI。
- 不持续请求网络建议。
- 定时维护合并为一个低频任务。
- 笔记本电池模式降低索引并发。
- 系统进入休眠前提交 journal。

---

## 19. 配置与数据模型

### 19.1 配置文件

    ~/.config/ruyiseek/config.toml

保存：

- 一般开关。
- UI 主题。
- 热键。
- 索引根声明。
- 排除规则。
- 功能开关。
- 插件启用状态。

配置采用版本号和严格 schema。写入使用临时文件、fsync 和原子替换。

### 19.2 状态数据库

    ~/.local/share/ruyiseek/state.db

SQLite 开启 WAL。核心表：

    schema_migrations
    favorites
    recent_items
    usage_daily
    query_history
    index_roots
    index_jobs
    filters
    priority_rules
    commands
    actions
    web_providers
    menu_nodes
    aliases
    window_state
    integration_rules
    plugin_state

### 19.3 关键表定义

    CREATE TABLE favorites (
        id              TEXT PRIMARY KEY,
        target_kind     INTEGER NOT NULL,
        target_ref      BLOB NOT NULL,
        last_path       BLOB,
        title           TEXT,
        icon_ref        TEXT,
        created_at      INTEGER NOT NULL,
        sort_order      INTEGER NOT NULL
    );

    CREATE TABLE usage_daily (
        target_id       TEXT NOT NULL,
        day             INTEGER NOT NULL,
        open_count      INTEGER NOT NULL DEFAULT 0,
        reveal_count    INTEGER NOT NULL DEFAULT 0,
        action_count    INTEGER NOT NULL DEFAULT 0,
        last_used_at    INTEGER NOT NULL,
        PRIMARY KEY (target_id, day)
    );

    CREATE TABLE priority_rules (
        id              TEXT PRIMARY KEY,
        rule_kind       INTEGER NOT NULL,
        pattern         BLOB NOT NULL,
        priority        INTEGER NOT NULL,
        enabled         INTEGER NOT NULL,
        created_at      INTEGER NOT NULL,
        updated_at      INTEGER NOT NULL
    );

### 19.4 数据保留

- 使用历史默认长期保留并聚合。
- 查询文本默认保留最近 500 条，可配置关闭。
- 诊断日志默认滚动保留 14 天或 50 MB。
- 预览缓存按 LRU 和总大小限制清理。
- 删除索引根时询问是否保留该根的历史和收藏。

---

## 20. IPC 与扩展接口

### 20.1 内部 IPC

高频查询使用 Unix Domain Socket：

- 长度前缀二进制帧。
- MessagePack 或自定义稳定协议。
- 请求 ID。
- 协议版本。
- 取消请求。
- 流式分页。
- peer credential 校验，仅允许当前 UID。

D-Bus 用于：

- 显示窗口。
- 托盘与桌面激活。
- 系统快捷键。
- 外部自动化。
- 索引状态通知。

### 20.2 查询请求

    SearchRequest {
        request_id,
        query,
        mode,
        filters,
        context,
        limit,
        cursor,
        client_generation
    }

context 包含：

- 活动应用 ID。
- 当前目录。
- 文件对话框状态。
- 当前显示器。
- 是否允许网络结果。

响应：

    SearchResponse {
        request_id,
        snapshot_version,
        elapsed_us,
        results,
        next_cursor,
        warnings
    }

UI 对每次输入递增 client_generation，丢弃过期响应。

### 20.3 CLI

示例：

    ruyi
    ruyi search "年度报告"
    ruyi search "doc: 报告 in:~/工作"
    ruyi open ITEM_ID
    ruyi reveal ITEM_ID
    ruyi launcher
    ruyi deep-search
    ruyi index status
    ruyi index rebuild HOME
    ruyi index pause 1h
    ruyi config export backup.json

### 20.4 URI

- ruyiseek://show-launcher
- ruyiseek://show-search
- ruyiseek://search?q=...
- ruyiseek://reveal?id=...

危险动作不得通过未确认 URI 直接执行。

### 20.5 插件

插件类型：

- SearchProvider。
- PreviewProvider。
- ActionProvider。
- ContextProvider。
- IntegrationAdapter。

插件默认运行在单独进程。插件清单声明权限：

- 可读查询。
- 可读选中路径。
- 可访问网络。
- 可执行进程。
- 可写配置。

核心版本不要求插件市场。

---

## 21. 性能指标

以下指标是发布门槛，不是宣传估计。测试语料必须同时包含中文、英文、长路径、短文件名和多层目录。

### 21.1 交互延迟

| 指标 | P50 | P95 | 上限 |
| --- | ---: | ---: | ---: |
| 双击识别完成至首帧 | ≤ 16 ms | ≤ 35 ms | 60 ms |
| 输入至首批 9 项结果，100 万记录 | ≤ 8 ms | ≤ 20 ms | 40 ms |
| 输入至首批 9 项结果，500 万记录 | ≤ 12 ms | ≤ 35 ms | 60 ms |
| 启动器隐藏 | ≤ 16 ms | ≤ 25 ms | 50 ms |
| 深度窗口滚动帧率 | 60 FPS | 55 FPS | 不连续卡顿 |

### 21.2 资源

| 场景 | 目标 |
| --- | --- |
| UI 隐藏常驻内存 | ≤ 35 MB |
| 守护进程基础内存 | ≤ 30 MB |
| 100 万文件总内存 | ≤ 150 MB |
| 空闲 CPU | 平均低于 0.2% 单核 |
| 索引更新可见延迟 | 本地文件 P95 低于 500 ms |
| 设置和状态数据库 | 正常使用低于 100 MB |

### 21.3 初始索引

基准设备按 NVMe SSD、SATA SSD、机械盘、SMB 分开记录，不允许只给出最快设备数据。

目标参考：

- 100 万文件 NVMe 首次扫描：45 秒内。
- 100 万文件 SATA SSD：90 秒内。
- 扫描期间桌面前台操作无明显卡顿。
- 中断后可从已完成根或安全检查点恢复。

### 21.4 UI 线程约束

以下操作不得在 UI 线程执行：

- 文件扫描。
- MIME 深度探测。
- 图片和 PDF 解码。
- 网络建议。
- SQLite 写事务。
- 图标文件读取。
- shell 命令。

超过 8 ms 的 UI 线程任务记录到性能追踪。

---

## 22. 测试与验收

### 22.1 单元测试

- 双击 Ctrl 状态机。
- 查询词法和语法。
- Unicode 规范化。
- 拼音和首字母。
- 排序公式。
- 优先级冲突。
- 参数模板。
- Desktop Entry 解析。
- 原始非 UTF-8 路径。
- 索引快照和 journal 校验。
- SQLite 迁移。

双击 Ctrl 必测序列：

| 输入序列 | 期望 |
| --- | --- |
| Ctrl 下上，200 ms，Ctrl 下上 | 触发 |
| Ctrl 长按 1 秒，两次 | 不触发 |
| Ctrl+C，Ctrl | 不触发 |
| 左 Ctrl，右 Ctrl | 按设置决定 |
| Ctrl 自动重复 | 不触发 |
| 全屏应用内双击 | 默认不触发 |
| 锁屏时双击 | 不触发 |

### 22.2 索引语料

- 0 个文件。
- 1 万、10 万、100 万、500 万和 1000 万记录。
- 目录深度超过 100。
- 单目录 100 万子项。
- 中文简繁、日文、韩文、emoji。
- 非 UTF-8 名称。
- 同名不同路径。
- 符号链接环。
- bind mount。
- 权限拒绝。
- 文件持续重命名。
- inotify 队列溢出。
- 网络断开和恢复。

### 22.3 桌面环境矩阵

至少覆盖：

- 统信 UOS 当前稳定桌面，X11。
- 统信 UOS Wayland 会话。
- Deepin 当前稳定版。
- x86_64 和 aarch64。
- 单屏、双屏、不同 DPI 混合。
- 浅色、深色和高 DPI。

### 22.4 应用集成矩阵

- DDE 文件管理器。
- UOS 默认文件选择框。
- WPS Writer、Sheets、Presentation 和 PDF。
- LibreOffice。
- Chromium。
- Firefox。
- Qt 应用。
- GTK 应用。
- Electron 自定义文件对话框。
- 终端。

每个应用记录：

- Quick Switch 能否检测。
- 能否读取目录。
- 能否跳转。
- 覆盖层能否正确定位。
- Wayland 下的降级表现。
- 是否需要加入排除列表。

### 22.5 托盘与后台

- 登录后仅一个守护进程和一个 UI 实例。
- 托盘退出 UI 后索引按菜单语义继续或停止。
- UI 崩溃后自动恢复托盘。
- 守护进程崩溃后 UI 显示恢复状态。
- 休眠、唤醒后热键和索引仍可用。
- DDE shell 重启后托盘自动重新注册。
- 升级时不中断或破坏索引。

### 22.6 性能基准

基准输出实际值，不只输出相对提升：

- 索引时间。
- 每秒目录项。
- 索引大小。
- 常驻内存。
- 首次查询和重复查询延迟。
- 拼音查询延迟。
- 正则优先级开销。
- 多窗口并发查询。
- 文件事件洪峰恢复时间。

基准在 CI 保存历史趋势。任何核心指标回退超过 5% 必须标记。

### 22.7 视觉验收

- 100%、125%、150%、200% 缩放。
- 不同长度中文和英文。
- 长路径省略与悬浮显示。
- 输入法候选窗不被遮挡。
- 启动器出现时不闪白。
- 列表加载不跳动。
- 预览出现不改变搜索框位置。
- 毛玻璃关闭后仍有足够对比度。
- reduced motion 设置下禁用不必要动画。

---

## 23. 故障恢复与诊断

### 23.1 索引状态

每个索引根状态：

- Ready。
- Scanning。
- Updating。
- Paused。
- Offline。
- Dirty。
- PermissionDenied。
- Corrupted。
- Disabled。

### 23.2 自动恢复

- 快照校验失败：尝试 previous。
- journal 尾部损坏：截断至最后有效记录。
- inotify overflow：差异扫描。
- 网络离线：保留离线索引。
- 配置损坏：加载上次有效配置并提示。
- state.db 损坏：备份损坏文件，尝试 SQLite recover；索引文件不受影响。
- UI 无法连接 daemon：指数退避重连并提供重启按钮。

### 23.3 诊断页

展示：

- 当前版本和构建信息。
- 会话类型、DDE 版本、显示协议。
- 后端和能力矩阵。
- 索引根、记录数、快照大小和更新时间。
- inotify 使用量和上限。
- 最近失败任务。
- Wayland Portal 可用接口。
- 文件对话框适配器状态。
- 托盘注册状态。
- 数据库完整性检查。

支持导出脱敏诊断包。默认不包含用户完整查询、完整文件名和文件内容。

---

## 24. 安全边界

### 24.1 权限

- 默认只索引当前用户有权遍历的路径。
- 不以 root 建立全盘索引。
- 不保存网络密码。
- 不修改文件权限以完成搜索。
- 不静默修改 sysctl。
- 管理员动作使用最小化的一次性 Polkit helper。

### 24.2 IPC

- Unix Socket 位于 XDG_RUNTIME_DIR。
- 校验 peer UID。
- D-Bus 方法区分只读与有副作用操作。
- 外部 URI 不允许直接执行删除、关机和管理员命令。
- 请求大小、结果数量和字符串长度设上限。

### 24.3 命令和动作

- 默认无 shell。
- 执行前完成变量类型检查。
- 可执行文件使用绝对解析结果。
- 管理员命令显示完整程序和参数。
- 插件崩溃不能带崩 UI 或 daemon。

### 24.4 预览

复杂文档解析放在受限子进程：

- CPU 和内存上限。
- 超时。
- 只读文件描述符。
- 禁止网络。
- 禁止执行宏、脚本和嵌入对象。

---

## 25. 打包、安装与升级

### 25.1 交付格式

正式支持：

- UOS/Deepin 原生 DEB，x86_64。
- UOS/Deepin 原生 DEB，aarch64。

可选：

- AppImage，仅提供搜索和启动器能力。
- Flatpak，不承诺完整全盘索引和文件对话框集成。

需要深度桌面集成时必须推荐 DEB。

### 25.2 安装路径

    /usr/bin/ruyi
    /usr/bin/ruyiseek
    /usr/libexec/ruyiseek/ruyiseekd
    /usr/libexec/ruyiseek/integration-host
    /usr/share/applications/io.github.ethanbird.RuyiSeek.desktop
    /usr/share/icons/hicolor/
    /usr/share/dbus-1/services/
    /usr/lib/systemd/user/ruyiseekd.service
    /usr/share/ruyiseek/

### 25.3 升级

- 应用、协议、配置、SQLite 和索引格式分别版本化。
- 小版本升级尽量复用索引。
- 必须重建时先保留旧索引，后台重建完成后切换。
- 升级失败可回退程序且读取旧状态。
- 不在安装脚本中删除用户数据。

### 25.4 卸载

普通卸载保留用户配置和索引。提供单独命令：

    ruyi maintenance purge-user-data

执行前列出将删除的明确路径并二次确认。

### 25.5 许可证边界

deepin-anything 5.0.15 之后采用 GPL-3.0-or-later。如意寻若复制或链接其代码，需要遵守对应许可证。

建议：

- 自主核心采用独立实现。
- DeepinAnythingBackend 优先通过系统公开 IPC 使用。
- 需要链接 GPL 库的适配器作为独立 GPL 组件发布。
- 在正式确定许可证前完成依赖和链接方式审查。

---

## 26. 开发阶段

本文档不以“先做一个没有核心能力的演示”为目标。每个阶段都必须形成可运行、可测试的纵向切片。

### 阶段 A：常驻基础

- Rust workspace。
- ruyiseekd 和 ruyiseek-ui。
- D-Bus 单实例。
- StatusNotifierItem 托盘。
- systemd --user 和 DDE 自启动。
- Slint 启动器骨架。
- X11 双击 Ctrl。
- 基础设置页。

退出条件：

- 登录后后台稳定运行。
- 双击 Ctrl 可靠显示和隐藏窗口。
- 托盘和完全退出语义正确。

### 阶段 B：可用搜索

- DeepinAnythingBackend。
- Desktop Entry 应用索引。
- 文件、文件夹和应用统一搜索。
- 使用历史。
- 收藏。
- 打开与定位。
- 拼音和首字母。
- 性能基准框架。

退出条件：

- 100 万条记录首批结果达到性能门槛。
- 中文、英文、拼音查询可用。
- 结果打开、定位和历史排序正确。

### 阶段 C：原生索引

- 初始扫描。
- 索引快照。
- inotify 增量更新。
- 排除和索引根。
- 网络根。
- 损坏恢复和诊断。

退出条件：

- 无 deepin-anything 时仍完整工作。
- 断电式中断测试不破坏已完成索引。
- 文件变化在目标延迟内反映。

### 阶段 D：Listary 6 Pro 搜索对等

- 深度搜索窗口。
- 多窗口。
- 过滤器。
- 优先级。
- 预览。
- 动作。
- 命令。
- 网页搜索。
- 收藏菜单。
- 拖放。

退出条件：

- F-003 至 F-024、F-028 至 F-035 全部通过验收。

### 阶段 E：UOS 深度集成

- DDE 文件管理器上下文。
- 直接输入搜索。
- 空白双击和中键菜单。
- DDE/DTK Quick Switch。
- GTK、Qt 和 Portal 适配器。
- Wayland 热键与激活。
- 全屏免打扰。

退出条件：

- UOS 目标应用矩阵逐项通过。
- X11 达到完整体验。
- Wayland 对不支持项明确降级并给出原因。

### 阶段 F：发布品质

- x86_64/aarch64 DEB。
- 升级和卸载。
- 视觉回归。
- 压力与故障注入。
- 文档、首次引导、诊断导出。
- 发布签名和校验和。

---

## 27. 首次使用流程

1. 安装后启动如意寻。
2. 展示 3 步简短引导：
   - 双击 Ctrl 呼出。
   - 输入文件名、应用名或拼音。
   - Ctrl+Enter 打开所在目录。
3. 自动检测 deepin-anything。
4. 若系统后端可用，立即提供搜索并在后台准备原生索引。
5. 若不可用，询问索引主目录和常用工作目录。
6. 展示托盘图标和索引进度。
7. 让用户测试一次双击 Ctrl。
8. 根据会话类型说明当前 Quick Switch 能力。

引导不得强制用户注册账号或联网。

---

## 28. 默认设置

| 设置 | 默认值 |
| --- | --- |
| 双击 Ctrl | 开启 |
| 全屏免打扰 | 开启 |
| 登录自启动 | 开启 |
| 托盘图标 | 显示 |
| 搜索隐藏文件 | 关闭 |
| 搜索离线网络结果 | 关闭 |
| 使用历史排序 | 开启 |
| 查询历史 | 开启，最近 500 条 |
| 网络建议 | 仅进入网页搜索模式时 |
| Quick Switch | 能力可用时开启 |
| 文件管理器直接输入 | X11 可用时开启 |
| 空白双击菜单 | 默认关闭，避免改变习惯 |
| 空白中键菜单 | 默认开启 |
| 主题 | 跟随 UOS |
| 动画 | 开启，遵循 reduced motion |
| 自动更新索引 | 开启 |
| 低电量节流 | 开启 |

---

## 29. 完整发布验收清单

### 常驻

- [ ] 登录后托盘出现。
- [ ] 后台索引服务正常。
- [ ] 仅有一个 UI 和一个 daemon。
- [ ] 关闭窗口不退出后台。
- [ ] 完全退出停止所有当前用户进程。

### 呼出

- [ ] 双击 Ctrl 可靠。
- [ ] 不影响 Ctrl+C、Ctrl+V。
- [ ] 全屏默认不触发。
- [ ] 多显示器出现在活动屏幕。
- [ ] 输入框立即获得焦点。

### 搜索

- [ ] 文件。
- [ ] 文件夹。
- [ ] 应用。
- [ ] 中文。
- [ ] 拼音。
- [ ] 首字母。
- [ ] 路径。
- [ ] 过滤器。
- [ ] 优先级。
- [ ] 网络目录。
- [ ] 历史和收藏。

### 深度窗口

- [ ] 多列。
- [ ] 筛选。
- [ ] 虚拟滚动。
- [ ] 多窗口。
- [ ] 预览。
- [ ] 多选和拖放。

### 操作

- [ ] 打开。
- [ ] 定位。
- [ ] 复制路径。
- [ ] 回收站。
- [ ] 自定义动作。
- [ ] 自定义命令。
- [ ] 网页搜索。
- [ ] 收藏菜单。

### 快速切换

- [ ] DDE 文件对话框。
- [ ] GTK 文件对话框。
- [ ] Qt 文件对话框。
- [ ] WPS 文件对话框。
- [ ] Ctrl+G。
- [ ] 最近和收藏目录。
- [ ] Wayland 降级说明准确。

### 稳定性

- [ ] 索引损坏可恢复。
- [ ] inotify 溢出可恢复。
- [ ] 网络中断不崩溃。
- [ ] 休眠唤醒可恢复。
- [ ] DDE shell 重启后托盘恢复。
- [ ] 升级不丢设置和收藏。

### 性能

- [ ] 100 万记录指标达标。
- [ ] 500 万记录指标达标。
- [ ] 空闲 CPU 达标。
- [ ] 常驻内存达标。
- [ ] UI 无明显卡顿和闪白。

---

## 30. 结论

如意寻不是 FSearch 的换皮，也不是只带一个双击 Ctrl 的搜索框。它的完整产品边界是：

> 高速文件索引 + 全局启动器 + 深度搜索 + 文件对话框快速切换 + 文件管理器增强 + 动作与命令系统 + 右下角托盘与后台服务。

技术上应把“高频、低延迟”的搜索核心与“高耦合、易变化”的桌面集成分开：

- 搜索核心保持纯 Rust、可基准、可替换后端。
- UI 常驻且预热，保证呼出没有冷启动。
- X11 完整复刻双击 Ctrl 和窗口贴附。
- Wayland 通过 Portal 与 DDE 插件获得合法的系统级能力。
- deepin-anything 用于快速落地，但原生索引保证长期独立性。
- 托盘、后台、恢复和诊断从第一阶段就进入架构，而不是发布前补充。

当本文件的功能矩阵和验收清单全部通过时，如意寻才可称为“统信 UOS 上完整的 Listary 6 Pro 对等工具”。
