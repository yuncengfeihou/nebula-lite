//! 主视图：左侧文件树 + 右侧多标签内容区。
//!
//! 内容区对 md/图片给出「源码 / 预览」切换，其余文件一律按文本打开——包括
//! 二进制：解码层做有损解码并转只读，所以打开看到的是字符而不是"用默认应用
//! 打开"。
//!
//! 每个标签就是一个 [`Document`]：它自带输入订阅与外部改动监听，生命周期完全
//! 跟着标签走——关掉标签即退订、即停止监听。同一路径只允许一个标签，所以不会
//! 出现"两个标签监听同一个文件、各自重载"的重复监听（打开已开着的文件只会把
//! 那个标签激活）。

use std::cell::RefCell;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    Anchor, AppContext as _, Bounds, ClipboardItem, Context, DismissEvent,
    Entity, Focusable as _, HighlightStyle, InteractiveElement as _,
    IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement as _,
    Pixels, Point, Render, RenderImage, ScrollWheelEvent, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, UniformListScrollHandle, Window,
    actions,
    anchored, canvas, deferred, div, point, prelude::FluentBuilder as _, px, relative,
    size, uniform_list,
};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, TitleBar, WindowExt as _, h_flex,
    input::Input, input::InputEvent,
    input::InputState,
    input::{TextDecoration, TextDecorationCollection},
    menu::{PopupMenu, PopupMenuItem},
    scroll::ScrollableElement as _, text::TextView, text::TextViewStyle, v_flex,
};
use image::Frame;

use crate::file_tree::{FileTree, INDENT, ROW_HEIGHT, Row};
use crate::fonts;
use crate::icons;
use crate::image_geom::{Area, ImageGeometry};
use crate::np3;
use crate::outline;
use crate::shell;
use crate::text_file::{self, FileKind, SaveError, TextSnapshot};
use crate::urls;
use crate::watch::{self, FileWatch, Signal};

actions!(nebula_lite, [SaveDocument, CloseTab, NextTab, PreviousTab, IncreaseFontSize, DecreaseFontSize, ResetFontSize, OpenFolder]);


/// 侧栏宽度（逻辑 px）。宽松布局：给长文件名留出余量。也是拖动调宽后的初始值。
const SIDEBAR_WIDTH: f32 = 268.0;
/// 侧栏宽度的可拖范围（逻辑 px）。下限要容得下过滤框，上限不能把内容区挤没。
const MIN_SIDEBAR_WIDTH: f32 = 160.0;
const MAX_SIDEBAR_WIDTH: f32 = 560.0;
/// 拖动分割线时，鼠标横向移动多少逻辑 px 就改多少侧栏宽度（1:1）。
const RESIZE_HANDLE_WIDTH: f32 = 5.0;
/// 顶部横带高度：侧栏「文件」、标签条、内容区头部、过滤行**共用**这一个值。
///
/// 以前它们是三个不同的值（40 / 34 / 32），于是三家的文字基线、分割线、内边距
/// 全都错开——顶栏看起来"没对齐"就是这么来的。统一成一条横带的高度后，分栏线
/// 与顶栏线能连成一条、行心也落在同一水平线上。
const BAND_HEIGHT: f32 = 36.0;
/// 顶部横带左右内边距：侧栏表头、过滤框、内容头部、标签条统一用它，
/// 让各栏首字的左边距对齐（Pebrel 的 `TITLE_BAR_LEFT_PADDING` 同为 12）。
const BAND_PAD_X: f32 = 12.0;
/// 内容区顶栏高度（文件全名 + 动作按钮那一行）。与 [`BAND_HEIGHT`] 同值。
const HEADER_HEIGHT: f32 = BAND_HEIGHT;
/// 标签条高度。与 [`BAND_HEIGHT`] 同值。
const TAB_HEIGHT: f32 = BAND_HEIGHT;
/// 大纲面板宽度（内容区左侧的一竖条）。
const OUTLINE_WIDTH: f32 = 200.0;
/// 单个标签里的文件名字段上限，超出截断并显示省略号。
const TAB_LABEL_WIDTH: f32 = 148.0;

/// Markdown 预览的**阅读栏宽**（逻辑 px）。
///
/// 满屏宽的一行太长：眼睛从行尾回到行首时容易串行，这是"读起来累"的主因。
/// 860 这个数与 Pebrel 的 `MAX_COLUMN_W` 同值（`display/markdown_view.rs`），
/// 也是排版上常说的 70–80 字符一行在 14px 等宽下的宽度量级。
const MARKDOWN_COLUMN_WIDTH: f32 = 860.0;
/// 编辑器行高倍率，与 Pebrel 的代码视图一致。
const LINE_HEIGHT: f32 = 1.55;
/// 编辑器字号下限 / 上限（逻辑 px），与 Pebrel 终端 `zoom_font_size` 同档。
const MIN_FONT_SIZE: f32 = 8.0;
const MAX_FONT_SIZE: f32 = 32.0;
/// 默认编辑器字号，也是 `Ctrl+0` 的归位值。
const DEFAULT_FONT_SIZE: f32 = 13.0;
/// 一次滚轮"档"的像素基准，与 Pebrel 图片缩放的 `pixel_delta(px(40.0))` 同源。
const WHEEL_STEP_PX: f32 = 40.0;
/// gpui 把一格滚轮换算成 3 行（`window.rs::SCROLL_LINES = 3`，与 x11 平台保持
/// 一致）。图片缩放按"行"归一化后要再除以它，一格滚轮才是 1 个 1.18× 的档。
/// 不除的话一格滚轮跳 3 档（≈1.64×），缩放一下幅度太大、很难停在想要的倍率上。
const WHEEL_LINES_PER_NOTCH: f32 = 3.0;

/// 文件树行缓存的最长有效期。
///
/// 到期后**下一次重绘**重新展平，所以文件树仍跟得上外部增删（打开的文档会触发
/// 重绘，光标闪烁也每秒带来两次）。这是有意的节流，不是"每帧重扫"：展平要同步
/// 读盘，而原实现每帧都做一次，大树/大目录下每帧都在 UI 线程上花掉数毫秒。
///
/// Pebrel 在这里更激进——`SidePanel` 完全不做定时重扫，目录只在 cwd/根变化或
/// 手动刷新时重建，代价是终端里新建/删除文件不再自动反映
/// （`display/side_panel/mod.rs` 里"目录识别不要轮询"的裁定）。本编辑器没有刷新
/// 按钮，而"文件树跟着外部变化刷新"是既有的已验证行为，所以保留刷新能力，只把
/// 频率从"每次重绘（约 2 次/秒）"降到"每秒一次"。
const TREE_REFRESH: std::time::Duration = std::time::Duration::from_secs(1);

pub struct Workspace {
    tree: FileTree,
    /// 已打开的文档，每个标签一个，按打开顺序排列。
    tabs: Vec<Document>,
    /// 激活标签在 `tabs` 里的下标。`tabs` 为空时无意义。
    active: usize,
    /// 打开失败的原因，只在没有任何标签时占满内容区。
    error: Option<String>,
    /// 最近一次动作的反馈（保存结果等），显示在状态栏。
    status: Option<String>,
    /// `--preview`：有预览面的文件一律以预览打开，而不是先给源码。
    prefer_preview: bool,
    /// 侧栏过滤框。空查询 = 不折叠，显示整棵树。
    filter: Entity<InputState>,
    /// 过滤框的输入订阅。必须持有：`Subscription` 一被丢弃就退订。
    _filter_changes: Option<Subscription>,
    /// 标签右键菜单的锚定状态；`None` = 没开着。
    tab_menu: Option<TabMenu>,
    /// 文件树右键菜单的锚定状态；`None` = 没开着。结构与 `tab_menu` 同源。
    tree_menu: Option<TreeMenu>,
    /// 正在就地重命名的行（新建文件 / 新建文件夹 / 重命名共用）。`None` = 没有。
    renaming: Option<Rename>,
    /// 侧栏宽度（逻辑 px）。拖动分割线改它，见 [`Self::resize_sidebar`]。
    sidebar_width: f32,
    /// 正在拖侧栏分割线：由分割线的 `on_mouse_down` 立起、`on_mouse_up` 清掉。
    sidebar_dragging: bool,
    /// 侧栏是否展开。点标题栏左侧的应用图标切换；收起后内容区占满整行。
    sidebar_open: bool,
    /// 标题栏左上角的应用图标（`main.rs::load_app_icon` 解码好的 BGRA 图）。
    /// `None` 时那一位回落成文字标签，窗口一样能开。
    app_icon: Option<Arc<RenderImage>>,
    /// 编辑器与预览的字号（逻辑 px）。`Ctrl+滚轮` / `Ctrl+=` / `Ctrl+-` 改它。
    font_size: f32,
    /// `true` 时不画编辑器的默认当前行高亮：当前行只在那之后被测到过鼠标点击 /
    /// 键盘输入（`Document::caret_touched`）时才亮。
    ///
    /// 组件库的当前行高亮**无条件**跟着光标走（`editor_active_line` 令牌），
    /// 打开文件、滚动、点选都会让某一行常亮，用户要的是"不点不亮"。所以这里把令牌
    /// 那层关掉，改由 `render_source` 在 `caret_touched` 为真时自绘一条同色 quad。
    hide_default_current_line: bool,
    /// 文件树的展开行缓存：当前过滤词下要显示的行。
    ///
    /// 渲染只读它，不再每帧展平——展平要同步读盘，大树下每帧都在 UI 线程上走
    /// 目录就是卡顿的来源。Pebrel 的 `SidePanel` 同样把展平行缓存在 `rows` /
    /// `tree_rows` 里（`display/side_panel/mod.rs:401-406`），渲染只读缓存。
    rows: Vec<Row>,
    /// `rows` 是按哪个过滤词算出来的，用来判断缓存是否过期。
    rows_query: String,
    /// `rows` 是按哪个树版本算出来的；展开集合或文件系统结构一变就自增。
    rows_rev: u64,
    /// 树版本。展开 / 收起、reveal、外部结构改动都会自增它，从而让行缓存失效。
    tree_rev: u64,
    /// 行缓存上次重建的时刻。超过 [`TREE_REFRESH`] 就重新展平一次，让文件树的
    /// 外部增删仍然跟得上（见常量的注释）。
    rows_at: std::time::Instant,
    /// 侧栏 `uniform_list` 的滚动句柄。必须持有：`uniform_list` 通过它读写
    /// 滚动位置，不持有的话滚动位置每帧都会重置。
    tree_scroll: UniformListScrollHandle,
    /// 当前已装入全局 `highlight_theme` 的语法语言。
    ///
    /// 组件库的高亮主题是全局单例（没有"每个输入框各一份"的口子），所以"按语言
    /// 配色"靠切换文档时重装。这里记住上一次装的是谁，避免每帧都重建那份主题。
    /// 见 `crate::theme::install_syntax_theme`。
    installed_syntax: Option<&'static str>,
    /// 设置面板是否打开。
    settings_open: bool,
    /// 设置面板里的字体过滤词（两个字体列表共用一份）。
    font_filter: Entity<InputState>,
    /// 过滤框的输入订阅（同 `_filter_changes` 的约定：需持有）。
    _font_filter_changes: Option<Subscription>,
    /// 可选字体目录，启动时从系统取一次（`cx.text_system().all_font_names()`）。
    /// `Arc` 便于原样传进渲染闭包，不必每帧重建。
    font_catalog: Arc<Vec<String>>,
    /// **界面 / 标题字体**：标题栏、标签条、侧栏、状态栏都用它（写进全局
    /// `Theme::font_family`）。默认取 `theme::UI_FONT_FAMILY`。
    ui_font: SharedString,
    /// **编辑器 / 预览字体**：源码面编辑器与 Markdown 预览正文用它。默认取
    /// `fonts::REQUIRED_FONT_FAMILY`（内嵌的 Maple Mono）。**文件树的图标列不受它
    /// 影响**——那些是 Nerd Font 私有码点，必须钉在 Maple 上（见 `icons.rs`）。
    editor_font: SharedString,
    /// 被顶置的界面 / 编辑器字体（按顶置先后，最后顶置的在最前）。列表里排到最
    /// 上面，并跟着设置一起落盘（见 `crate::settings`）。存**名字**而不是下标：
    /// 字体目录来自系统枚举，装/卸一个字体就会让下标错位。
    ui_font_pins: Vec<String>,
    editor_font_pins: Vec<String>,
    /// Ctrl 是否按着。由窗口级修饰键监听维护（见 `render_pan_layer`），决定源码面
    /// 是否盖一层"抓手"平移层、以及鼠标是不是换成抓手图标。
    ctrl_down: bool,
    /// 正在用 Ctrl+左键拖动平移源码面：`(按下时的鼠标 x, 那时编辑器的横向偏移)`。
    ///
    /// 存**绝对锚点**而不是逐帧增量：`InputState::set_scroll_offset` 是延后一帧
    /// 生效的（组件库内部走 `deferred_scroll_offset`），逐帧累加会把这份延迟和
    /// 自己的舍入误差一起累进结果里，拖久了就偏。
    pan_drag: Option<(f32, f32)>,
}

/// 标签右键菜单的宿主。
///
/// 菜单不挂在标签行的子孙树上，而是由工作区根上唯一一份 `deferred(anchored)`
/// 绘制。Pebrel 的 `tab_menu.rs` 记录了这么做的原因：`ContextMenu` 的
/// `ElementId` 在组件库里是硬编码的 `"context-menu"`，每个标签行算出的 id 路径
/// 相同、共享同一份 element state，于是菜单一打开**所有**行都渲染同一个菜单、
/// 落在同一个锚点；popover 阴影是半透明的，叠 N 层就是"标签越多阴影越厚"。
struct TabMenu {
    menu: Entity<PopupMenu>,
    position: Point<Pixels>,
    /// 菜单里每条命令作用的标签下标。菜单挂着时若标签集合变了，这一份就不该再画。
    index: usize,
    /// `PopupMenu` 一被丢弃就退订（`Subscription` 的既有约定）。
    _subscription: Subscription,
}

/// 文件树右键菜单的宿主。与 [`TabMenu`] 同一套约定：菜单挂在工作区根上唯一一份
/// `deferred(anchored)`，行只记锚点；命令目标（路径、是不是目录）由菜单自己的
/// 闭包捕获——菜单开着时那一行的展平下标可能已经变了，路径才是稳定身份。
struct TreeMenu {
    menu: Entity<PopupMenu>,
    position: Point<Pixels>,
    _subscription: Subscription,
}

/// 设置面板里的两个字体槽位：界面 / 标题字体、编辑器 / 预览字体。用来区分同一份
/// 字体列表在两次渲染里点的是哪一项，也用来给行元素编稳定的 id。
#[derive(Clone, Copy, Debug)]
enum FontSlot {
    Ui,
    Editor,
}

/// 文件树的就地重命名状态（新建文件 / 新建文件夹 / 重命名三处共用）。
///
/// 不做模态输入框：直接让那一行渲染成一个 `InputState`，回车提交、Esc 取消。
/// 这样新建与重命名走同一条路径，也省掉一个对话框。
struct Rename {
    /// 正在编辑的行对应的磁盘路径。
    path: PathBuf,
    /// 输入框（初值是当前名字，新建文件则是默认名）。
    input: Entity<InputState>,
    /// 提交后是否打开这个文件（新建文件用；用户多半想接着往里写东西）。
    open_after: bool,
    _changes: Option<Subscription>,
}

struct Document {
    path: PathBuf,
    kind: FileKind,
    /// 该文档的语法语言（`lang::language_for_path` 的结果，`text` = 不着色）。
    /// 切换到这个标签时按它装入高亮主题——组件库的主题是全局一份，见
    /// `Workspace::installed_syntax`。
    language: &'static str,
    snapshot: TextSnapshot,
    /// 源码面：始终存在，因为任何文件都能以文本形态显示。
    input: Entity<InputState>,
    /// 图片预览的解码结果；只有图片会有。
    image: Option<Arc<RenderImage>>,
    /// 图片预览的缩放 / 平移几何；只有图片有意义（其它类型保持默认值）。
    image_geometry: ImageGeometry,
    /// 图片查看区的上一帧矩形（窗口坐标），缩放/拖拽的事件换算用它。
    /// 绘制本身不依赖它——canvas 的 paint 拿的是当帧 bounds。
    image_area: Rc<RefCell<Bounds<Pixels>>>,
    /// Markdown 大纲面板是否展开。只有 Markdown 用得着。
    outline_open: bool,
    /// true 显示预览面，false 显示源码面。
    preview: bool,
    /// 编辑缓冲与磁盘基线是否不同。
    dirty: bool,
    /// 是否"固定"。默认（未固定）时标签条只显示当前这一个文档，切到别的文件就把
    /// 它替换掉（单文件模式）；固定的标签会一直留在标签条上，于是能看到多个。
    /// 有未保存改动时会自动固定，免得编辑被替换掉（见 `Workspace::open`）。
    pinned: bool,
    /// 磁盘那份与本地未保存的改动冲突（外部改过、缓冲又脏）。此时既不自动重载
    /// 也不自动覆盖，把裁决权交给用户。
    conflict: bool,
    /// 文件已从磁盘上消失（外部删除或改名）。缓冲原样留着，保存可以把它写回来。
    missing: bool,
    /// 输入事件订阅。必须持有：`Subscription` 一被丢弃就退订。
    _changes: Subscription,
    /// 外部改动监听。必须持有：一被丢弃就停止监听，重载任务也随之结束。
    _watch: Option<FileWatch>,
    /// 当前匹配到的两个括号的字节区间（`None` = 光标不在括号上）。
    /// 在光标移动时由 `refresh_brace_marks` 重算；渲染时按它自绘圆角框。
    brace_matches: Option<(std::ops::Range<usize>, std::ops::Range<usize>)>,
    /// 光标移动的观察订阅。必须持有：一被丢弃括号高亮就不再更新。
    _brace_observer: Subscription,
    /// 裸 URL 的高亮装饰层（Notepad3 的 "Hyperlink Hotspots"）。
    ///
    /// 组件库的树上高亮不认裸 URL，所以照 Notepad3 的做法单开一层：`refresh_hotspots`
    /// 按它的正则扫描缓冲、把命中区间塞进这个集合。必须持有——集合跟着标签生命周期走，
    /// 一被丢弃编辑器就不再画这些装饰。见 `urls` 模块。
    hotspots: TextDecorationCollection,
    /// Notepad3 补充层的**前景**装饰（批处理关键字 / 标题色等）。见 `np3` 模块。
    /// 与 `hotspots` 一样是装饰集合，在语法样式**之后**合成，能盖过 tree-sitter。
    overlay: TextDecorationCollection,
    /// Notepad3 补充层的**背景**色块（代码 / 变量 / 标签 / 段落名 / 标题条）。
    ///
    /// 组件库的 `ThemeStyle` 没有背景色字段、编辑器也不画 `ShapedLine` 的背景，
    /// 所以这一层由 `render_source` 在编辑器**下方**垫一张 canvas 自己铺 quad。
    /// 存 `Rc<RefCell<..>>` 是因为 canvas 的 paint 闭包是 `'static`，不能借用 `doc`。
    backgrounds: Rc<RefCell<Vec<np3::Span>>>,
    /// 需要自绘的层里是否有内容（有带底色的段）。没有就不加那张 canvas——
    /// 省掉每帧一次多余的布局/绘制，也省掉每帧对全部段做一次扫描。
    has_backgrounds: bool,
    /// 当前行"该不该亮"。
    ///
    /// `false`（打开文档后的初始态）= 不画当前行高亮；只在编辑器上发生过**鼠标点击**
    /// 或**键盘输入**之后才置 `true`。这实现了用户要的"不默认显示当前行、点了才亮"。
    /// 组件库自己那套当前行高亮已经关掉（见 `Workspace::hide_default_current_line`），
    /// 渲染时由 `render_source` 按本标记自绘。
    caret_touched: bool,
    /// 底色层上一帧绘制时的滚动偏移，用来抵消**一帧滞后**。
    ///
    /// 底色 canvas 是编辑器的**前一个**兄弟节点，所以它在编辑器之前绘制、只能读到
    /// 编辑器**上一帧**写下的 `last_layout`（`element.rs` 在 paint 末尾才写
    /// `state.last_layout`）。滚动时这会让底色比文字慢一帧、看起来"追不上"。
    /// 组件库没有"在编辑器内部当帧画底下"的公开口子（内部的 `document_colors` 是
    /// LSP 通道，异步 + 100ms 防抖，且要额外依赖），所以这里用公开的
    /// `InputState::scroll_offset()` 自己补偿：把它与上一帧记下的值相减，把 quad
    /// 平移同样的距离。不滚动时差值为 0，不影响任何静态画面。
    bg_offset: Rc<RefCell<Option<Point<Pixels>>>>,
}

