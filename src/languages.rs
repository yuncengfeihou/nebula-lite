//! 组件库覆盖不到的语法：注册进它**公开**的语言注册表。
//!
//! gpui-component 的 `tree-sitter-languages` feature 拿不到这些语言的 highlights，
//! 分两种情况（判据见 `lang.rs::colorable`）：
//!
//! - **注册表里连名字都没有**：XML / PowerShell / INI（INI 在它那张 `Language` 枚举
//!   里压根不存在）、Erlang、Haskell。
//! - **注册了语法，但 highlights 查询写成空串**：CMake / C# / Swift —— 代码就在
//!   `languages.rs` 里写着 `Self::CSharp => (tree_sitter_c_sharp::LANGUAGE, "", "", "")`。
//!   这一类组件库拦不住，编辑器会**每次编辑都白跑一轮 tree-sitter 解析却一个颜色都不出**，
//!   所以这里不只是"补上颜色"，也顺带省掉那轮解析。
//!
//! 这些语法的 crate 全都自带 `HIGHLIGHTS_QUERY`（或者语法本身就能解析，见下），
//! 走组件库公开的 `LanguageRegistry::register` 注进去即可——不改组件库源码，
//! 也不动编辑器控件。注册是**覆盖**：同名 insert 会把组件库那条空查询顶掉。
//!
//! 注册名必须与 `lang.rs::EXTENSIONS` 给出的语言 id 逐字一致，否则注册了也命不中
//! （注册表查找是 `languages.get(name)`，对不上就回落内置别名、再回落 `text`）。
//!
//! 查询里的带点捕获名（`keyword.conditional`、`function.call`…）不需要我们补映射：
//! 组件库 `SyntaxColors::style` 对带点的名字有前缀回落（`keyword.modifier` -> `keyword`）。
//!
//! ## 时机是结构保证的，不靠调用方自觉
//!
//! 注册必须早于编辑器解析语言名。这一点由 [`ensure_registered`] 保证：它被
//! `lang::language_for_path` 调用，而那里是**语言 id 的唯一产地**（全仓库只有
//! `app.rs` 一处调用，产出什么名字就交给编辑器什么名字）。因此任何可能产出这些
//! 语言名的代码路径，都必然先跑过注册——不依赖某人在 `main` 里记得加一行。
//!
//! ## 为什么这里自己引 `tree-sitter`
//!
//! 这些语法 crate 里，有的对 `tree-sitter` 只声明 **dev** 依赖、正常依赖是
//! `tree-sitter-language`（只提供 `LanguageFn`）；有的（c-sharp / swift / cmake /
//! javascript）本来就是组件库的传递依赖。要拿到 `LanguageConfig` 需要的
//! `tree_sitter::Language`，必须把 `LanguageFn` 过一遍 `tree_sitter::Language::new`，
//! 于是本 crate 得自己引 `tree-sitter`，且版本必须与 gpui-component 的 `tree-sitter = "0.26"`
//! 归一——否则 cargo 会编出两份 tree-sitter，两边 `Language` 不是同一个类型。
//!
//! 这条约束**实打实地淘汰了两个语言**：`tree-sitter-dockerfile` 把
//! `tree-sitter = "^0.20"` 声明为正常依赖，`tree-sitter-vim` 声明 `>=0.21.0`，
//! 引入它们会让 Cargo.lock 里出现 tree-sitter 0.20 与 0.26 两份（实测过），
//! 而 0.20 的 `Language` 传不进 0.26 的 `LanguageConfig`。等上游跟上再补。

use std::sync::Once;

use gpui_component::highlighter::{LanguageConfig, LanguageRegistry};
use tree_sitter::Language;

/// 保证所有自带语言已注册，且只注册一次。
///
/// 幂等且廉价（`Once` 在热路径上就是一次原子读），由 `lang::language_for_path` 调用。
pub fn ensure_registered() {
    static ONCE: Once = Once::new();
    ONCE.call_once(register);
}

/// 批处理只保留注释的语法着色；其余全交给 `np3`（见 `entries()` 里 batch 条目
/// 的长注释：关键字被语法 inline 掉、查询捕不到，且重叠的胜负不稳定）。
const BATCH_HIGHLIGHTS: &str = "; 批处理：语法层只留注释，其余由 np3 逐词判色。\n(comment) @comment\n";

/// 一个语言的注册项：注册名、语法、highlights 查询。
type Entry = (&'static str, Language, &'static str);

/// 待注册的语言表（不含 JSX，它要拼两段查询、见 `register`）。
///
/// 测试直接遍历这张表，所以"注册了哪些"与"断言了哪些"不会分家。
fn entries() -> Vec<Entry> {
    vec![
        // —— 组件库注册表里没有名字的那几个 ——
        (
            "xml",
            Language::new(tree_sitter_xml::LANGUAGE_XML),
            tree_sitter_xml::XML_HIGHLIGHT_QUERY,
        ),
        // DTD（`.dtd`）：tree-sitter-xml 另带一份 DTD 语法与查询。Notepad3 把
        // `.dtd` 归 HTML lexer 管——都是标记文档，这里用真正的 DTD 语法更准。
        (
            "dtd",
            Language::new(tree_sitter_xml::LANGUAGE_DTD),
            tree_sitter_xml::DTD_HIGHLIGHT_QUERY,
        ),
        (
            "powershell",
            Language::new(tree_sitter_powershell::LANGUAGE),
            tree_sitter_powershell::HIGHLIGHTS_QUERY,
        ),
        (
            "ini",
            Language::new(tree_sitter_ini::LANGUAGE),
            tree_sitter_ini::HIGHLIGHTS_QUERY,
        ),
        (
            "erlang",
            Language::new(tree_sitter_erlang::LANGUAGE),
            tree_sitter_erlang::HIGHLIGHTS_QUERY,
        ),
        (
            "haskell",
            Language::new(tree_sitter_haskell::LANGUAGE),
            tree_sitter_haskell::HIGHLIGHTS_QUERY,
        ),
        // Windows 批处理（.bat / .cmd）；Notepad3 的 `styleLexBAT.c` 管这两个扩展名。
        //
        // **故意用一份最小查询**，而不是 crate 自带的 `HIGHLIGHTS_QUERY`：批处理的
        // 颜色由 `np3::spans("batch", …)` 逐词判（照着 `styleLexBAT.c` 的关键字表与
        // 分隔符集合），因为 `tree-sitter-batch` 把 `if` / `call` / `goto` 这些关键字
        // **inline 掉了**——`to_sexp` 里看不到这些词，任何查询都捕不到；它只能整条语句地
        // 捕（`(if_stmt) @keyword`），于是整行（含操作数与标签）都被涂成关键字蓝，
        // 与 Notepad3 差得很远。
        //
        // 更关键的是**重叠**：组件库合成样式时把语法样式与装饰丢进 `FxHashSet` 折成
        // 一条，重叠处谁赢取决于哈希迭代序（实测不稳定）。所以这里让语法层**只留
        // 注释**（一个真实的叶节点，且与 `np3` 给的绿完全一致，重叠也无害），其余
        // 全交给 `np3`，从根本上避免重叠。查询非空也让 `lang::colorable` 放行。
        (
            "batch",
            Language::new(tree_sitter_batch::LANGUAGE),
            BATCH_HIGHLIGHTS,
        ),
        // —— 组件库注册了语法、但查询是空串的那几个（这里把它们顶掉）——
        (
            "cmake",
            Language::new(tree_sitter_cmake::LANGUAGE),
            tree_sitter_cmake::HIGHLIGHTS_QUERY,
        ),
        (
            "csharp",
            Language::new(tree_sitter_c_sharp::LANGUAGE),
            tree_sitter_c_sharp::HIGHLIGHTS_QUERY,
        ),
        (
            "swift",
            Language::new(tree_sitter_swift::LANGUAGE),
            tree_sitter_swift::HIGHLIGHTS_QUERY,
        ),
    ]
}

/// 实际的注册动作。重复调用安全（注册表内部是 `HashMap`，同名覆盖），但正常只走一次。
fn register() {
    let registry = LanguageRegistry::singleton();

    for (name, language, highlights) in entries() {
        registry.register(name, &config(name, language, highlights));
    }

    register_jsx(registry);
    register_tsx(registry);
    register_c_family(registry);
    register_fixed_python(registry);
    register_fixed_kotlin(registry);
    register_fixed_css(registry);
    register_fixed_java(registry);
    register_fixed_csharp(registry);
    register_fixed_powershell(registry);
    // 归一化**所有**已注册语言（含组件库自带那批）的捕获名，再把 Markdown 的
    // 注入查询修掉——顺序上先归一化、后改注入，免得把刚写好的注入又洗一遍。
    normalize_registered_queries(registry);
    register_markdown_without_combined_inline(registry);
    register_markdown_link_brackets(registry);
    register_markdown_heading_ownership(registry);
}

