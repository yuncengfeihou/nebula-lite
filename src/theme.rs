//! 外观：**macOS 观感的中性灰白 + 系统蓝**，写进 `gpui_component::Theme`。
//!
//! 这一版的外观基准**不再是** Pebrel 的 Paper 皮肤。用户要的是"简约、舒适"的
//! 苹果风格：冷灰白底、系统蓝作强调色、层次靠留白与阴影而不是描边。Paper 那份
//! 调色板（暖纸白 `#fcfbf9` + 低饱和墨色）与本文件的其余部分**没有**关系，不要再
//! 往回照抄——它在 AGENTS.md 的历史里留了记录。
//!
//! 字段映射仍照 Pebrel 的 `nebula_app/src/gpui_shell/theme.rs::apply_skin_tokens`：
//! 哪些 `gpui_component::Theme` 字段吃哪个令牌是有讲究的（例如 accent 是水洗层、
//! primary 才是实色块），不能凭字段名猜。
//!
//! **编辑器里的代码配色与这里无关**：那部分照 Notepad3 的默认浅色方案、按语言
//! 取自它的 `styleLex*.c`，见本节「代码配色」与 `crate::syntax`。

use std::sync::Arc;

use gpui::{App, Hsla, Pixels, Rgba, px};
use gpui_component::Theme;
use gpui_component::highlighter::SyntaxColors;

/// 不透明墨色/语义色的字面量。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Rgb {
    r: u8,
    g: u8,
    b: u8,
}

