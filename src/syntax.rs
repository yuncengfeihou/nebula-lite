//! 代码配色：**按语言**照抄 Notepad3 的默认浅色方案。
//!
//! ## 为什么需要"按语言"
//!
//! Notepad3（Scintilla / Lexilla）给每个词法分析器各配一张样式表，写在各
//! `src/StyleLexers/styleLex*.c` 的 `EDITLEXER` 里。同一类 token 在不同语言里
//! 颜色往往不同，几个例子（都是各文件里写死的默认 `fore:` 值）：
//!
//! - 关键字：JS / Java / PHP 是橙粗体 `#A46000`，C / C++ / Go 是深蓝粗体
//!   `#0A246A`，Rust 是绿粗体 `#248112`，Python 是深蓝粗体 `#00007F`。
//! - Rust 连"内建类型"单独一色 `#A9003D`，注释是斜体青灰 `#488080`。
//! - 有的语言**给函数名单独上色**（Python `#660066`、Ruby `#007F7F`、Kotlin
//!   `#A46000`），有的**不给**（C / JS / Java 一律正文黑）。
//!
//! 组件库 `gpui-component` 的高亮主题是一张**全局**表：`SyntaxColors` 把
//! tree-sitter 的**捕获名**（`keyword`、`string`、`function`…）映射到颜色，
//! 与语言无关，而且字段固定为 [`SUPPORTED_CAPTURES`] 那 41 个。之前在
//! `theme.rs` 里整张表按 JS 的样式配死，所以只有 JS（以及单独打过补丁的
//! Markdown）看着对，别的语言一律套着 JS 的配色。
//!
//! 这里做三件事：
//!
//! 1. [`palette`]：每个语言一张"角色 → 样式"的调色板，逐值来自该语言的
//!    `styleLex*.c`。Notepad3 没有对应 lexer 的语言（Swift / Zig / Erlang /
//!    Haskell）回落到一份中性的 C 系配色。
//! 2. [`role`]：把组件库那 41 个捕获名落到语义角色上；少数语言有例外（例如
//!    Python 的 `constant` 是 SCREAMING_CASE 普通标识符）。
//! 3. [`canonical_capture`]：各语言的 tree-sitter 查询里有些捕获名**不在**组件
//!    库那 41 个里（`@escape`、`@delimiter`、`@media`…），组件库会静默渲染成
//!    黑色。`languages.rs` 用这个函数把它们改写成组件库认得的名字。
//!
//! ## 与 `theme.rs` 的分工
//!
//! 这里只管"捕获名 + 语言 → 颜色"，不碰 GUI、不碰全局主题。
//! `theme.rs::install_syntax_theme` 拿着本模块的结果去改写 `highlight_theme`。
//!
//! ## 已知近似
//!
//! tree-sitter 的捕获名与 Scintilla 的样式槽不是一一对应，少数地方只能取近似，
//! 都在 [`role`] 旁边的注释里写明。最典型的是**一个捕获名承载两类 token**：
//! Rust 的查询把数字与布尔一起塞进 `@constant.builtin`（前缀回落到 `constant`），
//! 而 Notepad3 里数字是灰、`true/false` 是关键字——这里按数字处理。

/// 一种样式：颜色 + 字重/字形。与 Notepad3 的 `fore:` / `bold` / `italic` 对应。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Style {
    pub rgb: (u8, u8, u8),
    pub bold: bool,
    pub italic: bool,
}

const fn st(r: u8, g: u8, b: u8, bold: bool, italic: bool) -> Style {
    Style { rgb: (r, g, b), bold, italic }
}

/// Notepad3 的正文色（`STYLE_DEFAULT` 没有 `fore:`，即编辑器前景色黑）。
pub const TEXT: Style = st(0x00, 0x00, 0x00, false, false);

/// 组件库 `SyntaxColors` 支持的 41 个捕获名。`role` 只会收到这些名字。
/// 顺序与组件库 `SyntaxColors` 的字段一致，便于核对。
pub const SUPPORTED_CAPTURES: [&str; 41] = [
    "attribute",
    "boolean",
    "comment",
    "comment.doc",
    "constant",
    "constructor",
    "embedded",
    "emphasis",
    "emphasis.strong",
    "enum",
    "function",
    "hint",
    "keyword",
    "label",
    "link_text",
    "link_uri",
    "number",
    "operator",
    "predictive",
    "preproc",
    "primary",
    "property",
    "punctuation",
    "punctuation.bracket",
    "punctuation.delimiter",
    "punctuation.list_marker",
    "punctuation.special",
    "string",
    "string.escape",
    "string.regex",
    "string.special",
    "string.special.symbol",
    "tag",
    "tag.doctype",
    "text.code.span",
    "text.literal",
    "title",
    "type",
    "variable",
    "variable.special",
    "variant",
];

/// 语义角色。调色板按角色取值，捕获名按角色归类——两边解耦。
///
/// 这是所有 Notepad3 lexer 样式槽的并集（C 的 "Typedefs/Classes"、Python 的
/// "Function Name"、Rust 的 "Built-In Type"、Ruby 的 "Symbol"…）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(usize)]
pub enum Role {
    Text = 0,
    Keyword,
    /// 二级关键字（C 的 `SCE_C_WORD2`、Python 的 "Keyword 2nd"）。
    Keyword2,
    /// 类型 / 内建类型 / typedef。
    Type,
    /// 函数名（Python "Function Name"、Ruby "Function Name"、Kotlin "Function"）。
    Function,
    /// 类名 / 构造（Python "Class Name"、Ruby "Class Name"）。
    Class,
    /// 常量 / 枚举 / 布尔。
    Constant,
    /// 变量 / 参数 / 字段。
    Variable,
    /// 属性 / 键（JSON "Property Name"、YAML/TOML 的 Key）。
    Property,
    /// 注解 / 装饰器（Python "Attribute"、Kotlin "Annotation"）。
    Attribute,
    /// 字符串。
    String,
    /// 另一种字符串（Python 三引号、Kotlin verbatim）。
    StringAlt,
    /// 转义序列。
    Escape,
    /// 正则字面量。
    Regex,
    /// 符号（Ruby / Elixir 的 `:sym`）。
    Symbol,
    /// 数字。
    Number,
    /// 注释。
    Comment,
    /// 文档注释。
    CommentDoc,
    /// 预处理 / 指令 / import。
    Preproc,
    /// 运算符与标点。
    Operator,
    /// 标签（HTML / XML）。
    Tag,
    /// 标签 / 生命周期（Rust lifetime、Lua label）。
    Label,
    /// 宏定义。
    Macro,
    /// 模块 / 命名空间。
    Namespace,
    /// 嵌入内容（HTML 里的 JS/PHP）。
    Embedded,
    /// 标题（Markdown）。
    Title,
    /// 列表标记（Markdown）。
    ListMarker,
    /// 链接（Markdown / JSON URI）。
    Link,
    /// 行内代码（Markdown）。
    InlineCode,
    /// 值（HTML/XML 属性值、CSS 数值）。
    Value,
    /// 强调（Markdown 斜体）。
    Emphasis,
    /// 加粗（Markdown 粗体）。
    Strong,
    /// CSS 的 `!important`（`SCE_CSS_IMPORTANT` = `bold; fore:#C80000`）。只有 CSS
    /// 用它；`Role::fallback` 把它落回正文黑，所以别的语言不受影响。
    Important,
}