/// 把 ATX 标题的着色从 Markdown **语法层**摘掉，交给 `np3` 全权负责。
///
/// 起因是**重叠的胜负不确定**：组件库把语法样式与装饰丢进 `FxHashSet` 折成一条，
/// 重叠处谁赢取决于哈希迭代序。ATX 标题在两边都有——语法给 `@title`（正文）与
/// `@punctuation.special`（`#` 标记），`np3` 给整行的级别前景 + 条带底色。实测
/// 结果是"看运气"：H1 的 `#` 与正文都没拿到 `np3` 的级别色。
///
/// 所以把这两处语法捕获摘掉：
/// - `(atx_heading (inline) @title)` 一行删除（**保留** `(setext_heading …) @title`，
///   Setext 标题 `np3` 不处理，仍需语法给色）；
/// - 标记者那条列表里的六个 `(atx_hN_marker)` 删掉（同一列表里的
///   `setext_hN_underline` 保留）。
///
/// 这样 ATX 标题在语法层没有颜色，`np3` 的前景/底色都稳定生效，且与 Notepad3 的
/// `SCE_MARKDOWN_HEADERn`（整行套级别样式）一致。
fn register_markdown_heading_ownership(registry: &LanguageRegistry) {
    let Some(mut markdown) = registry.language("markdown") else {
        return;
    };
    let original = markdown.highlights.to_string();
    let fixed = fix_markdown_heading_query(&original);
    if fixed != original {
        markdown.highlights = fixed.into();
        registry.register("markdown", &markdown);
    }
}

/// 对 Markdown 查询做上面那两处删除（纯函数，便于单测）。
fn fix_markdown_heading_query(source: &str) -> String {
    let mut out = source.replace("(atx_heading (inline) @title)\n", "");
    for n in 1..=6 {
        out = out.replace(&format!("  (atx_h{n}_marker)\n"), "");
    }
    out
}

/// 改写组件库（实为 tree-sitter-css）的 CSS 查询，让 `.class` / `#id` 与属性名分开。
///
/// tree-sitter-css 的查询把 `class_name` / `id_name` / `namespace_name` **和**
/// `property_name` 一起捕获成 `@property`——渲染出来是同一个颜色。Notepad3 的
/// `styleLexCSS.c` 里这是两个槽：`.class` / `#id` 是 "Tag-Class" / "Tag-ID"
/// （橄榄 `#648000`），属性名才是 "CSS Property"（橙 `#FF4000`）。所以这里把
/// class/id 改捕到 `@constant`（在 CSS 调色板里取橄榄），伪类 / 伪元素改捕到
/// `@label`（洋红），`unit` 改捕到 `@number`（Notepad3 的 "Value" 色）。
///
/// 顺带：`(class_name) @property` 之后的模式顺序很重要——组件库**保留先命中的**
/// 捕获名，而伪类 / 伪元素那两条本来就排在前面，所以伪类里的 `class_name` 不会被
/// 后面的 `@constant` 抢走。
fn register_fixed_css(registry: &LanguageRegistry) {
    let Some(mut css) = registry.language("css") else {
        return;
    };
    let original = css.highlights.to_string();
    let fixed = fix_css_query(&original);
    if fixed != original {
        css.highlights = fixed.into();
        registry.register("css", &css);
    }
}

/// 对 CSS 查询做上面那几处捕获名改写（纯函数，便于单测）。
///
/// 除了把 class/id 与属性名分开，还补上 Notepad3 明确着色、而 tree-sitter-css
/// 查询**没捕**的几类 token：
///
/// - `{` `}` `;`：Notepad3 的 `SCE_CSS_OPERATOR`（`styleLexCSS.c`：`fore:#B000B0`）
///   按 `IsCssOperator` 着色。只补这三个：`(` `)` 在 Notepad3 里于 VALUE 状态下
///   **不**转 operator（`url(…)` 的括号保持值色），`[` `]` 走属性选择器的斜体槽，
///   都不该染成洋红。
/// - `!important`：`SCE_CSS_IMPORTANT` = `bold; fore:#C80000`。原查询把它捕成
///   `@keyword`（深蓝 `#0A246A`），而组件库**保留先命中的**捕获名，所以不能靠
///   末尾追加——必须把那条**就地**改成 `@boolean`，再由 `syntax::language_override`
///   落成 `Role::Important`。
/// - `color_value`（`#RRGGBB` 等）与 `plain_value`（`center` / `red` / `auto` 这些
///   裸值词）：Notepad3 里它们都在 "Value" 槽（`fore:#3A6EA5` 蓝）。原查询把它们
///   漏成正文黑，用户报的"`center center` 没高亮"就是 `plain_value` 这条。
///   `@string.special` 在 CSS 调色板里正好回落到 `Role::Value`。
///
/// 顺序上这几条追加在末尾：组件库保留**先命中**的捕获名，所以 `[href=...]` 里的
/// `plain_value`（早已被上面的 `@string` 捕住）不会被新加的 `@string.special` 抢走。
/// 十六进制颜色同理：`color_value` 的 `#` 会被更早那条 `"#" @punctuation.delimiter`
/// 拿走，但那只是一格分隔符，`@string.special` 覆盖的是后面的十六进制数字。
fn fix_css_query(source: &str) -> String {
    let mut out = source.to_owned();
    for (from, to) in [
        ("(class_name) @property", "(class_name) @constant"),
        ("(id_name) @property", "(id_name) @constant"),
        ("(namespace_name) @property", "(namespace_name) @constant"),
        (
            "(pseudo_element_selector (tag_name) @attribute)",
            "(pseudo_element_selector (tag_name) @label)",
        ),
        (
            "(pseudo_class_selector (class_name) @attribute)",
            "(pseudo_class_selector (class_name) @label)",
        ),
        ("(unit) @type", "(unit) @number"),
        // 就地改写：组件库保留先命中的捕获名，追加改不动这条。
        ("(important) @keyword", "(important) @boolean"),
    ] {
        out = out.replace(from, to);
    }
    // `{` `}` `;` → 运算符（Notepad3 的 SCE_CSS_OPERATOR）。
    out.push_str(
        "\n\"{\" @operator\n\"}\" @operator\n\";\" @operator\n\
         ; 十六进制颜色与裸值词 → Notepad3 的 \"Value\" 槽（蓝）。\n\
         (color_value) @string.special\n\
         (plain_value) @string.special\n",
    );
    out
}

/// C / C++ 的语言查询修正，两件事：
///
/// 1. **C++ 要拼上 C 的基础查询**。tree-sitter-cpp 自带的 `HIGHLIGHT_QUERY` 只补
///    C++ 专有构造（模板、`namespace`、`co_await`…），**不含** C 的基础槽——单用它时
///    `.cpp` 里的 `if` / `for` / `int` / 运算符 / 数字 / 字符串几乎全无颜色（实测）。
///    C++ 语法是 C 的超集，两份查询编进同一份即可，这也是 nvim-treesitter 的既有做法。
/// 2. **预处理指令改捕到 `@preproc`**。Notepad3 的 "Preprocessor" 是真橙 `#FF8000`
///    （`styleLexCPP.c`），tree-sitter 却把 `#include` / `#define` 归 `@keyword`（深蓝）。
///    就地改写（组件库保留先命中的捕获名，追加改不动）。
fn register_c_family(registry: &LanguageRegistry) {
    // C：只改预处理指令。
    if let Some(mut c) = registry.language("c") {
        let original = c.highlights.to_string();
        let fixed = fix_preproc_query(&original);
        if fixed != original {
            c.highlights = fixed.into();
            registry.register("c", &c);
        }
    }
    // C++：C 基础查询 + C++ 查询，再改预处理。
    if let Some(cpp_language) = registry.language("cpp").and_then(|config| config.language) {
        let combined = fix_preproc_query(&format!(
            "{}\n{}",
            tree_sitter_c::HIGHLIGHT_QUERY,
            tree_sitter_cpp::HIGHLIGHT_QUERY
        ));
        registry.register("cpp", &config("cpp", cpp_language, &combined));
    }
}

/// 把 C / C++ 查询里的预处理指令捕获改到 `@preproc`，并把内建类型改到 `@keyword`。
///
/// **为什么内建类型也要改**：Notepad3 的 `styleLexCPP.c` 把 `int` / `void` / `char` /
/// `unsigned` / `bool` / `size_t` 这些**放在关键字表里**（`KeyWords_CPP`），涂关键字
/// 的深蓝粗体 `#0A246A`；而 tree-sitter-c 把 `(primitive_type)` / `(sized_type_specifier)`
/// 捕成 `@type`，落进 `Role::Type`（深红斜体 `#800000`）——"`int` 是红的"正是用户看到
/// 的偏差。就地改写这两条，`type_identifier`（用户 typedef / 类名）仍留在 `@type`。
fn fix_preproc_query(source: &str) -> String {
    let mut out = source.replace("(preproc_directive) @keyword", "(preproc_directive) @preproc");
    for directive in [
        "#define", "#elif", "#else", "#endif", "#if", "#ifdef", "#ifndef", "#include",
    ] {
        out = out.replace(&format!("\"{directive}\" @keyword"), &format!("\"{directive}\" @preproc"));
    }
    // 宏定义的函数名（`#define F(x)` 的 `F`）也归 Preprocessor。
    out = out.replace("@function.special", "@preproc");
    // 内建类型 → 关键字（Notepad3 把它们编在关键字表里）。
    out = out.replace("(primitive_type) @type", "(primitive_type) @keyword");
    out = out.replace("(sized_type_specifier) @type", "(sized_type_specifier) @keyword");
    out
}

