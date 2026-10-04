# nebula-lite

基于 [Pebrel 1.9.1](https://github.com/Kuddev/pebrel) 的 GPUI 栈裁出来的本地文本编辑器：左侧文件树、右侧**多标签**内容区，Markdown 与图片提供「源码 / 预览」切换，其余文件一律按文本打开。外观按**苹果风格的中性灰白 + 系统蓝**做（见「与 Pebrel 的关系」里那一条）。

**仅支持 Windows**，不做跨平台。以自用为主，已公开发布，代码按 `GPL-3.0-or-later` 开源（见 [`LICENSE`](LICENSE)）。

## 与 Pebrel 的关系

复用面**只取叶子能力**，没有复制终端/SSH/Lua/AI 钩子那些无关部分：

| 能力 | 来源（Pebrel 1.9.1） |
| --- | --- |
| **外观基准：macOS 观感的中性灰白 + 系统蓝**（外壳 `#f5f5f7`、纸面白、强调色 `#007aff`、分隔线 `#d2d2d7`；控件圆角 6px、面板 10px） | 用户明确要"简约、舒适"的苹果风格，所以 `src/theme.rs` 的 `APPLE` 直接照苹果浅色模式的系统色写。**这是有意偏离 Pebrel 的一处**：原先逐值照抄的 Paper 暖纸色调色板不再使用（历史见 AGENTS.md） |
| 调色板 → `gpui_component::Theme` 字段映射 | `nebula_app/src/gpui_shell/theme.rs` 的 `apply_skin_tokens`（**映射规则**照抄：哪些字段吃水洗、哪些吃实色有讲究；取的颜色换成了上面这份） |
| **编辑器首次解析走同步**（修掉"打开文件后几百毫秒才上色"） | 上游 `gpui-component` 的 `input/mode.rs`；本项目把该 crate 截取到 `vendor/` 并打了这一处补丁 |
| 字体槽位与顶置项的持久化 | `%APPDATA%\nebula-lite\settings.json`（本项目自己定的，无上游） |
| 资源管理器右键菜单 | `windows/install-context-menu.ps1`（只写 `HKCU\Software\Classes`，不需要管理员） |
| **编辑器代码配色（按语言）**（C 关键字深蓝、JS/Java 橙、Rust 绿、Python 深蓝…关键字粗体、运算符洋红、数字红、字符串绿、注释灰、白底 + 浅灰行号栏；`.cmd` 深蓝关键字；裸 URL 热点蓝） | **`[源码]Notepad3/src/StyleLexers/*.c` 的默认 `fore:` 值**（不是 Pebrel；外壳用上面那份苹果中性色，编辑器里的代码照 Notepad3，两者互不影响。**每个 lexer 一张表**，颜色随语言变；裸 URL 取自 `styleLexStandard.c` 的 "Hyperlink Hotspots"，见 AGENTS.md「代码配色」「裸 URL 高亮」） |
| 内嵌字体 | `assets/fonts/MapleMonoNormal-NF-CN-Regular.ttf`（改名自 Maple Monon NF CN） |
| 扩展名 → 语言 id 映射 | `nebula_app/src/gpui_shell/code_tab.rs` 的 `language_for_extension` |
| 文件树图标字形表（文件夹 / 按扩展名的文件图标 / 折叠箭头） | `nebula_app/src/display/side_panel/icons.rs` |
| 图片预览的缩放 / 平移几何 | `nebula_app/src/display/image_viewer.rs` 的 `ImageView`（与旧壳渲染共用同一份数学） |
| 标签右键菜单的结构（先选中该行再开菜单、菜单宿主提升到根） | `nebula_app/src/gpui_shell/workspace/tab_menu.rs` |
| 在资源管理器中显示（`/select,` 的引号规则） | `nebula_app/src/platform/file_manager.rs` |
| 原生「选择文件夹」对话框（`IFileOpenDialog` + COM vtable） | `nebula_app/src/display/file_dialog/folder.rs`（去掉旧壳 winit 入口与 WSL 侧栏） |
| 字号步进约定（±1 逻辑 px、钳 4–64） | `nebula_app/src/gpui_shell/terminal/view.rs::zoom_font_size` |
| 「任何文件当文本打开」的解码规则 | `nebula_app/src/text_document.rs` |
| 编辑器控件与**内建查找替换面板** | `gpui_shell/code_tab.rs` 的 `InputState::code_editor(...)`（面板、匹配高亮、逐个/全部替换都是组件库自带） |
| Markdown 预览的图片解析 | `gpui-component` 的 `TextViewStyle::image_base`（相对路径）与 gpui 图片管线的 HTTP 客户端（网络图片）；客户端用 zed 自带的 `reqwest_client` |

GPUI、gpui-component 的 Git 来源与 rev 与 Pebrel **完全一致**（`Kuddev/zed`、`Kuddev/gpui-component` 的固定 SHA）。Cargo 的 source identity 含 URL，混用官方与自有 URL 即使 SHA 相同也会解析成两套不兼容类型，所以必须照抄。网络图片用的 `reqwest_client` 也取自同一个 `Kuddev/zed` 源，这样 `http_client::HttpClient` 才是 gpui 认的那一个。

许可证：本项目是 [Pebrel](https://github.com/Kuddev/pebrel)（GPL-3.0）的派生作品，沿用 **`GPL-3.0-or-later`**，全文见 [`LICENSE`](LICENSE)。`vendor/gpui-component/` 是上游 `Kuddev/gpui-component`（Apache-2.0）的截取副本，其许可证原样保留在该目录下。再分发时整个项目需按 GPL-3.0 开源。

## 构建

### 前置：必须用 gnu 宿主工具链

本机没装 Visual Studio（且 Git Bash 的 `/usr/bin/link.exe` 会被误当成 MSVC 链接器）。关键点是 **cargo 的 `--target` 只影响最终产物，build script 与 proc-macro 永远按宿主三元组编译**——宿主是 msvc 就一定要 MSVC 的 `link.exe`。因此要用 gnu 宿主：

```powershell
rustup toolchain install 1.97.1-x86_64-pc-windows-gnu
```

`rust-toolchain.toml` 已固定到该工具链；mingw-w64 的 gcc 需在 PATH 中（本机由 WinGet 的 WinLibs 提供）。

构建产物落在 `C:/Users/lenovo/.nebula-lite/target`（见 `.cargo/config.toml`）：项目目录名含中文，mingw 的 C 编译器对非 ASCII 路径会出编码问题，挪出去也避免几个 GB 的中间产物留在归档目录。

### 拉依赖：本机直连 GitHub 不稳时借镜像

依赖里有几个 Git 仓库，本机直连会超时或被 reset。用 git 的 URL 重写借镜像（提交仍按 SHA 校验，镜像无法替换内容）：

```bash
cd nebula-lite
export GIT_CONFIG_COUNT=1
export GIT_CONFIG_KEY_0="url.https://gh-proxy.com/https://github.com/.insteadOf"
export GIT_CONFIG_VALUE_0="https://github.com/"
cargo fetch
```

实测吞吐：`gh-proxy.com` ≈ 616 KB/s，直连 ≈ 217 KB/s，`ghfast.top` 只有 13 KB/s。

### 编译与测试

```bash
cargo build     # 全量约 10 分钟（依赖已缓存时增量 <10 秒）
cargo test      # 58 个单元测试
cargo build --release   # 日常使用的优化版
```

首次全量编译约 10 分钟，debug 可执行文件约 320 MB（`opt-level` 只对依赖生效，自己的代码仍是增量 debug，改一处重编译只要几秒）。

> **release 构建需要额外的处理**：`gpui_windows` 的 `build.rs` 在 `not(debug_assertions)` 时用 `fxc.exe` 预编译 HLSL 着色器，而该工具随 Windows SDK 分发、本机没装，直接构建会 `panic: Failed to find fxc.exe`。
> 解决方式写在 `Cargo.toml` 的 `[profile.release.package]` 里：只给 `gpui_windows` 这一个 crate 打开 `debug-assertions`，它就退回**运行时着色器路径**（debug 构建实际走的那条，渲染已验证正常），其余 crate 保持全优化。
> 如果以后装了 Windows SDK，删掉那一行即可让 release 走预编译着色器；也可以设 `GPUI_FXC_PATH` 指向 `fxc.exe`（该 build.rs 支持这个环境变量）。

> 链接时会有 `warning: linker stderr: .rsrc merge failure: multiple non-default manifests`。这是 mingw `ld` 同时看到 mingw CRT 默认清单和 GPUI 清单的告警，**不影响结果**：已从 exe 里取出 RT_MANIFEST 验证，实际生效的是带 `dpiAware=true/pm` 与 `dpiAwareness=PerMonitorV2` 的那份，高分屏不会发虚。

> **应用图标由 `build.rs` 编进 exe**（`windows/nebula-lite.rc` + `embed-resource`，走 mingw 的 `windres`）。图标放在资源 ID 1：GPUI 起窗口类时用 `LoadImageW(module, MAKEINTRESOURCE(1), IMAGE_ICON, ...)` 取图标，ID 1 两条取图标路径（GPUI 的窗口类、任务栏的最小 ID 组图标）都能命中。`.rc` 里**不放清单**——DPI 清单已经由 GPUI 嵌好，再塞一份会让多份非默认清单的归属变得不确定。

### 依赖里那一处本地补丁（vendor）

`gpui-component` 是从它的 Git 源**截取**进 `vendor/gpui-component/` 的一份副本（只保留 `crates/ui`、`crates/assets`、`crates/macros` 与 `themes/`），`Cargo.toml` 末尾的 `[patch."https://github.com/Kuddev/gpui-component"]` 把这两个包指过去。

原因只有一个：上游把编辑器的**首次**语法解析也放进"2ms 前台预算 → 超时转后台 → 后台先等 150ms 防抖"那条路，于是刚打开一个文件时有几百毫秒正文是没颜色的。补丁让首次解析走同步（Notepad3 / Scintilla 的做法），只改 `crates/ui/src/input/mode.rs` 里的一小段，说明见 [`vendor/gpui-component/NEBULA-LITE-PATCH.md`](vendor/gpui-component/NEBULA-LITE-PATCH.md)。

依赖来源不受影响：`[patch]` 就是把这一个包**整体**换成本地路径，不会出现同名 crate 的两套实例。`gpui-component-assets` 必须一起换——它带 `links`，同名的两份会让 cargo 直接报重复链接。

## 运行

### 日常用：双击入口（不用敲命令）

编译产物在 `C:/Users/lenovo/.nebula-lite/target/release/nebula-lite.exe`。**不要直接双击它**——那会以 exe 所在的临时目录为根打开，看到的是一堆编译中间产物。给人用有两个入口：

- 工作区根的 **`启动 nebula-lite.cmd`**：双击 = 打开桌面目录；把文件夹或文件**拖到它上面** = 打开该目标。
- **桌面快捷方式** `nebula-lite.lnk`：指向上面那个 release exe，工作目录是桌面，图标取 exe 内嵌的那份。

两个入口都指向 release 产物，所以改完代码要重新 `cargo build --release` 才会反映出来。

### 命令行

```bash
nebula-lite                      # 打开当前目录
nebula-lite <目录>                # 打开该目录
nebula-lite <文件>                # 打开该文件，并在树里展开它所在目录
nebula-lite --preview <文件>      # 有预览面的文件以预览打开（md / 图片）
```

可执行文件无外部 DLL 依赖（已核对导入表：只剩系统库与 `api-ms-win-*` 转发），所以拷到别处、换个工作目录也能跑；但**双击时根目录取决于它所在位置**，给人用请走上面的 `.cmd` / 快捷方式。

### 快捷键

| 键 | 动作 |
| --- | --- |
| `Ctrl+S` | 保存当前文件（只读文档会明确拒绝并提示，不静默丢弃） |
| `Ctrl+F` | 查找（面板内 `Aa` 切大小写敏感、`<` `>` 或回车跳转、`Esc` 关闭） |
| `Ctrl+H` | 查找并替换（多出替换行：`替换` / `全部替换`） |
| `Ctrl+W` | 关闭当前标签 |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | 下一个 / 上一个标签（环绕） |
| `Ctrl+=` / `Ctrl+-` | 放大 / 缩小字号（一步 1px，钳 8–32） |
| `Ctrl+0` | 字号回默认（13px） |
| `Ctrl+Shift+O` | 打开工作区目录（弹系统原生目录选择器；也可点侧栏表头左侧的目录图标） |
| `Ctrl+滚轮` | 同上，鼠标入口——在**图片预览**面上可用（那时编辑器不渲染、键盘绑定派发不到）；源码面与 Markdown 预览面留着滚动内容 |
| 单击目录行 | 展开 / 收起 |
| 单击文件行 | 打开（图片默认预览，Markdown 与其它文本默认源码）；**已在标签里开着就只切过去，不重开** |
| 单击标签 | 切换到这个文件。**默认只有一个标签**：切换到别的文件会替换当前这个（标签条显示完整路径）；有未保存改动时会自动变为固定（见下），不会丢编辑 |
| 右键标签 | 菜单：**固定 / 取消固定**、关闭此标签 / 关闭其它 / 关闭右侧、复制完整路径、在资源管理器中显示、用默认程序打开 |
| 单击标签上的 `×` | 关闭这个标签（不会顺带激活它） |
| 侧栏过滤框 | 按名称过滤（忽略大小写），穿透收起的目录；命中项的祖先目录保留以显示层级；清空即恢复 |
| 侧栏表头目录图标 | 选一个目录 / 文件打开（等同于拖一个文件夹到启动器上） |
| 右键文件树 | 新建文件 / 新建文件夹 / 打开（文件）/ 复制完整路径（文件）/ 在资源管理器中显示 / 重命名 / 删除。新建与重命名是**就地输入**（回车确认、Esc / 失焦取消）；删除会**先弹确认框** |
| 拖动侧栏与编辑器之间的分割线 | 调整侧栏宽度（钳 160–560px） |
| 点侧栏表头的**齿轮图标** | 打开设置面板（「字体管理」：两个字体槽位各自选字、行尾图钉顶置常用项，改动即时生效并落盘） |
| **`Ctrl` + 左键拖动正文** | 水平平移编辑区（抓手光标；松开 Ctrl 立即回到文本选择）。只动横向 |
| 右键文件 / 文件夹 / 文件夹空白处 / 盘符 | 「用 nebula-lite 打开」——由 [`windows/install-context-menu.cmd`](windows/install-context-menu.cmd) 注册（只写 HKCU，可一键卸） |
| 点标题栏左上角的**应用图标** | 开合左侧边栏（收起后内容区占满整行） |
| 标签条右端的药丸 | 「大纲」开合 Markdown 标题面板（仅文档里有标题时出现）；「源码 / 预览」切换显示面（仅 md 与图片有）；「保存」写回磁盘；冲突时改为「重新加载 / 覆盖」；图片缩放后出现「复位」 |
| 标题栏右上三个按钮 | 最小化 / 最大化（还原）/ 关闭；标题栏空白处可拖动窗口、双击最大化 |
| 图片预览面 | 滚轮缩放（围绕指针、0.25×–8×）、按住左键拖动平移、标签条右端「复位」药丸回到适应视图 |
| Markdown 大纲 | 点标题把源码光标移到那一行（预览面上也生效，切回源码即停在标题处） |

**标签：默认单文件模式。** 默认不显示多个标签——每次切不同文件只显示当前这一个（标签条给完整路径）。如果你想让某个文件留在标签条上，右键它的标签选**固定**；一旦有标签被固定，标签条就转为显示全部标签（固定项与当前项并存，标签名回到短名）。固定会把内容固定住不被替换，所以编辑时切去看别的文件、再切回来，内容还在。（有未保存改动的标签在切文件时会自动转为固定，避免你的编辑被替换掉。）

打开文件或切换标签时焦点都会交给编辑器，所以**可以直接打字、直接用上面这些快捷键**，不必先点一下正文。例外是**预览面激活**时：那时编辑器不参与渲染、没有节点持有焦点，`Ctrl+S` / `Ctrl+W` / `Ctrl+Tab` 这些窗口级快捷键会失效，请改用鼠标入口（标签条右端的药丸、点标签）。

## 行为约定

- **标签：默认单文件模式**：打开或切换到另一个文件时，标签条**只显示当前这一个**（并显示完整路径），隐式的旧标签会被替换掉——这是刻意的默认行为，不是"关掉了别的文件"。想让文件留在标签条上，右键它的标签选**固定**；只要有一个标签被固定，标签条就显示全部标签（固定项与当前项并存，标签名回到短名）。**有未保存改动的标签在切换文件时会自动转为固定**，所以编辑不会被替换掉。同一路径只会有一个标签（再打开就切过去），不会出现"两个标签监督同一个文件、互相重载"。未保存的文档在标签条带一个 `•` 并出现「保存」药丸；关标签用 `×` 或 `Ctrl+W`，关到最后一个回到"从左侧选择一个文件"。标签的身份是路径而不是位置，关掉左边的标签不会把激活项挪到别的文件上。
- **任何文件都能打开**：不合法 UTF-8 或含 NUL 的字节用**有损解码**出字符并转只读，所以二进制看到的是一屏乱码而不是「用默认应用打开」，也不会因一次误保存写坏原文件。超过 8 MiB 截断显示并转只读。
- **保留文件原有约定**：BOM 与 CRLF 记录下来，保存时原样写回；保存前比对磁盘内容，外部改动过就拒绝覆盖。
- **外部改动自动跟进**：监听当前文档所在目录，文件被别的程序改写时自动重载并提示。规则是「先比内容、再比脏标记」——磁盘内容与打开时一致就什么都不做（顺带挡掉自己保存引发的自激重载）；缓冲**干净**才直接采用磁盘内容；缓冲**有未保存改动**时只提示冲突，把裁决权交给你（标题栏出现「重新加载 / 覆盖」），绝不静默吞掉你敲进去的字。文件被外部删除时缓冲原样保留，「保存」可以把它写回来。
- **文件树跟着外部变化刷新**：目录里的增删会立刻反映到树上（树每次都重读磁盘）。
- **打开或切换标签即接管键盘**：焦点随激活动作交给编辑器，所以打开或切过去后能直接打字、直接用快捷键。窗口里没有节点持有焦点时，GPUI 根本不会解析按键绑定（详见开发笔记 `AGENTS.md`，未随本仓库发布）。
- **预览用编辑缓冲的当前内容**，不是磁盘那份——改几个字再切预览看得到改动。
- **预览里的图片**：Markdown 中相对路径的图片按**文档所在目录**解析（`image_base`）后直接读盘；`http(s)://` 与 `data:` 图片走 gpui 的 HTTP 客户端（README 顶部记录的 `reqwest_client`）。
- **自绘标题栏**：`WindowOptions.titlebar` 用 `appears_transparent` 把系统标题栏藏掉，那块区域整个交给应用，所以 `app.rs` 必须自己画 `TitleBar`——不画就一个窗口按钮都没有。左侧是应用名与当前树根，右侧是最小化 / 最大化 / 关闭三个按钮，按钮的点击由系统按 `WM_NCHITTEST` 完成（组件库给每个按钮打了 `WindowControlArea`），不额外挂 `on_click`。
- **文件树图标**：文件夹、按扩展名的文件图标、折叠箭头全部是内嵌 Maple Mono Nerd Font 的私有使用区码点，渲染时显式指定该字体族（界面主字体微软雅黑没有这些码点，不指定就是方框）。字形表逐值照抄 Pebrel 的 `side_panel/icons.rs`。
- **侧栏过滤穿透展开状态**：过滤时忽略"哪些目录展开着"，直接看整棵树——否则命中项藏在收起目录里就一条都看不到，过滤等于失效。匹配项的祖先目录保留（层级才看得出来），无关邻居不带进来。清空查询词即回到普通展平。过滤态点目录仍会翻转展开集合（虽然当下看不出效果），清空过滤词后便落在刚点出来的状态上。
- **路径显示剥掉 verbatim 前缀**：树根与启动文件都 `canonicalize` 过（Windows 上是 `\\?\C:\…`）。标题栏、复制路径、状态栏用 `shell::friendly_path` 剥掉 `\\?\` 再显示——那个前缀对文件 API 有意义，摆给人看只是噪音。传给系统命令时用原始路径。
- **字号是全局的**：`Ctrl+=` / `Ctrl+-` / `Ctrl+0` / `Ctrl+滚轮` 改的是工作区一份字号，影响编辑器与 Markdown 预览（预览正文跟随，标题基准字号也一起偏）。不是每个标签一份——"字太小"是对整个应用的判断，切个标签就变回去会很别扭。
- **图片缩放围绕指针**：滚轮缩放时指针下的那个像素位置保持不变（几何是 Pebrel `ImageView` 的同一份数学）；基准比例是"图片宽度铺满视图宽度"，`zoom` 是相对它的倍率，所以改窗口大小不会累积漂移。平移钳在图片边缘、图小则居中。换图（外部改动）时缩放与平移重置——旧倍率套到尺寸不同的新图上只会得到位置莫名其妙的画面。
- **预览面用滚轮调字号**：预览态下编辑器不参与渲染、没有节点持有焦点，`Ctrl+=` 这类按键的派发目标是根节点（见上一段"预览面激活时窗口级快捷键是哑的"），所以预览的字号入口是 `Ctrl+滚轮`（鼠标事件按命中位置派发，预览面接得住）。这一条目前只落在**图片预览**面上：Markdown 预览的滚动容器在滚动时会 `stop_propagation`，外层的滚轮监听收不到，所以那里的 `Ctrl+滚轮` 仍旧是滚动内容。
- **当前行高亮不默认显示**：打开文件时没有任何一行常亮；只有当你在正文里**点过一下或打过字**之后，光标所在的那一行才铺上 Notepad3 的淡黄底色（组件库那层无条件的当前行高亮被关掉，改由应用自绘）。切换文件后该状态按文件重置。
- **窗口尺寸夹进显示器可用区**：高缩放比下逻辑尺寸换算成物理像素可能超过物理屏，不夹住的话窗口右端（含右上角按钮）会被推到屏幕外。
- **设置会被记住**：字体槽位的选择、每列字体列表的顶置项都写进 `%APPDATA%\nebula-lite\settings.json`，下次启动照它恢复（字体名会**对着系统字体目录校验**：字体被卸载过就回落默认值，而不是让 gpui 静默换一副字）。设置面板里的两个字体列表**各自独立滚动**、也各有各的顶置顺序。
- **标签默认透明**：标签条上不再每个标签都铺底色，只有**固定**的标签与**鼠标悬停**的那一个才有底色；"当前是哪个文件"改由字色加重 + 加粗说明。
- **`Ctrl` + 左键拖动正文 = 水平平移**：按住 Ctrl 时光标变成抓手（未按下是张开的手、拖动时攥住），拖动移动编辑区的横向滚动位置；松开 Ctrl 立刻回到正常的文本选择。只动横向，纵向仍归滚轮。

## 源码结构

```
src/
  main.rs        入口、命令行解析、字体注册、HTTP 客户端注册、应用图标解码、主题应用、按键绑定（保存 / 关标签 / 切换标签 / 字号 / 打开目录）、中文 locale
  theme.rs       外观令牌（macOS 中性灰白 + 系统蓝，`APPLE` 表）+ 派生函数 + 写入 gpui_component::Theme；
                 编辑器表面配色（白底、浅灰行号栏、当前行淡黄）与"按语言装入语法主题"；
                 当前行高亮令牌按需装（默认关，由 app.rs 自绘）
  syntax.rs      代码配色：**按语言**逐值照抄 Notepad3 各 `styleLex*.c` 的默认样式表
                 （捕获名 → 语义角色 → 该语言的颜色），非标准捕获名的归一化表
  fonts.rs       内嵌 Maple Mono 的注册
  http.rs        把 gpui 的图片管线接到 reqwest 客户端（网络图片才加载得出来）
  app.rs         主视图：自绘标题栏（应用图标 + 窗口按钮，点图标开合侧栏）、可开合可拖宽的侧栏（目录图标 + 过滤框 + 文件树 + 右键菜单） | 单标签/固定标签内容区、标签生命周期与右键菜单、标签条右端的文档动作（大纲/源码-预览/保存）、预览切换、图片缩放平移、Markdown 大纲、括号匹配高亮、当前行高亮自绘、打开工作区目录、保存、冲突裁决
  brackets.rs    括号匹配（光标贴着括号时找配对的那个；纯函数，跳过字符串/注释里的括号）
  folder_picker.rs  原生「选择文件夹」对话框（IFileOpenDialog；专用线程跑模态框）
  file_tree.rs   文件树状态（展开集合 + 每次渲染展平 / 按名称过滤；可换根）
  icons.rs       文件树图标字形表（照抄 Pebrel 的 side_panel/icons.rs）
  image_geom.rs  图片缩放 / 平移几何（照抄 Pebrel 的 image_viewer.rs，纯数学可单测）
  outline.rs     Markdown ATX 标题解析（跳过大纲用的标题列表来源）
  shell.rs       「在资源管理器中显示」/「用默认程序打开」
  text_file.rs   文本快照：解码/编码/冲突检测/文件类型分流、保存与覆盖保存
  lang.rs        扩展名 → tree-sitter 语言 id
  languages.rs   组件库覆盖不到的语言注册（含 `.bat`/`.cmd`）+ 各语言查询的捕获名归一化 /
                 Kotlin 查询修复 / CSS class-id 与属性名分离
  urls.rs        裸 URL 扫描（Notepad3 的 "Hyperlink Hotspots"，纯函数可单测）
  watch.rs       目录级文件监听 + 外部改动的处置裁定
  settings.rs    设置持久化：两个字体槽位 + 字体列表顶置项 → %APPDATA%\nebula-lite\settings.json
build.rs         把 windows/nebula-lite.rc（图标 + 版本信息）编进 exe
windows/         应用图标 .ico 与 .rc
vendor/          gpui-component 的截取副本 + 本地补丁说明（见 `NEBULA-LITE-PATCH.md`）
```

## 已知限制

- 外部改动的监听只覆盖**每个已打开文档所在的目录**（非递归）：其它目录里的外部增删不会立刻反映到文件树上，要等下一次自然重渲染。
- 外部重载后视野回到文件开头：组件库里 `scroll_to` 是 crate 内可见，公开 API 没有恢复阅读位置的办法。
- 未做：标签拖拽重排、安装包与代码签名、**会话恢复**（上次的标签 / 窗口位置 / 侧栏状态）。设置项（两个字体槽位 + 各自顶置的字体）已经持久化了，见上表。
- **图片预览没有双击复位**：这一版 gpui 没有双击事件（`ClickEvent` 带 `click_count`，但要和拖拽区分得自己攒双击状态），复位入口在头部「复位」药丸上。
- **Markdown 预览的 `Ctrl+滚轮` 仍是滚动**（见上面「预览面用滚轮调字号」）：字号入口是键盘 `Ctrl+=` 或图片预览面的滚轮。
- **大纲只认 ATX 标题**（`#` 形式），不认 Setext（下一行是 `===` / `---` 的那种）——`---` 还兼任分隔线，误判会把正文当标题，宁缺勿错。围栏代码块里的 `#` 不计入。
- Markdown 预览的网络图片是**同步解码后一次性绘制**的：加载中或加载失败时该处就是空白，没有占位提示。
- Rust 侧语法高亮依赖 `gpui-component` 的 `tree-sitter-languages`（约 35 个语法），本仓库另补 9 个。不认识的扩展名回落纯文本，仍可正常打开。
- **编辑器代码配色按语言取自 Notepad3**：C/C++ 与 Go 的关键字是深蓝 `#0A246A`、JS/Java/PHP 是橙 `#A46000`、Rust 是绿 `#248112`、Python 是深蓝 `#00007F`……这与"外壳用苹果中性色"是两套来源（互不影响：换外壳色不该动编辑器里的代码色）。组件库的高亮主题是全局一份（没有"每个输入框各一份"的口子），所以这是在**切换文档时重装**的，见 AGENTS.md「代码配色」。
- **Notepad3 认不出 / 没有对应 lexer 的语言**（Swift、Zig、Erlang、Haskell）用一份中性的 C 系配色兜底，不是逐语言对照。
- **裸 URL 高亮**取自 Notepad3 的 "Hyperlink Hotspots"：`https://…`/`ftp://…`/`mailto:…`/`www.…` 涂成 `#0060B0`。组件库的 tree-sitter 高亮认不出裸 URL（只认 Markdown 的 `[x](url)` / `<url>`），所以这是单独扫一层装饰补的；每次缓冲变更全文重扫，超过 1 MiB 的文件不做（保证大文件打字延迟），见 AGENTS.md「裸 URL 高亮」。
- 头部的小药丸刻意自绘而不用组件库的 `Button`（尺寸与悬停反馈受控）。注意 gpui-component 0.5.2 的按钮配色回归**只落在 `ButtonVariant::Custom`**：`Default` / `Ghost` / `Text` 等变体读的是主题令牌，在浅色主题下清晰可读——查找面板用的正是这几个变体。


- **第一次高亮是同步的，之后的增量不是**：首次解析（刚打开文件 / 刚换语言）走**同步**全量解析（预算 250ms、上限 2 MiB），超限或超时才退回组件库那条"后台线程 + 150ms 防抖"的路。所以超过 2 MiB 的文件打开时仍会先没颜色、过一会儿才上色，其余文件是首帧就有色。
- **`gpui-component` 现在是仓库内的截取副本**（`vendor/gpui-component/`），只打了一处补丁（首次解析改同步），有标记注释可搜。升级依赖时要按 [`NEBULA-LITE-PATCH.md`](vendor/gpui-component/NEBULA-LITE-PATCH.md) 重新截取并重放补丁，别直接改 `vendor/` 里别的地方。
- **右键菜单在 Windows 11 的二级菜单里**：注册的是经典式 shell 动词，出现在「显示更多选项」（或 Shift+F10）；一级菜单需要 MSIX 稀疏包，本项目不做。
- **Markdown 预览的围栏代码块读的是全局高亮主题**（= 当前文档语言那一份 Notepad3 配色），不是围栏里标注的语言——组件库只接受一份全局主题，所以 ```rust 的块用的是 markdown 那套色。