const ROLE_COUNT: usize = Role::Important as usize + 1;

/// 未显式指定时的回落目标。让调色板只写"与回落不同"的那些槽。
fn fallback(role: Role) -> Option<Role> {
    use Role::*;
    match role {
        // 转义 / 另一种字符串 / 正则 / 符号：都是"字符串那一类"，默认跟字符串同色。
        Escape | StringAlt | Regex | Symbol => Some(String),
        // 文档注释跟注释同色（除非该语言单独给色）。
        CommentDoc => Some(Comment),
        // 二级关键字、常量、预处理、宏、列表标记：默认跟关键字同色。
        Keyword2 | Constant | Preproc | Macro | ListMarker => Some(Keyword),
        // 类名默认跟类型同色。
        Class => Some(Type),
        // 其余一律正文黑（Notepad3 只给一部分槽上色）。
        Keyword | Type | Function | Variable | Property | Attribute | String | Number
        | Comment | Operator | Tag | Label | Namespace | Embedded | Title | Link
        | InlineCode | Value | Emphasis | Strong | Important => Some(Text),
        Text => None,
    }
}

/// 一份语言调色板：只写该语言与回落不同的角色。
#[derive(Clone, Copy)]
pub struct Palette([Option<Style>; ROLE_COUNT]);

impl Palette {
    const fn new() -> Self {
        Palette([None; ROLE_COUNT])
    }

    const fn set(mut self, role: Role, style: Style) -> Self {
        self.0[role as usize] = Some(style);
        self
    }

    pub fn get(&self, role: Role) -> Style {
        let mut role = role;
        // 回落链最多两跳，给足余量后兜底正文色。
        for _ in 0..4 {
            if let Some(style) = self.0[role as usize] {
                return style;
            }
            match fallback(role) {
                Some(next) => role = next,
                None => return TEXT,
            }
        }
        TEXT
    }
}