/// 把 Python 的装饰器改捕到 `@preproc`。
///
/// Notepad3 的 Python 表给 "Decorator" 单独一槽（`fore:#F2B600` 金），而
/// tree-sitter-python 的查询把 `@decorator` 整个捕成 `@function`——那是 "Function
/// Name" 的紫 `#660066`，与 Notepad3 差得很远。就地改写那两条（组件库保留先命中的
/// 捕获名，追加改不动）。
fn register_fixed_python(registry: &LanguageRegistry) {
    let Some(mut python) = registry.language("python") else {
        return;
    };
    let original = python.highlights.to_string();
    let mut fixed = original
        .replace("(decorator) @function", "(decorator) @preproc")
        .replace("(decorator\n  (identifier) @function)", "(decorator\n  (identifier) @preproc)");
    // `@function.special` 之类若存在也一并归一（当前 Python 查询没有，留作护栏）。
    fixed = fixed.replace("@function.special", "@preproc");
    if fixed != original {
        python.highlights = fixed.into();
        registry.register("python", &python);
    }
}

/// 把 Java 的内建类型（`int` / `void` / `boolean` / `char`…）改捕到 `@keyword`。
///
/// Notepad3 的 `styleLexJAVA.c` 把 `int` / `void` / `boolean` / `char` / `float` /
/// `double` / `long` / `short` / `byte` 全编在**关键字表**里（涂橙粗体 `#A46000`；
/// Java 表没有 Global Class 槽，`String` 这类类名才是正文黑）。tree-sitter-java 却把
/// `[ (boolean_type) (integral_type) (floating_point_type) (void_type) ]` 捕成
/// `@type.builtin`，落进 `Role::Type`——Java 调色板没给 `Type` 设色，于是这些词
/// **整片黑色**。Java 的 `@type.builtin` 只有这一个块，整块改捕即可。
fn register_fixed_java(registry: &LanguageRegistry) {
    let Some(mut java) = registry.language("java") else {
        return;
    };
    let original = java.highlights.to_string();
    let fixed = original.replace("@type.builtin", "@keyword");
    if fixed != original {
        java.highlights = fixed.into();
        registry.register("java", &java);
    }
}

/// 把 C# 的预定义类型（`int` / `string` / `bool` / `void`…）改捕到 `@keyword`。
///
/// 与 Java 同理：Notepad3 的 `styleLexCS.c` 把这些词编在关键字表（`bold; fore:#804000`），
/// 而 tree-sitter-c-sharp 的 `(predefined_type) @type.builtin` 会落进 C# 的
/// Global Class 槽（浅蓝 `#2B91AF`）。
fn register_fixed_csharp(registry: &LanguageRegistry) {
    let Some(mut csharp) = registry.language("csharp") else {
        return;
    };
    let original = csharp.highlights.to_string();
    let fixed = original.replace("(predefined_type) @type.builtin", "(predefined_type) @keyword");
    if fixed != original {
        csharp.highlights = fixed.into();
        registry.register("csharp", &csharp);
    }
}

/// 修掉 PowerShell 查询里**过量捕获**的赋值右值。
///
/// tree-sitter-powershell 的 `(assignment_expression value: (pipeline) @assignvalue)`
/// 把 `$x = "Hi"` 里**整条右值**（字符串、命令调用、任意表达式）都捕成 `assignvalue`，
/// 而归一化把 `assignvalue` 落成 `@operator`。PowerShell 的 Operator 是"粗体、不变色"
/// （`styleLexPS.c`：`Operator` 只有 `bold`），于是**赋值语句的右值整片变成黑色粗体**
/// ——最常见的一类 PowerShell 代码（`$x = ...`）直接失去了字符串/数字/命令的颜色。
/// 这一条删掉：赋值号 `=` 本就不是 Notepad3 的着色重点，删掉比误伤整条右值好。
fn register_fixed_powershell(registry: &LanguageRegistry) {
    let Some(mut powershell) = registry.language("powershell") else {
        return;
    };
    let original = powershell.highlights.to_string();
    let fixed =
        original.replace("(assignment_expression\n  value: (pipeline) @assignvalue)", "");
    if fixed != original {
        powershell.highlights = fixed.into();
        registry.register("powershell", &powershell);
    }
}

/// 修掉组件库自带 Kotlin 查询里的**无效字面量**（这条会让整个查询编译失败）。
///
/// 组件库 `highlighter/languages/kotlin/highlights.scm` 是上游
/// `tree-sitter-kotlin-sg` 的改写版，但在拼接字面量时引用了**该语法里不存在**的
/// 令牌：`"!is"` / `"!in"`（语法只有 `"is"` / `"in"` 与单独的 `"!"`）、
/// `"$"` / `"${"`（语法用的是具名节点 `interpolation_identifier_start` /
/// `interpolation_expression_start`）。`Query::new` 遇到未知字面量会直接报错，
/// 组件库随即 `build_inert`——**整个 Kotlin 文件一个颜色都没有**，且不报错。
///
/// 这里按上游的写法把那两段插值模式换掉、删掉两个无效运算符字面量，其余逐字保留
/// （组件库这份把"具体捕获排在 `@variable` 兜底之前"，不能整段换成上游，否则
/// 每条 `simple_identifier` 都会先命中 `@variable`）。
fn register_fixed_kotlin(registry: &LanguageRegistry) {
    let Some(mut kotlin) = registry.language("kotlin") else {
        return;
    };
    let original = kotlin.highlights.to_string();
    let fixed = fix_kotlin_query(&original);
    if fixed != original {
        kotlin.highlights = fixed.into();
        registry.register("kotlin", &kotlin);
    }
}

/// 对组件库的 Kotlin 查询做上面那两处修正（纯函数，便于单测）。
fn fix_kotlin_query(source: &str) -> String {
    // 无效的插值模式 → 上游用具名节点的写法。
    const BROKEN_STRING: &str = "(string_literal\n\t\"$\" @punctuation.special\n\t(interpolated_identifier) @variable)";
    const BROKEN_TEMPLATE: &str = "(string_literal\n\t\"${\" @punctuation.special\n\t(interpolated_expression)\n\t\"}\" @punctuation.special)";
    const FIXED_STRING: &str = "(string_literal\n\t(interpolation_identifier_start) @punctuation.special\n\t(interpolated_identifier))";
    const FIXED_TEMPLATE: &str = "(string_literal\n\t(interpolation_expression_start) @punctuation.special\n\t(interpolated_expression)\n\t(interpolation_expression_end) @punctuation.special)";

    let mut out = source
        .replace(BROKEN_STRING, FIXED_STRING)
        .replace(BROKEN_TEMPLATE, FIXED_TEMPLATE);
    // 删掉两个语法里不存在的运算符字面量（保留其所在列表的其余项）。
    out = out.replace("\t\"!is\"\n", "").replace("\t\"!in\"\n", "");
    out
}

/// 把各语言查询里的非标准捕获名改写成组件库认得的名字。
///
/// 组件库的 `SyntaxColors` 只认 [`crate::syntax::SUPPORTED_CAPTURES`] 那 41 个名字，
/// 且只对**带点**的名字做前缀回落。各语法 crate 的查询里有一批**单字**捕获名不在
/// 表里（`@escape`、`@delimiter`、`@character`、CSS 的 `@media`、C# 的 `@module`…），
/// 组件库遇到它们会返回 `None` → 静默渲染成黑色。这里按 token 改写：
///
/// - 只改 `highlights`，不动 `injections` / `locals`——注入查询里的
///   `@injection.content` 等名字有独立含义，改写会破坏注入。
/// - **连同谓词里的引用一起改**（`#match? @constant …` 与 `@constant` 是同一个
///   token 文本），所以按整段文本做 token 替换是一致的。
/// - 逐个 `@name` token 处理，不做子串替换（`@type` 与 `@type.builtin` 是两个
///   token，互不影响）。
fn normalize_registered_queries(registry: &LanguageRegistry) {
    for name in registry.languages() {
        let Some(mut config) = registry.language(&name) else {
            continue;
        };
        if config.highlights.is_empty() {
            continue;
        }
        let normalized = normalize_query(&config.highlights);
        if normalized != config.highlights.as_ref() {
            config.highlights = normalized.into();
            registry.register(&name, &config);
        }
    }
}

