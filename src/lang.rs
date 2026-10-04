//! 扩展名 -> tree-sitter 语言 id。
//!
//! `EXTENSIONS` 逐条照抄 Pebrel 的 `gpui_shell/code_tab.rs::language_for_extension`
//! （连它与本组件库 feature 集对不上的那些条目一起照抄），语言 id 必须与组件库
//! highlighter 的注册名完全一致。
//!
//! 这张表是**意图**，不是事实：Pebrel 开的是同一份 `tree-sitter-languages`
//! feature 集（`nebula_app/Cargo.toml`），所以那份映射里有一部分语言组件库根本
//! 不认识，另有一部分注册了语法但 highlights 查询是空串。两者都拿不到颜色。因此
//! 交付给编辑器之前先过 [`colorable`]，让"这份表声明了什么"与"实际有没有高亮"
//! 不再能悄悄分家——见该函数与测试里的 `unsupported_languages_fall_back_to_plain_text`。
//!
//! XML / PowerShell / INI 原先属于"组件库不认识"那一类，现在由 `languages.rs`
//! 注册进组件库的公开注册表，`colorable` 会自动放行——判据读的是注册表现状，
//! 不是一张写死的名单，所以这里不需要为它们开特例。

use std::path::Path;

use gpui_component::highlighter::LanguageRegistry;

/// 扩展名 -> 语言 id 的意图表，条目与顺序都取自 Pebrel 的匹配分支。
pub const EXTENSIONS: &[(&str, &str)] = &[
    ("rs", "rust"),
    ("md", "markdown"),
    ("markdown", "markdown"),
    ("py", "python"),
    ("pyi", "python"),
    ("js", "javascript"),
    ("mjs", "javascript"),
    ("cjs", "javascript"),
    ("ts", "typescript"),
    ("mts", "typescript"),
    ("cts", "typescript"),
    ("tsx", "tsx"),
    ("jsx", "jsx"),
    ("c", "c"),
    ("h", "c"),
    ("cpp", "cpp"),
    ("cc", "cpp"),
    ("cxx", "cpp"),
    ("hpp", "cpp"),
    ("hh", "cpp"),
    ("cs", "csharp"),
    ("go", "go"),
    ("java", "java"),
    ("rb", "ruby"),
    ("php", "php"),
    ("swift", "swift"),
    ("kt", "kotlin"),
    ("kts", "kotlin"),
    ("lua", "lua"),
    ("sh", "bash"),
    ("bash", "bash"),
    ("zsh", "bash"),
    // Notepad3 的 `styleLexBAT.c` 管 `bat; cmd`。
    ("bat", "batch"),
    ("cmd", "batch"),
    ("ps1", "powershell"),
    ("psm1", "powershell"),
    ("psd1", "powershell"),
    ("toml", "toml"),
    ("yaml", "yaml"),
    ("yml", "yaml"),
    ("html", "html"),
    ("htm", "html"),
    // Notepad3 的 `lexHTML` 把下面这些一并算作 "Web Source Code"（`styleLexHTML.c`
    // 的扩展名表 `html; htm; asp; aspx; shtml; htd; xhtml; …`）。`.xhtml` 是其中
    // 最常被打开的一个，此前不在表里、整篇回落纯文本——用户报的"XHTML 没高亮"
    // 就是它。这几个都按 XML/HTML 语法解析，与 Notepad3 的 `SCLEX_HTML` 对齐。
    ("xhtml", "html"),
    ("shtml", "html"),
    ("hta", "html"),
    ("htc", "html"),
    ("css", "css"),
    ("scss", "scss"),
    ("sql", "sql"),
    ("zig", "zig"),
    ("cmake", "cmake"),
    ("dockerfile", "dockerfile"),
    ("ex", "elixir"),
    ("exs", "elixir"),
    ("erl", "erlang"),
    ("hs", "haskell"),
    ("ini", "ini"),
    ("cfg", "ini"),
    ("conf", "ini"),
    ("vim", "vim"),
    ("xml", "xml"),
    // Notepad3 的 `lexXML`（`styleLexXML.c`）：`xml; xsl; rss; svg; xul; xsd; xslt;
    // axl; rdf; xaml; vcproj; … resx; plist; xrc; … manifest`。这些全是 XML 文档，
    // 用同一份 tree-sitter-xml 语法解析。补齐它们，免得 `.svg` / `.xsl` / `.xsd`
    // 这类常见文件打开是纯文本。
    ("xsl", "xml"),
    ("xslt", "xml"),
    ("svg", "xml"),
    ("xul", "xml"),
    ("xsd", "xml"),
    ("rss", "xml"),
    ("atom", "xml"),
    ("xaml", "xml"),
    ("plist", "xml"),
    ("resx", "xml"),
    ("vcxproj", "xml"),
    ("csproj", "xml"),
    ("props", "xml"),
    ("targets", "xml"),
    // DTD 有独立的语法（`tree_sitter_xml::LANGUAGE_DTD`），与 Notepad3 把 `.dtd`
    // 归 HTML lexer 的意图一致（都是"标记文档"）。
    ("dtd", "dtd"),
    ("json", "json"),
    ("jsonl", "json"),
    ("ndjson", "json"),
    ("txt", "text"),
    ("log", "text"),
    ("text", "text"),
];