/// 该语言的调色板。未收录的语言全黑（Notepad3 认不出的文件不着色）。
pub fn palette(language: &str) -> Palette {
    use Role::*;
    match language {
        // ---- C / C++：styleLexCPP.c ----
        "c" | "cpp" => Palette::new()
            .set(Comment, st(0x00, 0x80, 0x00, false, false))
            .set(CommentDoc, st(0x80, 0x80, 0x80, false, false))
            .set(Keyword, st(0x0A, 0x24, 0x6A, true, false))
            .set(Keyword2, st(0x3C, 0x6C, 0xDD, true, true))
            .set(Type, st(0x80, 0x00, 0x00, true, true))
            .set(Constant, st(0xA4, 0x60, 0x00, true, false)) // User Literal
            .set(String, st(0x00, 0x80, 0x00, false, false))
            .set(StringAlt, st(0xB0, 0x00, 0xB0, false, false)) // Verbatim
            .set(Regex, st(0x00, 0x66, 0x33, false, false))
            .set(Number, st(0xFF, 0x00, 0x00, false, false))
            .set(Operator, st(0xB0, 0x00, 0xB0, false, false))
            .set(Preproc, st(0xFF, 0x80, 0x00, false, false)),

        // ---- JavaScript / TypeScript / JSX：styleLexJS.c ----
        "javascript" | "jsx" | "typescript" | "tsx" => js_palette(),

        // ---- Python：styleLexPY.c ----
        "python" => Palette::new()
            .set(Comment, st(0x00, 0x7F, 0x00, false, false))
            .set(Keyword, st(0x00, 0x00, 0x7F, true, false))
            .set(Keyword2, st(0x00, 0x00, 0x88, false, false))
            .set(Constant, st(0x00, 0x00, 0x7F, true, false)) // None/True/False 归关键字
            .set(Attribute, st(0x00, 0x8E, 0x8E, false, false))
            .set(String, st(0x00, 0x88, 0x00, false, false))
            .set(StringAlt, st(0x88, 0xB6, 0x34, false, false)) // 三引号
            .set(Number, st(0xFF, 0x40, 0x00, false, false))
            .set(Operator, st(0x66, 0x66, 0x00, true, false))
            .set(Function, st(0x66, 0x00, 0x66, false, false))
            .set(Class, st(0x91, 0x00, 0x48, false, false))
            // "Decorator" 是单独一槽（`@app.route` 的 `@app`），金 `#F2B600`；
            // Python 没有预处理器，`@preproc` 只承载装饰器（见 `languages.rs`）。
            .set(Preproc, st(0xF2, 0xB6, 0x00, false, false)),

        // ---- Go：styleLexGo.c ----
        "go" => Palette::new()
            .set(Comment, st(0x00, 0x80, 0x00, false, false))
            .set(Comment, st(0x00, 0x80, 0x00, false, false))
            .set(CommentDoc, st(0x00, 0x40, 0xA0, false, false))
            .set(Keyword, st(0x0A, 0x24, 0x6A, true, false))
            .set(Constant, st(0x0A, 0x24, 0x6A, true, false)) // true/false/nil/iota
            .set(Type, st(0x0A, 0x24, 0x6A, false, true)) // Typedef 斜体
            .set(String, st(0x3C, 0x6C, 0xDD, false, true)) // 斜体
            .set(Number, st(0xFF, 0x00, 0x00, false, false))
            .set(Operator, st(0xB0, 0x00, 0xB0, false, false))
            .set(Preproc, st(0xFF, 0x80, 0x00, false, false)),

        // ---- Java：styleLexJAVA.c ----
        // 注意 Java 那张表**没有** Typedefs/Classes 槽（`styleLexJAVA.c` 是 C 系表里
        // 唯一不带 `SCE_C_GLOBALCLASS` 的），所以类名 / 类型名在 Notepad3 里是正文黑，
        // 这里也不给 `Type` 设色。
        "java" => Palette::new()
            .set(Comment, st(0x64, 0x64, 0x64, false, false))
            .set(Keyword, st(0xA4, 0x60, 0x00, true, false))
            .set(Constant, st(0xA4, 0x60, 0x00, true, false))
            .set(String, st(0x00, 0x80, 0x00, false, false))
            .set(StringAlt, st(0xB0, 0x00, 0xB0, false, false))
            .set(Regex, st(0x00, 0x66, 0x33, false, false))
            .set(Number, st(0xFF, 0x00, 0x00, false, false))
            .set(Operator, st(0xB0, 0x00, 0xB0, false, false))
            .set(Preproc, st(0xFF, 0x80, 0x00, false, false)),

        // ---- C#：styleLexCS.c ----
        "csharp" => Palette::new()
            .set(Comment, st(0x00, 0x80, 0x00, false, false))
            .set(Keyword, st(0x80, 0x40, 0x00, true, false))
            .set(Constant, st(0x80, 0x40, 0x00, true, false))
            .set(Type, st(0x2B, 0x91, 0xAF, false, false)) // Global Class
            .set(String, st(0x00, 0x80, 0x00, false, false))
            .set(Regex, st(0x00, 0x66, 0x33, false, false))
            .set(Number, st(0xFF, 0x00, 0x00, false, false))
            .set(Operator, st(0xB0, 0x00, 0xB0, false, false))
            .set(Preproc, st(0xFF, 0x80, 0x00, false, false)),

        // ---- Ruby：styleLexRUBY.c ----
        "ruby" => Palette::new()
            .set(Comment, st(0x00, 0x80, 0x00, false, false))
            .set(Keyword, st(0x00, 0x00, 0x7F, false, false))
            .set(Type, st(0x00, 0x00, 0xFF, false, false)) // Class Name
            .set(Number, st(0x00, 0x80, 0x80, false, false))
            .set(String, st(0xFF, 0x80, 0x00, false, false))
            .set(Function, st(0x00, 0x7F, 0x7F, false, false))
            .set(Symbol, st(0xC0, 0xA0, 0x30, false, false))
            .set(Namespace, st(0xA0, 0x00, 0xA0, false, false)) // Module Name
            .set(Variable, st(0xB0, 0x00, 0x80, false, false)) // Instance Var
            .set(Property, st(0xB0, 0x00, 0x80, false, false))
            .set(StringAlt, st(0xFF, 0x80, 0x00, false, false)),

        // ---- Lua：styleLexLUA.c ----
        "lua" => Palette::new()
            .set(Comment, st(0x00, 0x80, 0x00, false, false))
            .set(Keyword, st(0x00, 0x00, 0x7F, false, false))
            .set(Constant, st(0x00, 0x00, 0x7F, false, false)) // true/false/nil
            .set(Function, st(0x00, 0x00, 0x7F, false, false)) // 内建函数同色
            .set(String, st(0xB0, 0x00, 0xB0, false, false))
            .set(Number, st(0x00, 0x80, 0x80, false, false))
            .set(Label, st(0x80, 0x80, 0x00, false, false))
            .set(Preproc, st(0xFF, 0x80, 0x00, false, false)),

        // ---- Kotlin：styleLexKotlin.c ----
        "kotlin" => Palette::new()
            .set(Comment, st(0x60, 0x80, 0x60, false, false))
            .set(CommentDoc, st(0x40, 0x80, 0x80, false, false))
            .set(Keyword, st(0x00, 0x00, 0xFF, false, false))
            .set(Constant, st(0x00, 0x00, 0xFF, false, false))
            .set(Attribute, st(0xFF, 0x80, 0x00, false, false)) // Annotation
            .set(Class, st(0x00, 0x80, 0xFF, false, false))
            .set(Type, st(0x00, 0x80, 0xFF, false, false))
            .set(Function, st(0xA4, 0x60, 0x00, false, false))
            .set(String, st(0x00, 0x80, 0x00, false, false))
            .set(StringAlt, st(0xF0, 0x80, 0x00, false, false)) // Verbatim
            .set(Escape, st(0x00, 0x80, 0xC0, false, false))
            .set(Label, st(0x7C, 0x5A, 0xF3, false, false))
            .set(Number, st(0xFF, 0x00, 0x00, false, false))
            .set(Variable, st(0x9E, 0x4D, 0x2A, false, false))
            .set(Property, st(0x9E, 0x4D, 0x2A, false, false))
            .set(Operator, st(0xB0, 0x00, 0xB0, false, false)),

        // ---- SQL：styleLexSQL.c ----
        "sql" => Palette::new()
            .set(Comment, st(0x00, 0x80, 0x80, false, false))
            .set(CommentDoc, st(0x80, 0x80, 0x80, false, true))
            .set(Keyword, st(0x3E, 0x3E, 0xFF, true, false))
            .set(Constant, st(0x3E, 0x3E, 0xFF, true, false))
            .set(Type, st(0x00, 0x00, 0x80, true, false)) // Value Type
            .set(String, st(0x80, 0x80, 0x80, false, false))
            .set(Number, st(0xA2, 0x00, 0xA2, false, false))
            .set(Operator, st(0xFF, 0x80, 0x00, true, false))
            .set(Property, st(0x00, 0x00, 0x80, false, false)), // Quoted Identifier

        // ---- TOML：styleLexTOML.c ----
        "toml" => Palette::new()
            .set(Comment, st(0x00, 0x80, 0x00, false, false))
            .set(Keyword, st(0xFF, 0x00, 0x80, true, false))
            .set(Type, st(0xFF, 0x80, 0x00, true, false)) // Table
            .set(Property, st(0x5E, 0x60, 0x8F, true, false)) // Key
            .set(String, st(0x60, 0x60, 0x60, false, true))
            .set(StringAlt, st(0x95, 0x00, 0x95, false, false)) // Date-Time
            .set(Number, st(0x00, 0x00, 0xE0, false, false))
            .set(Operator, st(0xB0, 0x00, 0xB0, false, false))
            .set(Escape, st(0x20, 0x20, 0x20, false, false)),

        // ---- YAML：styleLexYAML.c ----
        "yaml" => Palette::new()
            .set(Comment, st(0x00, 0x88, 0x00, false, false))
            .set(Comment, st(0x00, 0x88, 0x00, false, false))
            .set(Keyword, st(0x88, 0x00, 0x88, false, false))
            .set(Constant, st(0x88, 0x00, 0x88, false, false)) // null scalar
            .set(Type, st(0x88, 0x00, 0x88, false, false)) // tag
            .set(Property, st(0x0A, 0x24, 0x6A, true, false)) // Key
            .set(Attribute, st(0x0A, 0x24, 0x6A, true, false))
            .set(Label, st(0x00, 0x88, 0x88, false, false)) // Reference
            .set(Number, st(0xFF, 0x80, 0x00, false, false))
            .set(String, st(0x40, 0x40, 0x40, false, false)) // Text
            .set(Operator, st(0x33, 0x33, 0x66, false, false)),

        // ---- HTML：styleLexHTML.c ----
        "html" => Palette::new()
            .set(Comment, st(0x64, 0x64, 0x64, false, false))
            .set(Tag, st(0x64, 0x80, 0x00, false, false))
            .set(Attribute, st(0xFF, 0x40, 0x00, false, false))
            .set(String, st(0x3A, 0x6E, 0xA5, false, false))
            .set(Value, st(0x3A, 0x6E, 0xA5, false, false))
            .set(Constant, st(0xB0, 0x00, 0xB0, false, false)) // Entity
            .set(Operator, st(0x3A, 0x6E, 0xA5, false, false)),

        // ---- CSS / SCSS：styleLexCSS.c ----
        "css" | "scss" => Palette::new()
            .set(Comment, st(0x64, 0x64, 0x64, false, false))
            .set(Tag, st(0x0A, 0x24, 0x6A, true, false)) // HTML Tag
            .set(Constant, st(0x64, 0x80, 0x00, false, false)) // Tag-Class / Tag-ID
            .set(Label, st(0xB0, 0x00, 0xB0, false, false)) // Pseudo-Class
            .set(Attribute, st(0x64, 0x80, 0x00, false, true)) // Tag-Attribute 斜体
            .set(Keyword, st(0x0A, 0x24, 0x6A, true, false)) // Media / Directive
            .set(Property, st(0xFF, 0x40, 0x00, false, false)) // CSS Property
            .set(String, st(0x00, 0x80, 0x00, false, false))
            // 十六进制颜色、数值、单位在 Notepad3 里都归 "Value"（`#3A6EA5`）——
            // 那台 lexer 没有给"颜色"单独开槽，也没有给 CSS 数字开槽。
            .set(Number, st(0x3A, 0x6E, 0xA5, false, false)) // Value
            .set(Value, st(0x3A, 0x6E, 0xA5, false, false))
            .set(Function, st(0x3A, 0x6E, 0xA5, false, false))
            .set(Operator, st(0xB0, 0x00, 0xB0, false, false))
            .set(Variable, st(0xFF, 0x40, 0x00, true, false)) // --custom-prop
            // `!important`（`SCE_CSS_IMPORTANT` = `bold; fore:#C80000`）。承载它的是
            // `syntax::language_override` 里"CSS 的 boolean → Important"那一条。
            .set(Important, st(0xC8, 0x00, 0x00, true, false)),

        // ---- XML / DTD：styleLexXML.c ----
        // DTD 与 XML 同属"标记文档"，共用 Notepad3 的 XML 表。
        "xml" | "dtd" => Palette::new()
            .set(Comment, st(0x64, 0x64, 0x64, false, false))
            .set(Tag, st(0x88, 0x12, 0x80, false, false))
            .set(Attribute, st(0x99, 0x45, 0x00, false, false))
            .set(Property, st(0x99, 0x45, 0x00, false, false))
            .set(String, st(0x1A, 0x1A, 0xA6, false, false))
            .set(Value, st(0x1A, 0x1A, 0xA6, false, false))
            .set(Operator, st(0x1A, 0x1A, 0xA6, false, false))
            .set(Constant, st(0xB0, 0x00, 0xB0, false, false)), // Entity

        // ---- JSON：styleLexJSON5.c ----
        "json" => Palette::new()
            .set(Comment, st(0x64, 0x64, 0x64, false, false))
            .set(Keyword, st(0x95, 0x70, 0x00, true, false))
            .set(Constant, st(0x95, 0x70, 0x00, true, false)) // true/false/null
            .set(String, st(0x00, 0x80, 0x00, false, false))
            .set(Escape, st(0x0B, 0x98, 0x2E, false, false))
            .set(Number, st(0xFF, 0x00, 0x00, false, false))
            .set(Operator, st(0xB0, 0x00, 0xB0, false, false))
            .set(Property, st(0x00, 0x26, 0x97, false, false)) // Property Name
            .set(Link, st(0x00, 0x00, 0xFF, false, false)), // URL/IRI

        // ---- Shell：styleLexBASH.c ----
        "bash" => Palette::new()
            .set(Comment, st(0x00, 0x80, 0x00, false, false))
            .set(Keyword, st(0x00, 0x00, 0xFF, false, false))
            .set(String, st(0x00, 0x80, 0x80, false, false))
            .set(StringAlt, st(0x80, 0x00, 0x80, false, false)) // 单引号
            .set(Number, st(0x00, 0x80, 0x80, false, false))
            .set(Variable, st(0x80, 0x80, 0x00, false, false)) // Scalar
            .set(Property, st(0x80, 0x80, 0x00, false, false)),

        // ---- 批处理（.bat / .cmd）：styleLexBAT.c ----
        // Notepad3 的槽位：Comment 绿、Keyword 深蓝粗体、Identifier `#003CE6`、
        // Operator 洋红、Command 黑粗体、Label `#C80000`。注意它**没有**字符串槽，
        // 所以这里的字符串走 Identifier 色（与命令名之外的普通 token 同档）。
        "batch" => Palette::new()
            .set(Comment, st(0x00, 0x80, 0x00, false, false))
            .set(CommentDoc, st(0x80, 0x80, 0x80, false, true)) // Doc Comment 斜体灰
            .set(Keyword, st(0x0A, 0x24, 0x6A, true, false))
            .set(Constant, st(0x00, 0x3C, 0xE6, false, false)) // 选项 → Identifier
            .set(Variable, st(0x00, 0x3C, 0xE6, false, false)) // Identifier
            .set(Label, st(0xC8, 0x00, 0x00, false, false))
            .set(Operator, st(0xB0, 0x00, 0xB0, false, false))
            .set(Function, st(0x00, 0x00, 0x00, true, false)) // Command 黑粗体
            .set(String, st(0x00, 0x3C, 0xE6, false, false)) // 无字符串槽，同 Identifier
            .set(StringAlt, st(0x00, 0x3C, 0xE6, false, false))
            .set(Number, st(0x00, 0x3C, 0xE6, false, false)),

        // ---- PowerShell：styleLexPS.c ----
        "powershell" => Palette::new()
            .set(Comment, st(0x64, 0x64, 0x64, false, false))
            .set(Keyword, st(0x80, 0x40, 0x00, true, false))
            .set(Function, st(0x80, 0x40, 0x00, false, false)) // Cmdlet
            .set(String, st(0x00, 0x80, 0x00, false, false))
            .set(Number, st(0xFF, 0x00, 0x00, false, false))
            .set(Variable, st(0x0A, 0x24, 0x6A, false, false))
            .set(Attribute, st(0x0A, 0x24, 0x6A, true, false)) // Alias
            .set(Operator, st(0x00, 0x00, 0x00, true, false)), // 粗体无变色

        // ---- PHP：styleLexHTML.c 的 PHP 槽 ----
        "php" => Palette::new()
            .set(Comment, st(0xFF, 0x80, 0x00, false, false))
            .set(Comment, st(0xFF, 0x80, 0x00, false, false))
            .set(Keyword, st(0xA4, 0x60, 0x00, true, false))
            .set(String, st(0x00, 0x80, 0x00, false, false))
            .set(Number, st(0xFF, 0x00, 0x00, false, false))
            .set(Operator, st(0xB0, 0x00, 0xB0, false, false))
            .set(Variable, st(0x00, 0x00, 0x80, false, true)), // 变量斜体

        // ---- Rust：styleLexRust.c ----
        "rust" => Palette::new()
            .set(Comment, st(0x48, 0x80, 0x80, false, true)) // 斜体
            .set(Keyword, st(0x24, 0x81, 0x12, true, false))
            .set(Keyword2, st(0x24, 0x81, 0x12, false, true)) // Other Keyword
            // 查询把数字与布尔一起塞进 `@constant.builtin`（回落到 `constant`），
            // 且 SCREAMING_CASE 常量也走 `@constant`；按多数的"数字"取灰。
            .set(Constant, st(0x66, 0x66, 0x66, false, false)) // Number
            .set(Type, st(0xA9, 0x00, 0x3D, false, false)) // Built-In Type
            .set(String, st(0xB3, 0x1C, 0x1B, false, false))
            .set(Operator, st(0x66, 0x66, 0x66, false, false))
            .set(Macro, st(0x0A, 0x24, 0x6A, false, false)) // Macro Definition
            .set(Label, st(0xB0, 0x00, 0xB0, false, false)) // Lifetime
            .set(Attribute, st(0x0A, 0x24, 0x6A, false, false)) // 属性当宏色
            .set(StringAlt, st(0xC0, 0xC0, 0xC0, false, false)), // Byte String

        // ---- 配置文件（INI / cfg）：styleLexPROPS.c ----
        "ini" => Palette::new()
            .set(Comment, st(0x00, 0x80, 0x00, false, false))
            .set(Property, st(0x00, 0x00, 0x6D, false, false)) // Key
            // Section 在 Notepad3 是 `bold; fore:#000000; back:#FF8040`（橙底黑字）；
            // 组件库的语法样式没有背景色，取"黑粗体"这一半。
            .set(Type, st(0x00, 0x00, 0x00, true, false)) // Section
            .set(Operator, st(0xFF, 0x00, 0x00, false, false)) // Assignment
            .set(String, st(0xFF, 0x00, 0x00, false, false)),

        // ---- CMake：styleLexCMAKE.c ----
        "cmake" => Palette::new()
            .set(Comment, st(0x00, 0x80, 0x00, false, false))
            .set(Keyword, st(0x00, 0x00, 0x7F, true, false))
            .set(Function, st(0x00, 0x00, 0x7F, false, false))
            .set(String, st(0x7F, 0x00, 0x7F, false, false))
            .set(Variable, st(0xCC, 0x33, 0x00, false, false))
            .set(Number, st(0x00, 0x80, 0x80, false, false))
            .set(Type, st(0x80, 0x00, 0x20, false, false)),

        // ---- Markdown：styleLexMARKDOWN.c ----
        "markdown" => Palette::new()
            .set(Comment, st(0x00, 0x80, 0x00, false, false))
            .set(Title, st(0x33, 0x61, 0x93, true, false))
            .set(InlineCode, st(0x00, 0x00, 0x7F, false, false))
            .set(ListMarker, st(0x00, 0x80, 0xFF, true, false))
            .set(Link, st(0x00, 0x00, 0xFF, false, false))
            .set(Operator, st(0x00, 0x00, 0x7F, false, false)) // Pre Char
            .set(Emphasis, st(0x00, 0x00, 0x00, false, true))
            .set(Strong, st(0x00, 0x00, 0x00, true, false)),

        // ---- Notepad3 没有 lexer 的语言 ----
        // Elixir 借 Ruby（同为 `#` 注释 + atom 符号）。
        "elixir" => Palette::new()
            .set(Comment, st(0x00, 0x80, 0x00, false, false))
            .set(Keyword, st(0x00, 0x00, 0x7F, false, false))
            .set(Type, st(0xA0, 0x00, 0xA0, false, false)) // Module Name
            .set(Number, st(0x00, 0x80, 0x80, false, false))
            .set(String, st(0xFF, 0x80, 0x00, false, false))
            .set(Symbol, st(0xC0, 0xA0, 0x30, false, false)),
        // Swift / Zig / Erlang / Haskell 借中性 C 系配色。
        "swift" | "zig" | "erlang" | "haskell" => neutral(),

        // 未知 / 纯文本：全黑（Notepad3 的 null lexer 就是不着色）。
        _ => Palette::new(),
    }
}