/// 对一段 tree-sitter 查询做捕获名归一化（纯函数，便于单测）。
///
/// 扫描 `@` 后跟 `[A-Za-z0-9_.]` 的 token，经 [`crate::syntax::canonical_capture`]
/// 决定是否改名。`@` 前必须是行首、空白或 `(`，避免把字符串里的 `@` 当捕获名。
/// 名字本身是 ASCII，但源码里可能有非 ASCII 字面量，所以按 `char` 迭代、按字节
/// 切片取名，绝不把字节当 `char` 推回（那会把 UTF-8 拆坏）。
pub(crate) fn normalize_query(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut chars = source.char_indices().peekable();
    let mut prev: Option<char> = None;
    while let Some((i, c)) = chars.next() {
        let at_capture = c == '@' && prev.map(|p| p.is_whitespace() || p == '(').unwrap_or(true);
        if at_capture {
            out.push('@');
            let start = i + 1;
            let mut end = start;
            while let Some(&(j, nc)) = chars.peek() {
                if nc.is_ascii_alphanumeric() || nc == '_' || nc == '.' {
                    end = j + nc.len_utf8();
                    chars.next();
                } else {
                    break;
                }
            }
            let name = &source[start..end];
            out.push_str(crate::syntax::canonical_capture(name).unwrap_or(name));
            prev = source[start..end].chars().last().or(Some('@'));
        } else {
            out.push(c);
            prev = Some(c);
        }
    }
    out
}

/// 修掉组件库给 Markdown 加上的 `injection.combined`。
///
/// 组件库的 `highlighter/languages/markdown/injections.scm` 是上游
/// `tree_sitter_md::INJECTION_QUERY_BLOCK` 的**改写版**，唯一实质差别是它给行内注入
/// 加了一句 `(#set! injection.combined)`。那句话让组件库把**整篇文档**的所有 `inline`
/// 节点当成一棵树来解析（`compute_injection_layers` 的 combined 分支），于是解析耗时
/// 随文档大小增长，撞上它自己那条 `INJECTION_PARSE_TIMEOUT = 20ms`：
/// `parse_injection_layer` 一旦超时就 `return None`，**整个行内层被丢弃**。
/// 结果是文档里所有粗体 / 斜体 / 行内代码都以 `**` 和反引号原样显示。
///
/// 实测（release）：47 KB 的 AGENTS.md，192 个 inline 节点、39 KB 行内内容，
/// 一次合并解析要 **35–55 ms**，稳定超时；13 KB 的文件只要 9–12 ms，所以小文件正常——
/// 这正是"同一个编辑器里，大 Markdown 没格式、小 Markdown 有格式"的来源。
///
/// 这里换回上游那份**不带** `combined` 的查询：组件库改走逐个 `inline` 节点注入的
/// 分支，每个节点单独解析（微秒级），单次解析不可能超时；而且每个节点独立成层，
/// 也不存在组件库注释里担心的"跨条目反引号被合并"问题。
///
/// 只换 `injections`，其余字段沿用组件库那份——它的 highlights 已被改名成组件库自己的
/// 捕获名（`@title` 而不是上游的 `@text.title`），照抄上游会让标题丢色。
fn register_markdown_without_combined_inline(registry: &LanguageRegistry) {
    let Some(mut markdown) = registry.language("markdown") else {
        return;
    };
    let mut injections = String::from(tree_sitter_md::INJECTION_QUERY_BLOCK);
    // 再把 GFM 表格单元格也交给 markdown_inline。
    //
    // tree-sitter-md 的**块**语法把表格单元格内容当成纯文本：`pipe_table_cell` 的
    // 子节点里没有 `inline`，而上游注入查询只认 `(inline)`。于是表格里的 `**粗体**`
    // 与反引号行内代码一直原样显示——AGENTS.md 大半是表格，所以它看起来尤其明显
    // （正文里的行内代码是有色的，只有表格没有）。
    //
    // `pipe_table_cell` 两端不含 `|`（分隔符是独立节点），所以整段单元格文本可以
    // 直接当行内 Markdown 喂给 markdown_inline。没有触发字符的单元格会被组件库的
    // `should_include_injection_range` 跳过，不会白解析。
    injections.push_str(
        "\n((pipe_table_cell) @injection.content\n  (#set! injection.language \"markdown_inline\"))\n",
    );
    markdown.injections = injections.into();
    registry.register("markdown", &markdown);
}

/// 给 Markdown 行内层补上链接 / 图片的**括号标记**着色。
///
/// Notepad3 的 `SCE_MARKDOWN_LINK` 从 `[` 或 `![` 起一直染到 `)`（`LexMarkdown.cxx`），
/// 所以 `[`、`]`、`(`、`)`、`!` 这些标记本身也是链接蓝 `#0000FF`；而组件库那份
/// `markdown_inline` 查询只捕 `link_text`（内容）与 `link_uri`（地址），括号本身
/// 没人管，渲染成正文黑——用户说的"`[]` 字符本身也没有高亮"就是这个。
///
/// 这里把括号改捕到 `@link_uri`（在 Markdown 调色板里就是链接蓝）。`_link_text` /
/// `_image_description` 是隐藏规则，其 `[` `]` 直接内联成 link 节点的匿名子节点，
/// 所以用 `(inline_link "[")` 这样的兄弟模式捕得到。只改 `highlights`，`injections`
/// 保持组件库那份不动（改注入会破坏行内解析）。
fn register_markdown_link_brackets(registry: &LanguageRegistry) {
    let Some(mut inline) = registry.language("markdown_inline") else {
        return;
    };
    let mut query = inline.highlights.to_string();
    query.push_str(
        "\n; 链接 / 图片的括号标记（Notepad3 把它们也算进 Link 槽）\n\
         [\n  (inline_link)\n  (image)\n  (full_reference_link)\n  (collapsed_reference_link)\n  (shortcut_link)\n] \"[\" @link_uri\n\
         [\n  (inline_link)\n  (image)\n  (full_reference_link)\n  (collapsed_reference_link)\n  (shortcut_link)\n] \"]\" @link_uri\n\
         [\n  (inline_link)\n  (image)\n] \"(\" @link_uri\n\
         [\n  (inline_link)\n  (image)\n] \")\" @link_uri\n\
         (image \"!\" @link_uri)\n",
    );
    inline.highlights = query.into();
    registry.register("markdown_inline", &inline);
}

/// JSX 单独处理：tree-sitter 的 JavaScript 语法**本身就能解析 JSX**（grammar 里
/// 有 `jsx_element` / `jsx_text` 等节点），所以不需要另一个语法；但一个 .jsx
/// 文件里也有大量普通 JS 构造，只用它那份 JSX 追加查询（只有 tag / attribute /
/// punctuation.bracket）会让其余部分全无颜色。所以把基础 JS 查询与 JSX 追加查询
/// 拼起来用——两段出自同一个 crate，节点类型一致，能编进同一个查询。
fn register_jsx(registry: &LanguageRegistry) {
    let jsx_query = format!(
        "{}\n{}",
        tree_sitter_javascript::HIGHLIGHT_QUERY,
        tree_sitter_javascript::JSX_HIGHLIGHT_QUERY
    );
    registry.register(
        "jsx",
        &config(
            "jsx",
            Language::new(tree_sitter_javascript::LANGUAGE),
            &jsx_query,
        ),
    );
}

/// TSX 与 JSX 同因：TSX 语法本身能解析 JSX，但需要 TS 的**完整**高亮查询。
///
/// 组件库给 `tsx` 用的是 `tree_sitter_typescript::HIGHLIGHTS_QUERY`——那只是 TS
/// 的**追加**查询（type / keyword 那几条），**不含** JS 基础槽，于是 `.tsx` 里
/// `const` / `function` / 字符串 / 数字几乎全无颜色（实测整片黑）。组件库自己给
/// `typescript` 用的是那份**完整**的 `languages/typescript/highlights.scm`，但它在
/// 组件库源码里、没通过 crate 导出，本仓库拿不到。可用的替代是把 **JS 基础查询 +
/// TS 追加查询 + JSX 追加查询**拼起来：三份出自 tree-sitter 官方 crate，节点类型
/// 相容（TSX 是 JS/TS 的超集），能编进同一个查询——JS 基础槽因此补全，TS 专有
/// 构造与 JSX 也都着色。这与 `register_jsx`（JS + JSX）是同一个套路。
fn register_tsx(registry: &LanguageRegistry) {
    let tsx_query = format!(
        "{}\n{}\n{}",
        tree_sitter_javascript::HIGHLIGHT_QUERY,
        tree_sitter_typescript::HIGHLIGHTS_QUERY,
        tree_sitter_javascript::JSX_HIGHLIGHT_QUERY
    );
    registry.register(
        "tsx",
        &config(
            "tsx",
            Language::new(tree_sitter_typescript::LANGUAGE_TSX),
            &tsx_query,
        ),
    );
}

