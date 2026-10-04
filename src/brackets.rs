//! 括号匹配：光标贴着一个括号时，找出与它配对的那一个（Notepad3 的
//! `SciCall_BraceMatch` / Scintilla 的 `SCI_BRACEMATCH` 行为）。
//!
//! 这是一个**纯函数**模块，只吃文本与光标字节偏移，返回两个括号的字节区间，
//! 不碰 GUI —— 匹配规则可以单测，UI 侧（`app.rs`）只负责把结果转成装饰。
//!
//! 三条规则，对齐 Notepad3 的默认行为：
//! - **先看光标处、再看光标前一个字符**：Notepad3 的 `Edit.c` 是先
//!   `BraceMatch(iCurPos)`、失败再 `BraceMatch(iPosBefore)`，所以光标停在
//!   `}` 右边时也能匹配到它。
//! - **按同类型计数，不建栈**：Scintilla 的 `SCI_BRACEMATCH` 只数**同一种**括号
//!   （匹配 `{` 时完全忽略 `(` / `[`），所以 `{]}` 里 `{` 会跨过 `]` 去配后面的
//!   `}`。这与"严格嵌套栈"不同，但那是 Notepad3 的实际行为。
//! - **字符串与注释里的括号不算**：Scintilla 走 lexer，会跳过 `'...'` / `"..."`
//!   / `` `...` `` 与 `//` / `/* */` 里的括号。这里用一次 C 系词法扫描近似它——
//!   覆盖不到的语言（例如用 `#` 注释的）会退化成"把注释里的括号也算进去"，
//!   那只是少标或错标一次，不会崩。
//!
//! 复杂度：只有当光标**确实贴着某个括号**时才做那趟词法扫描（见 [`matching`]
//! 的前置判断），且只扫一遍 `O(n)`；光标在普通字符上时是 O(1) 直接返回。

use std::ops::Range;

/// 三对括号。开括号 → 闭括号。
const PAIRS: [(u8, u8); 3] = [(b'(', b')'), (b'[', b']'), (b'{', b'}')];

/// 给定文本与光标字节偏移，返回 `(光标侧的括号区间, 配对括号区间)`。
///
/// 找不到配对（跨到文件头尾、类型不匹配）时返回 `None`。
/// 返回的区间是**单个字符**的字节范围（括号都是 ASCII，1 字节）。
///
/// 光标不在括号上时是 O(1)（先做这个判断，再决定要不要扫全文）。
pub fn matching(text: &str, cursor: usize) -> Option<(Range<usize>, Range<usize>)> {
    let (here, ch) = pick_bracket(text, cursor)?;
    let (open, close) = pair_of(ch)?;
    let match_pos = if ch == open {
        match_forward(text, here, open, close)?
    } else {
        match_backward(text, here, open, close)?
    };
    Some((here..here + 1, match_pos..match_pos + 1))
}

/// 光标侧那个括号的字节位置与字符：先看光标处，再看前一个字符。
fn pick_bracket(text: &str, cursor: usize) -> Option<(usize, u8)> {
    let bytes = text.as_bytes();
    if cursor < bytes.len() && is_bracket(bytes[cursor]) {
        return Some((cursor, bytes[cursor]));
    }
    // 光标停在 `}` 右边（即 `cursor == close + 1`）时，回看一个字符。
    if cursor > 0 && is_bracket(bytes[cursor - 1]) {
        return Some((cursor - 1, bytes[cursor - 1]));
    }
    None
}

fn is_bracket(c: u8) -> bool {
    PAIRS.iter().any(|(o, cl)| c == *o || c == *cl)
}

/// 找到 `ch` 所属的那一对 `(open, close)`。`ch` 一定是括号。
fn pair_of(ch: u8) -> Option<(u8, u8)> {
    PAIRS.iter().copied().find(|(o, c)| ch == *o || ch == *c)
}

/// 从开括号 `here` 向后、按同类型计数找配对的闭括号。
fn match_forward(text: &str, here: usize, open: u8, close: u8) -> Option<usize> {
    let events = bracket_events(text, open, close);
    let mut depth = 0i32;
    for (pos, is_open) in events.into_iter().filter(|(p, _)| *p >= here) {
        if pos == here {
            depth = 1;
            continue;
        }
        if is_open {
            depth += 1;
        } else {
            depth -= 1;
            if depth == 0 {
                return Some(pos);
            }
        }
    }
    None
}

/// 从闭括号 `here` 向前、按同类型计数找配对的开括号。
fn match_backward(text: &str, here: usize, open: u8, close: u8) -> Option<usize> {
    let events = bracket_events(text, open, close);
    let mut depth = 0i32;
    for (pos, is_open) in events.into_iter().filter(|(p, _)| *p <= here).rev() {
        if pos == here {
            depth = 1;
            continue;
        }
        if !is_open {
            depth += 1;
        } else {
            depth -= 1;
            if depth == 0 {
                return Some(pos);
            }
        }
    }
    None
}