/// 按意图表把扩展名翻成语言 id（大小写不敏感）。
///
/// 返回的是**意图**：可能是一个组件库上不了色的语言。要拿"实际用哪个语言"必须走
/// [`language_for_path`]。
pub fn language_for_extension(extension: &str) -> Option<&'static str> {
    let extension = extension.to_ascii_lowercase();
    EXTENSIONS
        .iter()
        .find(|(name, _)| *name == extension)
        .map(|(_, language)| *language)
}

/// 该语言 id 在组件库当前注册表下是否真能出颜色。
///
/// 判据直接取自组件库的注册表（`LanguageConfig` 的字段是 pub），因此既不会与它
/// 漂移，也会自动认下本仓库自己注册的语言（`languages.rs` 注册的那批走的就是这条路）。
/// 两类会**静默**变纯文本，都拦在这里：
///
/// - 注册表里没有这个名字、本仓库也没注册：`dockerfile` / `vim`。
///   组件库在 `InputMode` 里会用 `has_grammar()` 拦一道、干脆不建 highlighter，
///   所以这里拦下来只是让本文件的声明变诚实。
/// - 注册了语法但 highlights 查询是空串。组件库原本对 `cmake` / `csharp` / `swift`
///   就是这么写的，`languages.rs` 已经把它们的查询顶掉；这一分支留着是为了兜住
///   将来新出现的同类情况——组件库拦不住它（`has_grammar()` 为真），会**每次都白跑
///   一轮 tree-sitter 解析却一个颜色都不出**。
///
/// `text` 是"无高亮"的合法答案，单独放行。
///
/// 注意：判据依赖 [`crate::languages::ensure_registered`] 已经跑过（`language_for_path`
/// 会先调它），否则本仓库注册的语言在注册表里查不到、会被自己的判据拦成纯文本。
fn colorable(language: &str) -> bool {
    if language == "text" {
        return true;
    }
    LanguageRegistry::singleton()
        .language(language)
        .is_some_and(|config| !config.highlights.is_empty())
}