/// 只有一个 highlights 查询的普通语言（无注入、无 locals）。
fn config(name: &'static str, language: Language, highlights: &str) -> LanguageConfig {
    LanguageConfig::new(name, language, Vec::new(), highlights, "", "")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    /// 每条测试都**只走生产路径**（`lang::language_for_path`）来触发注册，不直接调用
    /// `ensure_registered`。原因：注册表是进程级全局，cargo 又让同一 binary 的测试共享
    /// 一个进程，所以只要有任何一条测试直接注册过，别的测试就会在"注册表已被填好"的
    /// 状态下通过——那样把 `ensure_registered` 从 `language_for_path` 上摘掉也不会有
    /// 测试变红。全部走生产路径之后，那条调用一旦被删，最先跑到的那条测试就会失败。
    fn resolve(path: &str) -> &'static str {
        crate::lang::language_for_path(Path::new(path))
    }

    /// 本仓库注册的每个语言都必须被注册表收下，且带语法与 highlights 查询。
    /// 只看 `colorable`（也就是 `lang.rs` 的判据）还不够——它只查 highlights 非空；
    /// 这里把语法也一并断言，免得"查询抄对了、语法忘了给"这种半拉子状态蒙混过关。
    #[test]
    fn every_registered_language_has_grammar_and_query() {
        assert_eq!(resolve("a.xml"), "xml"); // 走生产路径把注册带起来
        for (name, _, _) in entries() {
            let config = LanguageRegistry::singleton()
                .language(name)
                .unwrap_or_else(|| panic!("{name} 没被注册进注册表"));
            assert!(config.has_grammar(), "{name} 注册了但没有语法");
            assert!(
                !config.highlights.is_empty(),
                "{name} 注册了但没有 highlights 查询，拿不到任何颜色"
            );
        }
        // jsx 不在 entries() 里（查询是拼出来的），单独查一遍。
        let jsx = LanguageRegistry::singleton()
            .language("jsx")
            .expect("jsx 没被注册进注册表");
        assert!(jsx.has_grammar(), "jsx 注册了但没有语法");
        assert!(!jsx.highlights.is_empty(), "jsx 没有 highlights 查询");
    }

    /// 扩展名 -> 该解析成的语言，**且**那次解析必须真的拿到颜色。
    ///
    /// `resolve` 的返回值由 `colorable` 过滤过：若注册名与意图表对不上、或注册没被
    /// 触发，这里就会退化成 `"text"` 而失败。这是"注册真的接上了"的端到端证据。
    #[test]
    fn every_extension_resolves_to_a_highlighted_language() {
        for (path, expected) in [
            ("a.xml", "xml"),
            ("a.ps1", "powershell"),
            ("a.psm1", "powershell"),
            ("a.psd1", "powershell"),
            ("a.ini", "ini"),
            ("a.cfg", "ini"),
            ("a.conf", "ini"),
            ("a.erl", "erlang"),
            ("a.hs", "haskell"),
            ("a.cmake", "cmake"),
            ("a.cs", "csharp"),
            ("a.swift", "swift"),
            ("a.jsx", "jsx"),
            ("a.bat", "batch"),
            ("a.cmd", "batch"),
            // XHTML / XML 家族的扩展名对齐（用户报的"XHTML 没高亮"）。
            ("a.xhtml", "html"),
            ("a.shtml", "html"),
            ("a.svg", "xml"),
            ("a.xsl", "xml"),
            ("a.xsd", "xml"),
            ("a.plist", "xml"),
            ("a.dtd", "dtd"),
        ] {
            assert_eq!(
                resolve(path),
                expected,
                "{path} 应当解析成 {expected}，而不是回落纯文本"
            );
        }
    }

    /// C# / CMake / Swift 是被**覆盖**（组件库原本注册了空查询），要确认覆盖真的生效：
    /// 注册表里那条查询非空。组件库若哪天自己补上查询，这条会给出提示。
    #[test]
    fn empty_builtin_queries_are_overridden() {
        assert_eq!(resolve("a.cs"), "csharp");
        for name in ["csharp", "cmake", "swift"] {
            let config = LanguageRegistry::singleton()
                .language(name)
                .unwrap_or_else(|| panic!("{name} 没被注册进注册表"));
            assert!(
                !config.highlights.is_empty(),
                "{name} 的 highlights 查询仍是空的——组件库那条空查询没被顶掉"
            );
        }
    }

    /// 注册要幂等：重复走生产路径不能把注册表越撑越大，也不能抹掉已有注册
    /// （`Once` 保证只真跑一次）。大小写不敏感是既有契约，顺带在这条里再夹一次。
    #[test]
    fn registration_is_idempotent() {
        assert_eq!(resolve("a.xml"), "xml"); // 触发注册
        let before = LanguageRegistry::singleton().languages().len();
        assert_eq!(resolve("a.XML"), "xml", "扩展名匹配应当大小写不敏感");
        assert_eq!(resolve("A.Swift"), "swift");
        assert_eq!(
            LanguageRegistry::singleton().languages().len(),
            before,
            "重复触发注册不应改变注册表规模"
        );
    }

    /// 查询归一化把组件库不认得的单字捕获名改写掉，同时保持其余部分逐字节不变。
    #[test]
    fn normalize_query_rewrites_only_unsupported_captures() {
        assert_eq!(normalize_query("(escape_sequence) @escape"), "(escape_sequence) @string.escape");
        assert_eq!(normalize_query("\",\" @delimiter"), "\",\" @punctuation.delimiter");
        assert_eq!(normalize_query("(module) @module"), "(module) @type");
        // 已在表里 / 前缀可回落的保持原样。
        assert_eq!(normalize_query("(comment) @comment"), "(comment) @comment");
        assert_eq!(
            normalize_query("(integer_literal) @constant.builtin"),
            "(integer_literal) @constant.builtin"
        );
        // 谓词里的引用与捕获是同一个 token，一起改。
        assert_eq!(
            normalize_query("((x) @escape (#eq? @escape \"n\"))"),
            "((x) @string.escape (#eq? @string.escape \"n\"))"
        );
        // 非 ASCII 字面量（查询里有中文/emoji 时）不能被拆坏。
        assert_eq!(normalize_query("; 注释 — @escape"), "; 注释 — @string.escape");
        // `@` 不在捕获名位置（如字符串里）不动。
        assert_eq!(normalize_query("\"a@b\" @comment"), "\"a@b\" @comment");
    }

    /// 归一化真的接到了注册表上：CSS 的 `@media` 被换成 `keyword`——组件库不认
    /// `media`，不换就会静默渲染成黑色。
    #[test]
    fn registered_queries_are_normalized() {
        assert_eq!(resolve("a.css"), "css"); // 走生产路径触发注册
        for name in ["css", "c", "csharp", "swift", "zig", "cmake"] {
            let config = LanguageRegistry::singleton()
                .language(name)
                .unwrap_or_else(|| panic!("{name} 不在注册表里"));
            let q = config.highlights.as_ref();
            // 归一化后不应再出现任何"组件库不认得"的捕获名。
            for token in q.split(|c: char| c.is_whitespace() || "()[]".contains(c)) {
                if let Some(capture) = token.strip_prefix('@') {
                    if !capture.is_empty() && crate::syntax::canonical_capture(capture).is_some() {
                        panic!("{name} 的查询里仍有未归一化的捕获名 @{capture}");
                    }
                }
            }
        }
        // 注入查询没有被归一化动过（`@injection.content` 之类不能改名）。
        let markdown = LanguageRegistry::singleton().language("markdown").unwrap();
        assert!(
            markdown.injections.contains("@injection.content"),
            "注入查询里的 @injection.content 不该被改写"
        );
    }

    /// **每一个**注册语言（含组件库自带那批）的 highlights 查询都必须能编译。
    ///
    /// 查询编译失败时组件库静默 `build_inert`——那个语言一个颜色都不出、也不报错。
    /// 组件库自带的 Kotlin 查询就坏在这上面（引用了语法里不存在的字面量），
    /// `register_fixed_kotlin` 修掉之后这条才转绿。这条同时给"自己加的注册项"兜底。
    #[test]
    fn every_registered_query_compiles() {
        assert_eq!(resolve("a.rs"), "rust"); // 走生产路径触发全部注册
        let registry = LanguageRegistry::singleton();
        for name in registry.languages() {
            let Some(config) = registry.language(&name) else {
                continue;
            };
            let Some(grammar) = config.language.as_ref() else {
                continue;
            };
            if config.highlights.is_empty() {
                continue;
            }
            match tree_sitter::Query::new(grammar, config.highlights.as_ref()) {
                Ok(_) => {},
                Err(error) => panic!(
                    "{name} 的 highlights 查询编译失败（该语言会静默变成零颜色）：{} \
                     @ row {} col {}",
                    error.message, error.row, error.column
                ),
            }
        }
    }
}