impl Workspace {
    pub fn new(
        root: PathBuf,
        initial_file: Option<PathBuf>,
        prefer_preview: bool,
        app_icon: Option<Arc<RenderImage>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // 过滤框先建好：它是侧栏的一部分，`render_sidebar` 每帧都要读它的值。
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("按名称过滤…"));
        let font_filter =
            cx.new(|cx| InputState::new(window, cx).placeholder("搜索字体…"));
        // 字体目录只在建视图时取一次：`all_font_names` 每次都会走一遍系统字体集，
        // 放进每帧渲染就是每帧白烧。内嵌的 Maple 显式排在前面，免得某些平台上
        // 系统枚举没把它列出来（那样"编辑器字体"就选不回默认值了）。
        let mut font_catalog = vec![fonts::REQUIRED_FONT_FAMILY.to_string()];
        for name in cx.text_system().all_font_names() {
            if !font_catalog.contains(&name) {
                font_catalog.push(name);
            }
        }
        // 设置里存的字体名要**对着这份目录校验**：字体被卸载、或名字写错时回落
        // 默认值，而不是交给 gpui 静默换一副字——那样"设置没生效"看上去毫无原因。
        let settings = crate::settings::Settings::load();
        let ui_font = resolve_font(
            settings.ui_font.as_deref(),
            &font_catalog,
            crate::theme::UI_FONT_FAMILY,
        );
        let editor_font = resolve_font(
            settings.editor_font.as_deref(),
            &font_catalog,
            fonts::REQUIRED_FONT_FAMILY,
        );
        let mut workspace = Self {
            tree: FileTree::new(root),
            tabs: Vec::new(),
            active: 0,
            error: None,
            status: None,
            prefer_preview,
            filter,
            _filter_changes: None,
            tab_menu: None,
            tree_menu: None,
            renaming: None,
            sidebar_width: SIDEBAR_WIDTH,
            sidebar_dragging: false,
            sidebar_open: true,
            app_icon,
            font_size: DEFAULT_FONT_SIZE,
            hide_default_current_line: true,
            rows: Vec::new(),
            rows_query: String::new(),
            rows_rev: u64::MAX,
            tree_rev: 0,
            rows_at: std::time::Instant::now(),
            tree_scroll: UniformListScrollHandle::new(),
            installed_syntax: None,
            settings_open: false,
            font_filter,
            _font_filter_changes: None,
            font_catalog: Arc::new(font_catalog),
            ui_font: ui_font.clone(),
            editor_font: editor_font.clone(),
            ui_font_pins: settings.pinned_ui_fonts,
            editor_font_pins: settings.pinned_editor_fonts,
            ctrl_down: false,
            pan_drag: None,
        };
        // 字体设置必须在**首个窗口渲染之前**写进全局主题：标题栏、标签条、侧栏、
        // 状态栏都没有显式指定字族，吃的就是 `Theme::font_family`。晚一步会先闪
        // 一帧默认字体。编辑器与预览另有 `self.editor_font`（见 `set_editor_font`）。
        {
            let theme = gpui_component::Theme::global_mut(cx);
            theme.font_family = ui_font;
            theme.mono_font_family = editor_font;
        }
        // 过滤词一变就只重渲染：树的展平在 `render_sidebar` 里现算，不需要缓存。
        workspace._filter_changes =
            Some(cx.subscribe_in(&workspace.filter, window, |_, _, event, _, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }));
        // 设置面板的字体过滤词同理：一变就重渲染（两个字体列表跟着筛）。
        workspace._font_filter_changes =
            Some(cx.subscribe_in(&workspace.font_filter, window, |_, _, event, _, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }));
        // 支持 `nebula-lite <文件>` 直接打开该文件，与 `code <file>` 同义。
        if let Some(file) = initial_file {
            workspace.open(file, window, cx);
        }
        workspace
    }

    /// 当前过滤词（已 trim；空串 = 不过滤）。
    fn filter_query(&self, cx: &gpui::App) -> String {
        self.filter.read(cx).value().to_string()
    }

    /// 展开集合或文件系统结构变了：让行缓存失效。
    fn invalidate_tree(&mut self) {
        self.tree_rev = self.tree_rev.wrapping_add(1);
    }

    /// 打开一个工作区目录：把文件树换到那个根，并停掉不再需要的目录监听。
    ///
    /// 已打开的标签**不清**（本次需求只要"打开指定目录"这个动作）：用户可能是想
    /// 在保留当前文档的同时换个目录看。换根之后旧的每文档目录监听已无意义（它们
    /// 是跟着旧文档走的，跟树根无关），主动丢掉、让树靠自身刷新跟上；当前激活文档
    /// 要重新起一份监听，它仍是"标签所在目录"那一层，与树根无关。
    fn set_root(&mut self, root: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if root == self.tree.root() {
            self.status = Some(format!("已在该目录：{}", shell::friendly_path(&root)));
            cx.notify();
            return;
        }
        self.tree.set_root(root.clone());
        // 换根让整份展平行缓存作废；并把上次展平时间戳清掉，保证下一帧就重读。
        self.invalidate_tree();
        self.rows_at = std::time::Instant::now() - TREE_REFRESH;
        // 过滤框与旧根绑定，留着会立刻把新树筛空。
        self.filter.update(cx, |state, cx| state.set_value("", window, cx));
        // 停掉旧的目录监听；激活文档重起一份（见上面注释）。
        for doc in &mut self.tabs {
            doc._watch = None;
        }
        let active = self.active;
        if active < self.tabs.len() {
            let path = self.tabs[active].path.clone();
            let (watch, watch_error) = match watch_document(&path, window, cx) {
                Ok(watch) => (Some(watch), None),
                Err(error) => (None, Some(error)),
            };
            self.tabs[active]._watch = watch;
            if let Some(error) = watch_error {
                self.status = Some(format!("无法监听外部改动（{error}）：自动重载已关闭"));
            }
        }
        self.status = Some(format!("已打开目录：{}", shell::friendly_path(&root)));
        cx.notify();
    }

    /// 「打开目录」入口：弹原生目录选择器，选完换根。
    fn open_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        #[cfg(windows)]
        {
            let receiver = crate::folder_picker::pick_folder_async(window, "打开工作区目录");
            // 选择器跑在专用线程上（模态对话框会重入消息泵，不能在本借用里跑），
            // 结果由前台任务取回；用户在对话框里停留多久都不会冻结界面。
            cx.spawn_in(window, async move |this, cx| {
                let Ok(selected) = receiver.recv().await else {
                    return;
                };
                let Some(root) = selected else {
                    return;
                };
                cx.update(|window, cx| {
                    this.update(cx, |this, cx| this.set_root(root, window, cx)).ok();
                })
                .ok();
            })
            .detach();
        }
        #[cfg(not(windows))]
        {
            // 其它平台没有移植的原生选择器（本编辑器只在 Windows 上构建/分发）。
            let _ = window;
            self.status = Some(String::from("此平台暂不支持目录选择器"));
            cx.notify();
        }
    }

    /// 需要时重建行缓存，并返回当前应为可见的行。
    ///
    /// 三种情况会重新展平：过滤词变了、树版本变了（展开/收起、外部改动事件），
    /// 或者缓存已经超过 [`TREE_REFRESH`] 那么旧。后者是"文件树跟着外部变化刷新"
    /// 那条既有行为的落脚点——展平要同步读盘，原实现每帧都做，这里把它节流到
    /// 每秒一次；重绘本身仍然照旧（光标闪烁约每秒两次）。
    fn sync_rows(&mut self, cx: &gpui::App) {
        let query = self.filter_query(cx);
        let stale = self.rows_at.elapsed() >= TREE_REFRESH;
        if self.rows_rev == self.tree_rev && self.rows_query == query && !stale {
            return;
        }
        self.rows = self.tree.rows_filtered(&query);
        self.rows_query = query;
        self.rows_rev = self.tree_rev;
        self.rows_at = std::time::Instant::now();
    }

    fn active_doc(&self) -> Option<&Document> {
        self.tabs.get(self.active)
    }

    fn active_doc_mut(&mut self) -> Option<&mut Document> {
        self.tabs.get_mut(self.active)
    }

    /// 打开一个文件。
    ///
    /// 已经开着就只把那个标签激活，不再开第二个——两个标签盯同一个文件会各自
    /// 监听、各自重载。打开失败不改变现有标签，只把错误挂到状态栏。
    fn open(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.tabs.iter().position(|doc| doc.path == path) {
            self.activate(index, window, cx);
            return;
        }

        // **单文件模式**：没有任何标签被固定时，"打开另一个文件"等于替换当前这个，
        // 而不是新开一个标签（用户要的默认行为）。唯一的例外是当前标签**有未保存
        // 改动**——那时不能替换（会丢掉编辑），把它转成固定、让新文件另开一个标签订。
        // 见 `collapse_unpinned` 与 `visible_tabs`。
        if !self.tabs.iter().any(|doc| doc.pinned) {
            if self.tabs.iter().any(|doc| doc.dirty) {
                for doc in &mut self.tabs {
                    doc.pinned = true;
                }
            } else {
                self.tabs.clear();
                self.active = 0;
            }
        }

        let kind = FileKind::of(&path);
        let snapshot = match text_file::load(&path) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                let message = format!("无法打开 {}：{error}", path.display());
                self.error = Some(message.clone());
                self.status = Some(message);
                cx.notify();
                return;
            },
        };

        // 源码面：语言按扩展名决定高亮，未知扩展名回落纯文本。
        let language = crate::lang::language_for_path(&path);
        let text = snapshot.text.clone();
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .code_editor(language)
                .line_number(true)
                .indent_guides(true)
                .soft_wrap(false)
        });
        input.update(cx, |state, cx| state.set_value(text.clone(), window, cx));

        // 裸 URL 高亮（Notepad3 的 "Hyperlink Hotspots"）：按它的正则扫一遍缓冲，
        // 把命中区间作为独立装饰层交给编辑器。装饰在组件库里是**在语法样式之后**
        // 合成的，所以能盖过词法色（例如 Markdown 链接里那段 URL 的绿色）。
        let hotspots = input.update(cx, |state, cx| {
            state.create_decorations_collection(hotspot_decorations(&state.value()), cx)
        });

        // Notepad3 补充层：批处理的前景逐词判色 + 代码 / 变量 / 标签 / 标题等底色。
        // 前景走装饰集合，底色走 `render_source` 下方那张 canvas。见 `np3` 模块。
        let Overlay { foreground: np3_foreground, backgrounds: np3_backgrounds } =
            compute_overlay(language, &text);
        let has_backgrounds = np3_backgrounds.iter().any(|span| span.ink.bg.is_some());
        let overlay = input.update(cx, |state, cx| {
            state.create_decorations_collection(np3_foreground, cx)
        });
        let backgrounds = Rc::new(RefCell::new(np3_backgrounds));

        let image = if kind == FileKind::Image { decode_image(&path).ok() } else { None };
        // 尺寸来自解码结果，供缩放/平移的基准比例用；非图片留 `None`。
        let image_dimensions = image.as_ref().and_then(image_dimensions);

        // 图片的"字符"没有阅读意义，默认落在预览面；Markdown 与普通文本默认
        // 落在源码面（先显示字符，需要渲染时再切）。`--preview` 让有预览面的
        // 文件一律以预览打开。
        let preview = kind.has_preview() && (kind == FileKind::Image || self.prefer_preview);

        // 订阅必须挂在 `set_value` 之前创建、并持有到标签关闭：否则第一次
        // 载入内容的变化事件就会漏掉。事件按**输入实体**回找标签：标签下标会随
        // 关闭而移动、路径会在重命名时变，只有 `input` 实体是全程稳定的身份。
        let changed_input = input.clone();
        let changes = cx.subscribe_in(&input, window, move |this, _, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                let changed_input = changed_input.clone();
                if let Some(doc) = this.tabs.iter_mut().find(|doc| doc.input == changed_input) {
                    let value = doc.input.read(cx).value();
                    doc.dirty = value.as_ref() != doc.snapshot.text.as_str();
                    // 一旦开始打字，说明用户已经在编辑器里操作了——点亮当前行。
                    doc.caret_touched = true;
                    // 缓冲变了，裸 URL 的位置也可能变——重扫装饰层。
                    doc.hotspots.set(hotspot_decorations(value.as_ref()), cx);
                    // Notepad3 补充层同理：前景装饰重建、背景区间重算。
                    let Overlay { foreground, backgrounds } =
                        compute_overlay(doc.language, value.as_ref());
                    doc.overlay.set(foreground, cx);
                    doc.has_backgrounds = backgrounds.iter().any(|span| span.ink.bg.is_some());
                    *doc.backgrounds.borrow_mut() = backgrounds;
                    // 内容一变，布局也随之变；清掉滚动偏移缓存，免得下一帧拿旧值平移。
                    *doc.bg_offset.borrow_mut() = None;
                }
                cx.notify();
            }
        });

        // 括号匹配：订阅编辑器的通知（光标移动会 notify），回调按**输入实体**回找
        // 标签，与输入订阅同一套身份约定。匹配结果存进 `Document::brace_matches`，
        // 渲染时由 `render_source` 自绘圆角框。
        let brace_input = input.clone();
        let _brace_observer = cx.observe(&input, move |this, _, cx| {
            if let Some(doc) = this.tabs.iter_mut().find(|doc| doc.input == brace_input) {
                refresh_brace_marks(doc, cx);
            }
        });

        // 监听在这里就起来，等下面的标签挂上去：任务真正跑起来要等这一整段同步
        // 代码让出执行权，所以事件落到 `apply_external_change` 时标签已经在列表里。
        let (watch, watch_error) = match watch_document(&path, window, cx) {
            Ok(watch) => (Some(watch), None),
            Err(error) => (None, Some(error)),
        };

        self.tabs.push(Document {
            path,
            kind,
            language,
            snapshot,
            input,
            image,
            image_geometry: ImageGeometry::new(image_dimensions),
            image_area: Rc::new(RefCell::new(Bounds::default())),
            outline_open: false,
            preview,
            dirty: false,
            pinned: false,
            conflict: false,
            missing: false,
            _changes: changes,
            _watch: watch,
            brace_matches: None,
            _brace_observer,
            hotspots,
            overlay,
            backgrounds,
            has_backgrounds,
            caret_touched: false,
            bg_offset: Rc::new(RefCell::new(None)),
        });
        let index = self.tabs.len() - 1;
        self.error = None;
        // 自动重载是后台能力，监听起不来不该挡住打开文件，但必须说出来。
        self.status =
            watch_error.map(|error| format!("无法监听外部改动（{error}）：自动重载已关闭"));
        self.activate(index, window, cx);
    }

    /// 激活某个标签：更新文件树选中态、把焦点交给它的编辑器、同步窗口标题。
    ///
    /// 焦点是必须的：一来切过去就能直接打字，二来 GPUI 的按键绑定必须有一个
    /// 持有焦点的节点才能解析——没有焦点时派发目标是根节点本身，挂在更深处视图
    /// 上的 `on_action` 收不到动作，`Ctrl+F` / `Ctrl+H`（组件库绑在 `"Input"` 上）
    /// 以及本应用的 `Ctrl+S` 都会静默失效。预览面不抢焦点：那时编辑器根本没渲染。
    fn activate(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.tabs.len() {
            return;
        }
        self.active = index;
        let path = self.tabs[index].path.clone();
        let preview = self.tabs[index].preview;
        self.tree.reveal(&path);
        self.tree.selected = Some(path);
        // reveal 可能展开了若干祖先目录，行缓存要作废（否则树还停在旧内容上）。
        // 这里**不**主动滚到选中行：选中行本来就是用户刚点的那一行，而切换标签
        // 触发的选中项是否滚进视野由 `uniform_list` 自身的滚动状态决定。原实现
        // 也没有强制滚动（组件库的 `scroll_to` 当时不可达），保持这一行为。
        self.invalidate_tree();
        if !preview {
            let input = self.tabs[index].input.clone();
            input.update(cx, |state, cx| state.focus(window, cx));
        }
        // 编辑器里的代码配色是**按语言**的（Notepad3 每个 lexer 一张表），而组件库的
        // 高亮主题是全局一份 —— 所以每次激活标签都要把当前语言的主题装上。预览面
        // 不渲染编辑器，装了也不显示，但装上没坏处（切回源码时 `toggle_preview`
        // 会再确认一次）。
        self.sync_syntax_theme(cx);
        self.set_title(window);
        cx.notify();
    }

    /// 把全局高亮主题换成当前激活文档语言的 Notepad3 配色。
    ///
    /// 组件库 `Theme::highlight_theme` 是全局单例，编辑器渲染时按 `cx.theme()` 取；
    /// 没有"每个输入框各一份主题"的口子。编辑器只在源码面渲染**当前激活**的标签，
    /// 所以全局一份足够——代价是切换标签要重装。用 `installed_syntax` 记住当前装的
    /// 语言，避免同语言重装（重装会 `Arc::new` 一份新主题，让组件库的样式缓存失效）。
    fn sync_syntax_theme(&mut self, cx: &mut Context<Self>) {
        let language = self.active_doc().map(|doc| doc.language).unwrap_or("text");
        if self.installed_syntax == Some(language) {
            return;
        }
        crate::theme::install_syntax_theme(cx, language, !self.hide_default_current_line);
        self.installed_syntax = Some(language);
    }

    /// 关闭某个标签。焦点落到相邻标签；关掉最后一个则回到"从左侧选择一个文件"。
    fn close_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.tabs.len() {
            return;
        }
        // 被关掉的标签带着它的订阅与监听一起 Drop：输入退订、外部改动监听停止。
        self.tabs.remove(index);
        if self.tabs.is_empty() {
            self.active = 0;
            self.tree.selected = None;
            self.status = None;
            // 没有文档了：把全局主题换回纯文本，免得下一个打开的文件在装载前
            // 沿用上一个文档的配色（虽然装上之前不渲染编辑器，但保持一致更省心）。
            self.sync_syntax_theme(cx);
            self.set_title(window);
            cx.notify();
            return;
        }
        self.active = active_after_close(self.active, index, self.tabs.len());
        // 收口：关掉之后若一个固定标签都不剩，把多余的（未固定的）标签也合掉，
        // 只留当前这个——否则它们既被关掉了显示，又还在后台监听，成了"隐形的标签"。
        self.collapse_unpinned(self.active);
        self.activate(self.active, window, cx);
    }

    /// 单文件模式的收口：没有任何标签被固定时只保留 `keep` 这一个，其余连同订阅、
    /// 外部改动监听一起 Drop。被关闭的标签的 `FileWatch` 一丢，后台重载任务随通道
    /// 关闭自行退出（见 `watch_document` 的注释）。
    fn collapse_unpinned(&mut self, keep: usize) {
        if self.tabs.iter().any(|doc| doc.pinned) || self.tabs.len() <= 1 {
            return;
        }
        let keep = keep.min(self.tabs.len() - 1);
        let doc = self.tabs.remove(keep);
        self.tabs.clear();
        self.tabs.push(doc);
        self.active = 0;
    }

    /// 切换某个标签的"固定"状态（标签右键菜单）。
    ///
    /// 固定后该标签不再被"打开其它文件"替换，标签条也转为显示全部标签（见
    /// `visible_tabs`）——这就是用户要的"固定时切换其它文件则显示多个标签页"。
    fn toggle_pin(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(doc) = self.tabs.get_mut(index) {
            doc.pinned = !doc.pinned;
        }
        // 取消最后一个"固定"之后要收口成单文件模式：否则会剩下一堆既看不见
        // （`visible_tabs` 只显示当前一个）又还在后台监听的标签，也再没有入口切过去。
        if !self.tabs.iter().any(|doc| doc.pinned) {
            self.collapse_unpinned(self.active);
        }
        cx.notify();
    }

    fn close_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_tab(self.active, window, cx);
    }

    /// 在标签之间循环（`delta` 为 +1 / -1）。
    fn cycle(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.len() < 2 {
            return;
        }
        let next = cycle_index(self.active, delta, self.tabs.len());
        self.activate(next, window, cx);
    }

    /// 放大 / 缩小编辑器与预览字号。`delta` 是逻辑 px 的步进。
    ///
    /// 与 Pebrel 的 `zoom_font_size` 同一套约定：一步 1 逻辑 px、钳在固定区间。
    /// 字号是**全局**的（工作区一份），不是每个标签一份——用户的"字太小"是对
    /// 整个应用的判断，切个标签就变回去会很别扭。
    fn bump_font_size(&mut self, delta: f32, cx: &mut Context<Self>) {
        let next = (self.font_size.round() + delta).clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
        if (next - self.font_size).abs() < f32::EPSILON {
            return;
        }
        self.font_size = next;
        self.status = Some(format!("字号 {next:.0}px"));
        self.sync_mono_font_size(cx);
        cx.notify();
    }

    /// `Ctrl+0`：字号回到默认值。
    fn reset_font_size(&mut self, cx: &mut Context<Self>) {
        if (self.font_size - DEFAULT_FONT_SIZE).abs() < f32::EPSILON {
            return;
        }
        self.font_size = DEFAULT_FONT_SIZE;
        self.status = Some(format!("字号 {DEFAULT_FONT_SIZE:.0}px"));
        self.sync_mono_font_size(cx);
        cx.notify();
    }

    /// 把当前字号同步给全局主题的 `mono_font_size`。
    ///
    /// 源码面编辑器自己有 `.text_size(...)`，但它**内部读主题**的那些地方——
    /// Markdown 预览里的围栏代码块与行内代码（组件库 `text/node.rs` 用的是
    /// `Theme::mono_font_size`）——不同步的话：放大字号后正文与源码面都跟着变大，
    /// 预览里的代码块仍是 13px，同一屏出现两种代码字号。
    fn sync_mono_font_size(&mut self, cx: &mut Context<Self>) {
        let size = self.font_size * crate::theme::MONO_FONT_RATIO;
        gpui_component::Theme::global_mut(cx).mono_font_size = px(size);
    }

    /// 预览面上的 `Ctrl+滚轮` 缩放字号；返回是否消费了这个滚轮事件。
    ///
    /// 为什么在预览面也要有：预览态下编辑器不参与渲染、没有节点持有焦点，`Ctrl+=`
    /// 这类按键绑定的派发目标是根节点（见 `activate` 的注释），键盘入口不可达；
    /// 而滚轮是鼠标事件，按命中位置派发，预览面接得住。所以预览的字号调整走滚轮。
    fn zoom_font_on_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(delta) = font_zoom_step(event) else {
            return false;
        };
        self.bump_font_size(delta, cx);
        true
    }

    /// 打开标签右键菜单（也用于标签的右键）。
    ///
    /// 先激活该标签再开菜单：菜单里的命令都作用在"当前标签"上，右键一个非激活
    /// 标签却不切过去，会让随后点「关闭」时关掉另一个文件——Pebrel 的标签菜单
    /// 同样是"先选中该行，再把命令交给根上那一份宿主"。
    fn open_tab_menu(
        &mut self,
        index: usize,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if index >= self.tabs.len() {
            return;
        }
        self.activate(index, window, cx);
        let tab_count = self.tabs.len();
        let pinned = self.tabs[index].pinned;
        let workspace = cx.entity().downgrade();
        let menu = PopupMenu::build(window, cx, move |mut menu, _window, _cx| {
            let run = |action: TabCommand| {
                let workspace = workspace.clone();
                move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut gpui::App| {
                    if let Some(workspace) = workspace.upgrade() {
                        workspace.update(cx, |this, cx| action.run(this, index, window, cx));
                    }
                }
            };
            menu = menu
                .item(
                    PopupMenuItem::new(if pinned { "取消固定" } else { "固定" })
                        .on_click(run(TabCommand::TogglePin)),
                )
                .separator()
                .item(
                    PopupMenuItem::new("关闭此标签")
                        .on_click(run(TabCommand::Close)),
                )
                .item(
                    PopupMenuItem::new("关闭其它标签")
                        .disabled(tab_count < 2)
                        .on_click(run(TabCommand::CloseOthers)),
                )
                .item(
                    PopupMenuItem::new("关闭右侧标签")
                        .disabled(index + 1 >= tab_count)
                        .on_click(run(TabCommand::CloseToRight)),
                )
                .separator()
                .item(
                    PopupMenuItem::new("复制完整路径")
                        .on_click(run(TabCommand::CopyPath)),
                )
                .item(
                    PopupMenuItem::new("在资源管理器中显示")
                        .on_click(run(TabCommand::Reveal)),
                )
                .item(
                    PopupMenuItem::new("用默认程序打开")
                        .on_click(run(TabCommand::OpenExternally)),
                );
            menu
        });
        // 菜单要有焦点才能接键盘（方向键 + 回车）；否则鼠标能点、键盘全瞎。
        menu.focus_handle(cx).focus(window, cx);
        // 菜单自己发 `DismissEvent`（点空白、Esc、选中条目后都会发）：据此把
        // 锚定状态清掉，否则下次开菜单时旧的会一起画出来。
        let subscription = cx.subscribe_in(&menu, window, |this, _, _: &DismissEvent, _, cx| {
            this.tab_menu = None;
            cx.notify();
        });
        self.tab_menu = Some(TabMenu { menu, position, index, _subscription: subscription });
        cx.notify();
    }

    /// 关闭 `index` 之外的全部标签。保留的那个成为激活项。
    fn close_other_tabs(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.tabs.len() {
            return;
        }
        let keep = self.tabs.remove(index);
        self.tabs.clear();
        self.tabs.push(keep);
        self.activate(0, window, cx);
    }

    /// 关闭 `index` 右侧的所有标签。
    fn close_tabs_to_right(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.tabs.len() {
            return;
        }
        self.tabs.truncate(index + 1);
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        }
        self.activate(self.active, window, cx);
    }

    /// 切到某个 Markdown 标题：把源码光标移到那一行的行首。
    ///
    /// 只动光标不改内容、不强制切面——在预览面上点大纲时也把源码光标移过去，
    /// 这样随后切回源码就停在那个标题上。`set_cursor_position` 内部会把光标滚进
    /// 视野（`move_to` → `scroll_to`），所以不需要额外的滚动。
    fn jump_to_heading(&mut self, heading: &outline::Heading, window: &mut Window, cx: &mut Context<Self>) {
        let Some(doc) = self.active_doc() else {
            return;
        };
        let input = doc.input.clone();
        let position = gpui_component::input::Position { line: heading.line as u32, character: 0 };
        input.update(cx, |state, cx| state.set_cursor_position(position, window, cx));
        cx.notify();
    }

    /// 开合 Markdown 大纲面板。
    fn toggle_outline(&mut self, cx: &mut Context<Self>) {
        if let Some(doc) = self.active_doc_mut() {
            doc.outline_open = !doc.outline_open;
            cx.notify();
        }
    }

    /// 图片查看区的当帧矩形（事件换算用）。没有打开文档时给一片零区域：
    /// 几何在零区域上的所有运算都是安全的 no-op。
    fn image_area_of_active(&self) -> Area {
        match self.active_doc() {
            Some(doc) => bounds_to_area(*doc.image_area.borrow()),
            None => (0.0, 0.0, 0.0, 0.0),
        }
    }

    /// 标签右键菜单的画法：`None` = 没开着。
    ///
    /// `deferred` + `anchored` + `with_priority`：浮层要盖在正文之上、不受父级
    /// 裁剪，并且锚定在右键时的鼠标位置（贴近窗口边缘时 `snap_to_window_with_margin`
    /// 会把它推回来，不会开在屏幕外）。
    fn render_tab_menu(&self) -> Option<gpui::AnyElement> {
        let state = self.tab_menu.as_ref()?;
        // 菜单挂着的时候标签集合变了（关标签、外部动作）就不再画——那一份的
        // 下标已经指不到原标签了。
        if state.index >= self.tabs.len() {
            return None;
        }
        Some(
            deferred(
                anchored()
                    .position(state.position)
                    .anchor(Anchor::TopLeft)
                    .snap_to_window_with_margin(px(8.0))
                    .child(state.menu.clone()),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// 文件树右键菜单的画法：`None` = 没开着。与 [`Self::render_tab_menu`] 同源。
    fn render_tree_menu(&self) -> Option<gpui::AnyElement> {
        let state = self.tree_menu.as_ref()?;
        Some(
            deferred(
                anchored()
                    .position(state.position)
                    .anchor(Anchor::TopLeft)
                    .snap_to_window_with_margin(px(8.0))
                    .child(state.menu.clone()),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// 打开文件树右键菜单。
    ///
    /// 菜单里的命令作用在 `path` 上；新建类命令作用在"右键那个目录"里（右键文件时
    /// 用它所在的目录）。菜单挂在工作区根上唯一一份 `deferred(anchored)`，不挂在
    /// 行上——与标签菜单同一个理由（`ElementId` 相同的菜单会互相叠阴影）。
    fn open_tree_menu(
        &mut self,
        path: PathBuf,
        is_dir: bool,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let workspace = cx.entity().downgrade();
        let target = path.clone();
        let menu = PopupMenu::build(window, cx, move |mut menu, _window, _cx| {
            let run = |action: TreeCommand| {
                let workspace = workspace.clone();
                let target = target.clone();
                move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut gpui::App| {
                    if let Some(workspace) = workspace.upgrade() {
                        workspace.update(cx, |this, cx| action.run(this, target.clone(), window, cx));
                    }
                }
            };
            let is_file = !is_dir;
            menu = menu
                .item(PopupMenuItem::new("新建文件").on_click(run(TreeCommand::NewFile)))
                .item(PopupMenuItem::new("新建文件夹").on_click(run(TreeCommand::NewFolder)))
                .separator()
                .when(is_file, |this| {
                    this.item(PopupMenuItem::new("打开").on_click(run(TreeCommand::Open)))
                        .item(PopupMenuItem::new("复制完整路径").on_click(run(TreeCommand::CopyPath)))
                })
                .item(PopupMenuItem::new("在资源管理器中显示").on_click(run(TreeCommand::Reveal)))
                .separator()
                .item(PopupMenuItem::new("重命名").on_click(run(TreeCommand::Rename)))
                .item(PopupMenuItem::new("删除").on_click(run(TreeCommand::Delete)));
            menu
        });
        menu.focus_handle(cx).focus(window, cx);
        let subscription = cx.subscribe_in(&menu, window, |this, _, _: &DismissEvent, _, cx| {
            this.tree_menu = None;
            cx.notify();
        });
        self.tree_menu = Some(TreeMenu { menu, position, _subscription: subscription });
        cx.notify();
    }

    /// 窗口标题跟着激活标签走；没有标签时退回树根路径。
    fn set_title(&self, window: &mut Window) {
        window.set_window_title(&format!(
            "{}\u{2009}—\u{2009}nebula-lite",
            self.window_title()
        ));
    }

    /// 写回磁盘（Ctrl+S）。
    fn save(&mut self, cx: &mut Context<Self>) {
        self.write_to_disk(false, cx);
    }

    /// 冲突下用户选择「覆盖」：用本地内容盖掉外部改动。
    fn save_over(&mut self, cx: &mut Context<Self>) {
        self.write_to_disk(true, cx);
    }

    /// 写盘的唯一出口。`overwrite_external` 标记"用户已就冲突做过裁决"。
    ///
    /// 只读文档直接拒绝而不是静默丢弃：用户按下保存却什么都没发生是最糟的反馈。
    fn write_to_disk(&mut self, overwrite_external: bool, cx: &mut Context<Self>) {
        let Some(doc) = self.active_doc_mut() else {
            return;
        };
        if doc.snapshot.read_only {
            self.status = Some(String::from("该文档是只读的，未写入磁盘"));
            cx.notify();
            return;
        }

        let text = doc.input.read(cx).value().to_string();
        let written = if overwrite_external {
            text_file::save_over(&doc.path, &doc.snapshot, &text)
        } else {
            text_file::save(&doc.path, &doc.snapshot, &text)
        };
        match written {
            Ok(()) => {
                // 更新基线并保留 BOM/CRLF 约定，后续保存与冲突检测都以新内容为准。
                match doc.snapshot.rebase(&text) {
                    Ok(()) => {
                        doc.dirty = false;
                        doc.conflict = false;
                        doc.missing = false;
                        self.status = Some(format!("已保存 {}", text_file::display_name(&doc.path)));
                        self.error = None;
                    },
                    Err(error) => {
                        self.status = Some(format!("已写入磁盘，但基线更新失败：{error}"));
                    },
                }
            },
            Err(error) => {
                if matches!(error, SaveError::Changed) {
                    // 冲突检测拦下了这次保存：把裁决出口摆出来，而不是只报一句错
                    // 让用户对着一个按不动的保存按钮发愣。
                    doc.conflict = true;
                }
                self.status = Some(format!("保存失败：{error}"));
            },
        }
        cx.notify();
    }

    /// 处理一次「目标文件被外部改动」的事件。
    ///
    /// 处置规则见 [`watch::decide`]：内容没变就不动（顺带挡掉我们自己保存触发的那
    /// 一次）；缓冲干净就直接采用磁盘内容；缓冲有未保存改动时只挂冲突标记——自动
    /// 重载绝不能吞掉用户刚敲进去的字。
    fn apply_external_change(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        // 按路径回找标签：旧标签关掉之后监听器漏过来的事件不能作用到别的文档上。
        let Some(index) = self.tabs.iter().position(|doc| doc.path == path) else {
            return;
        };
        let dirty = self.tabs[index].dirty;
        let baseline = self.tabs[index].snapshot.bytes.clone();

        let disk = match std::fs::read(path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => {
                // 权限不足、被独占占用之类照实说，不一律猜成"删除"。
                self.status = Some(format!("外部改动：读取失败（{error}）"));
                cx.notify();
                return;
            },
        };

        // 文件还在不在磁盘上，四种裁定都要用到：Unchanged 也意味着刚读到了内容。
        self.tabs[index].missing = disk.is_none();

        match watch::decide(dirty, disk.as_deref(), &baseline) {
            watch::Decision::Unchanged => {},
            watch::Decision::Missing => {
                // 缓冲原样留着：文件被外面删掉，不该连带丢掉用户没保存的内容。
                self.tabs[index].conflict = false;
                self.status = Some(String::from("文件已在外部被删除或改名（保存可重新写回）"));
            },
            watch::Decision::Conflict => {
                self.tabs[index].conflict = true;
                self.status = Some(String::from("文件在外部被改动，本地还有未保存的编辑"));
            },
            watch::Decision::Reload => {
                let bytes = disk.expect("Reload 判定必然带着读到的内容");
                self.adopt_disk_content(index, bytes, window, cx);
                self.status = Some(String::from("已重新加载：文件在外部被修改"));
            },
        }
        cx.notify();
    }

    /// 用户在冲突里选择「重新加载」：丢掉本地改动，采用磁盘内容。
    fn reload_from_disk(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.is_empty() {
            return;
        }
        let index = self.active;
        let path = self.tabs[index].path.clone();
        match std::fs::read(&path) {
            Ok(bytes) => {
                self.adopt_disk_content(index, bytes, window, cx);
                self.status = Some(String::from("已重新加载：本地改动被丢弃"));
            },
            Err(error) => {
                self.status = Some(format!("重新加载失败：{error}"));
            },
        }
        cx.notify();
    }

    /// 用磁盘上的内容替换某个标签的编辑缓冲。
    ///
    /// 只在两处调用：缓冲干净时的自动重载，以及用户在冲突里明确选了「重新加载」
    /// ——那时丢弃本地改动正是他的意思。
    fn adopt_disk_content(
        &mut self,
        index: usize,
        bytes: Vec<u8>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(doc) = self.tabs.get_mut(index) else {
            return;
        };
        // 用刚读到的那份字节建快照，不再读第二遍：中间文件可能又变了。
        let snapshot = text_file::load_bytes(&doc.path, bytes);
        let text = snapshot.text.clone();
        doc.snapshot = snapshot;
        doc.dirty = false;
        doc.conflict = false;
        doc.missing = false;
        // 图片被换成另一张时预览面要跟着换，否则显示的还是旧图。
        if doc.kind == FileKind::Image {
            doc.image = decode_image(&doc.path).ok();
            // 换图时重置缩放/平移：旧倍率套到一张尺寸不同的新图上只会得到一张
            // 位置莫名其妙的画面（几何的 `reset` 注释记了同一条）。
            doc.image_geometry.reset(doc.image.as_ref().and_then(image_dimensions));
        }
        // `set_value` 刻意不发 `InputEvent::Change`，脏标记只能在这里手动清；组件库
        // 内部仍会刷新搜索面板的匹配区间（`replace_text` 里调了 `update_search`）。
        // 视野会回到文件开头：`scroll_to` 在组件库里是 crate 内可见，公开 API 没有
        // 恢复阅读位置的办法。
        doc.input.update(cx, |state, cx| state.set_value(text, window, cx));
        // 外部重载会整段换掉缓冲，裸 URL 位置与 Notepad3 补充层全变了——两层都重扫。
        let text_now = doc.input.read(cx).value();
        doc.hotspots.set(hotspot_decorations(&text_now), cx);
        let Overlay { foreground, backgrounds } = compute_overlay(doc.language, &text_now);
        doc.overlay.set(foreground, cx);
        doc.has_backgrounds = backgrounds.iter().any(|span| span.ink.bg.is_some());
        *doc.backgrounds.borrow_mut() = backgrounds;
        *doc.bg_offset.borrow_mut() = None;
        cx.notify();
    }

    /// 用户在源码面上按下鼠标 / 敲入字符：点亮当前行高亮。
    ///
    /// 组件库自带的当前行高亮是**无条件**跟着光标走的（打开文件、滚动都会让某行
    /// 常亮），用户要的是"点了才亮"。所以那条令牌被关掉（见
    /// `hide_default_current_line`），改由这里在第一次交互时把文档的
    /// `caret_touched` 置起来，`render_source` 据此把那层淡黄自绘出来。
    fn note_editor_interaction(&mut self, cx: &mut Context<Self>) {
        let Some(doc) = self.active_doc_mut() else {
            return;
        };
        if doc.caret_touched {
            return;
        }
        doc.caret_touched = true;
        cx.notify();
    }

    /// 切换源码 / 预览面。
    ///
    /// 切回源码面时把焦点交还编辑器：预览态下编辑器不参与渲染、焦点已回落到根
    /// 节点（见 `activate` 的注释），不还焦点的话切回来还得再点一下正文才能打字。
    /// 切到预览面没有可聚焦的目标，什么都不做。
    fn toggle_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(doc) = self.active_doc_mut() else {
            return;
        };
        doc.preview = !doc.preview;
        let preview = doc.preview;
        let input = doc.input.clone();
        // 抓手平移层只活在源码面上，切面之后没人再更新 Ctrl 状态了——清掉，
        // 免得切回源码面时先挂着一层不该有的抓手（下一次鼠标移动也会自纠）。
        self.ctrl_down = false;
        self.pan_drag = None;
        if !preview {
            input.update(cx, |state, cx| state.focus(window, cx));
        }
        cx.notify();
    }

    /// 自绘标题栏：左侧是应用名与当前树根，右侧是组件库的最小化 / 最大化 / 关闭。
    ///
    /// 为什么必须自己画：`WindowOptions.titlebar` 用 `appears_transparent` 把系统
    /// 标题栏藏掉（Windows 上即 `hide_title_bar`，走 `WM_NCCALCSIZE` 抹掉非客户区），
    /// 于是那块区域整个交给应用——不画就**一个窗口按钮都没有**，只剩系统边框。
    /// 这正是之前的状况：`main.rs` 传了 `TitleBar::title_bar_options()` 却没渲染
    /// `TitleBar`，窗口右上角空着。
    ///
    /// 按钮的点击不用自己接管：组件库给每个控制块打了 `WindowControlArea`，
    /// Windows 的 `WM_NCHITTEST` 据此返回 `HTMINBUTTON` / `HTMAXBUTTON` / `HTCLOSE`，
    /// 由系统完成最小化、缩放与关闭（见 `gpui_windows/src/events.rs::handle_hit_test_msg`）。
    /// 所以这里只管摆位置，不要给按钮补 `on_click`——那会和系统行为重复。
    fn render_title_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = cx.theme();
        let muted = tokens.muted_foreground;
        // 左上角图标（点它开合侧栏）。图标缺失时回落到文字，窗口照样能用。
        let icon_size = px(16.0);
        let app_icon = self.app_icon.clone().map(|icon| {
            gpui::img(gpui::ImageSource::Render(icon))
                .w(icon_size)
                .h(icon_size)
                .flex_shrink_0()
        });
        let title = self.window_title();

        TitleBar::new()
            // 与标签条、侧栏同色：标题栏 + 标签条连成一条外壳色带，内容区才是纸面。
            // 默认的 `title_bar` 令牌回落到 `background`（纸面），会把标题栏也染成
            // 纸色，与下面那条外壳色带断开。
            .bg(crate::theme::shell_hsla())
            .border_b_0()
            .child(
                h_flex()
                    .h_full()
                    .min_w_0()
                    .items_center()
                    .gap(px(8.0))
                    .text_sm()
                    .child(
                        h_flex()
                            .id("title-app-icon")
                            .flex_shrink_0()
                            .items_center()
                            .justify_center()
                            .w(px(24.0))
                            .h(px(24.0))
                            .rounded(px(5.0))
                            .cursor_pointer()
                            // 关键：`occlude` 让这颗图标在命中测试里"吃掉"整条标题栏。
                            // 组件库给标题栏那条横带打了 `WindowControlArea::Drag`，
                            // Windows 的 `WM_NCHITTEST` 会把它整片识别成 `HTCAPTION`，
                            // 点击于是被系统当成"拖动窗口"、根本不会派发到应用——光有
                            // `on_click` 也点不动。`occlude`（`HitboxBehavior::BlockMouse`）
                            // 使命中测试在这颗图标的 hitbox 处**停止**、不再把后面的
                            // Drag 区域算进去，`on_hit_test_window_control` 因此回 `None`，
                            // 该点落回系统的默认 `HTCLIENT`，点击就送到了 `on_click`。
                            .occlude()
                            .hover(|this| this.bg(tokens.list_hover))
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_sidebar(cx)))
                            .when_some(app_icon, |this, icon| this.child(icon))
                            .when(self.app_icon.is_none(), |this| {
                                this.text_color(tokens.foreground).child("N")
                            }),
                    )
                    .child(div().min_w_0().truncate().text_color(muted).child(title)),
            )
    }

    /// 窗口标题栏里显示的那行字：**打开文件时给完整路径**，否则给树根。
    ///
    /// 用户要求"打开的文件在弹窗左上角直接显示完整路径"——就是这一行。路径剥掉
    /// `\\?\` verbatim 前缀（`shell::friendly_path`），否则摆给人看的是一串转义。
    fn window_title(&self) -> String {
        match self.active_doc() {
            Some(doc) => shell::friendly_path(&doc.path),
            None => shell::friendly_path(self.tree.root()),
        }
    }

    /// 点标题栏图标：开合侧栏。
    fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar_open = !self.sidebar_open;
        cx.notify();
    }

    /// 打开 / 关闭设置面板。
    fn toggle_settings(&mut self, cx: &mut Context<Self>) {
        self.settings_open = !self.settings_open;
        cx.notify();
    }

    fn close_settings(&mut self, cx: &mut Context<Self>) {
        self.settings_open = false;
        cx.notify();
    }

    /// 应用**界面 / 标题字体**：写进全局 `Theme::font_family`（标题栏、标签条、
    /// 侧栏、状态栏等一切没有显式指定字族的文字都吃它）。
    ///
    /// 只改全局主题是不够的——`render_source` / `render_preview` 里的编辑器与预览
    /// 各自钉了自己的字族（`editor_font`），所以改 UI 字体不会动到代码区，这是
    /// 有意的：两个设置项互不干扰。
    fn set_ui_font(&mut self, font: SharedString, cx: &mut Context<Self>) {
        self.ui_font = font.clone();
        gpui_component::Theme::global_mut(cx).font_family = font;
        // 改了就落盘。不落盘的话重启会回到默认值（这正是用户报的那个 bug）。
        self.save_settings();
        cx.notify();
    }

    /// 应用**编辑器 / 预览字体**：既写进全局 `Theme::mono_font_family`（Markdown
    /// 预览里的代码块读它），也存进 `self.editor_font` 供源码面编辑器与预览正文
    /// 显式取用。文件树图标列不受影响（见字段注释）。
    fn set_editor_font(&mut self, font: SharedString, cx: &mut Context<Self>) {
        self.editor_font = font.clone();
        gpui_component::Theme::global_mut(cx).mono_font_family = font;
        self.save_settings();
        cx.notify();
    }

    /// 某个字体槽位当前的顶置列表。
    ///
    /// 两个槽位各有一份顶置名单：字体列表是两列独立的候选，顶置也该各管各的
    /// （用户不会因为把界面字体顶上去，顺便把编辑器字体那列也改了顺序）。
    fn font_pins(&self, slot: FontSlot) -> &[String] {
        match slot {
            FontSlot::Ui => &self.ui_font_pins,
            FontSlot::Editor => &self.editor_font_pins,
        }
    }

    /// 顶置 / 取消顶置一个字体。
    ///
    /// 顶置的排到列表最前，顺序是"最后顶置的在最上面"（新顶置的插到最前）。
    /// 立刻落盘：顶置顺序就是用户排的偏好，重启不该丢。
    fn toggle_font_pin(&mut self, slot: FontSlot, name: String, cx: &mut Context<Self>) {
        let pins = match slot {
            FontSlot::Ui => &mut self.ui_font_pins,
            FontSlot::Editor => &mut self.editor_font_pins,
        };
        match pins.iter().position(|pin| *pin == name) {
            Some(index) => {
                pins.remove(index);
            }
            None => pins.insert(0, name),
        }
        self.save_settings();
        cx.notify();
    }

    /// 把两个字体槽位当前的选择与顶置项写进设置文件。
    ///
    /// 每次改动立即写：设置面板里的改动本来就是低频的，攒着一起写只会多出
    /// "什么时候该写"这个状态（还要处理关窗时的收尾）。
    fn save_settings(&self) {
        crate::settings::Settings {
            ui_font: Some(self.ui_font.to_string()),
            editor_font: Some(self.editor_font.to_string()),
            pinned_ui_fonts: self.ui_font_pins.clone(),
            pinned_editor_fonts: self.editor_font_pins.clone(),
        }
        .save();
    }

    /// 设置面板（目前只有"字体管理"）。
    ///
    /// 用自绘的居中卡片 + 半透明遮罩，而不是组件库的 `Dialog` / `Settings`：后者带
    /// 一整套按钮 / 侧栏导航的样式与状态机，对"两个下拉选字体"这种体量过重，而且
    /// `Dialog` 的命令式宿主会多一层 `Entity`。这里直接由 `render` 在根上叠一层
    /// `deferred`（和两个右键菜单同一手法），点遮罩关、点卡片内不关。
    fn render_settings(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let tokens = cx.theme();
        let card_bg = tokens.popover;
        let list_bg = tokens.background;
        let hairline = tokens.border;
        let muted = tokens.muted_foreground;
        let foreground = tokens.foreground;
        let hover_bg = tokens.list_hover;
        let active_bg = tokens.tab_active;
        let active_fg = tokens.tab_active_foreground;
        // 过滤词：两个字体列表共用（一次搜索同时筛两项的候选）。
        let query = self.font_filter.read(cx).value().trim().to_lowercase();

        // 每个字体候选一行：名字用**候选字体本身**渲染，一眼看出长什么样。当前选中
        // 的一行用水洗底色标出，点了就应用；行尾那颗图钉把常用字体顶到列表最前
        // （顶置顺序跟设置一起落盘，见 `toggle_font_pin`）。
        let font_list = |selected: &SharedString, apply: FontSlot| -> gpui::AnyElement {
            let icon_family = SharedString::from(fonts::REQUIRED_FONT_FAMILY);
            let pins = self.font_pins(apply);
            let ordered = crate::settings::ordered(&self.font_catalog, pins);
            let rows: Vec<gpui::AnyElement> = ordered
                .into_iter()
                .filter(|name| query.is_empty() || name.to_lowercase().contains(&query))
                .take(400)
                .map(|name| {
                    let is_pinned = pins.iter().any(|pin| pin == name);
                    let name = SharedString::from(name.to_owned());
                    let clicked = name.clone();
                    let toggled = name.clone();
                    let is_selected = &name == selected;
                    h_flex()
                        .id(SharedString::from(format!("font:{apply:?}:{name}")))
                        .h(px(30.0))
                        .w_full()
                        .min_w_0()
                        .items_center()
                        .gap(px(4.0))
                        .pl(px(10.0))
                        .pr(px(4.0))
                        .rounded(px(6.0))
                        .cursor_pointer()
                        .when(is_selected, |this| this.bg(active_bg))
                        .when(!is_selected, |this| this.hover(move |this| this.bg(hover_bg)))
                        .text_sm()
                        .text_color(if is_selected { active_fg } else { foreground })
                        // 名字按候选字体族渲染：选 `Microsoft YaHei UI` 就显示成雅黑，
                        // 选 Maple 就是等宽。行高固定，不同字体的行也整齐。
                        .font_family(name.clone())
                        .child(div().flex_1().min_w_0().truncate().child(name.clone()))
                        .when(is_selected, |this| {
                            this.child(div().flex_shrink_0().text_color(active_fg).child("✓"))
                        })
                        // 图钉：顶置 / 取消顶置。它必须是**独立的可点区域**，所以
                        // 先掐断冒泡，免得点图钉顺带把这一行的字体也应用了。
                        //
                        // 字族必须显式钉在 Maple：图钉是 Nerd Font 的私有使用区
                        // 码点，而这一行的字族是**候选字体**，拿它渲染图钉多半是
                        // 一个方框（文件树的图标列同理，见 `icons.rs`）。
                        //
                        // 钉住/没钉住用**同一个字形 + 不同颜色**区分，不换字形：
                        // Nerd Font 里没有对应的"空心图钉"，硬凑一个别的符号反而
                        // 认不出来。
                        .child(
                            h_flex()
                                .id(SharedString::from(format!("font-pin:{apply:?}:{name}")))
                                .w(px(20.0))
                                .h(px(20.0))
                                .flex_shrink_0()
                                .items_center()
                                .justify_center()
                                .rounded(px(4.0))
                                .font_family(icon_family.clone())
                                .text_color(if is_pinned { active_fg } else { muted })
                                .when(!is_pinned, |this| {
                                    this.hover(move |this| this.text_color(foreground))
                                })
                                .cursor_pointer()
                                .child(icons::ICON_PIN)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.toggle_font_pin(apply, toggled.to_string(), cx);
                                })),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| match apply {
                            FontSlot::Ui => this.set_ui_font(clicked.clone(), cx),
                            FontSlot::Editor => this.set_editor_font(clicked.clone(), cx),
                        }))
                        .into_any_element()
                })
                .collect();

            let list = v_flex()
                .id(SharedString::from(format!("font-list:{apply:?}")))
                .w_full()
                .h(px(168.0))
                .min_h_0()
                .rounded(px(8.0))
                .border_1()
                .border_color(hairline)
                .bg(list_bg)
                .p(px(4.0))
                .children(rows);

            // ⚠ **两个列表必须在不同的源码行上调用 `overflow_y_scrollbar`。**
            //
            // 组件库的 `Scrollable::new` 用 `ElementId::CodeLocation(*Location::caller())`
            // ——也就是**调用点的源码位置**——当滚动状态的 key。同一个调用点渲染出
            // 两份（这里正是如此：一个闭包被两个槽位各调一次），两份就共享同一个
            // `ScrollHandle`，表现成"滚其中一个字体列表，另一个跟着一起滚"。
            // 拆成两个 match 分支 = 两个源码位置 = 两份独立的滚动状态。
            match apply {
                FontSlot::Ui => list.overflow_y_scrollbar().into_any_element(),
                FontSlot::Editor => list.overflow_y_scrollbar().into_any_element(),
            }
        };

        // 一项设置：标题 + 当前值 + 候选列表。
        let section = |title: &str, current: &SharedString, apply: FontSlot| -> gpui::AnyElement {
            v_flex()
                .w_full()
                .min_w_0()
                .gap(px(6.0))
                .child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .child(title.to_string()),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child(format!("当前：{current}")),
                        ),
                )
                .child(font_list(current, apply))
                .into_any_element()
        };


        let backdrop = div()
            .id("settings-backdrop")
            .absolute()
            .inset_0()
            .bg(gpui::hsla(0.0, 0.0, 0.0, 0.35))
            // 点遮罩关面板。`occlude` 让遮罩吃掉点击，下面的编辑器不会被动到。
            .occlude()
            .on_click(cx.listener(|this, _, _, cx| this.close_settings(cx)))
            .flex()
            .items_center()
            .justify_center()
            .child(
                v_flex()
                    .id("settings-card")
                    .w(px(520.0))
                    // 高度上限要容得下"标题 + 三行说明 + 过滤框 + 两段字体列表"：
                    // 两段各 168 的行高加上间距约 400，整体算下来要 ~580。原来的
                    // 560 在只有一行说明时勉强够，加了两行说明就会把下面那段列表
                    // 顶出卡片（卡片没有 `overflow_hidden`，溢出会画在圆角外面）。
                    .max_h(px(640.0))
                    .rounded(px(12.0))
                    .border_1()
                    .border_color(hairline)
                    .bg(card_bg)
                    .shadow_lg()
                    .p(px(16.0))
                    .gap(px(14.0))
                    // 点卡片内部不关：掐断冒泡，别让遮罩的 `on_click` 收到。
                    .on_click(cx.listener(|_, _, _, cx| cx.stop_propagation()))
                    .child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .justify_between()
                            .child(div().text_base().font_weight(gpui::FontWeight::SEMIBOLD).child("设置"))
                            .child(
                                h_flex()
                                    .id("settings-close")
                                    .w(px(24.0))
                                    .h(px(24.0))
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(6.0))
                                    .cursor_pointer()
                                    .text_color(muted)
                                    .hover(move |this| this.bg(hover_bg).text_color(foreground))
                                    .child(Icon::new(IconName::Close).small())
                                    .on_click(cx.listener(|this, _, _, cx| this.close_settings(cx))),
                            ),
                    )
                    // 一句话说清三件事：改动立即生效、会被记住（用户报过"改了没
                    // 记住"）、以及图钉是干什么的。
                    .child(
                        v_flex()
                            .gap(px(2.0))
                            .text_xs()
                            .text_color(muted)
                            .child("字体管理 · 改动即时生效，并自动记住（重启后仍是这次选的）")
                            .child("行尾的图钉把常用字体顶到列表最前；两列各有各的顶置顺序")
                            .child("设置写在 %APPDATA%\\nebula-lite\\settings.json"),
                    )
                    .child(
                        Input::new(&self.font_filter)
                            .w_full()
                            .min_w_0()
                            .small()
                            .rounded(px(6.0)),
                    )
                    .child(
                        v_flex()
                            // 不写 `flex_1`：卡片是内容撑高（只有 `max_h`），父级没有
                            // 确定高度，`flex_1` + `min_h_0` 会把这一列塌成 0——两个列表
                            // 就都看不见了（踩过）。两个列表各自是固定高度（见 `font_list`），
                            // 所以这里按内容排布即可。
                            .w_full()
                            .min_w_0()
                            .gap(px(16.0))
                            .child(section("界面 / 标题字体", &self.ui_font, FontSlot::Ui))
                            .child(section(
                                "编辑器 / 预览字体",
                                &self.editor_font,
                                FontSlot::Editor,
                            )),
                    ),
            );

        deferred(backdrop).with_priority(2).into_any_element()
    }

    /// 侧栏拖动的移动 / 松开层：一张覆盖整窗、无 hitbox 的 canvas，在 `paint` 里
    /// 注册窗口级鼠标监听。见 `render` 里调用处的说明。
    fn render_sidebar_drag_layer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let workspace = cx.entity().downgrade();
        canvas(
            |bounds, _window, _cx| bounds,
            move |_bounds, _state, window, _cx| {
                window.on_mouse_event({
                    let workspace = workspace.clone();
                    move |event: &MouseMoveEvent, _, _, cx| {
                        if let Some(workspace) = workspace.upgrade() {
                            workspace.update(cx, |this, cx| {
                                if this.sidebar_dragging {
                                    this.resize_sidebar(f32::from(event.position.x), cx);
                                }
                            });
                        }
                    }
                });
                window.on_mouse_event({
                    let workspace = workspace.clone();
                    move |_: &MouseUpEvent, phase, _, cx| {
                        if !phase.bubble() {
                            return;
                        }
                        if let Some(workspace) = workspace.upgrade() {
                            workspace.update(cx, |this, cx| {
                                if this.sidebar_dragging {
                                    this.sidebar_dragging = false;
                                    cx.notify();
                                }
                            });
                        }
                    }
                });
            },
        )
        .absolute()
        .inset_0()
    }

    /// Ctrl + 拖拽水平平移源码面用的那一层。
    ///
    /// 两个职责：
    ///
    /// 1. **注册窗口级监听**（修饰键、鼠标移动、松开）。挂在 canvas 的 `paint` 里
    ///    ——和侧栏分割线同一个手法：`window.on_mouse_event` 是**全局**的，指针
    ///    拖到编辑器外面、甚至拖到侧栏上都收得到；而 `div().on_mouse_move` 只在
    ///    指针悬停该元素时触发，拖出边界就丢事件，拖到一半会卡住。
    /// 2. **按住 Ctrl 时盖一层"抓手"**。它有两个作用：挡住编辑器的文本选择
    ///    （平移时不该顺手选中一段文字），以及把鼠标换成抓手图标。
    ///
    /// 为什么用 `on_modifiers_changed` 而不是"鼠标动一下看一眼 `event.modifiers`"：
    /// 后者要等鼠标先动，按/松 Ctrl 的瞬间图标不变。两条都留着——修饰键事件在某些
    /// 输入法状态下不一定发得出来，鼠标事件里带的修饰键状态是兜底。
    fn render_pan_layer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let workspace = cx.entity().downgrade();
        let listeners = canvas(
            |bounds, _window, _cx| bounds,
            move |_bounds, _state, window, _cx| {
                window.on_modifiers_changed({
                    let workspace = workspace.clone();
                    move |event: &gpui::ModifiersChangedEvent, _, cx| {
                        if let Some(workspace) = workspace.upgrade() {
                            workspace.update(cx, |this, cx| {
                                this.set_ctrl_down(event.modifiers.control, cx)
                            });
                        }
                    }
                });
                window.on_mouse_event({
                    let workspace = workspace.clone();
                    move |event: &MouseMoveEvent, _, _, cx| {
                        if let Some(workspace) = workspace.upgrade() {
                            workspace.update(cx, |this, cx| {
                                // 兜底：修饰键事件没来的时候，靠鼠标移动里带的
                                // 修饰键状态也能把抓手对上。
                                this.set_ctrl_down(event.modifiers.control, cx);
                                if this.pan_drag.is_some() && event.dragging() {
                                    this.pan_to(f32::from(event.position.x), cx);
                                }
                            });
                        }
                    }
                });
                window.on_mouse_event({
                    let workspace = workspace.clone();
                    move |_: &MouseUpEvent, phase, _, cx| {
                        if !phase.bubble() {
                            return;
                        }
                        if let Some(workspace) = workspace.upgrade() {
                            workspace.update(cx, |this, cx| this.end_pan(cx));
                        }
                    }
                });
            },
        )
        .absolute()
        .inset_0();

        // 光标换成"抓手"：正拖着是攥住的（grabbing），只按住 Ctrl 还没按下时是张开
        // 的（grab）。gpui 的 `CursorStyle` 里这两个是 `ClosedHand` / `OpenHand`，
        // 用通用的 `.cursor(...)` 设——宏生成的那批 `cursor_*` 没有对应的名字。
        let dragging = self.pan_drag.is_some();
        let grabber = self.ctrl_down.then(|| {
            div()
                .id("pan-grabber")
                .absolute()
                .inset_0()
                // `occlude`：这层要真的挡住编辑器的鼠标事件（否则一边平移一边选文本）。
                // 松开 Ctrl 时整层消失，编辑器照旧。
                .occlude()
                .cursor(if dragging {
                    gpui::CursorStyle::ClosedHand
                } else {
                    gpui::CursorStyle::OpenHand
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, event: &MouseDownEvent, _, cx| {
                        this.begin_pan(f32::from(event.position.x), cx)
                    }),
                )
        });

        div()
            .absolute()
            .inset_0()
            .child(listeners)
            .children(grabber)
    }

    /// 记录 Ctrl 的按下状态。
    ///
    /// 只在状态**真的**变了时重渲染：按着 Ctrl 打字时，每个按键事件都会带一遍修饰
    /// 键状态，逐次 notify 就是白烧。
    fn set_ctrl_down(&mut self, down: bool, cx: &mut Context<Self>) {
        if self.ctrl_down == down {
            return;
        }
        self.ctrl_down = down;
        if !down {
            // 松开 Ctrl 等于放弃这次平移：留着状态的话，下次按 Ctrl 会"接着上次"。
            self.pan_drag = None;
        }
        cx.notify();
    }

    /// 开始平移：记下按下点的鼠标 x 与编辑器**当时**的横向偏移。
    ///
    /// 存绝对锚点而不是逐帧增量——`set_scroll_offset` 延后一帧才生效，逐帧累加会
    /// 把这份延迟与舍入误差一起累进结果里。
    fn begin_pan(&mut self, mouse_x: f32, cx: &mut Context<Self>) {
        let Some(doc) = self.active_doc() else {
            return;
        };
        let offset = doc.input.read(cx).scroll_offset();
        self.pan_drag = Some((mouse_x, f32::from(offset.x)));
        cx.notify();
    }

    /// 拖动中：把鼠标位移换算成横向滚动偏移。
    ///
    /// 方向与"抓住纸面拖"一致：鼠标往右拖，内容跟着往右走，于是滚动偏移**变小**。
    /// 只动 x——纵向仍归滚轮。
    fn pan_to(&mut self, mouse_x: f32, cx: &mut Context<Self>) {
        let Some((anchor_x, anchor_offset)) = self.pan_drag else {
            return;
        };
        let Some(doc) = self.active_doc() else {
            return;
        };
        let input = doc.input.clone();
        let y = input.read(cx).scroll_offset().y;
        // 组件库的 `set_scroll_offset` 会把偏移钳进合法区间（并在下一次布局时生效），
        // 所以这里不做边界数学，直接把目标值交出去。
        let x = anchor_offset - (mouse_x - anchor_x);
        input.update(cx, |state, cx| {
            state.set_scroll_offset(gpui::point(px(x), y), cx);
        });
    }

    /// 松开左键：结束平移。
    fn end_pan(&mut self, cx: &mut Context<Self>) {
        if self.pan_drag.take().is_some() {
            cx.notify();
        }
    }

    /// 拖动侧栏分割线时，把鼠标的 x 位置当作侧栏宽度（钳进合法区间）。
    ///
    /// 用"分割线按下立 flag、窗口级监听里按需改宽度"的做法，而不是组件库那套
    /// `ResizablePanelGroup`：本应用的侧栏状态（宽度）要进 `Workspace`
    /// 参与"内容区宽度显式算"那条式子（见 `render`），拆成组件库的独立状态反而
    /// 要两处同步。
    fn resize_sidebar(&mut self, x: f32, cx: &mut Context<Self>) {
        let width = x.clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH);
        if (width - self.sidebar_width).abs() < 0.5 {
            return;
        }
        self.sidebar_width = width;
        cx.notify();
    }

    /// 侧栏文件树。
    ///
    /// 两处性能设计，都照抄 Pebrel：
    /// - **行缓存**：`sync_rows` 只在过滤词或树版本变化时展平，渲染只读缓存。
    ///   展平要同步读盘，放进每帧渲染就是每帧在 UI 线程上走目录。
    /// - **`uniform_list` 虚拟化**：只渲染可视区那十几行，而不是把整棵树都建成
    ///   元素交给 GPUI 布局。Pebrel 的新壳文件树同样用 `uniform_list`
    ///   （`gpui_shell/workspace/file_tree.rs:546`），它记录的正是"两套滚动模型
    ///   打架"的旧问题：`skip(scroll)` 与 `overflow_y_scrollbar` 各算各的。
    ///   这里 `uniform_list` 自己就是滚动容器，滑块读同一个 handle。
    ///
    /// 行缓存的同步（`sync_rows`）与"滚到选中行"由 [`Self::render`] 在调用本函数
    /// 之前做完：那两件都需要 `&mut self`，而渲染这里只借用视图。
    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = cx.theme();
        let hairline = tokens.border;
        let muted = tokens.muted_foreground;
        let foreground = tokens.foreground;
        let hover_bg = tokens.list_hover;
        // 行数量先取出来：下面 `uniform_list` 的闭包只借用视图，不再借用 `self`。
        let row_count = self.rows.len();

        v_flex()
            .w(px(self.sidebar_width))
            .h_full()
            .flex_shrink_0()
            // 右侧那条竖线由外层分割线（`sidebar-divider`）画，这里**不再**加
            // `border_r_1`：两条 1px 并排会看成分隔线变粗。
            .bg(crate::theme::shell_hsla())
            // 表头一行：左边「目录」图标按钮（点它选一个目录/文件打开），右边是过滤框。
            // 去掉了原来那条"文件"标题——用户要求，而且它只是一行死标签、没有信息量。
            .child(
                h_flex()
                    .h(px(HEADER_HEIGHT))
                    .w_full()
                    .min_w_0()
                    .flex_shrink_0()
                    .items_center()
                    .gap(px(6.0))
                    .pl(px(6.0))
                    .pr(px(BAND_PAD_X))
                    .border_b_1()
                    .border_color(hairline)
                    .child(
                        h_flex()
                            .id("open-folder")
                            .flex_shrink_0()
                            .items_center()
                            .justify_center()
                            .w(px(26.0))
                            .h(px(24.0))
                            .rounded(px(6.0))
                            .cursor_pointer()
                            .text_color(muted)
                            .hover(move |this| this.bg(hover_bg).text_color(foreground))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_folder(window, cx)
                            }))
                            .child(Icon::new(IconName::FolderOpen).small()),
                    )
                    // 设置：打开应用设置面板（目前只有"字体管理"）。放在目录按钮右边、
                    // 过滤框左边——用户要求"搜索框左侧加一个设置图标"。
                    .child(
                        h_flex()
                            .id("open-settings")
                            .flex_shrink_0()
                            .items_center()
                            .justify_center()
                            .w(px(26.0))
                            .h(px(24.0))
                            .rounded(px(6.0))
                            .cursor_pointer()
                            .text_color(muted)
                            .hover(move |this| this.bg(hover_bg).text_color(foreground))
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_settings(cx)))
                            .child(Icon::new(IconName::Settings).small()),
                    )
                    // 过滤框：大树里按名字找文件。空查询时不折叠，行为与没有它一样。
                    .child(
                        div().flex_1().min_w_0().child(
                            Input::new(&self.filter)
                                .w_full()
                                .min_w_0()
                                .small()
                                .bordered(false)
                                .focus_bordered(false)
                                .rounded(px(6.0)),
                        ),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .w_full()
                    .min_w_0()
                    .min_h_0()
                    .relative()
                    .overflow_hidden()
                    .child(
                        uniform_list(
                            "file-tree-rows",
                            row_count,
                            cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
                                range.map(|index| this.render_tree_row(index, cx)).collect()
                            }),
                        )
                        // 内边距照旧 6px（`uniform_list` 的 prepaint 会计入 padding，
                        // 见 uniform_list.rs 的 `bounds.origin + padding`）。
                        .p(px(6.0))
                        // 高度交给外层 flex 解析出的定高；不要在这里再挂
                        // `overflow_*`，见上面的注释。
                        .size_full()
                        .track_scroll(&self.tree_scroll),
                    ),
            )
    }

    /// 文件树的一行。索引是**展平后**的行序（`uniform_list` 只给可视区）。
    ///
    /// 每行高度必须精确等于 `ROW_HEIGHT`：`uniform_list` 只测量第一项就按定高
    /// 排布其余项，行高不一致会让后面的行错位。
    fn render_tree_row(&self, index: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
        let tokens = cx.theme();
        let muted = tokens.muted_foreground;
        let foreground = tokens.foreground;
        let selected_bg = tokens.sidebar_accent;
        let hover_bg = tokens.list_hover;
        // 行被删掉（缓存比列表晚一帧失效）时补一个等高空行，避免整列为空。
        let Some(row) = self.rows.get(index) else {
            return div().h(px(ROW_HEIGHT)).w_full().into_any_element();
        };
        let path = row.path.clone();
        let click_path = path.clone();
        let is_dir = row.is_dir;
        let is_selected = self.tree.selected.as_deref() == Some(path.as_path());
        let filtering = !self.rows_query.trim().is_empty();
        // 图标与折叠箭头都走 Nerd Font 私有使用区码点（见 `icons` 模块），
        // 必须显式指定 Maple 字体族——界面主字体是微软雅黑，那个字体里没有
        // 这些码点，不指定就是一片方框。Pebrel 的 GPUI 文件树同样把图标列的
        // font_family 钉在 REQUIRED_FONT_FAMILY 上，理由相同。
        let icon_family = SharedString::from(fonts::REQUIRED_FONT_FAMILY);
        // 只有目录有折叠箭头；文件那一列留空，靠图标列对齐。过滤态整棵树
        // 都算展开，箭头一律指下。
        let chevron = if row.is_dir && !filtering {
            icons::chevron_icon(row.expanded)
        } else if row.is_dir {
            icons::chevron_icon(true)
        } else {
            ""
        };
        // 目录用文件夹图标、文件按扩展名挑图标（未知扩展名回落通用文件图标）。
        let glyph = if is_dir {
            icons::folder_icon(row.expanded || filtering)
        } else {
            icons::file_type_icon(&row.name)
        };
        let color = if is_selected || row.is_dir { foreground } else { muted };
        let depth = row.depth;
        let name = row.name.clone();
        // 这一行是否正在被就地重命名：是就拿输入框换掉名字标签（新建 / 重命名共用）。
        let is_renaming = self.renaming.as_ref().is_some_and(|r| r.path == path);
        let menu_path = path.clone();

        h_flex()
            // 行的 id 用下标而不是路径：`uniform_list` 的 id 只需要在本列表内
            // 唯一，下标天然唯一且不受路径里的怪字符影响（Pebrel 同款）。
            .id(SharedString::from(format!("tree-row:{index}")))
            .h(px(ROW_HEIGHT))
            .w_full()
            .flex_shrink_0()
            .pl(px(10.0 + depth as f32 * INDENT))
            .pr(px(8.0))
            .gap(px(4.0))
            .items_center()
            .rounded(px(6.0))
            .when(is_selected, |this| this.bg(selected_bg))
            .when(!is_selected, |this| this.hover(move |this| this.bg(hover_bg)))
            .cursor_pointer()
            .text_sm()
            .text_color(color)
            // 右键菜单：与标签菜单同一套宿主约定（挂在根上、按路径回找）。
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.open_tree_menu(menu_path.clone(), is_dir, event.position, window, cx);
                }),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                // 重命名那一行里的点击归输入框（不要顺手打开文件 / 折叠目录）。
                if this.renaming.as_ref().is_some_and(|r| r.path == click_path) {
                    return;
                }
                if is_dir {
                    // 过滤态下点击目录的展开/收起**看不见效果**（此时整棵树都
                    // 在显示），但仍照常翻转展开集合：清空过滤词后就落在用户
                    // 刚才点出来的那个状态上，不会白点。
                    this.tree.toggle(&click_path);
                    // 展开集合变了，行缓存要作废（否则下次渲染还是旧行）。
                    this.invalidate_tree();
                    cx.notify();
                } else {
                    this.open(click_path.clone(), window, cx);
                }
            }))
            .child(
                div()
                    .w(px(12.0))
                    .flex_shrink_0()
                    .font_family(icon_family.clone())
                    .text_color(muted)
                    .child(chevron),
            )
            .child(
                div()
                    .w(px(16.0))
                    .flex_shrink_0()
                    .font_family(icon_family)
                    .child(glyph),
            )
            // 重命名中：名字位置换成输入框；否则是普通的名字标签。
            .when(is_renaming, |this| {
                let input = self.renaming.as_ref().map(|r| r.input.clone());
                match input {
                    Some(input) => this.child(
                        div().flex_1().min_w_0().child(
                            Input::new(&input)
                                .small()
                                .bordered(false)
                                .focus_bordered(false)
                                .w_full()
                                .min_w_0(),
                        ),
                    ),
                    None => this,
                }
            })
            .when(!is_renaming, |this| {
                this.child(div().flex_1().min_w_0().truncate().child(name))
            })
            .into_any_element()
    }

    /// 标签条上应当显示的标签下标。
    ///
    /// **默认（没有任何标签被固定）只显示当前这一个**：用户要的就是"每次切换文件
    /// 只显示一个文件、并把完整路径显示出来"，而不是一排标签。一旦有标签被固定
    /// （右键菜单「固定」），就显示全部——那时固定项与当前项并存，形成多个标签。
    fn visible_tabs(&self) -> Vec<usize> {
        visible_tab_indices(
            &self.tabs.iter().map(|doc| doc.pinned).collect::<Vec<_>>(),
            self.active,
        )
    }

    /// 标签条：默认只显示当前文档一个标签，右侧是「大纲 / 源码-预览」动作。
    ///
    /// 标签多了横向滚动——外面这层 `overflow_x_scrollbar` 是滚动容器，内层
    /// `h_flex` 才是标签的真实容器（同侧栏那条约定的反向应用：滚动容器不能直接
    /// 当 flex 容器用，否则换行与收缩语义都会被 `Scrollable` 重建的外层吃掉）。
    /// 右侧的动作区固定在右边，不随标签滚动（用户要求"移到标签栏最右侧、右对齐"）。
    fn render_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = cx.theme();
        let hairline = tokens.border;
        let active_bg = tokens.tab_active;
        let active_fg = tokens.tab_active_foreground;
        let idle_fg = tokens.tab_foreground;
        let hover_bg = tokens.list_hover;
        let visible = self.visible_tabs();
        // 标签只显示**文件名**，不显示完整路径——用户明确要求"右侧编辑器上面的
        // 文件名只显示其文件名，不显示完整路径地址"。单标签时给纯文件名；多标签
        // 时 `tab_labels` 会在重名（不同目录下的同名文件）时补一层父目录名加以区分。
        let single = visible.len() <= 1;
        let labels = tab_labels(self.tabs.iter().map(|doc| doc.path.as_path()));

        let tabs = visible.into_iter().map(|index| {
            let is_active = index == self.active;
            let doc = &self.tabs[index];
            let label = if single { text_file::display_name(&doc.path) } else { labels[index].clone() };
            let dirty = doc.dirty;
            let pinned = doc.pinned;
            let close_fg = idle_fg;

            h_flex()
                .id(SharedString::from(format!("tab:{index}")))
                .h(px(TAB_HEIGHT - 6.0))
                .flex_shrink_0()
                .items_center()
                .gap(px(6.0))
                .pl(px(10.0))
                .pr(px(6.0))
                .rounded(px(6.0))
                // 标签**默认透明**：一整条横带排满方块底色看着很吵，而且"当前在
                // 哪个文件"不需要每个标签都抢一次注意力。只有两种情况才给底色——
                // 这个标签被**固定**了（它是一直留在标签条上的常驻项），或者鼠标
                // **悬停**在它上面（提示这里可点）。
                .when(pinned, |this| this.bg(active_bg))
                .hover(move |this| this.bg(hover_bg))
                .cursor_pointer()
                .text_sm()
                // 底色让给了"固定 / 悬停"这两个状态，于是**激活态改由文字本身说
                // 明**：激活的字色更重、字重加粗。这是这一版标签可读性的关键——
                // 少了底色之后，只剩字色深浅的话在浅色水洗上看不出差别。
                .when(is_active, |this| this.font_weight(gpui::FontWeight::SEMIBOLD))
                .text_color(if is_active { active_fg } else { idle_fg })
                .on_click(cx.listener(move |this, _, window, cx| this.activate(index, window, cx)))
                // 右键开菜单。菜单由工作区根上唯一一份 `deferred(anchored)` 画，
                // 不挂在标签行上（理由见 `TabMenu` 的注释）。
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        this.open_tab_menu(index, event.position, window, cx);
                    }),
                )
                // 固定标记：固定后切文件不会替换它，一眼能看出这个标签"钉住了"。
                .when(pinned, |this| {
                    this.child(div().flex_shrink_0().text_color(active_fg).child("⏷"))
                })
                .child(
                    div()
                        .max_w(px(if single { 10_000.0 } else { TAB_LABEL_WIDTH }))
                        .min_w_0()
                        .truncate()
                        .child(label),
                )
                .when(dirty, |this| this.child(div().flex_shrink_0().child("•")))
                .child(
                    h_flex()
                        .id(SharedString::from(format!("tab-close:{index}")))
                        .w(px(16.0))
                        .h(px(16.0))
                        .flex_shrink_0()
                        .items_center()
                        .justify_center()
                        .rounded(px(4.0))
                        .text_color(close_fg)
                        .hover(move |this| this.bg(hover_bg).text_color(active_fg))
                        .cursor_pointer()
                        .child("×")
                        // 关标签不该顺带激活它：先掐断冒泡，父标签的 on_click 就收不到。
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.close_tab(index, window, cx);
                        })),
                )
        });

        h_flex()
            .h(px(TAB_HEIGHT))
            .w_full()
            .min_w_0()
            .flex_shrink_0()
            .items_center()
            .border_b_1()
            .border_color(hairline)
            .bg(crate::theme::shell_hsla())
            // 左：标签（自己横向滚动）。高度必须显式给：`Scrollable` 渲染出的根 div
            // 是 `size_full()`，不钉住高度它会把整条横带吃光。
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h(px(TAB_HEIGHT))
                    .overflow_x_scrollbar()
                    .child(
                        h_flex()
                            .h(px(TAB_HEIGHT))
                            .items_center()
                            .gap(px(4.0))
                            .pl(px(8.0))
                            .children(tabs),
                    ),
            )
            // 右：文档动作（大纲 / 源码-预览 ……），固定在最右端、右对齐。
            .child(self.render_actions(cx))
    }

    /// 标签条右端的文档动作区：大纲 / 源码-预览 / 冲突裁决 / 保存 / 图片复位。
    ///
    /// 这些原本占一整行（内容区顶栏），现在搬到标签条右侧、省掉那一行——用户要的
    /// 就是"去掉文件名那一行"。没有打开文档时给一个空元素（不占位置）。
    fn render_actions(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(doc) = self.active_doc() else {
            return div().into_any_element();
        };
        let tokens = cx.theme();
        let hairline = tokens.border;
        let foreground = tokens.foreground;
        let hover = tokens.list_hover;
        let conflict = doc.conflict;
        let missing = doc.missing;
        let dirty = doc.dirty;
        let read_only = doc.snapshot.read_only;
        let preview = doc.preview;
        let has_preview = doc.kind.has_preview();
        let outline_open = doc.outline_open;
        let has_headings = doc.kind == FileKind::Markdown
            && !outline::headings(&doc.input.read(cx).value()).is_empty();
        let reset_image =
            doc.kind == FileKind::Image && doc.image_geometry.zoom() != 1.0;
        let restart = crate::theme::shell_hsla();

        h_flex()
            .flex_shrink_0()
            .h_full()
            .items_center()
            .gap(px(6.0))
            .pl(px(6.0))
            .pr(px(BAND_PAD_X))
            // 未保存标记：原来在内容区顶栏那一行，现在并到动作区左边。
            .when(dirty, |this| this.child(div().text_sm().text_color(tokens.muted_foreground).child("•")))
            .when(has_headings, |this| {
                this.child(pill(
                    SharedString::from("toggle-outline"),
                    if outline_open { "收起大纲" } else { "大纲" },
                    foreground,
                    restart,
                    hover,
                    hairline,
                    cx.listener(|this, _, _, cx| this.toggle_outline(cx)),
                ))
            })
            .when(conflict, |this| {
                this.child(pill(
                    SharedString::from("reload-doc"),
                    "重新加载",
                    foreground,
                    restart,
                    hover,
                    hairline,
                    cx.listener(|this, _, window, cx| this.reload_from_disk(window, cx)),
                ))
                .child(pill(
                    SharedString::from("overwrite-doc"),
                    "覆盖",
                    foreground,
                    restart,
                    hover,
                    hairline,
                    cx.listener(|this, _, _, cx| this.save_over(cx)),
                ))
            })
            .when((dirty || missing) && !read_only && !conflict, |this| {
                this.child(pill(
                    SharedString::from("save-doc"),
                    "保存",
                    foreground,
                    restart,
                    hover,
                    hairline,
                    cx.listener(|this, _, _, cx| this.save(cx)),
                ))
            })
            .when(reset_image, |this| {
                this.child(pill(
                    SharedString::from("reset-image"),
                    format!("复位 {:.0}%", doc.image_geometry.zoom() * 100.0),
                    foreground,
                    restart,
                    hover,
                    hairline,
                    cx.listener(|this, _, _, cx| {
                        if let Some(doc) = this.active_doc_mut()
                            && doc.image_geometry.reset_view()
                        {
                            cx.notify();
                        }
                    }),
                ))
            })
            .when(has_preview, |this| {
                this.child(pill(
                    SharedString::from("toggle-face"),
                    if preview { "源码" } else { "预览" },
                    foreground,
                    restart,
                    hover,
                    hairline,
                    cx.listener(|this, _, window, cx| this.toggle_preview(window, cx)),
                ))
            })
            .into_any_element()
    }

    fn render_content(&self, width: gpui::Pixels, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = cx.theme();
        let hairline = tokens.border;
        let foreground = tokens.foreground;
        let muted = tokens.muted_foreground;

        let Some(doc) = self.active_doc() else {
            return v_flex()
                .w(width)
                .h_full()
                .flex_shrink_0()
                .items_center()
                .justify_center()
                .gap(px(16.0))
                .text_color(muted)
                .child(match &self.error {
                    Some(error) => error.clone(),
                    None => String::from("从左侧选择一个文件"),
                })
                // 空态也给一个「打开目录」入口：想切到另一个目录时，不用先摸清
                // 侧栏表头上那个小按钮在哪。快捷键是 `Ctrl+Shift+O`。
                .child(pill(
                    SharedString::from("open-folder-empty"),
                    "打开目录…",
                    foreground,
                    crate::theme::shell_hsla(),
                    tokens.list_hover,
                    hairline,
                    cx.listener(|this, _, window, cx| this.open_folder(window, cx)),
                ))
                .into_any_element();
        };

        let preview = doc.preview;
        // 大纲只对 Markdown 有意义（解析的是标题）。面板据此出现。
        let is_markdown = doc.kind == FileKind::Markdown;
        let outline_open = doc.outline_open;
        // Markdown 的标题列表：每帧从编辑缓冲现算。文档不大时这远比缓存一份
        // "标题索引 + 失效规则"省事，也不会出现"改了标题大纲没跟上"。
        let headings = if is_markdown {
            outline::headings(&doc.input.read(cx).value())
        } else {
            Vec::new()
        };
        let has_headings = !headings.is_empty();

        let body = if preview {
            // Markdown 预览要渲染编辑缓冲的当前内容（而不是磁盘上的那份），
            // 否则"改几个字再切预览"看不到改动。
            let markdown = (doc.kind == FileKind::Markdown).then(|| doc.input.read(cx).value());
            render_preview(
                doc,
                markdown,
                muted,
                self.font_size,
                self.editor_font.clone(),
                cx,
            )
            .into_any_element()
        } else {
            // 源码面外面再包一层 `relative` 容器，好把"Ctrl + 拖拽水平平移"那层
            // 抓手盖在编辑器上面。层本身由 `render_pan_layer` 提供，见那里的注释。
            div()
                .relative()
                .w_full()
                .h_full()
                .min_w_0()
                .min_h_0()
                .child(render_source(
                    doc,
                    foreground,
                    self.font_size,
                    self.editor_font.clone(),
                    cx.listener(|this, _, _, cx| this.note_editor_interaction(cx)),
                ))
                .child(self.render_pan_layer(cx))
                .into_any_element()
        };

        // 大纲面板：只有开了面板、且文档里有标题时才占位置。与正文并排，
        // 面板自身固定宽度，正文拿剩余宽度。
        //
        // 高度必须显式给全（`h_full`）：`h_flex` 的行高由内容决定，不给高度的
        // 话正文那一列会按内容收缩，编辑器里的 `h_full` 解析到不定高度后塌成
        // 一行、再被横向 flex 居中——表现成"正文只剩第 1 行飘在中间"。
        let content_row = if outline_open && has_headings {
            h_flex()
                .flex_1()
                .w_full()
                .h_full()
                .min_h_0()
                .min_w_0()
                .child(render_outline(&headings, cx))
                .child(v_flex().flex_1().h_full().min_w_0().min_h_0().child(body))
                .into_any_element()
        } else {
            body
        };

        // 动作反馈优先于常态提示：用户刚按下保存，最该看到的是那次操作的结果。
        let footer = self.status.clone().or_else(|| self.status_note());

        v_flex()
            .w(width)
            .h_full()
            .flex_shrink_0()
            .overflow_hidden()
            .child(self.render_tabs(cx))
            .child(content_row)
            .when_some(footer, |this, note| {
                this.child(
                    h_flex()
                        .h(px(24.0))
                        .flex_shrink_0()
                        .items_center()
                        .px(px(12.0))
                        .border_t_1()
                        .border_color(hairline)
                        .text_xs()
                        .text_color(muted)
                        .child(note),
                )
            })
            .into_any_element()
    }

    /// 状态栏提示：截断 / 非 UTF-8 / 只读 / 外部冲突，这些直接影响"能不能编辑"。
    fn status_note(&self) -> Option<String> {
        let doc = self.active_doc()?;
        let mut notes = Vec::new();
        if doc.conflict {
            notes.push(String::from("外部已改动，等待裁决"));
        }
        if doc.missing {
            notes.push(String::from("磁盘上已无此文件"));
        }
        if doc.snapshot.truncated {
            notes.push(format!("已截断到 {} MiB", text_file::MAX_BYTES / 1024 / 1024));
        }
        if doc.snapshot.invalid_encoding {
            notes.push(String::from("非 UTF-8 文本（按有损方式显示）"));
        }
        if doc.snapshot.read_only && !doc.snapshot.truncated && !doc.snapshot.invalid_encoding {
            notes.push(String::from("文件系统标记为只读"));
        }
        if doc.snapshot.read_only {
            notes.push(String::from("此处不可编辑"));
        }
        if notes.is_empty() { None } else { Some(notes.join(" · ")) }
    }
}

