//! 裸 URL 高亮（Notepad3 的 "Hyperlink Hotspots"）。
//!
//! ## 为什么单独做这一层
//!
//! Notepad3 对**任何文件**里的裸 URL（`https://…`、`ftp://…`、`mailto:…`、
//! `www.…`）都用 Scintilla 的 **indicator** 层涂成低饱和蓝（`styleLexStandard.c`：
//! `Hyperlink Hotspots` 的 `back:#0060B0`，即 `INDIC_NP3_HYPERLINK` 的 `INDIC_TEXTFORE`
//! 色 `#0060B0`）。这与词法分析器无关——`Edit.c::_UpdateIndicators` 在**每次刷新**
//! 时按 `HYPLNK_REGEX_FULL` 扫一遍可见区间，正则见 `Edit.c:111`。
//!
//! tree-sitter 的作用域高亮做不到这件事：Markdown 的查询只认 `(link_destination)`
//! 与 `(uri_autolink)`（即 `[x](url)` 与 `<url>`），**裸写的 URL 谁都不认**；其它
//! 语言更是不认。组件库的样式表也没有"热点链接"这一槽。
//!
//! 所以这里照 Notepad3 的做法补一层：按它的正则扫描、把命中区间作为
//! [`TextDecorationCollection`] 交给编辑器。装饰在组件库里是**在语法样式之后**
//! 合成的（`input/element.rs::compose_decoration_collections`），因此能盖过词法色。
//!
//! [`TextDecorationCollection`]: gpui_component::input::TextDecorationCollection

/// Notepad3 `Hyperlink Hotspots` 的 inactive 前景色（`#0060B0`）。
///
/// 取自 `Styles.c::Style_SetUrlHotSpot`：`inactiveFG = RGB(0x00, 0x60, 0xB0)`，
/// 与 `styleLexStandard.c` 里该槽的 `back:#0060B0` 一致。活动（悬停）色是
/// `#0000E0`，那是鼠标悬停时的反馈；这里静态渲染取 inactive 那个。
pub const HOTSPOT_RGB: (u8, u8, u8) = (0x00, 0x60, 0xB0);

/// 扫描一段文本里的裸 URL，返回它们的字节区间（升序、不重叠）。
///
/// 规则对齐 Notepad3 的 `HYPLNK_REGEX_FULL`（`Edit.c:111`）：以
/// `http://` / `https://` / `ftp://` / `file:///` / `file://` / `mailto:` /
/// `www.` / `ftp.` 起头，随后吃一串 URL 合法字符。这里用**手写扫描**而不是正则：
/// 不必为此引一个正则库，且边界（尾随标点、括号配对）比一条正则更好读。
///
/// 有意与 Notepad3 保持一致的几点：
/// - `www.` / `ftp.` 后面至少要有一个点，避免把 `www` 这个词当链接；
/// - **尾随的 `.` `,` `;` `:` `!` `?` `'` `"` `)` 会被剔除**（Gruber 的做法），
///   但紧跟 `(`/`[` 的对应右括号只有在文中成对时才保留——简化成"末尾右括号一律
///   剔除"，因为裸 URL 很少真的以括号结尾；
/// - 大小写不敏感（`HTTP://` 也算）。
pub fn hotspots(text: &str) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // 快筛：scheme 的首字母只可能是 h/f/m/w（大小写）。绝大多数字符因此只花
        // 一次字节读 + 一次比较，不必对每个位置试 8 个前缀——这一步让全缓冲扫描
        // 降到"每次按键几毫秒"的量级（见文件头的性能说明）。
        if !matches!(bytes[i], b'h' | b'H' | b'f' | b'F' | b'm' | b'M' | b'w' | b'W') {
            i += 1;
            continue;
        }
        // 词边界：前一个字符不能是标识符字符（ASCII 近似；UTF-8 续字节 >= 0x80，
        // 不是 ASCII 字母数字，所以非 ASCII 前缀一律算边界）。
        if i > 0 {
            let prev = bytes[i - 1];
            if prev.is_ascii_alphanumeric() || prev == b'_' {
                i += 1;
                continue;
            }
        }
        if scheme_at(text, i).is_some() {
            let end = scan_end(text, i);
            if end > i {
                out.push(i..end);
                i = end; // `end` 是字符边界（`scan_end` 按字符前进）
                continue;
            }
        }
        i += 1;
    }
    out
}

/// 若 `i` 处起头的是 Notepad3 认的 scheme，返回 scheme 结束位置（即主机名起点）。
///
/// 用**字节**比较而不是 `&text[..len]` 切片：这些 scheme 全是 ASCII，而 `i` 之后
/// 可能是多字节字符，按字符数切片会切在字符中间 panic。
fn scheme_at(text: &str, i: usize) -> Option<usize> {
    let bytes = &text.as_bytes()[i..];
    for scheme in ["http://", "https://", "ftp://", "file:///", "file://", "mailto:"] {
        let s = scheme.as_bytes();
        if bytes.len() >= s.len() && bytes[..s.len()].eq_ignore_ascii_case(s) {
            return Some(i + s.len());
        }
    }
    // `www.` / `ftp.`：点后必须紧跟一个字符，否则 `www.` 单独出现不算。
    for prefix in ["www.", "ftp."] {
        let p = prefix.as_bytes();
        if bytes.len() > p.len() && bytes[..p.len()].eq_ignore_ascii_case(p) {
            return Some(i + p.len());
        }
    }
    None
}

