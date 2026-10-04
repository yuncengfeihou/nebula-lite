//! Markdown 标题解析：给「大纲」面板提供条目。
//!
//! 只认 ATX 标题（`#` 到 `######`），这是 Markdown 里最常用的写法，也是
//! 代码块里唯一不会误判的形态——解析时**跳过围栏代码块**（``` / ~~~），否则
//! 文档里示例代码中的 `# 注释` 会被列成标题。
//!
//! 刻意不引 markdown 解析库：这里要的是"行号 + 层级 + 文本"这三样，一个
//! 逐行扫描就够了，而组件库的 `TextView` 走的是它自己的 AST，两边对不上也没
//! 关系——大纲只用来**跳转**（把源码光标的行移过去），不参与渲染。
//!
//! 已知取舍：不识别 Setext 标题（下一行是 `===` / `---` 的那种）。它少见，
//! 且 `---` 还兼任分隔线，识别错会把正文行当成标题；宁缺勿错。

/// 一个大纲条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Heading {
    /// 1–6，对应 `#` 的个数。
    pub level: usize,
    /// 标题文本（去掉前导 `#` 与收尾的 `#`，两端去空白）。
    pub text: String,
    /// 0 基行号，用来把源码光标移过去。
    pub line: usize,
}

/// 扫描 `text` 里的 ATX 标题，按出现顺序返回。
///
/// 空文本、没有标题、纯代码块里的 `#` 都返回空列表——调用侧据"是否为空"决定
/// 要不要显示大纲面板。
pub fn headings(text: &str) -> Vec<Heading> {
    let mut out = Vec::new();
    let mut fence: Option<&str> = None;

    for (index, raw_line) in text.lines().enumerate() {
        let line = raw_line.trim_end();
        let trimmed = line.trim_start();

        // 围栏代码块：开栅栏记下标记（``` 或 ~~~），后续行全部跳过，直到同种
        // 标记再出现。缩进最多 3 空格的栅栏才算（与 CommonMark 一致）。
        let indent = line.len() - trimmed.len();
        let marker = if indent <= 3 {
            if trimmed.starts_with("```") {
                Some("```")
            } else if trimmed.starts_with("~~~") {
                Some("~~~")
            } else {
                None
            }
        } else {
            None
        };
        if let Some(marker) = marker {
            match fence {
                // 同种标记 = 闭合；别种标记在围栏内是普通内容。
                Some(open) if open == marker => fence = None,
                None => fence = Some(marker),
                Some(_) => {},
            }
            continue;
        }
        if fence.is_some() {
            continue;
        }

        // 缩进 4 空格及以上是缩进代码块（CommonMark），里面以 `#` 开头的行是
        // 注释或示例，不是标题。这个检查和围栏是两回事：不能因为缩进就跳过
        // 围栏标记的识别（围栏只允许 ≤3 空格缩进），所以放在围栏判断之后。
        if indent > 3 {
            continue;
        }

        if let Some(heading) = parse_atx(trimmed, index) {
            out.push(heading);
        }
    }

    out
}

/// 解析单行 ATX 标题；不是标题返回 `None`。
///
/// CommonMark 要求 `#` 后面是空格或行尾，所以 `#hashtag` 不是标题、
/// `# 标题` 才是。
fn parse_atx(trimmed: &str, line: usize) -> Option<Heading> {
    let hashes = trimmed.len() - trimmed.trim_start_matches('#').len();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let rest = &trimmed[hashes..];
    // `#` 之后必须是空格/制表/行尾；`#x` 是普通文本。
    if !rest.is_empty() && !rest.starts_with([' ', '\t']) {
        return None;
    }
    // 收尾的 `#`（闭合式 `## 标题 ##`）连同前面的空白一起去掉。
    let text = rest.trim().trim_end_matches('#').trim_end();
    Some(Heading { level: hashes, text: text.to_owned(), line })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_levels_and_text() {
        let source = "# 一级\n\n## 二级\n\n###### 六级\n";
        let found = headings(source);
        assert_eq!(
            found,
            vec![
                Heading { level: 1, text: "一级".into(), line: 0 },
                Heading { level: 2, text: "二级".into(), line: 2 },
                Heading { level: 6, text: "六级".into(), line: 4 },
            ]
        );
    }

    /// 七个 `#` 不是标题（CommonMark：最多六级）；`#紧贴文字` 也不是标题。
    #[test]
    fn rejects_more_than_six_hashes_and_missing_space() {
        assert!(headings("####### 七级\n").is_empty());
        assert!(headings("#hashtag 不是标题\n").is_empty());
        assert_eq!(headings("#\n").len(), 1, "只有 # 一行是空标题，仍是标题");
    }

    /// 围栏代码块里的 `#` 是注释或示例，不能当作标题。
    #[test]
    fn ignores_hash_inside_fenced_code_blocks() {
        let source = "```bash\n# 这是注释\n```\n\n# 真标题\n";
        let found = headings(source);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].text, "真标题");
        assert_eq!(found[0].line, 4);
    }

    /// ～～～ 围栏与 ``` 是两种标记，不能互相闭合。
    #[test]
    fn tilde_and_backtick_fences_do_not_close_each_other() {
        let source = "~~~\n# 在波浪围栏里\n```\n# 还在围栏里\n~~~\n# 出来了\n";
        let found = headings(source);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].text, "出来了");
    }

    /// 闭合式标题的收尾 `#` 要去掉；行尾空白不影响。
    #[test]
    fn strips_closing_hashes_and_trailing_space() {
        let found = headings("## 标题 ##   \n");
        assert_eq!(found[0].text, "标题");
    }

    /// 缩进 4 空格的 `#` 是缩进代码块，不是标题。
    #[test]
    fn indented_four_spaces_is_code_not_a_heading() {
        assert!(headings("    # 缩进代码\n").is_empty());
    }
}