/// 把一个存下来的字体名解析成实际可用的字族。
///
/// 设置里存的是**名字**，而名字可能已经不存在了（字体被卸载、或者设置文件被手改
/// 过）。这时候回落默认值，而不是原样交给 gpui——gpui 对认不出的族名会**静默**
/// 换一副系统字，用户只会看到"设置没生效"却毫无线索。
fn resolve_font(saved: Option<&str>, catalog: &[String], fallback: &str) -> SharedString {
    match saved {
        Some(name) if catalog.iter().any(|known| known == name) => {
            SharedString::from(name.to_owned())
        }
        _ => SharedString::from(fallback),
    }
}

/// 标签标题：文件名重名（不同目录下的同名文件）时补上父目录名加以区分。
///
/// 纯函数，好测。路径去重保证同一标签条里不会出现"同父目录 + 同文件名"，所以
/// 加了父目录名之后一定唯一。
fn tab_labels<'a>(paths: impl IntoIterator<Item = &'a Path>) -> Vec<String> {
    let entries: Vec<(String, String)> = paths
        .into_iter()
        .map(|path| {
            let name = text_file::display_name(path);
            let parent = path
                .parent()
                .and_then(Path::file_name)
                .map(|dir| dir.to_string_lossy().into_owned())
                .unwrap_or_default();
            (name, parent)
        })
        .collect();

    let mut labels: Vec<String> = entries.iter().map(|(name, _)| name.clone()).collect();
    for (index, (name, parent)) in entries.iter().enumerate() {
        let duplicated = entries.iter().filter(|(other, _)| other == name).count() > 1;
        if duplicated && !parent.is_empty() {
            labels[index] = format!("{parent}/{name}");
        }
    }
    labels
}