/// JS 系（JS / TS / JSX）共用 `styleLexJS.c`。
fn js_palette() -> Palette {
    use Role::*;
    Palette::new()
        .set(Comment, st(0x64, 0x64, 0x64, false, false))
        .set(CommentDoc, st(0x80, 0x80, 0x80, false, false))
        .set(Keyword, st(0xA4, 0x60, 0x00, true, false))
        .set(Constant, st(0xA4, 0x60, 0x00, true, false))
        .set(String, st(0x00, 0x80, 0x00, false, false))
        .set(StringAlt, st(0xB0, 0x00, 0xB0, false, false)) // verbatim
        .set(Regex, st(0x00, 0x66, 0x33, false, false))
        .set(Number, st(0xFF, 0x00, 0x00, false, false))
        .set(Operator, st(0xB0, 0x00, 0xB0, false, false))
        .set(Preproc, st(0xFF, 0x80, 0x00, false, false))
}

/// 一份中性的 C 系配色，给 Notepad3 没有对应 lexer 的语言兜底。
fn neutral() -> Palette {
    use Role::*;
    Palette::new()
        .set(Comment, st(0x00, 0x80, 0x00, false, false))
        .set(Keyword, st(0x0A, 0x24, 0x6A, true, false))
        .set(Constant, st(0xA4, 0x60, 0x00, true, false))
        .set(Type, st(0xA9, 0x00, 0x3D, false, false))
        .set(String, st(0x00, 0x80, 0x00, false, false))
        .set(Number, st(0xFF, 0x00, 0x00, false, false))
        .set(Operator, st(0xB0, 0x00, 0xB0, false, false))
        .set(Symbol, st(0xC0, 0xA0, 0x30, false, false))
}