/// 带 alpha 的水洗层字面量（hover / hairline / 轨道一族）。
#[derive(Clone, Copy)]
struct RgbA {
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

const fn rgb(r: u8, g: u8, b: u8) -> Rgb {
    Rgb { r, g, b }
}

const fn rgba(r: u8, g: u8, b: u8, a: u8) -> RgbA {
    RgbA { r, g, b, a }
}

// ---------------------------------------------------------------- 颜色助手
// 下面五个派生规则逐行移植自 Pebrel 的 gpui_shell/theme.rs；它们决定同一个
// 令牌在"成片色块"和"发丝细线"两种尺度上看是否刺眼，是 Paper 观感的一部分。

pub(crate) fn to_hsla(r: u8, g: u8, b: u8) -> Hsla {
    Rgba { r: f32::from(r) / 255.0, g: f32::from(g) / 255.0, b: f32::from(b) / 255.0, a: 1.0 }
        .into()
}

/// 不透明 ink。
fn ink(c: Rgb) -> Hsla {
    to_hsla(c.r, c.g, c.b)
}

/// 保留 alpha 的水洗层。
fn wash(c: RgbA) -> Hsla {
    Rgba {
        r: f32::from(c.r) / 255.0,
        g: f32::from(c.g) / 255.0,
        b: f32::from(c.b) / 255.0,
        a: f32::from(c.a) / 255.0,
    }
    .into()
}

/// 当不透明用的令牌（panel / danger / toggle 一族 alpha 本就是 255）。
fn solid(c: RgbA) -> Hsla {
    to_hsla(c.r, c.g, c.b)
}

fn luma(r: u8, g: u8, b: u8) -> f32 {
    0.2126 * f32::from(r) + 0.7152 * f32::from(g) + 0.0722 * f32::from(b)
}

/// hover/active 派生：深色往白提、浅色往黑压，幅度 `k`。
fn shift3(r: u8, g: u8, b: u8, k: f32) -> Hsla {
    let target = if luma(r, g, b) < 140.0 { 255.0 } else { 0.0 };
    let mix = |v: u8| (f32::from(v) + (target - f32::from(v)) * k).round().clamp(0.0, 255.0) as u8;
    to_hsla(mix(r), mix(g), mix(b))
}

/// 压在语义色块上的文字：深块配近白、浅块配近黑。
fn on_solid(c: RgbA) -> Hsla {
    if luma(c.r, c.g, c.b) < 150.0 { to_hsla(248, 250, 252) } else { to_hsla(15, 23, 42) }
}

/// 同色加浓（滚动条拖拽这类只调 alpha 的反馈）。
fn wash_scaled(c: RgbA, f: f32) -> Hsla {
    Rgba {
        r: f32::from(c.r) / 255.0,
        g: f32::from(c.g) / 255.0,
        b: f32::from(c.b) / 255.0,
        a: (f32::from(c.a) / 255.0 * f).min(1.0),
    }
    .into()
}

// ------------------------------------------------------------------ Paper 皮肤

#[derive(Clone, Copy)]
struct Skin {
    /// 外壳面（侧栏 / 标签条 / 状态栏 / 标题栏）。内容区不用它。
    shell: Rgb,
    panel: RgbA,
    card: RgbA,
    veil: RgbA,
    ink: Rgb,
    ink_dim: Rgb,
    ink_strong: Rgb,
    ink_on_accent: Rgb,
    accent: Rgb,
    accent_soft: RgbA,
    /// 选中 / 激活态的水洗底色（文件树选中行、激活标签、选中列表项）。
    ///
    /// 取**系统蓝**的 14% 铺底：苹果的选中态就是一层低饱和的淡色，而不是一块
    /// 实色。这也是为什么 `background` 与 `hover` 都是中性灰——整套里只有这一个
    /// 有色令牌，选中的位置一眼可辨，别的区域不抢注意力。
    select: RgbA,
    danger: RgbA,
    ok: RgbA,
    warn: RgbA,
    hairline: RgbA,
    surface: RgbA,
    hover: RgbA,
    track_off: RgbA,
    toggle_track_off: RgbA,
    knob_on: RgbA,
}

/// 外观基准：**macOS 的中性灰白 + 系统蓝**。
///
/// 取自苹果在浅色模式下的那套系统色（`systemGray6` 外壳、`#1d1d1f` 主文字、
/// `#007aff` 强调、`#d2d2d7` 分隔线、`#ff3b30 / #34c759 / #ff9500` 三个语义色），
/// 不是逐值照抄某个主题文件——所以这里没有"上游出处"可引，值本身就是出处。
const APPLE: Skin = Skin {
    // 外壳 = `systemGray6`。侧栏与标签条铺它，内容区铺白。
    shell: rgb(0xf5, 0xf5, 0xf7),
    // 弹窗 / 卡片 / 菜单：白底。层次靠阴影而不是描边。
    panel: rgba(0xff, 0xff, 0xff, 255),
    card: rgba(0xff, 0xff, 0xff, 255),
    veil: rgba(0x00, 0x00, 0x00, 60),
    ink: rgb(0x1d, 0x1d, 0x1f),
    ink_dim: rgb(0x86, 0x86, 0x8b),
    ink_strong: rgb(0x00, 0x00, 0x00),
    ink_on_accent: rgb(0xff, 0xff, 0xff),
    accent: rgb(0x00, 0x7a, 0xff),
    accent_soft: rgba(0x00, 0x7a, 0xff, 40),
    // 选中水洗：系统蓝 36/255 ≈ 14%。
    select: rgba(0x00, 0x7a, 0xff, 36),
    danger: rgba(0xff, 0x3b, 0x30, 255),
    ok: rgba(0x34, 0xc7, 0x59, 255),
    warn: rgba(0xff, 0x95, 0x00, 255),
    hairline: rgba(0xd2, 0xd2, 0xd7, 255),
    surface: rgba(0xff, 0xff, 0xff, 255),
    hover: rgba(0xec, 0xec, 0xef, 255),
    track_off: rgba(0x8e, 0x8e, 0x93, 86),
    toggle_track_off: rgba(0xe9, 0xe9, 0xeb, 255),
    knob_on: rgba(0xff, 0xff, 0xff, 255),
};

/// 界面文字字体族（默认值）。
///
/// 用户在设置面板里选过别的字体会覆盖它，并且会落盘（见 `crate::settings`）。
/// 想更接近 macOS 那种中性无衬线，可以在设置里换成 `Segoe UI`（或
/// `Segoe UI Variable Text`）；默认保持微软雅黑是因为中文字形最稳。
pub const UI_FONT_FAMILY: &str = "Microsoft YaHei UI";

/// 外壳色（侧栏 / 标签条 / 状态栏 / 标题栏）：`APPLE.shell`。
///
/// 内容区的纸面是白色（`theme.background`），外壳比它深一档——chrome 沉下去、
/// 正文浮起来。`gpui_component::Theme` 没有独立字段对应"外壳"，所以按需直接取。
pub fn shell_hsla() -> Hsla {
    ink(APPLE.shell)
}

/// 编辑器纸面底色（Notepad3 白底）。
///
/// `render_source` 用它给**容器**上底色：编辑器自身改用 `Input::appearance(false)`
/// 露出下方的背景色 canvas（Notepad3 的 `back:` 槽），白底由容器提供才不会一起被
/// 抹掉。见 `np3` 模块与 `render_source` 的绘制说明。
pub fn editor_bg_hsla() -> Hsla {
    ink(np3::EDITOR_BG)
}

/// 括号匹配的指示色。Notepad3 的 `styleLexStandard.c` 里
/// `Matching Braces (Indicator)` 默认是 `fore:#00FF40; alpha:80; alpha2:80;
/// indic_roundbox`，由 Scintilla 的 **indicator** 层画成一个**半透明绿填充 +
/// 圆角绿边**的框（`AlphaRectangle` 同时吃 fill 与 outline，两者同色同 alpha）。
///
/// 这里同色同 alpha 比例（80/255），在编辑器上方叠一层自绘圆角框同时填这个色、
/// 描这个边。做法与为什么不能靠文本装饰见 AGENTS.md「括号匹配」。
pub fn brace_match_hsla() -> Hsla {
    to_hsla(0x00, 0xff, 0x40).opacity(80.0 / 255.0)
}

/// 当前行高亮的颜色（Notepad3 `Current Line Background` 的淡黄）。
///
/// 组件库那层已经被关掉（见 `notepad3_highlight_theme` 的 `current_line` 参数），
/// 这个值现在给 `app.rs::render_source` 自绘当前行用；两边必须同源，否则"点一下
/// 亮起来的颜色"会和旧观感对不上。
pub fn current_line_hsla() -> Hsla {
    ink(np3::CUR_LINE).opacity(np3::CUR_LINE_ALPHA)
}

// ---------------------------------------------------------------- 代码配色
//
// 编辑器里的**代码配色照 Notepad3 的默认浅色方案**，不是 Paper 调色板——用户
// 明确要"和 Notepad3 一样的高亮"。两者管的是不同东西：上面的 `Skin`/Paper 管
// 外壳（侧栏、标题栏），这一段管编辑器里的代码与纸面。
//
// **语法色是"按语言"的**：Notepad3 每个 lexer（`src/StyleLexers/styleLex*.c`）
// 各有一张样式表，同类 token 在不同语言里颜色不同（C 关键字深蓝、JS 橙、Rust
// 绿…）。那部分逐语言数据在 `crate::syntax`；本文件只负责把它装进组件库的全局
// `highlight_theme`，并管住与语言无关的编辑器表面。
//
// - 语法 token 色：见 `crate::syntax`（按语言的 Notepad3 调色板）。
// - 编辑器表面：Notepad3 白底、行号栏浅灰 `#f0f0f0`、当前行淡黄。
//
// 不写 `theme.highlight_theme` 的话，编辑器吃的是 gpui-component 内置的
// **Zed One Light**（关键字与数字同为 `#0433ff`、注释 `#007fff`）——那正是用户
// 截图里"和 Notepad3 不一样"的来源。
//
// 组件库的高亮主题是**全局单例**（没有"每个输入框各一份"的口子），所以按语言
// 配色要靠切换文档时重新装入：见 `install_syntax_theme` 与 `app.rs::sync_syntax_theme`。

/// Notepad3 的编辑器表面颜色（与语法着色无关的那几个槽）。语法色见 `crate::syntax`。
mod np3 {
    use super::{Rgb, rgb};