/// 源码面：可直接编辑的代码编辑器（未知语言回落纯文本）。
///
/// 结构是三层叠放（都在同一个 `relative` 容器里，按树的顺序从下往上画）：
///
/// 1. **底色 canvas**：Notepad3 的 `back:` 槽（代码 / 变量 / 标签 / 段落名 / 标题条），
///    以及"用户碰过编辑器之后"的当前行淡黄。组件库的高亮主题没有背景色字段、
///    编辑器也不调 `ShapedLine::paint_background`，所以只能在字形下面自己铺 quad。
///    放**第一个**子元素 = 画在最底层。
/// 2. **编辑器**：用 `Input::appearance(false)` 关掉它自带的不透明底色，否则会盖住
///    上面那层 canvas。白底改由容器提供（见 `theme::editor_bg_hsla`）。
/// 3. **括号匹配 canvas**：Notepad3 用 Scintilla 的 indicator 层画配对括号的圆角框，
///    这里同样用 `range_to_bounds` + `paint_quad` 自绘，放**最后** = 画在最上层。
///
/// 两条 canvas 都靠组件库公开的 `range_to_bounds`（"字节区间 → 当帧窗口矩形"）定位；
/// 它返回的已经是**窗口坐标**，直接交给 `paint_quad`，不要再叠加 canvas 自己的
/// `bounds.origin`（加了就画到屏幕外）。见 AGENTS.md「括号匹配」。
///
/// 为什么括号高亮不用文本装饰：装饰做不出"框"这种图元，且 `TextDecorationCollection::set`
/// 是无条件 `notify()`，挂在 `observe` 上会自激打满 CPU。见 `refresh_brace_marks`。
///
/// `on_interact` 在源码面上发生鼠标按下时被调用：`Workspace` 用它把"用户碰过编辑器"
/// 记下来，从而点亮当前行（见 `Document::caret_touched`）。
fn render_source(
    doc: &Document,
    foreground: gpui::Hsla,
    font_size: f32,
    font_family: SharedString,
    on_interact: impl Fn(&MouseDownEvent, &mut Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    let read_only = doc.snapshot.read_only;
    let editor = Input::new(&doc.input)
        .appearance(false)
        .h_full()
        .w_full()
        .min_w_0()
        .disabled(read_only)
        .bordered(false)
        .focus_bordered(false)
        .rounded(px(0.0))
        .font_family(font_family)
        .text_size(px(font_size))
        .text_color(foreground)
        .line_height(relative(LINE_HEIGHT))
        // 左右 padding 设成 0：行号栏（gutter）从 `paddings.left` 起画，留了 10px
        // 就会在"文件树右边框"与"行号栏"之间露出一条白缝，看起来两区被隔开。
        // 用户要"行号区和文件管理区贴在一起"，所以这里不加水平 padding——行号自身
        // 仍有 `LINE_NUMBER_RIGHT_MARGIN` 的右边距，不会顶到边上。
        .px(px(0.0))
        .py(px(8.0));

    let mut container = div()
        .id("source-view")
        .relative()
        .w_full()
        .h_full()
        .min_w_0()
        .min_h_0()
        // 鼠标在源码面按下 = 用户开始在这里编辑 / 选行，当前行高亮随之点亮。
        .on_mouse_down(MouseButton::Left, on_interact)
        .bg(if read_only {
            // 只读（二进制有损解码）文件的旧观感是"半透明白底"，那是编辑器自身
            // 带 `appearance` 时对 disabled 的处理。这里改用 `appearance(false)` 露
            // 出下方的背景色 canvas，所以那一层要自己补回来，否则只读文件看起来
            // 和可编辑的一模一样。
            crate::theme::editor_bg_hsla().opacity(0.5)
        } else {
            crate::theme::editor_bg_hsla()
        });

    // 第一层：当前行底色 + Notepad3 补充层的背景色块。只在真有内容时加 canvas
    // （省掉每帧多余的布局/绘制）。两层合用一个 canvas 是因为它们都要按上一帧的
    // 滚动偏移做同样的补偿（`dy`），拆成两张就得各记一份偏移、还容易写重。
    let cur_line = doc.caret_touched;
    if doc.has_backgrounds || cur_line {
        let input = doc.input.clone();
        let spans = doc.backgrounds.clone();
        let last_offset = doc.bg_offset.clone();
        let cur_color = crate::theme::current_line_hsla();
        container = container.child(
            canvas(
                move |bounds, _window, _cx| (input, spans, last_offset, bounds, cur_line, cur_color),
                move |bounds, (input, spans, last_offset, _container, cur_line, cur_color), window, cx| {
                    let state = input.read(cx);
                    // 抵消一帧滞后：`state.last_layout` 是上一帧的，而滚动偏移已经
                    // 是本帧的，所以把 quad 平移两者的差。不滚动时差值为 0。
                    let now = state.scroll_offset();
                    let dy = last_offset
                        .borrow()
                        .map(|prev| now.y - prev.y)
                        .unwrap_or(px(0.));
                    *last_offset.borrow_mut() = Some(now);
                    // 裁剪到编辑器视口：`dy` 平移会把这层暂时推出视口上沿，
                    // 不裁的话它会在标签栏/头部那条带上闪一下（用户报的"背景
                    // 填充条跑到编辑区上方闪烁"）。canvas 自身没有 overflow，
                    // 所以显式套一层内容蒙版。
                    window.with_content_mask(
                        Some(gpui::ContentMask { bounds }),
                        |window| {
                            // 当前行：光标所在行的整行淡黄。用光标的零宽区间拿
                            // 当帧矩形（高度即行高），再把矩形整行铺满容器宽度。
                            // 只在用户碰过编辑器之后画——这就是"点哪行才亮哪行"。
                            if cur_line {
                                let caret = state.cursor();
                                if let Some(caret_bounds) = state.range_to_bounds(&(caret..caret)) {
                                    let rect = gpui::Bounds {
                                        origin: gpui::point(
                                            bounds.origin.x,
                                            caret_bounds.origin.y + dy,
                                        ),
                                        size: gpui::size(
                                            bounds.size.width,
                                            caret_bounds.size.height,
                                        ),
                                    };
                                    window.paint_quad(gpui::fill(rect, cur_color));
                                }
                            }

                            let spans = spans.borrow();
                            // **只画可见行**：`range_to_bounds` 内部要遍历可见行找字节位置，
                            // 对全文每一段都调一次是白烧（一份 90 KB 的 Markdown 有上千个行内
                            // 代码段）。段按 `row` 升序，所以二分出可见区间、只处理它。多算前后
                            // 各几行，给上面那点平移留余量。
                            let visible = state.visible_row_range();
                            let (lo, hi) = match &visible {
                                Some(rows) => (
                                    spans.partition_point(|span| span.row + 2 < rows.start),
                                    spans.partition_point(|span| span.row < rows.end + 2),
                                ),
                                None => (0, spans.len()),
                            };
                            let right = bounds.origin.x + bounds.size.width;
                            for span in &spans[lo..hi] {
                                let Some((r, g, b)) = span.ink.bg else { continue };
                                let Some(mut bounds) = state.range_to_bounds(&span.range)
                                else {
                                    continue;
                                };
                                // 围栏代码块：宽度取**块里最宽那行**，于是块内每行
                                // 右缘对齐、拼成一整块矩形（用户要的"矩形填充"）。
                                if let Some(src) = &span.width_from
                                    && let Some(w) = state.range_to_bounds(src)
                                {
                                    let target = w.origin.x + w.size.width;
                                    bounds.size.width =
                                        (target - bounds.origin.x).max(bounds.size.width);
                                }
                                bounds.origin.y += dy;
                                // `eolfilled`：底色铺到容器右缘（标题条 / 段落名 / 标签）。
                                if span.ink.eol {
                                    bounds.size.width =
                                        (right - bounds.origin.x).max(bounds.size.width);
                                }
                                window.paint_quad(gpui::fill(bounds, hsla(r, g, b)));
                            }
                        },
                    );
                },
            )
            .absolute()
            .top(px(0.0))
            .left(px(0.0))
            .size_full(),
        );
    }

    container = container.child(editor);

    // 第三层：括号匹配框（没有匹配时省掉这层）。
    if let Some((open, close)) = doc.brace_matches.clone() {
        let input = doc.input.clone();
        let brace_color = crate::theme::brace_match_hsla();
        container = container.child(
            canvas(
                move |bounds, _window, _cx| (input, open, close, bounds),
                move |_bounds, (input, open, close, _cb), window, cx| {
                    let state = input.read(cx);
                    let stroke = gpui::BorderStyle::default();
                    for range in [open, close] {
                        if let Some(bounds) = state.range_to_bounds(&range) {
                            // 左右各放宽 1px：Scintilla 的 indicator box 也贴着字形外沿。
                            let bounds = gpui::Bounds {
                                origin: gpui::point(bounds.origin.x - px(1.0), bounds.origin.y),
                                size: gpui::size(bounds.size.width + px(2.0), bounds.size.height),
                            };
                            // 填充与描边都画：Notepad3 的 `alpha:80` 与 `alpha2:80` 同色，
                            // 得到"半透明绿底 + 圆角绿边"；只描边会只剩一圈淡边。
                            window.paint_quad(gpui::quad(
                                bounds,
                                px(2.0),
                                brace_color,
                                px(1.0),
                                brace_color,
                                stroke,
                            ));
                        }
                    }
                },
            )
            .absolute()
            .top(px(0.0))
            .left(px(0.0))
            .size_full(),
        );
    }

    container.into_any_element()
}

/// 纸面上的小按钮。
///
/// 刻意不用组件库的 `Button`：gpui-component 0.5.2 重构按钮颜色推导时把
/// `text_color` 变成了从背景派生的值（Pebrel 根 Cargo.toml 记录过这次回归：
/// "透明底按钮字直接隐形"），而 Paper 的 `secondary` 与纸面同色，于是按钮既没底
/// 也看不见字。这里直接用主题令牌画一个可点的小药丸，两种尺度都受控。
fn pill(
    id: SharedString,
    label: impl Into<SharedString>,
    foreground: gpui::Hsla,
    rest: gpui::Hsla,
    hover: gpui::Hsla,
    border: gpui::Hsla,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    h_flex()
        .id(id)
        .h(px(24.0))
        .flex_shrink_0()
        .px(px(10.0))
        .items_center()
        .justify_center()
        .rounded(px(6.0))
        .border_1()
        .border_color(border)
        .bg(rest)
        .text_xs()
        .text_color(foreground)
        .cursor_pointer()
        .hover(move |this| this.bg(hover))
        .on_click(on_click)
        .child(label.into())
}

/// 预览面：图片走可缩放/平移的 canvas，Markdown 走组件库的富文本渲染器。
///
/// Markdown 的 `image_base` 指向文档所在目录，相对路径图片（`![](img/x.png)`）
/// 才解析得到；网络图片走 gpui 的 HTTP 客户端（见 `main.rs` 的 `set_http_client`）。
///
/// 图片为什么要用 `canvas` 而不是 `img`：`img` 只会按 `object_fit` 缩放，无法
/// 在"同一个元素里"做围绕指针的缩放与拖拽平移。canvas 的 paint 能拿到当帧
/// bounds，直接调 `paint_image` 画到计算出的矩形上——Pebrel 的 GPUI 图片查看
/// 就是这条路（`gpui_shell/doc_tabs.rs`）。
fn render_preview(
    doc: &Document,
    markdown: Option<SharedString>,
    muted: gpui::Hsla,
    font_size: f32,
    font_family: SharedString,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    match (doc.kind, doc.image.as_ref()) {
        (FileKind::Image, Some(image)) => {
            render_zoomable_image(doc, image.clone(), muted, cx).into_any_element()
        },
        (FileKind::Image, None) => note_panel("图片无法解码", muted),
        (FileKind::Markdown, _) => {
            // 预览的**样式**是这一版的重点。组件库的 `TextView` 解析上其实够用
            // （标题 / 列表 / 表格 / 引用 / 任务框 / 围栏代码 / 行内代码都有），
            // 缺的是一套像样的默认样式：默认的段落间距、标题梯度、代码块与表格的
            // 底色边框都停在"能用但难看"那一档。这里逐项给上：
            //
            // - 标题：1.75 / 1.40 / 1.20 / 1.08 的阶梯，H4 之后靠字重而不是字号；
            // - 代码块：浅灰底 + 发丝边 + 8px 圆角 + 12px 内边距。语法色读的是
            //   **全局高亮主题**（也就是 Notepad3 那套按语言的配色），所以预览里
            //   的代码块跟源码面同源；
            // - 行内代码：淡灰底、不加边框——底色已经够区分了；
            // - 表格：外边框 + 单元格内边距，宽度不够时**横向滚动**，而不是把整个
            //   内容区撑宽（撑宽会把标签条右端的按钮挤出窗口，踩过）；
            // - 正文局限在一条**阅读栏宽**（`MARKDOWN_COLUMN_WIDTH`）里并居中：
            //   满屏宽的一行太长，读的时候眼睛回行会串行——"舒适"主要来自这一条。
            let hairline = cx.theme().border;
            let surface = crate::theme::shell_hsla();

            // 组件库只接受 `StyleRefinement`（字段私有、没有构造函数），所以先在
            // 一个临时 div 上用常规 API 把样式写出来，再把那份 refinement clone 走。
            let code_block = {
                // 组件库只接受 `StyleRefinement`（字段私有、没有构造函数），所以先在
                // 一个临时 div 上用常规 API 把样式写出来，再把那份 refinement clone 走。
                // 注意 `Styled` 的方法都是**按值**收放 `self`，所以这里必须一次串完
                // 再取 `style()`，不能分两步写。
                let mut probe = div()
                    .bg(surface)
                    .border_1()
                    .border_color(hairline)
                    .rounded(px(8.0))
                    .p(px(12.0));
                probe.style().clone()
            };
            let table = {
                let mut probe = div()
                    .id("md-table-probe")
                    .border_1()
                    .border_color(hairline)
                    .rounded(px(8.0))
                    // 宽度不够时让表格**自己横向滚**，而不是把内容区撑宽。
                    .overflow_x_scroll();
                probe.style().clone()
            };
            let table_cell = {
                let mut probe = div().px(px(10.0)).py(px(6.0)).border_color(hairline);
                probe.style().clone()
            };

            let style = TextViewStyle {
                image_base: doc.path.parent().map(Arc::from),
                // 预览正文跟随字号（`Ctrl+滚轮` / `Ctrl+=`）：不跟的话放大编辑器
                // 字号只影响源码面，一切到预览又是小字，两边对不上。
                heading_base_font_size: px(font_size),
                heading_font_size: Some(Arc::new(|level: u8, base: gpui::Pixels| {
                    let factor = match level {
                        1 => 1.75,
                        2 => 1.40,
                        3 => 1.20,
                        4 => 1.08,
                        _ => 1.0,
                    };
                    base * factor
                })),
                paragraph_gap: gpui::rems(0.9),
                code_block,
                table,
                table_cell,
                inline_code: gpui::HighlightStyle {
                    background_color: Some(surface),
                    ..Default::default()
                },
                ..Default::default()
            };
            // 外面必须再包一层可滚动容器：TextView 自己只是内容，长文档不滚动就
            // 只看得到开头。`w_full` + `min_w_0` 是必须的——表格这类内容有自己的
            // 固有宽度，不钉住容器宽度就会把内容区撑宽。
            div()
                .flex_1()
                .w_full()
                .min_w_0()
                .min_h_0()
                .overflow_hidden()
                .overflow_y_scrollbar()
                .child(
                    // 居中的阅读栏用 `justify_center` + 子项 `max_w` 实现（这条链上
                    // 只用已经用过的 API，不依赖 taffy 的 auto margin 行为）。
                    div()
                        .flex()
                        .w_full()
                        .justify_center()
                        .child(
                            v_flex()
                                .w_full()
                                .max_w(px(MARKDOWN_COLUMN_WIDTH))
                                .px(px(32.0))
                                .py(px(28.0))
                                .text_size(px(font_size))
                                // 预览正文用"编辑器字体"（与源码面同源）：两者字号
                                // 已经共用一个 `font_size`，字体也一致才不会"切预览
                                // 就换一种字"。代码块另读全局 `mono_font_family`
                                // （`set_editor_font` 一并写入）。
                                .font_family(font_family)
                                .child(
                                    TextView::markdown(
                                        "md-preview",
                                        markdown.unwrap_or_default(),
                                    )
                                    .style(style),
                                ),
                        ),
                )
                .into_any_element()
        },
        (FileKind::Text, _) => note_panel("该文件没有预览面", muted),
    }
}

/// 可缩放 / 平移的图片查看区。
///
/// 结构：一层 `relative` 的容器铺满可用区，四种鼠标事件按命中位置派发到它身上；
/// 里面那张 `canvas` 在 paint 时读出当帧 bounds、按几何算出的矩形画图。
///
/// 为什么不需要滚动条、也不滚走：缩放围绕指针、平移钳在图片边缘，图片永远在
/// 视图内被合理摆放（缩小会居中、放大可拖），所以这里 `overflow_hidden`。
fn render_zoomable_image(
    doc: &Document,
    image: Arc<RenderImage>,
    muted: gpui::Hsla,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    // canvas 的 paint 闭包是 `'static`，所以几何要 clone 一份进去；它每帧
    // 拿的是当帧几何（`render_rect` 是纯函数），不会有首帧空窗。
    let geometry = doc.image_geometry.clone();
    let area_store = doc.image_area.clone();
    let painter = canvas(
        move |bounds, _, _| {
            // 记住当帧矩形：滚轮/拖拽事件里做数值换算要用（事件位置是窗口坐标，
            // 而几何需要视图矩形）。
            *area_store.borrow_mut() = bounds;
            bounds
        },
        move |container, pump_bounds: Bounds<Pixels>, window, _| {
            let area = bounds_to_area(pump_bounds);
            let target = geometry.target_rect(area);
            let target_bounds = Bounds::new(
                point(px(target.0), px(target.1)),
                size(px(target.2.max(1.0)), px(target.3.max(1.0))),
            );
            // 图片超出容器时要在容器边缘裁掉：`paint_image` 自己只按 image bounds
            // 画，不会管父容器。这一层 mask 就是裁剪框。
            window.with_content_mask(Some(gpui::ContentMask { bounds: container }), |window| {
                let _ = window.paint_image(
                    target_bounds,
                    target_bounds,
                    gpui::Corners::all(px(0.0)),
                    image.clone(),
                    0,
                    false,
                );
            });
        },
    )
    .absolute()
    .inset_0();

    div()
        .id("image-view")
        .flex_1()
        .w_full()
        .min_w_0()
        .min_h_0()
        .relative()
        .overflow_hidden()
        // 光标是唯一的"这里能不能拖"的提示：图片放大到超出视图时才给抓手，
        // 适应视图下就是普通箭头（那时拖着也没地方去，给了抓手指的是空气）。
        .when(
            doc.image_geometry.pannable(bounds_to_area(*doc.image_area.borrow())),
            |this| this.cursor_move(),
        )
        // 滚轮 = 缩放（围绕指针）；按住 Ctrl 时改为调字号，与预览面其它内容一致。
        .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
            if this.zoom_font_on_wheel(event, cx) {
                return;
            }
            let steps = wheel_steps(event);
            let anchor = (f32::from(event.position.x), f32::from(event.position.y));
            let area = this.image_area_of_active();
            if let Some(doc) = this.active_doc_mut()
                && doc.image_geometry.zoom_by(steps, anchor, area)
            {
                cx.notify();
            }
        }))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, event: &MouseDownEvent, _, cx| {
                let point = (f32::from(event.position.x), f32::from(event.position.y));
                let area = this.image_area_of_active();
                if let Some(doc) = this.active_doc_mut()
                    && doc.image_geometry.begin_drag(point, area)
                {
                    cx.notify();
                }
            }),
        )
        .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
            let area = this.image_area_of_active();
            let point = (f32::from(event.position.x), f32::from(event.position.y));
            if let Some(doc) = this.active_doc_mut() {
                if !doc.image_geometry.dragging() {
                    return;
                }
                // 松开左键（拖拽中途）就结束拖拽——事件不会因为松开而停送。
                if event.pressed_button != Some(MouseButton::Left) {
                    doc.image_geometry.end_drag();
                    return;
                }
                if doc.image_geometry.drag_to(point, area) {
                    cx.notify();
                }
            }
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|this, _: &MouseUpEvent, _, cx| {
                if let Some(doc) = this.active_doc_mut()
                    && doc.image_geometry.end_drag()
                {
                    cx.notify();
                }
            }),
        )
        .child(painter)
        // 右下角的缩放提示；图片被缩放/平移过之后才出现。复位入口在头部
        // 药丸上（见 `render_content`），不做双击——这一版 gpui 没有双击事件。
        .when(doc.image_geometry.zoom() != 1.0, |this| {
            this.child(
                div()
                    .absolute()
                    .right(px(12.0))
                    .bottom(px(10.0))
                    .px(px(8.0))
                    .py(px(3.0))
                    .rounded(px(6.0))
                    .bg(crate::theme::shell_hsla())
                    .text_xs()
                    .text_color(muted)
                    .child(format!("{:.0}%", doc.image_geometry.zoom() * 100.0)),
            )
        })
}