/// 捕获名 → 角色。少数语言对个别捕获名有例外。
pub fn role(language: &str, capture: &str) -> Role {
    if let Some(role) = language_override(language, capture) {
        return role;
    }
    generic_role(capture)
}

/// 语言对**组件库 41 个捕获名**之一的角色特例。
fn language_override(language: &str, capture: &str) -> Option<Role> {
    let role = match capture {
        // `constant` 在不同语言里含义不同：
        // - Python / Go / Java / JS / C# / Lua / Kotlin / SQL / YAML / JSON / C / C++：
        //   查询把布尔字面量（以及 `None` / `nil` / `null`）归进 `constant`，
        //   而 Notepad3 的关键字表包含它们 → 走关键字。
        // - Rust：`@constant` 是 SCREAMING_CASE 标识符、`constant.builtin` 是数字，
        //   统一取数字灰（见调色板注释）。
        // - Ruby：`@constant` 是类名 → 类型色。
        "constant" => match language {
            "rust" => Some(Role::Constant),
            "ruby" => Some(Role::Type),
            "python" | "go" | "java" | "javascript" | "typescript" | "tsx" | "jsx"
            | "csharp" | "lua" | "kotlin" | "sql" | "yaml" | "json" | "c" | "cpp" => {
                Some(Role::Keyword)
            },
            _ => None,
        },
        // Notepad3 里 `enum` 与关键字同表（各语言 lexer 都没单独槽），归关键字。
        "enum" => Some(Role::Keyword),
        // CSS 的 `!important`（Notepad3 `SCE_CSS_IMPORTANT` = 红粗体 `#C80000`）。
        // `languages.rs::fix_css_query` 把 `(important)` 捕成 `@boolean`（CSS 没有
        // 布尔字面量，这个槽空着），这里再把它落成专用角色。
        "boolean" if matches!(language, "css" | "scss") => Some(Role::Important),
        // `hint` 是组件库对"文档注释关键字"这类提示的归类，Notepad3 归注释。
        "hint" => Some(Role::CommentDoc),
        // CSS 的 `color_value`（`#fcfbf9` / `rgb(...)`）在 Notepad3 是 "Value"
        // （`#3A6EA5`），不是字符串绿——它的 "String" 槽只给带引号的字符串。
        "string.special" if matches!(language, "css" | "scss") => Some(Role::Value),
        // CSS/HTML 之外的语言 `label` 多是普通标识符；只有 Lua / Rust / CSS 有独立槽，
        // 那几个在调色板里给了色，未给色的语言回落正文黑即可，这里不强制。
        _ => None,
    };
    role
}