    pub(super) const TEXT: Rgb = rgb(0x00, 0x00, 0x00);
    /// 编辑器纸面与行号栏底色（Notepad3 白底 + 系统浅灰边栏）。
    pub(super) const EDITOR_BG: Rgb = rgb(0xff, 0xff, 0xff);
    pub(super) const GUTTER_BG: Rgb = rgb(0xf0, 0xf0, 0xf0);
    /// 当前行底色。Notepad3 `styleLexStandard.c` 的 `Current Line Background` 默认
    /// `back:#FFFF00; alpha:50`，且 `HighlightCurrentLine` 默认值就是 **1（背景档）**
    /// （`Config/Config.cpp` 的 `GET_INT_VALUE_FROM_INISECTION(HighlightCurrentLine, 1, 0, 2)`）。
    /// Scintilla 的 alpha 是 0–255，50/255 铺在白底上 ≈ `#ffffcd`，所以这里用
    /// `#ffff00` + `opacity(50/255)`。
    pub(super) const CUR_LINE: Rgb = rgb(0xff, 0xff, 0x00);
    pub(super) const CUR_LINE_ALPHA: f32 = 50.0 / 255.0;
}

fn hex_of(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
}

/// 把基线语法主题的颜色（与字重/字形）整体换成 `language` 对应的 Notepad3 方案，
/// 键集合原样保留。
///
/// 走 serde_json 是因为组件库 `ThemeStyle` 的字段私有、也没有构造函数——只能像
/// Pebrel 的 `syntax.rs` 那样序列化出来改再反序列化回去。
///
/// **注意这里带 `language`**：Notepad3 每个 lexer 一张样式表，颜色随语言变
/// （见 `crate::syntax`）。组件库的高亮主题却是全局单例，所以"按语言"这件事要靠
/// 调用方在切换文档时重新装入——见 [`install_syntax_theme`]。
fn recolor_syntax(baseline: &SyntaxColors, language: &str) -> SyntaxColors {
    let mut syntax = serde_json::to_value(baseline).expect("syntax theme is serializable");
    if let Some(entries) = syntax.as_object_mut() {
        // 遍历的是**捕获名 → 颜色**这张键表（`SyntaxColors` 的字段名就是捕获名，
        // 正好是 `crate::syntax::SUPPORTED_CAPTURES` 那 41 个）。
        for (name, style) in entries.iter_mut() {
            let style_for = crate::syntax::style(language, name);
            // 基线里没声明的捕获名会序列化成 null，先补成空对象再写。
            if style.is_null() {
                *style = serde_json::json!({});
            }
            if let Some(style) = style.as_object_mut() {
                style.insert(
                    "color".to_owned(),
                    serde_json::json!(hex_of(Rgb {
                        r: style_for.rgb.0,
                        g: style_for.rgb.1,
                        b: style_for.rgb.2,
                    })),
                );
                // 每个键都显式写一遍字重/字形：需要时写 700 / "italic"、否则清成
                // null，否则会残留 Zed One Light 里自带的那几个（例如 `link_uri`
                // 的斜体）。
                style.insert(
                    "font_weight".to_owned(),
                    if style_for.bold { serde_json::json!(700) } else { serde_json::json!(null) },
                );
                style.insert(
                    "font_style".to_owned(),
                    if style_for.italic {
                        serde_json::json!("italic")
                    } else {
                        serde_json::json!(null)
                    },
                );
            }
        }
    }
    serde_json::from_value(syntax).expect("reviewed syntax colors keep the schema")
}

/// 把 Notepad3 的编辑器表面与 `language` 的代码配色写进 `theme.highlight_theme`。
///
/// `current_line` 为 `false` 时**不装**当前行高亮令牌：组件库的当前行背景是无条件
/// 跟着光标走的（打开文件、滚动、点选都会让某行常亮），我们的做法是关掉它、改由
/// `app.rs::render_source` 在"用户碰过编辑器"之后自绘。见 `install_syntax_theme`。
fn apply_syntax_theme(theme: &mut Theme, language: &str, current_line: bool) {
    theme.highlight_theme = Arc::new(notepad3_highlight_theme(language, current_line));
}

/// 某语言的**完整**高亮主题：Notepad3 编辑器表面 + 该语言的语法色。
///
/// `apply_syntax_theme` 与测试共用这一份，保证"断言的就是装入的"——单测若拿
/// 组件库基线主题（`default_light`）去查颜色，会漏掉我们实际改过的那层。
pub fn notepad3_highlight_theme(
    language: &str,
    current_line: bool,
) -> gpui_component::highlighter::HighlightTheme {
    let mut highlighted = (*gpui_component::highlighter::HighlightTheme::default_light()).clone();
    // 编辑器表面：白底、正文黑、行号栏浅灰。
    //
    // 当前行背景（Notepad3 默认那层淡黄 `#ffff00` @ alpha 50/255）**按需**装：
    // `current_line == false` 时装 `None`，组件库就不画它；那个淡黄 quad 改由
    // `render_source` 在光标被用户碰过之后用 `range_to_bounds` 自绘。组件库没有
    // "条件性当前行高亮"的口子，所以只能这样把它接管过来。
    highlighted.style.editor_background = Some(ink(np3::EDITOR_BG));
    highlighted.style.editor_foreground = Some(ink(np3::TEXT));
    highlighted.style.editor_line_number = Some(ink(np3::TEXT));
    highlighted.style.editor_active_line_number = Some(ink(np3::TEXT));
    highlighted.style.editor_active_line = if current_line {
        Some(ink(np3::CUR_LINE).opacity(np3::CUR_LINE_ALPHA))
    } else {
        None
    };
    highlighted.style.editor_gutter_background = Some(ink(np3::GUTTER_BG));
    highlighted.style.syntax = recolor_syntax(&highlighted.style.syntax, language);
    highlighted.name = format!("Notepad3 ({language})");
    highlighted
}

/// 给当前文档的语言装入对应的高亮主题。
///
/// 组件库的高亮主题是**一张全局表**（`Theme::highlight_theme`），渲染时按
/// `cx.theme()` 取——没有"每个输入框各用一份主题"的口子。所以"按语言配色"只能
/// 在切换文档时把全局主题换掉。编辑器只在源码面渲染当前激活的标签，因此全局一份
/// 是够的；调用方（`app.rs::sync_syntax_theme`）负责在激活标签 / 从预览切回源码时
/// 调这里，并用语言记忆避免每帧重装。
///
/// `current_line` 见 [`apply_syntax_theme`]；传递方是 `Workspace`（`hide_default_
/// current_line`）。
pub fn install_syntax_theme(cx: &mut App, language: &str, current_line: bool) {
    let theme = Theme::global_mut(cx);
    apply_syntax_theme(theme, language, current_line);
}

/// 控件圆角（按钮 / 输入框 / 小药丸）。macOS 的小控件是 6px。
const RADIUS: Pixels = px(6.0);
/// 面板圆角（对话框 / 卡片 / 通知）。macOS 的面板在这一档。
const RADIUS_LG: Pixels = px(10.0);
/// 界面字号，对应 Pebrel 的默认 `ui_font_size`。
const UI_FONT_SIZE: Pixels = px(14.0);
/// 等宽字号相对界面字号的比值（沿用 Pebrel 的 13/14）。
///
/// `app.rs` 改字号时必须拿它同步 `Theme::mono_font_size`：Markdown 预览里的代码块
/// 与行内代码读的是那个字段，而不是编辑器的 `text_size`。
pub const MONO_FONT_RATIO: f32 = 13.0 / 14.0;

/// 把外观令牌写进 `gpui_component::Theme` 的全局单例。
///
/// 必须在 `gpui_component::init` 之后、任何窗口渲染之前调用。
pub fn apply_theme(cx: &mut App) {
    let sk = APPLE;
    let transparent = Hsla { h: 0.0, s: 0.0, l: 0.0, a: 0.0 };
    let theme = Theme::global_mut(cx);

    // 内容区纸面：白。外壳（侧栏 / 标签条 / 状态栏）铺 `shell` 的浅灰——两者分色
    // 是这套观感的第一条：chrome 沉下去，正文浮起来。
    theme.background = solid(sk.surface);

    // 文字。
    theme.foreground = ink(sk.ink);
    theme.muted_foreground = ink(sk.ink_dim);

    // 面与线。
    theme.border = wash(sk.hairline);
    theme.input = wash(sk.hairline);
    // `muted` 是**次级面**：组件库拿它给代码块 / 行内代码 / 骨架屏上底色，苹果的
    // 那一档就是外壳灰 `#f5f5f7`。以前给的是纸面色，等于"压根没有底色"。
    theme.muted = ink(sk.shell);
    theme.group_box = solid(sk.card);
    theme.group_box_foreground = ink(sk.ink);
    theme.popover = solid(sk.panel);
    theme.popover_foreground = ink(sk.ink);
    theme.overlay = wash(sk.veil);

    // 悬停 / 选中水洗。两者都是**水洗**而不是实色：苹果的选中态就是一层淡色铺底
    // （系统蓝 14%），实色块留给 `primary` 那一档。见 `Skin::select` 的注释。
    let hover = wash(sk.hover);
    let selected = wash(sk.select);
    theme.accent = hover;
    theme.accent_foreground = ink(sk.ink_strong);
    theme.list_hover = hover;
    theme.list_active = selected;
    theme.list_active_border = transparent;
    theme.sidebar_foreground = ink(sk.ink);
    theme.sidebar_accent = selected;
    theme.sidebar_accent_foreground = ink(sk.ink_strong);
    theme.tab_foreground = ink(sk.ink_dim);
    theme.tab_active = selected;
    theme.tab_active_foreground = ink(sk.ink_strong);

    // 按钮：面积色压饱和、细元素保持原值换辨识度。
    let soft_accent = sk.accent;
    theme.primary = ink(soft_accent);
    theme.primary_hover = shift3(soft_accent.r, soft_accent.g, soft_accent.b, 0.10);
    theme.primary_active = shift3(soft_accent.r, soft_accent.g, soft_accent.b, 0.18);
    theme.primary_foreground = ink(sk.ink_on_accent);
    // 次级按钮用外壳灰：苹果的次要按钮就是一块浅灰，压在白底上看得见边界。
    // （Paper 那版 `secondary` 与纸面同色，按钮等于没有底——这也是当初自绘药丸的
    // 原因之一。）
    theme.secondary = ink(sk.shell);
    theme.secondary_hover = hover;
    theme.secondary_active = selected;
    theme.secondary_foreground = ink(sk.ink);

    // 语义三色。
    theme.danger = solid(sk.danger);
    theme.danger_hover = shift3(sk.danger.r, sk.danger.g, sk.danger.b, 0.10);
    theme.danger_active = shift3(sk.danger.r, sk.danger.g, sk.danger.b, 0.18);
    theme.danger_foreground = on_solid(sk.danger);
    theme.success = solid(sk.ok);
    theme.success_hover = shift3(sk.ok.r, sk.ok.g, sk.ok.b, 0.10);
    theme.success_active = shift3(sk.ok.r, sk.ok.g, sk.ok.b, 0.18);
    theme.success_foreground = on_solid(sk.ok);
    theme.warning = solid(sk.warn);
    theme.warning_hover = shift3(sk.warn.r, sk.warn.g, sk.warn.b, 0.10);
    theme.warning_active = shift3(sk.warn.r, sk.warn.g, sk.warn.b, 0.18);
    theme.warning_foreground = on_solid(sk.warn);

    // 1.16 起 Button 有独立 token；沿用同一套语义配色接上新入口。
    theme.button = theme.secondary;
    theme.button_hover = theme.secondary_hover;
    theme.button_active = theme.secondary_active;
    theme.button_foreground = theme.secondary_foreground;
    theme.button_primary = theme.primary;
    theme.button_primary_hover = theme.primary_hover;
    theme.button_primary_active = theme.primary_active;
    theme.button_primary_foreground = theme.primary_foreground;
    theme.button_secondary = theme.secondary;
    theme.button_secondary_hover = theme.secondary_hover;
    theme.button_secondary_active = theme.secondary_active;
    theme.button_secondary_foreground = theme.secondary_foreground;
    theme.button_danger = theme.danger;
    theme.button_danger_hover = theme.danger_hover;
    theme.button_danger_active = theme.danger_active;
    theme.button_danger_foreground = theme.danger_foreground;
    theme.button_success = theme.success;
    theme.button_success_hover = theme.success_hover;
    theme.button_success_active = theme.success_active;
    theme.button_success_foreground = theme.success_foreground;
    theme.button_warning = theme.warning;
    theme.button_warning_hover = theme.warning_hover;
    theme.button_warning_active = theme.warning_active;
    theme.button_warning_foreground = theme.warning_foreground;

    // 焦点 / 选择 / 链接 / 拖拽。
    theme.ring = ink(sk.accent);
    theme.caret = ink(sk.accent);
    theme.selection = ink(sk.accent).opacity(0.3);
    theme.link = ink(sk.accent);
    theme.link_hover = shift3(sk.accent.r, sk.accent.g, sk.accent.b, 0.10);
    theme.link_active = shift3(sk.accent.r, sk.accent.g, sk.accent.b, 0.18);
    theme.drag_border = ink(sk.accent);
    theme.drop_target = wash(sk.accent_soft);

    // 开关 / 滑条 / 滚动条。
    theme.switch = solid(sk.toggle_track_off);
    theme.switch_thumb = solid(sk.knob_on);
    theme.slider_bar = ink(soft_accent);
    theme.slider_thumb = solid(sk.knob_on);
    theme.scrollbar = transparent;
    theme.scrollbar_thumb = wash(sk.track_off);
    theme.scrollbar_thumb_hover = wash_scaled(sk.track_off, 1.6);

    // 代码高亮：语法色 + 编辑器表面。不写这一步编辑器就用组件库内置的
    // Zed One Light 默认主题（见本节顶部说明）。启动时先装一份纯文本（全黑）
    // 调色板；打开/切换文档时 `app.rs::sync_syntax_theme` 会按语言重新装入。
    // 当前行高亮默认**不装**（`false`）——它是"用户碰过编辑器才亮"的那条规则，
    // 由 `sync_syntax_theme` 按工作区状态重装。
    apply_syntax_theme(theme, "text", false);

    // 字号与圆角。等宽是语义标记（路径 / 代码 / 数值），界面文字走 sans，
    // 与 Pebrel 的取舍一致：把用户字体组写进全局 theme 会让标题字宽跟着变。
    // 这两个字族是**默认值**：用户在设置面板里选过之后，`Workspace::new` 会用
    // 存下来的那份覆盖它们。
    theme.font_size = UI_FONT_SIZE;
    theme.mono_font_size = theme.font_size * MONO_FONT_RATIO;
    theme.mono_font_family = crate::fonts::REQUIRED_FONT_FAMILY.into();
    theme.font_family = UI_FONT_FAMILY.into();
    // 圆角按 macOS 的两档来：控件 6px、面板（对话框 / 卡片）10px。
    theme.radius = RADIUS;
    theme.radius_lg = RADIUS_LG;

    // 新组件读 ThemeTokens；在所有颜色覆写完成后一次性解析。
    theme.tokens = (&theme.colors).into();
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_component::highlighter::HighlightTheme;

    fn value(syntax: &SyntaxColors, key: &str) -> serde_json::Value {
        serde_json::to_value(syntax).unwrap()[key].clone()
    }

    /// 换色的净效果：One Light 的蓝被 Notepad3 顶掉，且颜色**随语言变**——同一份
    /// 基线换成 C 与换成 JS 得到的关键字色不同。
    #[test]
    fn recoloring_matches_notepad3_and_drops_one_light() {
        let baseline = HighlightTheme::default_light();
        let before = serde_json::to_string(&baseline.style.syntax).unwrap();
        assert!(before.contains("#0433ff"), "基线应当是 One Light 的蓝");

        // JS：关键字橙粗体（styleLexJS.c）。
        let js = recolor_syntax(&baseline.style.syntax, "javascript");
        assert_eq!(value(&js, "keyword")["color"], "#a46000ff");
        assert_eq!(value(&js, "keyword")["font_weight"], 700);
        assert_eq!(value(&js, "number")["color"], "#ff0000ff");
        assert_eq!(value(&js, "string")["color"], "#008000ff");
        assert_eq!(value(&js, "comment")["color"], "#646464ff");
        assert_eq!(value(&js, "operator")["color"], "#b000b0ff");
        // JS 不给函数名上色。
        assert_eq!(value(&js, "function")["color"], "#000000ff");

        // C：关键字深蓝粗体、注释绿（styleLexCPP.c）——与 JS 明显不同。
        let c = recolor_syntax(&baseline.style.syntax, "c");
        assert_eq!(value(&c, "keyword")["color"], "#0a246aff");
        assert_eq!(value(&c, "comment")["color"], "#008000ff");

        // Rust：关键字绿、注释斜体青灰（styleLexRust.c）。
        let rust = recolor_syntax(&baseline.style.syntax, "rust");
        assert_eq!(value(&rust, "keyword")["color"], "#248112ff");
        assert_eq!(value(&rust, "comment")["color"], "#488080ff");
        assert_eq!(value(&rust, "comment")["font_style"], "italic");

        // One Light 的蓝不该残留在任何一份里。
        for recolored in [&js, &c, &rust] {
            let after = serde_json::to_string(recolored).unwrap();
            assert!(!after.contains("#0433ff"), "One Light 的蓝不该残留");
            assert!(!after.contains("#007fff"), "One Light 的注释蓝不该残留");
        }
    }

    /// 编辑器表面：白底、行号栏浅灰、当前行淡黄（Notepad3 默认）。这条把"改回
    /// One Light 或误把当前行高亮删掉都会红"钉住。表面与语言无关。
    ///
    /// `current_line = true` 时才装那层淡黄——默认（`false`）不装，改由自绘，
    /// 这样"打开文件就有一行常亮"的观感才不会回来。
    #[test]
    fn editor_surface_is_notepad3() {
        let mut theme = Theme::default();
        apply_syntax_theme(&mut theme, "rust", true);
        let style = &theme.highlight_theme.style;
        assert_eq!(style.editor_background, Some(ink(np3::EDITOR_BG)));
        assert_eq!(style.editor_gutter_background, Some(ink(np3::GUTTER_BG)));
        assert_eq!(
            style.editor_active_line,
            Some(ink(np3::CUR_LINE).opacity(np3::CUR_LINE_ALPHA)),
            "已装入当前行高亮时应当是 Notepad3 的淡黄"
        );
        assert_eq!(theme.highlight_theme.name, "Notepad3 (rust)");
    }

    /// 默认（`current_line = false`）**不**装当前行高亮：组件库那层必须是 `None`，
    /// 否则打开文件就有一行常亮，正是用户要修掉的观感。
    #[test]
    fn current_line_highlight_is_off_by_default() {
        let mut theme = Theme::default();
        apply_syntax_theme(&mut theme, "rust", false);
        assert_eq!(
            theme.highlight_theme.style.editor_active_line, None,
            "默认不该装当前行高亮（改由用户碰过编辑器后自绘）"
        );
        // 表面其余部分照旧：关的只是当前行那一层。
        assert_eq!(theme.highlight_theme.style.editor_background, Some(ink(np3::EDITOR_BG)));
    }

    /// 外观基准就是"macOS 的中性灰白 + 系统蓝"：外壳 `#f5f5f7`、纸面白、强调色
    /// `#007aff`、主文字 `#1d1d1f`、分隔线 `#d2d2d7`，控件圆角 6px、面板 10px。
    ///
    /// 这些值决定的就是"简约、舒适"那条观感本身，不是随手可调的细节——改它们
    /// 应该让这条测试红，从而逼一次"确认要换"的思考。
    #[test]
    fn palette_is_the_macos_neutral_plus_system_blue() {
        assert_eq!(ink(APPLE.shell), to_hsla(0xf5, 0xf5, 0xf7));
        assert_eq!(solid(APPLE.surface), to_hsla(0xff, 0xff, 0xff));
        assert_eq!(ink(APPLE.accent), to_hsla(0x00, 0x7a, 0xff));
        assert_eq!(ink(APPLE.ink), to_hsla(0x1d, 0x1d, 0x1f));
        assert_eq!(ink(APPLE.ink_dim), to_hsla(0x86, 0x86, 0x8b));
        assert_eq!(solid(APPLE.hairline), to_hsla(0xd2, 0xd2, 0xd7));
        assert_eq!(RADIUS, px(6.0));
        assert_eq!(RADIUS_LG, px(10.0));

        // 选中水洗必须是**冷色**（系统蓝 14%）。这一条把"外壳不许回到暖色调"
        // 钉住：Paper 那支是暖橙，红通道远高于蓝通道，与这里正好相反。
        let select = wash(APPLE.select);
        let blue = Rgba {
            r: 0.0,
            g: f32::from(0x7a_u8) / 255.0,
            b: 1.0,
            a: 36.0 / 255.0,
        };
        assert_eq!(select, Hsla::from(blue));
    }
}