/// 滚轮位移 → 缩放档数。
///
/// Pebrel 的 `ImageTabView::on_scroll` 直接写 `delta / 40.0`；照抄过来一格滚轮
/// 会跳 3 档（见 `WHEEL_LINES_PER_NOTCH` 的注释），手感太冲。这里多除一次，
/// 让**一格滚轮 = 一档**；触摸板的像素级 delta 按同一比例缩放，粒度自然更细。
fn wheel_steps(event: &ScrollWheelEvent) -> f32 {
    event.delta.pixel_delta(px(WHEEL_STEP_PX)).y.as_f32() / WHEEL_STEP_PX
        / WHEEL_LINES_PER_NOTCH
}

/// 滚轮事件要不要拿去调字号，以及调多少（`None` = 不归字号管，交给滚动）。
///
/// 抽成纯函数是为了可测：`Ctrl+滚轮` 这个组合在锁定屏下**送不进去**
/// （gpui 的修饰键状态读 `GetKeyState`，投递的消息改不了它），所以判定逻辑
/// 只能靠单测兜住，而不是靠手点。
fn font_zoom_step(event: &ScrollWheelEvent) -> Option<f32> {
    // 只在按住 Ctrl（Windows 惯例）时接管；不带修饰键的滚轮该去滚动内容。
    if !event.modifiers.control {
        return None;
    }
    let steps = wheel_steps(event);
    if steps.abs() < f32::EPSILON {
        return None;
    }
    Some(if steps > 0.0 { 1.0 } else { -1.0 })
}