/// 一次词法扫描，按位置升序返回**不在**字符串 / 注释里的 `open` 或 `close`
/// 括号，带上"是开括号吗"。
///
/// 只收这一次扫描关心的那一对括号，别的一律略过——省内存，也让调用侧不用再过滤。
fn bracket_events(text: &str, open: u8, close: u8) -> Vec<(usize, bool)> {
    let bytes = text.as_bytes();
    let mut events = Vec::new();
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut string_delim: Option<u8> = None;
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        if in_line_comment {
            if c == b'\n' {
                in_line_comment = false;
            }
            i += 1;
            continue;
        }
        if in_block_comment {
            if c == b'*' && bytes.get(i + 1) == Some(&b'/') {
                in_block_comment = false;
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        if let Some(delim) = string_delim {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == delim {
                string_delim = None;
            }
            i += 1;
            continue;
        }
        match c {
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                in_line_comment = true;
                i += 2;
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                in_block_comment = true;
                i += 2;
            }
            b'\'' | b'"' | b'`' => {
                string_delim = Some(c);
                i += 1;
            }
            _ => {
                if c == open {
                    events.push((i, true));
                } else if c == close {
                    events.push((i, false));
                }
                i += 1;
            }
        }
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_forward_from_an_opening_bracket() {
        let src = "fn f() { let x = (1 + 2); }";
        let open = src.find('{').unwrap();
        let close = src.rfind('}').unwrap();
        assert_eq!(matching(src, open), Some((open..open + 1, close..close + 1)));
    }

    #[test]
    fn matches_backward_from_a_closing_bracket() {
        let src = "{ a }";
        let close = src.rfind('}').unwrap();
        assert_eq!(matching(src, close), Some((close..close + 1, 0..1)));
    }

    #[test]
    fn cursor_just_after_the_bracket_still_matches() {
        // Notepad3 会先试光标处、再试光标前一个字符。
        let src = "{}";
        assert_eq!(matching(src, 0), Some((0..1, 1..2)), "光标在开括号上");
        assert_eq!(matching(src, 1), Some((1..2, 0..1)), "光标停在闭括号左缘");
        // 光标在文本末尾（闭括号右边一格）：先看光标处没有字符，再回看闭括号。
        assert_eq!(matching(src, 2), Some((1..2, 0..1)), "光标在闭括号右侧");
    }

    #[test]
    fn counts_only_the_same_bracket_type() {
        // Scintilla 的 SCI_BRACEMATCH 只数同一种括号：这里 `{` 应跨过 `]`。
        let src = "{] }";
        let open = 0;
        let close = src.rfind('}').unwrap();
        assert_eq!(matching(src, open), Some((open..open + 1, close..close + 1)));
    }

    #[test]
    fn nested_same_type_pairs_by_depth() {
        let src = "{{}}";
        assert_eq!(matching(src, 0), Some((0..1, 3..4)));
        let (a, b) = matching(src, 1).unwrap();
        assert_eq!((a, b), (1..2, 2..3));
    }

    #[test]
    fn brackets_inside_strings_and_comments_are_ignored() {
        let src = "fn f() { let s = \"}\"; }";
        let open = src.find('{').unwrap();
        let (_, b) = matching(src, open).expect("应当跳过字符串里的 }");
        assert_eq!(b.start, src.rfind('}').unwrap());

        let src = "fn f() { // }\n}";
        let open = src.find('{').unwrap();
        let (_, b) = matching(src, open).expect("应当跳过行注释里的 }");
        assert_eq!(b.start, src.rfind('}').unwrap());

        let src = "f(/* ) */)";
        let (_, b) = matching(src, 1).expect("应当跳过块注释里的 )");
        assert_eq!(b.start, src.len() - 1);
    }

    #[test]
    fn an_unmatched_bracket_yields_nothing() {
        // `{` 在索引 7（未闭合）。
        assert!(matching("fn f() {", 7).is_none(), "只有开括号时不该报匹配");
        assert!(matching("}", 0).is_none(), "只有闭括号时不该报匹配");
        assert!(matching("let x = 1;", 4).is_none(), "普通字符不该匹配");
    }

    #[test]
    fn multibyte_text_keeps_byte_offsets_valid() {
        // 中文注释在括号前面，字节偏移不能错位。
        let src = "// 中文注释\n{ 值 }";
        let open = src.find('{').unwrap();
        let close = src.find('}').unwrap();
        assert_eq!(matching(src, open), Some((open..open + 1, close..close + 1)));
    }

    #[test]
    fn cursor_in_the_middle_of_a_multibyte_char_is_safe() {
        // 光标若落在多字节字符中间，不应 panic，也不该误判。
        let src = "中{中}";
        let open = src.find('{').unwrap();
        assert_eq!(matching(src, open), Some((open..open + 1, src.find('}').unwrap()..src.find('}').unwrap() + 1)));
        // 落在一个多字节字符内部（字节 1 是『中』的续字节）。
        assert_eq!(matching(src, 1), None);
    }
}