/// 不看语言的通用归类：组件库 41 个捕获名 → 角色。
fn generic_role(capture: &str) -> Role {
    match capture {
        "comment" | "comment.doc" => Role::Comment,
        "keyword" | "boolean" => Role::Keyword,
        "constant" => Role::Constant,
        "number" => Role::Number,
        "string" => Role::String,
        "string.escape" => Role::Escape,
        "string.regex" => Role::Regex,
        "string.special" => Role::String,
        "string.special.symbol" => Role::Symbol,
        "function" => Role::Function,
        "constructor" | "variant" => Role::Class,
        "type" => Role::Type,
        "variable" | "variable.special" => Role::Variable,
        "property" => Role::Property,
        "attribute" => Role::Attribute,
        "operator" | "punctuation" | "punctuation.bracket" | "punctuation.delimiter"
        | "punctuation.special" => Role::Operator,
        "punctuation.list_marker" => Role::ListMarker,
        "tag" => Role::Tag,
        "tag.doctype" => Role::Macro,
        "label" => Role::Label,
        "embedded" => Role::Embedded,
        "title" | "primary" => Role::Title,
        "text.literal" | "text.code.span" => Role::InlineCode,
        "link_text" | "link_uri" => Role::Link,
        "emphasis" => Role::Emphasis,
        "emphasis.strong" => Role::Strong,
        "preproc" | "predictive" => Role::Preproc,
        // 不在 41 个里（不该出现；`recolor_syntax` 只喂这 41 个），兜底正文黑。
        _ => Role::Text,
    }
}