/// `Bounds<Pixels>` → `image_geom::Area`（`(x, y, w, h)` 元组）。
fn bounds_to_area(bounds: Bounds<Pixels>) -> Area {
    (
        f32::from(bounds.origin.x),
        f32::from(bounds.origin.y),
        f32::from(bounds.size.width),
        f32::from(bounds.size.height),
    )
}

/// Markdown 大纲面板：标题树，点一条把源码光标移到那一行。
///
/// 层级用左缩进表达，不做折叠——一份文档的标题通常一屏就能看完，折叠交互
/// （要维护"哪些被折叠"的状态）在这里不划算。`level` 只用于缩进。
fn render_outline(headings: &[outline::Heading], cx: &mut Context<Workspace>) -> impl IntoElement {
    let tokens = cx.theme();
    let hairline = tokens.border;
    let foreground = tokens.foreground;
    let muted = tokens.muted_foreground;
    let hover_bg = tokens.list_hover;

    let rows = headings.iter().enumerate().map(|(index, heading)| {
        let label = if heading.text.is_empty() {
            String::from("(空标题)")
        } else {
            heading.text.clone()
        };
        let level = heading.level;
        let heading = heading.clone();
        h_flex()
            .id(SharedString::from(format!("outline:{index}")))
            .h(px(24.0))
            .w_full()
            .flex_shrink_0()
            .items_center()
            // 每级缩进 12px：一级 0、六级 60。标题文本本身已经带层级感，只需一点
            // 错位，不用画连接线。
            .pl(px(10.0 + (level.saturating_sub(1)) as f32 * 12.0))
            .pr(px(8.0))
            .rounded(px(4.0))
            .text_xs()
            .text_color(if level == 1 { foreground } else { muted })
            .truncate()
            .cursor_pointer()
            .hover(move |this| this.bg(hover_bg))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.jump_to_heading(&heading, window, cx);
            }))
            .child(label)
    });

    v_flex()
        .id("outline-panel")
        .w(px(OUTLINE_WIDTH))
        .h_full()
        .flex_shrink_0()
        .min_h_0()
        .border_r_1()
        .border_color(hairline)
        .child(
            h_flex()
                .h(px(HEADER_HEIGHT))
                .flex_shrink_0()
                .items_center()
                .px(px(12.0))
                .border_b_1()
                .border_color(hairline)
                .text_sm()
                .text_color(muted)
                .child("大纲"),
        )
        .child(
            div()
                .flex_1()
                .w_full()
                .min_w_0()
                .min_h_0()
                .overflow_y_scrollbar()
                .child(v_flex().w_full().p(px(6.0)).children(rows)),
        )
}

