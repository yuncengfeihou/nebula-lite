//! 文件树图标字形表，逐值照抄 Pebrel 1.9.1 的
//! `nebula_app/src/display/side_panel/icons.rs`。
//!
//! 字形全部取自内嵌的 Maple Mono Nerd Font：这些码点在 Unicode 私有使用区，
//! 系统等宽字体里没有，装上系统字体也未必有——所以渲染时**必须**显式指定
//! [`crate::fonts::REQUIRED_FONT_FAMILY`]（Pebrel 的 GPUI 文件树同样这么做：
//! 图标列的 `font_family` 固定 Maple，不让界面主字体改变折叠箭头与文件图标的
//! advance，否则列宽会跟着字体度量抖动）。
//!
//! 两处细节与 Pebrel 保持一致，不要"顺手改成更好看的 Unicode 符号"：
//! - 折叠箭头用 Nerd Font 的 chevron（`\u{eab6}` / `\u{eab4}`），不是 `▸` / `▾`
//!   ——后者是几何形状字符，与图标列的字重、基线都对不齐，看起来像两套字混排。
//! - 目录与文件的图标分属两个码位区段（`f1xx` 与 `exxx`），混用同一族里不存在的
//!   码点会渲染成方框。

/// 收起的文件夹。
pub const ICON_FOLDER: &str = "\u{f114}";
/// 展开的文件夹。
pub const ICON_FOLDER_OPEN: &str = "\u{f115}";
/// 脚本类文件的图标（bat / cmd 复用）。
pub const ICON_TERMINAL: &str = "\u{ea85}";
/// 未知扩展名文件的兜底图标。
pub const ICON_FILE: &str = "\u{ea7b}";
/// 收起的折叠箭头。
pub const ICON_CHEVRON_RIGHT: &str = "\u{eab6}";
/// 展开的折叠箭头。
pub const ICON_CHEVRON_DOWN: &str = "\u{eab4}";

/// 设置面板里"顶置这个字体"的图钉。
///
/// 与其余图标一样是 Nerd Font 的私有使用区码点（`nf-fa-thumb_tack`），所以渲染
/// 时必须显式指定 Maple 字族——字体设置那一行的字族是**候选字体**本身，拿它渲染
/// 图钉会是个方框。已用 fontTools 核对过内嵌的
/// `MapleMonoNormal-NF-CN-Regular.ttf` 里确有该码点。
pub const ICON_PIN: &str = "\u{f08d}";

/// 目录图标：展开与收起各一套。
pub fn folder_icon(expanded: bool) -> &'static str {
    if expanded { ICON_FOLDER_OPEN } else { ICON_FOLDER }
}

/// 折叠箭头：展开与收起各一套。
pub fn chevron_icon(expanded: bool) -> &'static str {
    if expanded { ICON_CHEVRON_DOWN } else { ICON_CHEVRON_RIGHT }
}

/// 按扩展名给文件挑图标，未知扩展名回落 [`ICON_FILE`]。
///
/// 大小写不敏感：Windows 上 `README.MD` 与 `readme.md` 是同一类文件，图标不该
/// 因为大小写而不同。点开头的配置文件（`.gitignore` 等）单独处理，因为它们
/// `rsplit_once('.')` 得到的是空扩展名。
pub fn file_type_icon(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    if lower.starts_with(".git") {
        return "\u{e65d}";
    }
    let ext = lower.rsplit_once('.').map(|(_, ext)| ext).unwrap_or("");
    match ext {
        "md" | "markdown" => "\u{eb1d}",
        "json" | "jsonl" | "ndjson" => "\u{eb0f}",
        "toml" => "\u{e6b2}",
        "yml" | "yaml" => "\u{e6a8}",
        "xml" => "\u{e619}",
        "rs" => "\u{e68b}",
        "py" => "\u{e606}",
        "js" | "mjs" | "cjs" | "jsx" => "\u{e60c}",
        "ts" | "tsx" => "\u{e628}",
        "html" | "htm" => "\u{e60e}",
        "css" | "scss" | "less" => "\u{e614}",
        "c" | "h" => "\u{e61e}",
        "cpp" | "cc" | "cxx" | "hpp" => "\u{e61d}",
        "cs" => "\u{e648}",
        "java" => "\u{e66d}",
        "go" => "\u{e627}",
        "sh" | "bash" | "zsh" => "\u{e691}",
        "ps1" | "psm1" | "psd1" => "\u{e683}",
        "bat" | "cmd" => ICON_TERMINAL,
        "sql" | "db" | "sqlite" => "\u{e64d}",
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" | "svg" => "\u{e60d}",
        "zip" | "7z" | "rar" | "gz" | "tar" | "xz" | "zst" => "\u{f1c6}",
        "pdf" => "\u{f1c1}",
        "lock" => "\u{e672}",
        "log" => "\u{f4ed}",
        "txt" => "\u{f0f6}",
        _ => ICON_FILE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 目录图标与折叠箭头都必须区分展开/收起，否则两态看起来一模一样。
    #[test]
    fn expanded_and_collapsed_glyphs_differ() {
        assert_ne!(folder_icon(true), folder_icon(false));
        assert_ne!(chevron_icon(true), chevron_icon(false));
    }

    /// 已知扩展名拿到专属图标，未知扩展名回落兜底——回落不能是空串，否则
    /// 文件树里会出现"没有图标"的空档。
    #[test]
    fn known_extensions_get_their_own_glyph_and_unknown_falls_back() {
        assert_ne!(file_type_icon("main.rs"), ICON_FILE);
        assert_eq!(file_type_icon("README.unknown"), ICON_FILE);
        assert!(!ICON_FILE.is_empty());
    }

    /// 扩展名匹配不区分大小写，`.git*` 开头的文件走专属分支。
    #[test]
    fn matching_is_case_insensitive_and_dotfiles_are_special() {
        assert_eq!(file_type_icon("MAIN.RS"), file_type_icon("main.rs"));
        assert_eq!(file_type_icon(".gitignore"), file_type_icon(".gitattributes"));
        assert_ne!(file_type_icon(".gitignore"), ICON_FILE);
    }

    /// 图钉必须是**一个**落在私有使用区的码点：多字符会被当普通字符串渲染成方框，
    /// 而落在普通区（例如某个 Unicode 符号）会被 Maple 之外的字形接走、与图标列的
    /// 字重基线对不齐——文件树的图标列也是这么约定的（见本文件顶部说明）。
    #[test]
    fn pin_glyph_is_a_single_private_use_codepoint() {
        let mut chars = ICON_PIN.chars();
        let glyph = chars.next().expect("图钉不能是空串");
        assert!(chars.next().is_none(), "图钉只能是单个码点");
        assert!(
            ('\u{e000}'..='\u{f8ff}').contains(&glyph),
            "图钉应当落在 Nerd Font 的私有使用区（U+E000..U+F8FF）"
        );
    }
}