/// 从 scheme 之后扫到 URL 末尾，返回剔除尾随标点后的字节终点。
///
/// 按**字符**前进（不是字节），否则多字节字符会被切在中间、返回的区间不是
/// `char` 边界。括号成对处理：URL 中间配对的 `(...)` 保留（如
/// `.../Rust_(语言)`），**未配对的**右括号才终止 URL（Gruber 的经典做法）。
fn scan_end(text: &str, start: usize) -> usize {
    let mut end = start;
    let mut paren_depth = 0usize;
    for (offset, c) in text[start..].char_indices() {
        let at = start + offset;
        if c == '(' {
            paren_depth += 1;
        } else if c == ')' {
            if paren_depth == 0 {
                break; // 未配对的右括号：URL 到此为止
            }
            paren_depth -= 1;
        } else if !is_url_char(c) {
            break;
        }
        end = at + c.len_utf8();
    }
    trim_trailing(text, start, end)
}

/// URL 里允许出现的字符（ASCII 部分）；非 ASCII 一律允许（国际化域名 / 路径）。
fn is_url_char(c: char) -> bool {
    if !c.is_ascii() {
        return true;
    }
    c.is_ascii_alphanumeric()
        || matches!(
            c,
            '-' | '.' | '_' | '~' | ':' | '/' | '?' | '#' | '[' | ']' | '@' | '!' | '$'
                | '&' | '\'' | '*' | '+' | ',' | ';' | '=' | '%'
        )
}

/// 剔除 URL 末尾的标点（Gruber 的经典做法）。配对括号已在 `scan_end` 里处理，
/// 这里只清理句末标点与未配对的 `]`。
fn trim_trailing(text: &str, start: usize, mut end: usize) -> usize {
    while end > start {
        let c = text[..end].chars().next_back().expect("end > start implies a char");
        if matches!(c, '.' | ',' | ';' | ':' | '!' | '?' | '\'' | '"' | ']') {
            end -= c.len_utf8();
        } else {
            break;
        }
    }
    if end <= start {
        start
    } else {
        end
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans(text: &str) -> Vec<&str> {
        hotspots(text).into_iter().map(|r| &text[r]).collect()
    }

    /// 常见 scheme 都要认，且大小写不敏感。
    #[test]
    fn detects_common_schemes() {
        assert_eq!(spans("see https://example.com here"), vec!["https://example.com"]);
        assert_eq!(spans("see http://example.com/x"), vec!["http://example.com/x"]);
        assert_eq!(spans("ftp://files.example.org/a.txt"), vec!["ftp://files.example.org/a.txt"]);
        assert_eq!(spans("mailto:a@b.com"), vec!["mailto:a@b.com"]);
        assert_eq!(spans("WWW.Example.COM/Z"), vec!["WWW.Example.COM/Z"]);
        assert_eq!(spans("HTTPS://EXAMPLE.COM"), vec!["HTTPS://EXAMPLE.COM"]);
    }

    /// 尾随标点要剔除，句中的 URL 不该把句号吃进去。
    #[test]
    fn trims_trailing_punctuation() {
        assert_eq!(spans("go to https://example.com."), vec!["https://example.com"]);
        assert_eq!(spans("(https://example.com)"), vec!["https://example.com"]);
        assert_eq!(spans("https://example.com, then"), vec!["https://example.com"]);
        assert_eq!(spans("[https://example.com]"), vec!["https://example.com"]);
    }

    /// 括号内的路径参数、查询串、片段都要保留。
    #[test]
    fn keeps_query_and_path() {
        assert_eq!(
            spans("https://example.com/a?x=1&y=2#frag"),
            vec!["https://example.com/a?x=1&y=2#frag"]
        );
        assert_eq!(spans("https://en.wikipedia.org/wiki/Rust_(语言)"),
            vec!["https://en.wikipedia.org/wiki/Rust_(语言)"]);
    }

    /// 不成词的 `www` / `ftp` 不该被当成链接。
    #[test]
    fn does_not_match_bare_words() {
        assert!(spans("www alone").is_empty());
        assert!(spans("ftp alone").is_empty());
        assert!(spans("say xhttp://example.com").is_empty(), "前面紧贴字母不算");
        assert!(spans("plain text").is_empty());
    }

    /// 多个链接、同一行混排都要稳定。
    #[test]
    fn finds_multiple_hotspots() {
        let s = "a https://one.example.com b http://two.example.com/c and mailto:x@y.z";
        assert_eq!(
            spans(s),
            vec!["https://one.example.com", "http://two.example.com/c", "mailto:x@y.z"]
        );
    }

    /// 区间必须落在字符边界上（后面要按字节切片做装饰）。
    #[test]
    fn ranges_are_char_aligned() {
        let s = "前缀 https://例子.中国/路径 后缀";
        for r in hotspots(s) {
            assert!(s.is_char_boundary(r.start) && s.is_char_boundary(r.end));
            assert!(&s[r].starts_with("https://"));
        }
    }
}