fn note_panel(message: &str, muted: gpui::Hsla) -> gpui::AnyElement {
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .text_color(muted)
        .child(message.to_owned())
        .into_any_element()
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 文件树只在需要时重新展平，之后整帧渲染都读缓存（见 `sync_rows`）。
        self.sync_rows(cx);
        let background = cx.theme().background;
        // 内容区宽度显式算出来，不靠 `flex_1` / `w_full` 自动解析：在宽度不确定的
        // 父级上 `w_full` 会回落到 max-content，于是长行（`soft_wrap(false)`）或
        // 预览里的宽表格会把内容区撑得比窗口还宽，标签条右端的按钮直接被窗口裁掉。
        // 这是 Pebrel 在 `Scrollable` 上记录过的同一类陷阱。侧栏宽度可变（拖动
        // 分割线），所以这一条要跟着 `sidebar_width` 走。
        let sidebar = if self.sidebar_open { self.sidebar_width } else { 0.0 };
        let content_width = (window.viewport_size().width - px(sidebar)).max(px(160.0));
        // 自绘标题栏在顶，工作区（文件树 | 内容区）占满其余高度。此前没有这一行，
        // 系统标题栏又被 `appears_transparent` 藏掉了，窗口就没有最小化/关闭按钮。
        v_flex()
            .size_full()
            .bg(background)
            .child(self.render_title_bar(cx))
            .child(
                h_flex()
                    // 撑满标题栏以下的剩余高度；`min_h_0` 让内部滚动区能正确收缩。
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    // 分割线（下面那个 `absolute` 的抓手）要以这一行为锚点。
                    .relative()
                    // 这几个动作都由窗口级按键绑定派发到这里。注意 GPUI 的按键绑定只在有
                    // 节点持有焦点时才解析得到：没有焦点时派发目标是根节点本身，挂在更深
                    // 处视图上的 `on_action` 收不到动作。`activate()` 把焦点交给编辑器，
                    // 就是为了让这类"全局"动作在打开文件之后一直可用。
                    .on_action(cx.listener(|this, _: &SaveDocument, _window, cx| this.save(cx)))
                    .on_action(cx.listener(|this, _: &CloseTab, window, cx| {
                        this.close_active(window, cx)
                    }))
                    .on_action(cx.listener(|this, _: &NextTab, window, cx| {
                        this.cycle(1, window, cx)
                    }))
                    .on_action(cx.listener(|this, _: &PreviousTab, window, cx| {
                        this.cycle(-1, window, cx)
                    }))
                    // 字号：`Ctrl+=` / `Ctrl+-` 与 `Ctrl+滚轮`（在预览面上）同源。
                    // 这两个动作在源码面上也可用——编辑器持有焦点时窗口级绑定照样
                    // 命中（见上面那条注释）。
                    .on_action(cx.listener(|this, _: &IncreaseFontSize, _, cx| {
                        this.bump_font_size(1.0, cx)
                    }))
                    .on_action(cx.listener(|this, _: &DecreaseFontSize, _, cx| {
                        this.bump_font_size(-1.0, cx)
                    }))
                    .on_action(cx.listener(|this, _: &ResetFontSize, _, cx| {
                        this.reset_font_size(cx)
                    }))
                    // 「打开目录」也走窗口级绑定（`Ctrl+Shift+O`），与侧栏按钮、
                    // 空态按钮同源。见 `main.rs` 的绑定与 `open_folder`。
                    .on_action(cx.listener(|this, _: &OpenFolder, window, cx| {
                        this.open_folder(window, cx)
                    }))
                    .when(self.sidebar_open, |this| this.child(self.render_sidebar(cx)))
                    .child(self.render_content(content_width, cx))
                    .when(self.sidebar_open, |this| {
                        // 分割线：叠在侧栏与内容区的交界上（**绝对定位**，不占布局宽度
                        // ——行号栏能紧贴侧栏，中间不留纸色缝）。视觉上就那条 1px 竖线。
                        // 抓手是个真交互元素（`occlude` + `on_mouse_down` 立起拖动标志、
                        // 并给左右拉伸光标）；移动与松开由**根上那张全窗 canvas** 的
                        // 窗口级监听处理（它每帧都在，不会因为指针跑到编辑器上而丢事件）。
                        let line_color = if self.sidebar_dragging {
                            cx.theme().drag_border
                        } else {
                            cx.theme().border
                        };
                        this.child(
                            div()
                                .id("sidebar-divider")
                                .absolute()
                                .top_0()
                                .left(px(self.sidebar_width - RESIZE_HANDLE_WIDTH))
                                .h_full()
                                .w(px(RESIZE_HANDLE_WIDTH * 2.0))
                                .flex()
                                .justify_center()
                                .cursor_col_resize()
                                .occlude()
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, _, _, cx| {
                                        this.sidebar_dragging = true;
                                        cx.notify();
                                    }),
                                )
                                .child(div().w(px(1.0)).h_full().bg(line_color)),
                        )
                    }),
            )
            // 侧栏拖动的移动/松开：根上一张覆盖整窗的 canvas 注册**窗口级**鼠标监听，
            // 拖拽期间指针跑到编辑器上也照样收得到（`on_mouse_move` 挂在元素上时只在
            // 指针悬停该元素触发，编辑器一盖住就丢事件）。canvas 没有 hitbox，只监听、
            // 不挡下面任何交互；这正是组件库 `ResizeHandle` 的机制
            // （`resizable/panel.rs` 的 `ResizePanelGroupElement::paint`）。
            .child(self.render_sidebar_drag_layer(cx))
            // 标签右键菜单 / 文件树右键菜单：各唯一一份，挂在工作区根上。画在最后
            // （`deferred` 保证盖在所有内容之上），锚点是右键时的鼠标位置。菜单开着
            // 时不能挂在那一行元素的子孙树上——理由见 `TabMenu` 的注释。
            .when_some(self.render_tab_menu(), |this, menu| this.child(menu))
            .when_some(self.render_tree_menu(), |this, menu| this.child(menu))
            // 设置面板：叠在最上层（比两个菜单还高一层），开着时吃掉所有点击。
            .when(self.settings_open, |this| this.child(self.render_settings(cx)))
    }
}

/// 文件树右键菜单里的命令。与 [`TabCommand`] 同一套"枚举 + 单次执行"的写法。
/// 新建 / 重命名都走就地输入（见 [`Rename`]）：新建先落一个默认名（唯一化），
/// 然后把那一行变成输入框让用户改名——这样行一定存在、有地方挂输入框。
#[derive(Clone, Copy)]
enum TreeCommand {
    NewFile,
    NewFolder,
    Open,
    CopyPath,
    Reveal,
    Rename,
    Delete,
}

impl TreeCommand {
    fn run(self, workspace: &mut Workspace, path: PathBuf, window: &mut Window, cx: &mut Context<Workspace>) {
        match self {
            Self::NewFile => workspace.create_in(&path, true, window, cx),
            Self::NewFolder => workspace.create_in(&path, false, window, cx),
            Self::Open => {
                if path.is_dir() {
                    workspace.tree.toggle(&path);
                    workspace.invalidate_tree();
                    cx.notify();
                } else {
                    workspace.open(path, window, cx);
                }
            },
            Self::CopyPath => {
                let shown = shell::friendly_path(&path);
                cx.write_to_clipboard(ClipboardItem::new_string(shown.clone()));
                workspace.status = Some(format!("已复制路径：{shown}"));
                cx.notify();
            },
            Self::Reveal => match shell::reveal_in_file_manager(&path) {
                Ok(()) => {
                    workspace.status =
                        Some(format!("已在资源管理器中显示：{}", shell::friendly_path(&path)))
                },
                Err(error) => workspace.status = Some(format!("无法在资源管理器中显示：{error}")),
            },
            Self::Rename => {
                let name = text_file::display_name(&path);
                workspace.begin_rename(path, name, false, window, cx);
            },
            Self::Delete => workspace.confirm_delete(path, window, cx),
        }
    }
}

impl Workspace {
    /// 在 `path`（目录则本身、文件则所在目录）里新建一个文件或文件夹，并把那一行
    /// 变成可就地改名的输入框。
    ///
    /// 先按默认名落盘（`新建文件.txt` / `新建文件夹`，重名自动加序号），再让用户
    /// 改名——这样文件树里立刻有一行可以承载输入框。用户取消改名时保留默认名，
    /// 不会出现"输入框悬空、文件还没建"的中间态。
    fn create_in(&mut self, path: &Path, is_file: bool, window: &mut Window, cx: &mut Context<Self>) {
        let dir = if path.is_dir() { path.to_path_buf() } else {
            match path.parent() {
                Some(parent) => parent.to_path_buf(),
                None => return,
            }
        };
        let (default_name, target) = if is_file {
            let name = unique_child(&dir, "新建文件", "txt");
            (name.clone(), dir.join(name))
        } else {
            let name = unique_child(&dir, "新建文件夹", "");
            (name.clone(), dir.join(name))
        };
        let created = if is_file {
            std::fs::write(&target, b"").map_err(|error| error.to_string())
        } else {
            std::fs::create_dir(&target).map_err(|error| error.to_string())
        };
        if let Err(error) = created {
            self.status = Some(format!("新建失败：{error}"));
            cx.notify();
            return;
        }
        // 展开所在目录，否则新建的行藏在收起的目录里看不见。
        self.tree.reveal(&target);
        self.invalidate_tree();
        self.begin_rename(target, default_name, is_file, window, cx);
    }

    /// 进入就地改名：把 `path` 那一行渲染成输入框，初值是 `initial`。回车提交、
    /// 失焦取消（`open_after` 为真时提交后把文件打开——新建文件用）。
    fn begin_rename(
        &mut self,
        path: PathBuf,
        initial: String,
        open_after: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = cx.new(|cx| InputState::new(window, cx));
        let len = initial.len();
        input.update(cx, |state, cx| {
            state.set_value(initial.clone(), window, cx);
            // 选中全部，用户直接打字就替换掉默认名。
            state.set_selected_range(0..len, cx);
            state.focus(window, cx);
        });
        let changes = cx.subscribe_in(&input, window, |this, _, event, window, cx| match event {
            InputEvent::PressEnter { .. } => this.commit_rename(window, cx),
            InputEvent::Blur => this.cancel_rename(cx),
            _ => {},
        });
        self.renaming = Some(Rename { path, input, open_after, _changes: Some(changes) });
        cx.notify();
    }

    /// 提交就地改名：把文件 / 目录改成输入框里的名字。名字没变就什么都不做。
    fn commit_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(renaming) = self.renaming.take() else {
            return;
        };
        let new_name = renaming.input.read(cx).value().trim().to_string();
        let old = renaming.path;
        if new_name.is_empty() {
            cx.notify();
            return;
        }
        let parent = match old.parent() {
            Some(parent) => parent.to_path_buf(),
            None => {
                cx.notify();
                return;
            },
        };
        let target = parent.join(&new_name);
        if target != old {
            if target.exists() {
                self.status = Some(format!("已存在同名项：{new_name}"));
                cx.notify();
                return;
            }
            if let Err(error) = std::fs::rename(&old, &target) {
                self.status = Some(format!("重命名失败：{error}"));
                cx.notify();
                return;
            }
            // 改名后仍开着的标签要指向新路径（否则保存会写回一个已不存在的文件名）。
            // 输入订阅按 `input` 实体（不是路径）回找文档，所以改名不影响脏标记。
            for doc in &mut self.tabs {
                if doc.path == old {
                    doc.path = target.clone();
                }
            }
        }
        self.invalidate_tree();
        self.status = Some(format!("已重命名为 {}", text_file::display_name(&target)));
        if renaming.open_after {
            self.open(target, window, cx);
        } else {
            cx.notify();
        }
    }

    /// 取消就地改名（输入框失焦）：清掉那一行的输入框，名字保持原样。
    fn cancel_rename(&mut self, cx: &mut Context<Self>) {
        if self.renaming.take().is_some() {
            cx.notify();
        }
    }

    /// 删除文件 / 目录前先确认——删除不可撤销，不做静默删除。
    fn confirm_delete(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let name = text_file::display_name(&path);
        let shown = shell::friendly_path(&path);
        let is_dir = path.is_dir();
        let workspace = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let workspace = workspace.clone();
            let path = path.clone();
            alert
                .title(format!("删除「{name}」？"))
                .description(format!(
                    "将{}删除：\n{shown}\n此操作不可撤销。",
                    if is_dir { "连同目录内全部内容一起" } else { "" }
                ))
                .button_props(
                    gpui_component::dialog::DialogButtonProps::default()
                        .ok_text("删除")
                        .cancel_text("取消"),
                )
                .show_cancel(true)
                .on_ok(move |_, window, cx| {
                    if let Some(workspace) = workspace.upgrade() {
                        workspace.update(cx, |this, cx| this.delete_path(&path, window, cx));
                    }
                    true
                })
        });
    }

    /// 真正执行删除（用户在确认框里点了「删除」）。
    fn delete_path(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let result = if path.is_dir() {
            std::fs::remove_dir_all(path)
        } else {
            std::fs::remove_file(path)
        };
        match result {
            Ok(()) => {
                // 删掉的路径若还开着标签，关掉它（避免对着一个不存在的文件编辑）。
                let open: Vec<usize> = self
                    .tabs
                    .iter()
                    .enumerate()
                    .filter(|(_, doc)| doc.path == path)
                    .map(|(index, _)| index)
                    .collect();
                // 从后往前关，避免下标偏移。
                for index in open.into_iter().rev() {
                    self.tabs.remove(index);
                }
                self.active = self.active.min(self.tabs.len().saturating_sub(1));
                if self.tabs.is_empty() {
                    self.tree.selected = None;
                    self.sync_syntax_theme(cx);
                    self.set_title(window);
                }
                self.invalidate_tree();
                self.status = Some(format!("已删除 {}", shell::friendly_path(path)));
            },
            Err(error) => self.status = Some(format!("删除失败：{error}")),
        }
        cx.notify();
    }
}