/// Markdown 行内样式的回归护栏。
///
/// 这里盯的是两个都踩过的坑，任何一个回归都会让用户看到 `**粗体**` 和反引号原样显示：
///
/// 1. 组件库给行内注入加的 `(#set! injection.combined)` 会把整篇文档的 inline 节点
///    合成一次解析，撞上它的 20ms 上限后**整层被丢弃**——文档越大越必然发生。
/// 2. 表格单元格（`pipe_table_cell`）在上游注入查询里根本没有对应模式，tree-sitter-md
///    的块语法把单元格当纯文本，所以单元格里的行内标记永远没人管。
///
/// 断言的是"确实产生了加粗/着色"，不是"代码调用了某个函数"——这样上面两条里任何一条
/// 被改回去，测试都会红。
#[cfg(test)]
mod markdown_inline_styles {
    use gpui_component::highlighter::{HighlightTheme, SyntaxHighlighter};

    /// 返回覆盖 `needle` 首字节的那一段样式。
    fn style_at(
        src: &str,
        needle: &str,
    ) -> (std::ops::Range<usize>, gpui::HighlightStyle) {
        crate::languages::ensure_registered();
        let rope = gpui_component::Rope::from_str(src);
        let mut highlighter = SyntaxHighlighter::new("markdown");
        highlighter.update(None, &rope, None);
        let styles = highlighter.styles(&(0..rope.len()), &HighlightTheme::default_light());
        let pos = src.find(needle).unwrap_or_else(|| panic!("样本里没有 {needle:?}"));
        styles
            .into_iter()
            .find(|(range, _)| range.contains(&pos))
            .unwrap_or_else(|| panic!("{needle:?} 落在没有任何样式的区间里"))
    }

    /// 表格单元格里的粗体必须真的加粗（坑 2）。
    #[test]
    fn bold_inside_a_table_cell_is_actually_bold() {
        let src = "| head | two |\n| --- | --- |\n| **加粗** | plain |\n";
        let (_, style) = style_at(src, "加粗");
        assert!(
            style.font_weight.is_some(),
            "表格单元格里的 **加粗** 没有得到粗体（pipe_table_cell 注入回归了）"
        );
    }

    /// 正文里的粗体与行内代码必须加粗 / 着色（坑 1：combined 超时会把整层丢掉）。
    #[test]
    fn bold_and_inline_code_in_prose_are_styled() {
        // 造一份足够长的文档，让"整篇合并解析"必然超过组件库 20ms 的注入上限：
        // 大量带行内标记的段落分布在很多块里，正是 combined 分支最吃力的形状。
        let mut src = String::new();
        for i in 0..1500 {
            src.push_str(&format!(
                "第 {i} 段：**加粗词{i}** 与 `code{i}` 以及普通文字，用来把文档撑到足以让整篇合并解析超过组件库那条 20ms 上限，从而复现 combined 回归。\n\n"
            ));
        }
        let (_, bold_style) = style_at(&src, "**加粗词7**");
        assert!(
            bold_style.font_weight.is_some(),
            "大文档正文里的 **加粗** 没有粗体——行内注入层可能又被 combined 的超时丢掉了"
        );

        let (_, code_style) = style_at(&src, "`code7`");
        assert!(
            code_style.color.is_some(),
            "大文档正文里的 `code` 没有着色——行内注入层可能又被 combined 的超时丢掉了"
        );
    }
}


#[cfg(test)]
mod c_family {
    use super::fix_preproc_query;
    use gpui_component::highlighter::SyntaxHighlighter;

    fn styles(src: &str, language: &str) -> (String, Vec<(std::ops::Range<usize>, gpui::HighlightStyle)>) {
        crate::languages::ensure_registered();
        let rope = gpui_component::Rope::from_str(src);
        let mut h = SyntaxHighlighter::new(language);
        h.update(None, &rope, None);
        let theme = crate::theme::notepad3_highlight_theme(language, true);
        (src.to_string(), h.styles(&(0..rope.len()), &theme))
    }

    /// C++ 必须拿到 C 的基础槽：`return` 是关键字深蓝、数字是红、字符串是绿。
    /// 只用 tree-sitter-cpp 那份专有查询的话这些都无色（用户报"常见语言没对齐"的一大项）。
    #[test]
    fn cpp_gets_the_c_base_slots() {
        let src = "int f() { if (x) return \"s\"; return 42; }\n";
        let (src, styles) = styles(src, "cpp");
        let color_at = |needle: &str| {
            let pos = src.find(needle).expect("needle");
            styles
                .iter()
                .find(|(r, _)| r.contains(&pos))
                .and_then(|(_, s)| s.color)
        };
        assert_eq!(color_at("return"), Some(crate::theme::to_hsla(0x0A, 0x24, 0x6A)), "关键字深蓝");
        assert_eq!(color_at("42"), Some(crate::theme::to_hsla(0xFF, 0x00, 0x00)), "数字红");
        assert_eq!(color_at("\"s\""), Some(crate::theme::to_hsla(0x00, 0x80, 0x00)), "字符串绿");
    }

    /// `#include` / `#define` 是 Notepad3 的 "Preprocessor" 橙 `#FF8000`，不是关键字深蓝。
    #[test]
    fn c_and_cpp_preprocessor_is_orange() {
        for language in ["c", "cpp"] {
            let src = "#include <stdio.h>\n#define N 3\nint main(void) { return N; }\n";
            let (src, styles) = styles(src, language);
            let color_at = |needle: &str| {
                let pos = src.find(needle).expect("needle");
                styles
                    .iter()
                    .find(|(r, _)| r.contains(&pos))
                    .and_then(|(_, s)| s.color)
            };
            let orange = crate::theme::to_hsla(0xFF, 0x80, 0x00);
            assert_eq!(color_at("#include"), Some(orange), "{language} 的 #include 应为橙");
            assert_eq!(color_at("#define"), Some(orange), "{language} 的 #define 应为橙");
        }
    }

    /// 修正函数只动预处理相关捕获，其余逐字保留。
    #[test]
    fn fix_preproc_query_rewrites_only_preproc() {        let source = "\"#define\" @keyword\n\"#if\" @keyword\n\"#endif\" @keyword\n\"return\" @keyword\n(preproc_directive) @keyword\n(preproc_function_def\n  name: (identifier) @function.special)\n";
        let fixed = fix_preproc_query(source);
        assert!(fixed.contains("\"#if\" @preproc"));
        assert!(fixed.contains("\"#define\" @preproc"));
        assert!(fixed.contains("(preproc_directive) @preproc"));
        assert!(fixed.contains("(identifier) @preproc"));
        assert!(!fixed.contains("@function.special"));
        // 普通关键字不动。
        assert!(fixed.contains("\"return\" @keyword"));
    }
}

#[cfg(test)]
mod python_query_fix {
    use gpui_component::highlighter::SyntaxHighlighter;

    /// 装饰器是 Notepad3 的 "Decorator" 金 `#F2B600`，不是 "Function Name" 的紫
    /// `#660066`（tree-sitter 默认捕成 `@function`）。
    #[test]
    fn python_decorator_is_gold() {
        crate::languages::ensure_registered();
        let src = "@app.route('/x')\ndef f():\n    return 1\n";
        let rope = gpui_component::Rope::from_str(src);
        let mut h = SyntaxHighlighter::new("python");
        h.update(None, &rope, None);
        let theme = crate::theme::notepad3_highlight_theme("python", true);
        let styles = h.styles(&(0..rope.len()), &theme);
        let color_at = |needle: &str| {
            let pos = src.find(needle).expect("needle");
            styles
                .iter()
                .find(|(r, _)| r.contains(&pos))
                .and_then(|(_, s)| s.color)
        };
        assert_eq!(
            color_at("@app"),
            Some(crate::theme::to_hsla(0xF2, 0xB6, 0x00)),
            "装饰器应当是金色"
        );
        // 函数名仍是紫色（Function Name）。锚 `f(`，别命中 `def` 里的那个 `f`。
        assert_eq!(color_at("f("), Some(crate::theme::to_hsla(0x66, 0x00, 0x66)));
    }
}

#[cfg(test)]
mod css_query_fix {
    use super::fix_css_query;
    use gpui_component::highlighter::{HighlightTheme, SyntaxHighlighter};

    /// 修正函数把 class/id 与属性名分开、伪类改捕到 label、unit 归 number，
    /// 其余逐字不动，并补上 `{` `}` `;` 的运算符着色。
    #[test]
    fn fix_css_query_rewrites_the_right_slots() {
        let source = "(class_name) @property\n(id_name) @property\n(property_name) @property\n(pseudo_class_selector (class_name) @attribute)\n(unit) @type\n(important) @keyword\n";
        let fixed = fix_css_query(source);
        assert!(fixed.contains("(class_name) @constant"));
        assert!(fixed.contains("(id_name) @constant"));
        assert!(fixed.contains("(property_name) @property"), "属性名要留着");
        assert!(fixed.contains("(pseudo_class_selector (class_name) @label)"));
        assert!(fixed.contains("(unit) @number"));
        // 花括号与分号归运算符（Notepad3 的 SCE_CSS_OPERATOR）。
        assert!(fixed.contains("\"{\" @operator"));
        assert!(fixed.contains("\"}\" @operator"));
        assert!(fixed.contains("\";\" @operator"));
        // `!important` 与裸值词 / 十六进制颜色。
        assert!(fixed.contains("(important) @boolean"), "important 要就地改成 @boolean");
        assert!(!fixed.contains("(important) @keyword"), "旧的关键字捕获不能再留着");
        assert!(fixed.contains("(color_value) @string.special"));
        assert!(fixed.contains("(plain_value) @string.special"));
    }