/// 便捷入口：某语言下某捕获名的样式。
pub fn style(language: &str, capture: &str) -> Style {
    palette(language).get(role(language, capture))
}

/// 把某个语言查询里的捕获名改写成组件库认得的名字。
///
/// 组件库的 `SyntaxColors` 只有 [`SUPPORTED_CAPTURES`] 那 41 个名字，且只对**带点**
/// 的名字做前缀回落（`constant.builtin` → `constant`）。**单个单词**且不在表里的
/// 捕获名会静默渲染成黑色——各语言语法 crate 的查询里有一批这种名字
/// （`@escape`、`@delimiter`、`@character`、CSS 的 `@media`、C# 的 `@module`…）。
///
/// 返回 `Some(新名)` 表示需要改写，`None` 表示保持原样。`languages.rs` 按 token
/// 逐个替换（连同 `#match?` 谓词里的引用一起，保证一致）。
///
/// 保留不动的：`injection.*` / `local.*` / `_` 开头的私有捕获 / `spell` /
/// `nospell` / `none` / `error` / 带点且前缀已受支持的名字。
pub fn canonical_capture(name: &str) -> Option<&'static str> {
    let renamed = match name {
        // 转义序列：Notepad3 各语言要么并入字符串、要么有独立色；先落到标准的
        // `string.escape`，再由调色板决定颜色（未设 Escape 的回落字符串色）。
        "escape" => "string.escape",
        // 字符字面量（Zig / Haskell / Kotlin）当字符串。
        "character" | "character.special" => "string",
        // 分隔符（C / PowerShell 的逗号、分号）在 Notepad3 归运算符 → 洋红。
        "delimiter" => "punctuation.delimiter",
        // 模块 / 命名空间归类型（Notepad3 没有单独槽）。
        "module" | "namespace" => "type",
        // CSS 的 at-rule（`@import` / `@media` / `@keyframes`…）与 Zig 的 `@import`：
        // Notepad3 的 CSS lexer 把这些归 "Media / Directive" → 关键字。
        "charset" | "import" | "keyframes" | "media" | "supports" => "keyword",
        "cImport" => "keyword",
        // PowerShell 的数组解引用与赋值。
        "array" => "variable",
        "assignvalue" => "operator",
        // HTML/XML 的非法标签：当普通标签（Notepad3 有红底错误样式，做不到）。
        "tag.error" => "tag",
        // Swift 的 `Regexp` 与 Elixir 的文档注释属性。
        "string.regexp" => "string.regex",
        "comment.doc.__attribute__" => "comment.doc",
        // Markdown 的引用式链接 / 自动链接 → 链接色。
        "text.reference" => "link_text",
        "text.uri" => "link_uri",
        _ => return None,
    };
    // 只接受组件库认得的目标名：写错目标等于没改（还更误导），宁可保持原样。
    SUPPORTED_CAPTURES.contains(&renamed).then_some(renamed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(language: &str, capture: &str) -> (u8, u8, u8) {
        style(language, capture).rgb
    }

    /// 核心诉求：关键字颜色**随语言不同**，不再是 JS 那一套套所有语言。
    #[test]
    fn keyword_color_is_language_specific() {
        assert_eq!(rgb("javascript", "keyword"), (0xA4, 0x60, 0x00));
        assert_eq!(rgb("c", "keyword"), (0x0A, 0x24, 0x6A));
        assert_eq!(rgb("cpp", "keyword"), (0x0A, 0x24, 0x6A));
        assert_eq!(rgb("rust", "keyword"), (0x24, 0x81, 0x12));
        assert_eq!(rgb("python", "keyword"), (0x00, 0x00, 0x7F));
        assert_eq!(rgb("go", "keyword"), (0x0A, 0x24, 0x6A));
        assert_eq!(rgb("csharp", "keyword"), (0x80, 0x40, 0x00));
        assert_eq!(rgb("kotlin", "keyword"), (0x00, 0x00, 0xFF));
        assert_eq!(rgb("sql", "keyword"), (0x3E, 0x3E, 0xFF));
        assert_eq!(rgb("ruby", "keyword"), (0x00, 0x00, 0x7F));
    }

    /// 同一语言里不同槽位也不同：注释、字符串、数字各取自该语言的表。
    #[test]
    fn slots_follow_the_language_table() {
        // Python 注释是 #007F00（不是 JS 的 #646464）。
        assert_eq!(rgb("python", "comment"), (0x00, 0x7F, 0x00));
        assert_eq!(rgb("java", "comment"), (0x64, 0x64, 0x64));
        // Python 数字是橙 #FF4000，JS 是纯红 #FF0000。
        assert_eq!(rgb("python", "number"), (0xFF, 0x40, 0x00));
        assert_eq!(rgb("javascript", "number"), (0xFF, 0x00, 0x00));
        // Rust 注释斜体青灰、字符串砖红。
        assert_eq!(rgb("rust", "comment"), (0x48, 0x80, 0x80));
        assert!(style("rust", "comment").italic);
        assert_eq!(rgb("rust", "string"), (0xB3, 0x1C, 0x1B));
        // 运算符在 C 系里都是洋红；C 的关键字是深蓝，Java 是橙。
        assert_eq!(rgb("c", "operator"), (0xB0, 0x00, 0xB0));
    }

    /// Notepad3 **只给一部分语言**的函数名上色：Python 有，C / JS 没有。
    #[test]
    fn function_coloring_is_language_specific() {
        assert_eq!(rgb("python", "function"), (0x66, 0x00, 0x66));
        assert_eq!(rgb("ruby", "function"), (0x00, 0x7F, 0x7F));
        assert_eq!(rgb("kotlin", "function"), (0xA4, 0x60, 0x00));
        assert_eq!(rgb("powershell", "function"), (0x80, 0x40, 0x00));
        assert_eq!(rgb("c", "function"), (0x00, 0x00, 0x00));
        assert_eq!(rgb("javascript", "function"), (0x00, 0x00, 0x00));
    }

    /// Java 那张表没有 Typedefs/Classes 槽，类名 / 类型名在 Notepad3 里是正文黑；
    /// 之前它错抄了 C 的 `#800000` 斜体。C / C++ / C# 才有该槽。
    #[test]
    fn java_types_stay_black() {
        assert_eq!(rgb("java", "type"), (0x00, 0x00, 0x00));
        assert_eq!(rgb("java", "constructor"), (0x00, 0x00, 0x00));
        // 对照：C 的类型槽是深红斜体，C# 是浅蓝。
        assert_ne!(rgb("c", "type"), (0x00, 0x00, 0x00));
        assert_ne!(rgb("csharp", "type"), (0x00, 0x00, 0x00));
    }

    /// `constant` 的角色随语言变（有的语言里它就是布尔字面量）。
    #[test]
    fn constant_role_is_language_specific() {
        // Python 的 constant 实为 None/True/False → 关键字色。
        assert_eq!(rgb("python", "constant"), (0x00, 0x00, 0x7F));
        // Ruby 的 constant 实为类名 → 类型色（#0000FF）。
        assert_eq!(rgb("ruby", "constant"), (0x00, 0x00, 0xFF));
        // Rust 的 constant 实为数字 → 灰。
        assert_eq!(rgb("rust", "constant"), (0x66, 0x66, 0x66));
        // JSON 的 constant 是 true/false/null → 该语言的关键字色。
        assert_eq!(rgb("json", "constant"), (0x95, 0x70, 0x00));
    }

    /// CSS 的问题是"class/id 与属性名在组件库里同色"：查询改写把 class/id 改捕到
    /// `@constant`（橄榄），属性名留在 `@property`（橙）——两条必须不同。
    #[test]
    fn css_class_and_property_differ() {
        assert_eq!(rgb("css", "constant"), (0x64, 0x80, 0x00)); // Tag-Class / Tag-ID
        assert_eq!(rgb("css", "property"), (0xFF, 0x40, 0x00)); // CSS Property
        // 伪类洋红（Pseudo-Class）。
        assert_eq!(rgb("css", "label"), (0xB0, 0x00, 0xB0));
        // 十六进制颜色归 "Value"（Notepad3 没给颜色单独开槽）。
        assert_eq!(rgb("css", "string.special"), (0x3A, 0x6E, 0xA5));
    }

    /// 批处理（.bat / .cmd）的调色板来自 `styleLexBAT.c`。
    #[test]
    fn batch_follows_notepad3_bat_table() {
        assert_eq!(rgb("batch", "keyword"), (0x0A, 0x24, 0x6A)); // 深蓝粗体
        assert!(style("batch", "keyword").bold);
        assert_eq!(rgb("batch", "comment"), (0x00, 0x80, 0x00));
        assert_eq!(rgb("batch", "label"), (0xC8, 0x00, 0x00));
        assert_eq!(rgb("batch", "function"), (0x00, 0x00, 0x00)); // Command 黑粗体
        assert!(style("batch", "function").bold);
        assert_eq!(rgb("batch", "operator"), (0xB0, 0x00, 0xB0));
        assert_eq!(rgb("batch", "variable"), (0x00, 0x3C, 0xE6)); // Identifier
    }

    /// 回落链：没单独设色的槽跟着"父角色"走，而不是掉成黑。
    #[test]
    fn fallback_chain_keeps_related_slots_colored() {
        // Python 没给转义设色 → 跟字符串绿。
        assert_eq!(rgb("python", "string.escape"), (0x00, 0x88, 0x00));
        // JS 没给 string.special 设色 → 跟字符串绿。
        assert_eq!(rgb("javascript", "string.special"), (0x00, 0x80, 0x00));
        // 各语言没给文档注释设色 → 跟注释色（Python 注释绿）。
        assert_eq!(rgb("python", "comment.doc"), (0x00, 0x7F, 0x00));
    }

    /// 未知语言 / 纯文本全黑，保证"认不出来就老老实实不着色"。
    #[test]
    fn unknown_language_is_plain() {
        for capture in ["keyword", "string", "number", "comment", "function"] {
            assert_eq!(style("text", capture).rgb, (0x00, 0x00, 0x00), "text/{capture}");
            assert_eq!(
                style("dockerfile", capture).rgb,
                (0x00, 0x00, 0x00),
                "dockerfile/{capture}"
            );
        }
    }

    /// 未在组件库 41 个里的单字捕获名要能被改写；带点且前缀受支持的不改。
    #[test]
    fn canonical_capture_renames_only_unsupported_names() {
        assert_eq!(canonical_capture("escape"), Some("string.escape"));
        assert_eq!(canonical_capture("delimiter"), Some("punctuation.delimiter"));
        assert_eq!(canonical_capture("media"), Some("keyword"));
        assert_eq!(canonical_capture("module"), Some("type"));
        assert_eq!(canonical_capture("character"), Some("string"));
        assert_eq!(canonical_capture("text.uri"), Some("link_uri"));
        // 前缀可回落的带点名字保持原样。
        assert_eq!(canonical_capture("constant.builtin"), None);
        assert_eq!(canonical_capture("keyword.conditional"), None);
        assert_eq!(canonical_capture("variable.parameter"), None);
        // 已在表里的名字保持原样。
        for name in ["keyword", "string", "number", "comment", "title", "type"] {
            assert_eq!(canonical_capture(name), None, "{name}");
        }
    }

    /// `role` 只应收到组件库支持的捕获名；每个都该有确定归类（不 panic）。
    #[test]
    fn every_supported_capture_maps_to_a_role() {
        for language in ["c", "cpp", "rust", "python", "javascript", "go", "java", "ruby",
            "kotlin", "sql", "toml", "yaml", "html", "css", "xml", "json", "bash",
            "powershell", "php", "ini", "cmake", "markdown", "elixir", "text"]
        {
            for capture in SUPPORTED_CAPTURES {
                let _ = style(language, capture);
            }
        }
    }
}