/// 标签条上应当显示的标签下标（纯函数，便于测试）。
///
/// **没有任何标签被固定时只显示当前这一个**（单文件模式）；只要有一个被固定就
/// 显示全部（那时固定项与当前项并存，形成多个标签）。见 `Workspace::visible_tabs`。
fn visible_tab_indices(pinned: &[bool], active: usize) -> Vec<usize> {
    if pinned.is_empty() {
        return Vec::new();
    }
    if pinned.iter().any(|pinned| *pinned) {
        (0..pinned.len()).collect()
    } else {
        vec![active.min(pinned.len() - 1)]
    }
}

/// 在 `dir` 下挑一个不重名的子项名：`base` 或 `base.ext`，重名时补 ` 2`、` 3`……
fn unique_child(dir: &Path, base: &str, ext: &str) -> String {
    let candidate = |n: usize| -> String {
        if n == 0 {
            if ext.is_empty() { base.to_owned() } else { format!("{base}.{ext}") }
        } else if ext.is_empty() {
            // 序号从 2 开始（第一个用默认名）：`新建文件夹 2`、`新建文件夹 3`……
            format!("{base} {}", n + 1)
        } else {
            format!("{base} {}.{ext}", n + 1)
        }
    };
    for n in 0..1000 {
        let name = candidate(n);
        if !dir.join(&name).exists() {
            return name;
        }
    }
    candidate(1000)
}

/// 标签右键菜单里的命令。
///
/// 用一个 `Copy` 的小枚举而不是给每个条目各写一个闭包：菜单构建处要生成六条
/// 命令，逐个手写捕获会让那段代码膨胀成一个闭包墙，且每条都要重复一次
/// "weak → upgrade → update" 的样板。这里把"哪条命令"与"怎么执行"分开。
#[derive(Clone, Copy)]
enum TabCommand {
    TogglePin,
    Close,
    CloseOthers,
    CloseToRight,
    CopyPath,
    Reveal,
    OpenExternally,
}

impl TabCommand {
    fn run(self, workspace: &mut Workspace, index: usize, window: &mut Window, cx: &mut Context<Workspace>) {
        let Some(path) = workspace.tabs.get(index).map(|doc| doc.path.clone()) else {
            return;
        };
        match self {
            Self::TogglePin => workspace.toggle_pin(index, cx),
            Self::Close => workspace.close_tab(index, window, cx),
            Self::CloseOthers => workspace.close_other_tabs(index, window, cx),
            Self::CloseToRight => workspace.close_tabs_to_right(index, window, cx),
            Self::CopyPath => {
                let shown = shell::friendly_path(&path);
                // 剪贴板里给"人用的"形式（去掉 `\\?\`）：粘进对话框、终端、聊天
                // 时 verbatim 前缀只是噪音。
                cx.write_to_clipboard(ClipboardItem::new_string(shown.clone()));
                workspace.status = Some(format!("已复制路径：{shown}"));
                cx.notify();
            },
            Self::Reveal => match shell::reveal_in_file_manager(&path) {
                Ok(()) => {
                    workspace.status =
                        Some(format!("已在资源管理器中显示：{}", shell::friendly_path(&path)))
                },
                Err(error) => workspace.status = Some(format!("无法在资源管理器中显示：{error}")),
            },
            Self::OpenExternally => match shell::open_with_default_app(&path) {
                Ok(()) => {
                    workspace.status =
                        Some(format!("已交给系统打开：{}", shell::friendly_path(&path)))
                },
                Err(error) => workspace.status = Some(format!("无法用默认程序打开：{error}")),
            },
        }
    }
}

/// 开始监听 `path` 的外部改动，并起一个前台任务把事件翻译成"重载或重渲染"。
///
/// 监听器由对应的 [`Document`] 持有：标签关闭时它被丢弃，发送端随之消失、通道
/// 关闭，这个任务自己就退出了——不需要额外的取消逻辑。任务里还比对一次路径，
/// 防止旧监听器漏过来的事件作用到别的标签上。
fn watch_document(
    path: &Path,
    window: &Window,
    cx: &mut Context<Workspace>,
) -> Result<FileWatch, String> {
    let (watch, events) = FileWatch::start(path).map_err(|error| error.to_string())?;
    let watched = path.to_path_buf();
    cx.spawn_in(window, async move |this, cx| {
        while let Ok(signal) = events.recv().await {
            // 一次保存往往连着好几条事件，先让写入方落定，再把这一批合并成一次处理。
            cx.background_executor().timer(watch::SETTLE).await;
            let mut target = signal == Signal::Target;
            while let Ok(more) = events.try_recv() {
                target |= more == Signal::Target;
            }
            let applied = cx.update(|window, cx| {
                this.update(cx, |this, cx| {
                    // 目录里出现了增删改：展平行缓存必须作废，否则树停在旧内容上
                    // （原先靠"每次渲染重读磁盘"隐式跟上，现在改为事件驱动）。
                    this.invalidate_tree();
                    if target {
                        this.apply_external_change(&watched, window, cx);
                    }
                    // 结构事件（目录里别的文件变了）到这里就够了：行缓存已作废，
                    // 下一次渲染会重新读盘。
                    cx.notify();
                })
            });
            if applied.is_err() {
                // 视图已经释放，这个任务没有存在的理由了。
                break;
            }
        }
    })
    .detach();
    Ok(watch)
}

/// 解码本地图片为 gpui 需要的 BGRA `RenderImage`。
///
/// 与 Pebrel `doc_tabs.rs::decode_bgra` 同路数：`image` crate 解出 RGBA，
/// 逐像素交换 R/B 通道得到 BGRA。
fn decode_image(path: &Path) -> Result<Arc<RenderImage>, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("无法读取：{error}"))?;
    let mut rgba = image::load_from_memory(&bytes)
        .map_err(|error| format!("无法解码：{error}"))?
        .into_rgba8();
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    Ok(Arc::new(RenderImage::new([Frame::new(rgba)])))
}

/// 已解码图片的像素尺寸（`RenderImage::size` 取的就是帧缓冲的原始像素数）。
fn image_dimensions(image: &Arc<RenderImage>) -> Option<(u32, u32)> {
    let size = image.size(0);
    (size.width.0 > 0 && size.height.0 > 0)
        .then(|| (size.width.0 as u32, size.height.0 as u32))
}

/// 关掉 `closed` 之后新的激活下标，`new_len` 是关闭之后的标签数（>0）。
///
/// 三条分支覆盖三种情况：关掉激活项左边的一个（激活项整体左移一格，仍指向原来
/// 那个文档）；关掉激活项本身或它右边的一个（激活项留在原下标，自然变成"右边
/// 那个"）；关掉末尾的激活项（回落到新的末尾）。
fn active_after_close(active: usize, closed: usize, new_len: usize) -> usize {
    if closed < active {
        active - 1
    } else if active >= new_len {
        new_len - 1
    } else {
        active
    }
}

/// 标签循环：`(active + delta)` 在 `[0, len)` 上取模（`rem_euclid` 保证负 delta
/// 也落在合法区间）。
fn cycle_index(active: usize, delta: isize, len: usize) -> usize {
    (active as isize + delta).rem_euclid(len as isize) as usize
}

/// 重算括号匹配并写进 `Document::brace_matches`。编辑器光标一动就被观察回调调用。
///
/// 先做 O(1) 预检（光标处或左侧不是括号就直接判空），这样正常打字——绝大多数
/// 按键都不贴着括号——不会付那次全文扫描。只有确实贴着括号时才取全文交给
/// `brackets::matching`（它一次 `O(n)` 词法扫描，跳过字符串/注释里的括号）。
///
/// **只存结果、不 notify**：`brace_matches` 是普通字段，写它不会触发重绘，所以
/// 不存在 "写 → notify → observe → 写" 的自激循环（早先用组件库的
/// `TextDecorationCollection::set` 做过一版，它的 `set` 无条件 `notify()`，直接
/// 把 CPU 打满——这也是改回自绘的原因之一）。渲染时按存下的区间画框。
fn refresh_brace_marks(doc: &mut Document, cx: &mut Context<Workspace>) {
    let matches = {
        let state = doc.input.read(cx);
        let rope = state.text();
        let cursor = state.cursor();
        let is_bracket = |i: usize| {
            matches!(rope.get_byte(i), Some(b'(' | b')' | b'[' | b']' | b'{' | b'}'))
        };
        if !is_bracket(cursor) && !(cursor > 0 && is_bracket(cursor - 1)) {
            None
        } else {
            crate::brackets::matching(&rope.to_string(), cursor)
        }
    };
    if matches == doc.brace_matches {
        return;
    }
    doc.brace_matches = matches;
    cx.notify();
}

/// 裸 URL 扫描的缓冲上限。
///
/// 这一层是**每次改动全文重扫**（URL 位置随编辑漂移，没法只补增量），实测优化后
/// 约 4 ms/MiB。为了不牺牲大文件的打字延迟，超过这个尺寸就不做热点高亮——现实中
/// 需要点链接的 Markdown / 文本都远小于它（本仓库的 AGENTS.md 也才 69 KB）。
/// Notepad3 只扫可见区间，那是 Scintilla 的 indicator 机制给的便利，这里没有
/// 对等的口子。
const HOTSPOT_MAX_BYTES: usize = 1024 * 1024;

/// 把缓冲里的裸 URL 扫成一组装饰（Notepad3 的 "Hyperlink Hotspots"）。
///
/// 扫描规则在 [`crate::urls::hotspots`]；这里只负责把它转成组件库的
/// [`TextDecoration`]。颜色用 Notepad3 `styleLexStandard.c` 里该槽的 inactive
/// 前景色（`#0060B0`）。
///
/// 装饰会在语法样式**之后**合成（`input/element.rs::compose_decoration_collections`）。
/// 与词法色**重叠**时（例如 Markdown 链接 `[文字](url)` 里那段 URL），最终颜色由
/// 组件库合并重叠样式时的集合迭代序决定——实测稳定地保留词法色（`#0000FF`），
/// 那同样是"链接蓝"、观感一致；**不与词法色重叠**的裸 URL（本层的目标）则拿到
/// `#0060B0`。两个色都来自 Notepad3 的链接族，不影响"和 Notepad3 一样"。
///
/// 这里只给 `color`，不动字重/字形——与 Notepad3 的 `INDIC_TEXTFORE`（只改文字色）
/// 一致。
fn hotspot_decorations(text: &str) -> Vec<TextDecoration> {
    if text.len() > HOTSPOT_MAX_BYTES {
        return Vec::new();
    }
    let (r, g, b) = urls::HOTSPOT_RGB;
    let style = HighlightStyle { color: Some(hsla(r, g, b)), ..Default::default() };
    urls::hotspots(text)
        .into_iter()
        .map(|range| TextDecoration::new(range, style))
        .collect()
}

fn hsla(r: u8, g: u8, b: u8) -> gpui::Hsla {
    gpui::Rgba {
        r: f32::from(r) / 255.0,
        g: f32::from(g) / 255.0,
        b: f32::from(b) / 255.0,
        a: 1.0,
    }
    .into()
}

/// 把 `np3` 层里**有前景色**的段转成组件库的装饰（批处理关键字、Markdown 标题前景）。
///
/// 与 `hotspot_decorations` 同一个道理：装饰在语法样式之后合成，能盖过 tree-sitter
/// 给错的色（批处理的 `if` / `call` 在语法里根本捕不到）。**没有前景色的段
/// （纯底色）跳过**——底色不吃装饰这条路，走 `render_source` 下方的 canvas。
fn foreground_decorations(spans: &[np3::Span]) -> Vec<TextDecoration> {
    spans
        .iter()
        .filter_map(|span| {
            let (r, g, b) = span.ink.fg?;
            let mut style = HighlightStyle { color: Some(hsla(r, g, b)), ..Default::default() };
            if span.ink.bold {
                style.font_weight = Some(gpui::FontWeight::BOLD);
            }
            Some(TextDecoration::new(span.range.clone(), style))
        })
        .collect()
}

/// 补充层的一整份计算结果（前景装饰 + 底色段 + 小色块）。
///
/// 三处调用点（打开、缓冲变更、外部重载）都用它，避免逻辑三份。
struct Overlay {
    foreground: Vec<TextDecoration>,
    backgrounds: Vec<np3::Span>,
}

/// 重算补充层。语言没有补充槽时直接给空，免掉扫描。
fn compute_overlay(language: &str, text: &str) -> Overlay {
    if !np3::has_overlay(language) {
        return Overlay { foreground: Vec::new(), backgrounds: Vec::new() };
    }
    let spans = np3::spans(language, text);
    Overlay { foreground: foreground_decorations(&spans), backgrounds: spans }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(PathBuf::from).collect()
    }

    /// 造一个滚轮事件：`lines` 行、可选 Ctrl。`ScrollDelta::Lines` 会经
    /// `pixel_delta` 换成像素，所以这里按"行"喂，贴近真实设备。
    fn wheel(lines: f32, control: bool) -> ScrollWheelEvent {
        let mut modifiers = gpui::Modifiers::default();
        modifiers.control = control;
        ScrollWheelEvent {
            position: gpui::point(px(0.0), px(0.0)),
            delta: gpui::ScrollDelta::Lines(gpui::point(0.0, lines)),
            modifiers,
            touch_phase: gpui::TouchPhase::default(),
        }
    }

    /// 不带 Ctrl 的滚轮不归字号管；带 Ctrl 但没位移的也不管（否则一次空滚轮
    /// 会把字号改动一格）。
    #[test]
    fn font_zoom_only_reacts_to_control_wheel_with_motion() {
        assert_eq!(font_zoom_step(&wheel(1.0, false)), None, "无 Ctrl 不调字号");
        assert_eq!(font_zoom_step(&wheel(0.0, true)), None, "零位移不调字号");
        assert_eq!(font_zoom_step(&wheel(-1.0, false)), None, "无 Ctrl 反向也不调");
    }

    /// Ctrl+滚轮的方向：向上加、向下减，且无论一格滚轮被换算成几行，结果都只是
    /// ±1（幅度归一化，别让一格滚轮跳好几档）。
    #[test]
    fn font_zoom_direction_is_one_step_regardless_of_magnitude() {
        assert_eq!(font_zoom_step(&wheel(1.0, true)), Some(1.0));
        assert_eq!(font_zoom_step(&wheel(3.0, true)), Some(1.0), "一格 = ±1，不随行数放大");
        assert_eq!(font_zoom_step(&wheel(30.0, true)), Some(1.0));
        assert_eq!(font_zoom_step(&wheel(-1.0, true)), Some(-1.0));
        assert_eq!(font_zoom_step(&wheel(-30.0, true)), Some(-1.0));
    }

    /// 缩放档数按"格"归一：一格滚轮 = 1 档（±1 步的 ±0.33 累积到 ±1）。
    /// 这条直接锁住那个"一格跳三档、缩放太冲"的回归。
    #[test]
    fn one_wheel_notch_is_one_zoom_step() {
        let steps = wheel_steps(&wheel(3.0, false));
        assert!((steps - 1.0).abs() < 0.001, "一格滚轮（3 行）应为 1 档，实际 {steps}");
        let steps_up = wheel_steps(&wheel(1.0, false));
        assert!((steps_up - 1.0 / 3.0).abs() < 0.001, "一行应为 1/3 档，实际 {steps_up}");
    }

    /// 重名文件补父目录名区分；不重名的标签只用文件名。
    #[test]
    fn duplicate_names_get_their_parent_directory() {
        let owned = paths(&["a/notes.md", "b/notes.md", "a/readme.md"]);
        let refs: Vec<&Path> = owned.iter().map(PathBuf::as_path).collect();
        let labels = tab_labels(refs);
        assert_eq!(labels, vec!["a/notes.md", "b/notes.md", "readme.md"]);
    }

    /// 不重名就保持简短的文件名，不无谓地加前缀。
    #[test]
    fn unique_names_stay_bare() {
        let owned = paths(&["src/main.rs", "docs/notes.md"]);
        let refs: Vec<&Path> = owned.iter().map(PathBuf::as_path).collect();
        assert_eq!(tab_labels(refs), vec!["main.rs", "notes.md"]);
    }

    /// 关闭之后激活项要落到相邻标签，而不是跳到别的文档或越界。
    #[test]
    fn closing_picks_the_neighbour() {
        // [A,B,C] 激活 C，关掉左边的 A：激活项左移一格仍是 C。
        assert_eq!(active_after_close(2, 0, 2), 1);
        // 关掉激活项 C 本身：回落成 B（新的末尾）。
        assert_eq!(active_after_close(2, 2, 2), 1);
        // 关掉激活项 A：留在 0，自然变成 B。
        assert_eq!(active_after_close(0, 0, 2), 0);
        // 关掉激活项右边的 C：激活项 A 不动。
        assert_eq!(active_after_close(0, 2, 2), 0);
    }

    /// 循环在两个方向上都要环绕，且不会越界。
    #[test]
    fn cycling_wraps_both_ways() {
        assert_eq!(cycle_index(0, 1, 3), 1);
        assert_eq!(cycle_index(2, 1, 3), 0);
        assert_eq!(cycle_index(0, -1, 3), 2);
        assert_eq!(cycle_index(1, -1, 3), 0);
    }

    /// 标签条可见集合：没有固定时只显示当前这一个（单文件模式），有固定时全显示。
    /// 这条钉住用户要的默认行为——"每次切换文件只显示一个文件"。
    #[test]
    fn single_tab_mode_shows_only_the_active_document() {
        // 三个标签、都没固定：只显示当前那个，即便激活项不是第一个。
        assert_eq!(visible_tab_indices(&[false, false, false], 2), vec![2]);
        assert_eq!(visible_tab_indices(&[false, false, false], 0), vec![0]);

        // 只要有一个固定，就显示全部（固定项与当前项并存）。
        assert_eq!(visible_tab_indices(&[true, false, false], 2), vec![0, 1, 2]);
        assert_eq!(visible_tab_indices(&[false, false, true], 1), vec![0, 1, 2]);

        // 空工作区什么都不显示。
        assert_eq!(visible_tab_indices(&[], 0), Vec::<usize>::new());

        // 越界的激活项安全钳回末尾，不会 panic。
        assert_eq!(visible_tab_indices(&[false, false], 9), vec![1]);
    }

    /// 新建时的重名唯一化：第一个用默认名，之后补序号；文件名带扩展名。
    #[test]
    fn unique_child_avoids_collisions() {
        let dir = std::env::temp_dir().join(format!("nebula-lite-unique-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        assert_eq!(unique_child(&dir, "新建文件", "txt"), "新建文件.txt");
        std::fs::write(dir.join("新建文件.txt"), b"").unwrap();
        assert_eq!(unique_child(&dir, "新建文件", "txt"), "新建文件 2.txt");
        std::fs::write(dir.join("新建文件 2.txt"), b"").unwrap();
        assert_eq!(unique_child(&dir, "新建文件", "txt"), "新建文件 3.txt");

        // 文件夹没有扩展名，也不带点。
        assert_eq!(unique_child(&dir, "新建文件夹", ""), "新建文件夹");
        std::fs::create_dir(dir.join("新建文件夹")).unwrap();
        assert_eq!(unique_child(&dir, "新建文件夹", ""), "新建文件夹 2");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 设置文件里存的字体名只在**目录里真有**时才采用。名字失配（字体被卸载、
    /// 或设置文件被手改过）回落默认值，而不是交给 gpui 静默换一副字——那样
    /// "设置没生效"会毫无线索。
    #[test]
    fn saved_fonts_are_validated_against_the_catalog() {
        let catalog = vec!["Microsoft YaHei UI".to_owned(), "Consolas".to_owned()];
        assert_eq!(
            resolve_font(Some("Consolas"), &catalog, "Maple").as_ref(),
            "Consolas"
        );
        assert_eq!(
            resolve_font(Some("已经被卸载的字体"), &catalog, "Maple").as_ref(),
            "Maple"
        );
        assert_eq!(resolve_font(None, &catalog, "Maple").as_ref(), "Maple");
    }
}