    /// 端到端：`.counter` 得的是 `@constant`（橄榄），属性名得的是 `@property`（橙）。
    /// 这是"组件库把两者混成一个 `@property`"那条差异的直接回归护栏。
    #[test]
    fn class_and_property_get_different_colors() {
        crate::languages::ensure_registered();
        let src = ".counter { color: red; }\n";
        let rope = gpui_component::Rope::from_str(src);
        let mut h = SyntaxHighlighter::new("css");
        h.update(None, &rope, None);
        let styles = h.styles(&(0..rope.len()), &HighlightTheme::default_light());
        let color_at = |needle: &str| {
            let pos = src.find(needle).expect("needle");
            styles
                .iter()
                .find(|(r, _)| r.contains(&pos))
                .and_then(|(_, s)| s.color)
                .unwrap_or_else(|| panic!("{needle} 没有颜色"))
        };
        // `.counter` 与 `color` 必须不同色——修复前两者都是 `#FF4000`。
        assert_ne!(
            color_at("counter"),
            color_at("color"),
            "class 与属性名应当是两种颜色（Notepad3 的 Tag-Class vs CSS Property）"
        );
    }

    /// CSS 的花括号必须拿到颜色（Notepad3 的 `SCE_CSS_OPERATOR` 洋红）。修复前
    /// tree-sitter-css 的查询不捕 `{` / `}`，它们渲染成正文黑——用户报的就是这个。
    #[test]
    fn css_braces_are_colored() {
        crate::languages::ensure_registered();
        let src = "a { color: red; }\n";
        let rope = gpui_component::Rope::from_str(src);
        let mut h = SyntaxHighlighter::new("css");
        h.update(None, &rope, None);
        // 用**实际装入**的 Notepad3 主题，而不是组件库基线（基线里 operator 可能没色）。
        let theme = crate::theme::notepad3_highlight_theme("css", true);
        let styles = h.styles(&(0..rope.len()), &theme);
        for needle in ["{", "}", ";"] {
            let pos = src.find(needle).expect("needle");
            let color = styles
                .iter()
                .find(|(r, _)| r.contains(&pos))
                .and_then(|(_, s)| s.color);
            assert!(color.is_some(), "CSS 的 `{needle}` 应当有颜色（Operator）");
        }
    }

    /// `!important` 必须是 Notepad3 的 `SCE_CSS_IMPORTANT`：红粗体 `#C80000`。
    /// 用户报"`important !` 是蓝的而 Notepad3 是红的"——修复前它命中 `@keyword`
    /// （深蓝 `#0A246A`）。
    #[test]
    fn css_important_is_red_bold() {
        crate::languages::ensure_registered();
        let src = "a { color: red !important; }\n";
        let rope = gpui_component::Rope::from_str(src);
        let mut h = SyntaxHighlighter::new("css");
        h.update(None, &rope, None);
        let theme = crate::theme::notepad3_highlight_theme("css", true);
        let styles = h.styles(&(0..rope.len()), &theme);
        let pos = src.find("!important").unwrap();
        let style = styles
            .iter()
            .find(|(r, _)| r.contains(&pos))
            .map(|(_, s)| *s)
            .expect("!important 应当有样式");
        assert_eq!(
            style.color,
            Some(crate::theme::to_hsla(0xC8, 0x00, 0x00)),
            "!important 应当是 Notepad3 的红色"
        );
        assert_eq!(
            style.font_weight,
            Some(gpui::FontWeight::BOLD),
            "!important 应当是粗体"
        );
    }

    /// 裸值词（`center`）与十六进制颜色都要拿到 Notepad3 的 "Value" 蓝 `#3A6EA5`
    /// （`SCE_CSS_VALUE`）。修复前它们没有任何捕获、渲染成正文黑。
    #[test]
    fn css_plain_values_and_hex_are_the_value_blue() {
        crate::languages::ensure_registered();
        let src = "a { background-position: center center; color: #ff0000; }\n";
        let rope = gpui_component::Rope::from_str(src);
        let mut h = SyntaxHighlighter::new("css");
        h.update(None, &rope, None);
        let theme = crate::theme::notepad3_highlight_theme("css", true);
        let styles = h.styles(&(0..rope.len()), &theme);
        let color_at = |needle: &str| {
            let pos = src.find(needle).expect("needle");
            styles
                .iter()
                .find(|(r, _)| r.contains(&pos))
                .and_then(|(_, s)| s.color)
                .unwrap_or_else(|| panic!("{needle} 没有颜色"))
        };
        let value_blue = crate::theme::to_hsla(0x3A, 0x6E, 0xA5);
        assert_eq!(color_at("center"), value_blue, "`center` 应当是 Value 蓝");
        // 锚十六进制数字而非 `#`：`#` 本身是分隔符（`@punctuation`），与 Notepad3
        // 把 `#` 也算进运算符的观感一致；颜色值那一截才是 "Value" 蓝。
        assert_eq!(color_at("ff0000"), value_blue, "十六进制颜色应当是 Value 蓝");
    }
}

#[cfg(test)]
mod markdown_query_fix {
    use super::fix_markdown_heading_query;
    use gpui_component::highlighter::SyntaxHighlighter;

    /// Markdown 链接 / 图片的括号标记要有链接蓝（Notepad3 的 `SCE_MARKDOWN_LINK`
    /// 把 `[` `]` `(` `)` 也算进 Link 槽）。修复前只有链接内容与地址有色。
    #[test]
    fn markdown_link_brackets_are_colored() {
        crate::languages::ensure_registered();
        let src = "see [text] and ![img](x.png)\n";
        let rope = gpui_component::Rope::from_str(src);
        let mut h = SyntaxHighlighter::new("markdown");
        h.update(None, &rope, None);
        let theme = crate::theme::notepad3_highlight_theme("markdown", true);
        let styles = h.styles(&(0..rope.len()), &theme);
        let color_at = |pos: usize| {
            styles
                .iter()
                .find(|(r, _)| r.contains(&pos))
                .and_then(|(_, s)| s.color)
        };
        // `[text]` 的 `[` 与 `]`；`![img](...)` 的 `!`。
        assert!(color_at(src.find('[').unwrap()).is_some(), "链接的 `[` 应当有颜色");
        assert!(color_at(src.find(']').unwrap()).is_some(), "链接的 `]` 应当有颜色");
        assert!(color_at(src.find('!').unwrap()).is_some(), "图片的 `!` 应当有颜色");
    }

    /// ATX 标题在**语法层**必须没有颜色，好让 `np3` 的前景/底色稳定生效
    /// （重叠的胜负取决于哈希序，不摘掉就会看运气）。
    #[test]
    fn markdown_atx_headings_are_left_to_the_overlay() {
        crate::languages::ensure_registered();
        let src = "# H1 title\n";
        let rope = gpui_component::Rope::from_str(src);
        let mut h = SyntaxHighlighter::new("markdown");
        h.update(None, &rope, None);
        let theme = crate::theme::notepad3_highlight_theme("markdown", true);
        for (range, style) in h.styles(&(0..rope.len()), &theme) {
            assert!(
                style.color.is_none(),
                "ATX 标题 {:?} 不该由语法层着色（会与 np3 的级别色打架）：{:?}",
                &src[range],
                style.color
            );
        }
    }

    /// 修正函数是纯文本删除：ATX 标题两处去掉，Setext 标题保留。
    #[test]
    fn fix_markdown_heading_query_removes_only_atx() {
        let source = "(atx_heading (inline) @title)\n(setext_heading (paragraph) @title)\n[\n  (atx_h1_marker)\n  (atx_h2_marker)\n  (setext_h1_underline)\n] @punctuation.special\n";
        let fixed = fix_markdown_heading_query(source);
        assert!(!fixed.contains("atx_heading"));
        assert!(!fixed.contains("atx_h1_marker"));
        assert!(!fixed.contains("atx_h2_marker"));
        assert!(fixed.contains("(setext_heading (paragraph) @title)"), "Setext 标题要保留");
        assert!(fixed.contains("(setext_h1_underline)"), "Setext 下划线要保留");
    }
}

#[cfg(test)]
mod kotlin_query_fix {
    use super::fix_kotlin_query;