/// 实际交给编辑器的语言 id：意图表给出的语言上不了色、或扩展名根本不在表里，
/// 都回落到 `text`（组件库的无高亮纯文本模式），因此不会有文件因为"不认识"而打不开。
///
/// 先 [`crate::languages::ensure_registered`]：本仓库自带的语法必须在任何人拿到语言
/// id 之前注册好，否则编辑器去注册表查不到语法、只能退化成纯文本。放在这里而不是
/// `main` 里，是因为本函数是语言 id 的**唯一产地**——注册与取用因此不可能错开。
pub fn language_for_path(path: &Path) -> &'static str {
    crate::languages::ensure_registered();
    path.extension()
        .and_then(|extension| extension.to_str())
        .and_then(language_for_extension)
        .filter(|language| colorable(language))
        .unwrap_or("text")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn known_extensions_map_to_language_ids() {
        assert_eq!(language_for_path(Path::new("main.rs")), "rust");
        assert_eq!(language_for_path(Path::new("a.toml")), "toml");
        assert_eq!(language_for_path(Path::new("README.md")), "markdown");
    }

    /// 扩展名大小写不敏感（Windows 上很常见）。
    #[test]
    fn extension_matching_is_case_insensitive() {
        assert_eq!(language_for_path(Path::new("MAIN.RS")), "rust");
        assert_eq!(language_for_path(Path::new("a.YAML")), "yaml");
    }

    /// 不认识的扩展名与没有扩展名都要回落到纯文本，而不是打不开。
    #[test]
    fn unknown_extensions_fall_back_to_plain_text() {
        assert_eq!(language_for_path(Path::new("data.bin")), "text");
        assert_eq!(language_for_path(Path::new("Makefile")), "text");
    }

    /// 意图表里每一条，最终交付的语言 id 要么是 `text`、要么真能上色。
    /// 不允许出现"声明了却静默变纯文本"的第三种情况——删掉 `language_for_path`
    /// 里的 `colorable` 过滤，这个测试就会红。
    #[test]
    fn every_declared_extension_is_plain_or_actually_highlighted() {
        for (extension, language) in EXTENSIONS {
            let path = PathBuf::from(format!("sample.{extension}"));
            let resolved = language_for_path(&path);
            assert!(
                resolved == "text" || colorable(resolved),
                ".{extension} 意图是 {language}，最终解析成 {resolved}，但 {resolved} 上不了色"
            );
        }
    }

    /// 当前拿不到颜色的语言必须如实回落纯文本。这一组是**快照**：每补上一个语言
    /// 它就应当缩小，缩小是好事，但要顺手改这里与 `colorable` 的注释。
    ///
    /// 现在只剩这两个，原因见 `colorable` 的文档：`dockerfile` 与 `vim` 的语法 crate
    /// 会把 `tree-sitter` 0.20 一并拉进来，与组件库要的 0.26 冲突，所以补不了。
    /// 两个都没有语法可选，因此这里不受"注册是否已发生"的影响。
    #[test]
    fn unsupported_languages_fall_back_to_plain_text() {
        for extension in ["dockerfile", "vim"] {
            let path = PathBuf::from(format!("sample.{extension}"));
            assert_eq!(
                language_for_path(&path),
                "text",
                ".{extension} 在当前注册表下拿不到颜色，应当回落纯文本"
            );
        }
    }

    /// 反向护栏：真能上色的语言不许被误判成纯文本。没有这一条，`colorable` 判据
    /// 写错（例如恒返回 false）会让"修好了"实为"高亮全没了"，而上面两个测试都还是绿的。
    #[test]
    fn supported_languages_stay_highlighted() {
        for extension in [
            "rs", "md", "markdown", "py", "js", "ts", "tsx", "c", "h", "cpp", "go", "java", "rb",
            "php", "kt", "lua", "sh", "toml", "yaml", "html", "css", "scss", "sql", "zig", "json",
            "ex",
        ] {
            let path = PathBuf::from(format!("sample.{extension}"));
            assert_ne!(
                language_for_path(&path),
                "text",
                ".{extension} 本该有高亮，却被降级成了纯文本"
            );
        }
    }

    /// 表里不能有重复扩展名：查找只认第一条，重复会让后面那条静默失效。
    #[test]
    fn extension_table_has_no_duplicates() {
        let mut seen = std::collections::HashSet::new();
        for (extension, _) in EXTENSIONS {
            assert!(
                seen.insert(*extension),
                "扩展名 {extension} 在 EXTENSIONS 里出现了两次"
            );
        }
    }
}