    /// Kotlin 的查询必须能编译。组件库自带那份引用了语法里不存在的字面量
    /// （`"!is"` / `"!in"` / `"$"` / `"${"`），`Query::new` 会整体报错、Kotlin
    /// 静默变成零颜色。`fix_kotlin_query` 修掉之后这条才是绿的。
    #[test]
    fn kotlin_query_compiles_after_fixup() {
        crate::languages::ensure_registered();
        let cfg = gpui_component::highlighter::LanguageRegistry::singleton()
            .language("kotlin")
            .expect("kotlin registered");
        let grammar = cfg.language.clone().expect("kotlin has a grammar");
        tree_sitter::Query::new(&grammar, cfg.highlights.as_ref())
            .expect("kotlin 的 highlights 查询应当能编译");
    }

    /// 修正函数是纯文本替换：无效字面量消失、插值模式换成上游写法、其余不动。
    #[test]
    fn fix_kotlin_query_rewrites_only_broken_bits() {
        let source = "\t\"!is\"\n\t\"!in\"\n(string_literal\n\t\"$\" @punctuation.special\n\t(interpolated_identifier) @variable)\n(string_literal\n\t\"${\" @punctuation.special\n\t(interpolated_expression)\n\t\"}\" @punctuation.special)\n(\"is\") @operator\n";
        let fixed = fix_kotlin_query(source);
        assert!(!fixed.contains("\"!is\""));
        assert!(!fixed.contains("\"!in\""));
        assert!(!fixed.contains("\"$\" @punctuation.special"));
        assert!(!fixed.contains("\"${\" @punctuation.special"));
        assert!(fixed.contains("(interpolation_identifier_start) @punctuation.special"));
        assert!(fixed.contains("(interpolation_expression_start) @punctuation.special"));
        // 合法的 `"is"` 与其余内容保持原样。
        assert!(fixed.contains("(\"is\") @operator"));
    }
}





#[cfg(test)]
mod notepad3_alignment {
    //! 常见语言与 Notepad3 逐槽对齐的回归护栏。
    //!
    //! 每条都断言"**真的拿到了 Notepad3 那一槽的颜色**"，而不是"代码调用了某个
    //! 函数"——这样把对应的 `register_fixed_*` 摘掉、或组件库/语法 crate 改了查询，
    //! 测试就会红。用的是**真正装入编辑器的那份主题**（`notepad3_highlight_theme`），
    //! 所以断言的等价关系（如"`int` 与 `public` 同色"）就是渲染出来的效果。
    use gpui_component::highlighter::SyntaxHighlighter;

    /// 取覆盖 `needle` 首字节的那一段样式的颜色。
    fn color_at(lang: &str, src: &str, needle: &str) -> Option<gpui::Hsla> {
        crate::languages::ensure_registered();
        let theme = crate::theme::notepad3_highlight_theme(lang, false);
        let rope = gpui_component::Rope::from_str(src);
        let mut h = SyntaxHighlighter::new(lang);
        h.update(None, &rope, None);
        let styles = h.styles(&(0..rope.len()), &theme);
        let pos = src.find(needle).unwrap_or_else(|| panic!("样本里没有 {needle:?}"));
        styles
            .into_iter()
            .find(|(range, _)| range.contains(&pos))
            .and_then(|(_, style)| style.color)
    }

    /// PowerShell：`$x = "Hi"` 的**字符串**必须拿到字符串色，而不是被
    /// `@assignvalue → @operator`（黑粗体）整条吞掉。此前赋值右值全是黑的。
    #[test]
    fn powershell_assignment_rhs_keeps_its_own_color() {
        // One Light 里 string 是 #008000 一类绿；operator 是另一档。直接对比
        // 字符串与数字各自拿到的颜色"不是同一个被 assignvalue 覆盖出来的槽"。
        let string = color_at("powershell", "$x = \"Hi\"\n", "\"Hi\"");
        assert!(string.is_some(), "字符串必须有着色，不能被赋值右值吞成无样式");
        let op = color_at("powershell", "1 -eq 2\n", "-eq");
        assert!(op.is_some(), "运算符仍应有着色");
        assert_ne!(string, op, "字符串不该与运算符同色（assignvalue 过量捕获回归了）");
    }

    /// Java：内建类型 `int` / `void` 必须落在**关键字**槽（不是无色的 type）。
    #[test]
    fn java_builtin_types_are_keywords() {
        let src = "public class C { void f() { int y = 1; } }\n";
        let keyword = color_at("java", src, "public").expect("public 是关键字");
        let int = color_at("java", src, "int y").expect("int 应当着色");
        assert_eq!(int, keyword, "`int` 应与关键字同色（Notepad3 把它编在关键字表里）");
    }

    /// C#：预定义类型 `int` / `void` / `string` 落在关键字槽，而不是浅蓝类名槽。
    #[test]
    fn csharp_predefined_types_are_keywords() {
        let src = "public class C { void F() { int y = 1; } }\n";
        let keyword = color_at("csharp", src, "public").expect("public 是关键字");
        let int = color_at("csharp", src, "int y").expect("int 应当着色");
        assert_eq!(int, keyword, "C# 的 `int` 应与关键字同色");
    }

    /// C / C++：内建类型 `int` / `void` 落在关键字槽（Notepad3 的 `KeyWords_CPP`）。
    #[test]
    fn c_family_builtin_types_are_keywords() {
        for lang in ["c", "cpp"] {
            let src = "int main(void) { return 0; }\n";
            let keyword = color_at(lang, src, "return").expect("return 是关键字");
            let int = color_at(lang, src, "int main").expect("int 应当着色");
            assert_eq!(int, keyword, "{lang} 的 `int` 应与关键字同色");
        }
    }

    /// TSX：普通 TS/JS 构造（`const` / 字符串 / 数字）必须有颜色。此前组件库只
    /// 挂了 TS 的追加查询，`.tsx` 整片黑。
    #[test]
    fn tsx_has_javascript_base_highlighting() {
        let src = "const x: number = 1;\n";
        let keyword = color_at("tsx", src, "const").expect("const 是关键字");
        let number = color_at("tsx", src, "1;").expect("数字应当着色");
        assert!(keyword != number, "TSX 的关键字与数字应当不同色");
    }

    /// 回归护栏：修 `register_fixed_java` 时别把 `@type`（类名）也一起改掉——
    /// Java 调色板里类名是正文黑，`String` 这类仍是 `@type`。
    #[test]
    fn java_class_names_stay_type_slot() {
        let src = "String s = null;\n";
        // `String` 被 TS/Java 查询捕成 @type（大写开头标识符）。不要求它有颜色，
        // 只要求它**不是**关键字色——即 `register_fixed_java` 没有误伤 `@type`。
        let class = color_at("java", src, "String");
        let keyword = color_at("java", "public\n", "public");
        assert_ne!(class, keyword, "类名不该被当成关键字（@type 被误改了）");
    }
}

/// **首次同步解析的预算够用** —— 这是 `vendor/gpui-component/NEBULA-LITE-PATCH.md`
/// 那处补丁的成立依据。
///
/// 打过补丁的 `InputMode::update_highlighter` 在**首次**解析（highlighter 还没有
/// 语法树，也就是刚打开文件 / 刚换语言）时给 250ms 前台预算；只要解析能在预算内
/// 跑完，它就不会走"后台线程 + 先等 150ms 防抖"那条路 —— 也就是"打开文件后几百
/// 毫秒才有颜色"消失。
///
/// 这条把那个前提钉住：拿本仓库最长的源文件（`app.rs`，约 190 KB）当样本，它必须
/// 能在这份预算里解析完。判据是 `update(.., Some(预算))` 的**返回值**（`true` 就是
/// "在预算内完成"），与补丁里那条分支的判据完全一致，不量秒表、不受机器负载影响。
/// 上游若把解析变慢到超预算，这条会红 —— 那正是要重新审视补丁预算的信号。
#[cfg(test)]
mod first_parse_budget {
    use std::path::Path;
    use std::time::{Duration, Instant};

    use gpui_component::highlighter::SyntaxHighlighter;

    /// 必须与补丁里的 `FIRST_PARSE_TIMEOUT` 一致。
    const FIRST_PARSE_BUDGET: Duration = Duration::from_millis(250);

    #[test]
    fn a_large_file_parses_within_the_budget() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("app.rs");
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("读不到样本 {}：{error}", path.display()));
        assert!(
            source.len() > 100 * 1024,
            "样本应当是一份真正的大文件（当前只有 {} 字节），否则这条测试没有意义",
            source.len()
        );

        let rope = gpui_component::Rope::from_str(&source);
        let started = Instant::now();
        let mut highlighter = SyntaxHighlighter::new("rust");
        let completed = highlighter.update(None, &rope, Some(FIRST_PARSE_BUDGET));
        eprintln!(
            "首次同步解析样本：{} 字节，完成={completed}，实测 {:?}（预算 {FIRST_PARSE_BUDGET:?}）",
            source.len(),
            started.elapsed(),
        );
        assert!(
            completed,
            "{} 字节没能在这份预算内解析完——首次高亮会退回后台 + 150ms 防抖",
            source.len()
        );
    }
}
