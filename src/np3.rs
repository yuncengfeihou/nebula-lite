//! Notepad3 的两层补充着色：**前景装饰**与**背景色块**，都是组件库与 tree-sitter
//! 覆盖不到的槽。
//!
//! ## 为什么会有这一层
//!
//! 组件库的 `SyntaxColors` 只有 41 个固定捕获名，且 `ThemeStyle` 里**没有背景色
//! 字段**；代码编辑器的那条文字绘制路径（`gpui::ShapedLine::paint`）也只画字形与
//! 下划线，从不调 `paint_background`。于是 Notepad3 里两类槽在这套管线下拿不到：
//!
//! 1. **带 `back:` 的槽**（Markdown 代码 `back:#EBEBEB`、Markdown 标题条、
//!    批处理变量 `back:#FFF1A8`、INI 段落名 `back:#FF8040`）——主题装不下、
//!    编辑器也不画。这一层由 `app.rs` 在编辑器**下方**垫一张 canvas 画出来。
//! 2. **tree-sitter 语法表达不出来的前景色**，批处理最典型：Notepad3 的
//!    `SCE_BAT_WORD`（内部命令）/ `SCE_BAT_COMMAND`（外部命令）/ `SCE_BAT_OPERATOR`
//!    是它自己按关键字表与分隔符集合逐词判的；`tree-sitter-batch` 把 `if` / `call` /
//!    `goto` 这些词整个 inline 掉了（`to_sexp` 里根本看不到），查询又只能整条语句地捕
//!    （`(if_stmt) @keyword`），颜色必然对不上。这一层由 `app.rs` 塞进
//!    `TextDecorationCollection`（装饰在语法样式**之后**合成，能盖过 tree-sitter）。
//!
//! 所以照 `urls.rs` / `brackets.rs` 的定位补一个**纯函数层**：这里只算"哪一段该是
//! 什么前景 / 底色"，不碰 GUI。
//!
//! ## 覆盖范围
//!
//! - `batch`（.bat / .cmd）：前景逐词判色 + 变量 / 标签底色（`styleLexBAT.c`）。
//! - `markdown`：代码 / 标题底色与标题前景（`styleLexMARKDOWN.c`）。
//! - `ini`：段落名底色（`styleLexPROPS.c`）。
//!
//! 未收录的语言返回空——它们的前景由 tree-sitter 查询负责，也没有背景槽。

use std::ops::Range;

/// 一段着色：前景色 / 背景色 / 是否加粗。
///
/// `eol = true` 表示底色要**铺到行尾**（Notepad3 的 `eolfilled`），绘制侧据此把
/// 矩形右边界推到容器右缘。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ink {
    pub fg: Option<(u8, u8, u8)>,
    pub bg: Option<(u8, u8, u8)>,
    pub bold: bool,
    pub eol: bool,
}

impl Ink {
    const fn fg(rgb: (u8, u8, u8)) -> Self {
        Self { fg: Some(rgb), bg: None, bold: false, eol: false }
    }

    const fn bold(rgb: (u8, u8, u8)) -> Self {
        Self { fg: Some(rgb), bg: None, bold: true, eol: false }
    }

    const fn bg(rgb: (u8, u8, u8)) -> Self {
        Self { fg: None, bg: Some(rgb), bold: false, eol: false }
    }
}

/// 一段着色区间。`range` 一定落在同一行内：背景层逐行铺，跨行区间会被
/// `range_to_bounds` 画成包围盒（见 `app.rs` 的绘制说明）。
///
/// `row` 是**缓冲行号**（0 基）。绘制侧拿它与 `InputState::visible_row_range()`
/// 求交，只对可见行的段调 `range_to_bounds`——否则每一帧要为全文所有段各算一次
/// 位置（几十 KB 的 Markdown 有上千段），滚动时白烧 CPU。
///
/// `width_from` 是该段**宽度来源**的字节区间（`None` = 用段自身宽度）。围栏代码块
/// 靠它把整块拉成"一个矩形"：块内每一行都取**块里最宽那行**的像素宽度，于是所有行
/// 右缘对齐、上下行高连续，看上去就是一整块矩形，而不是参差不齐的逐行底色。
/// "最宽"按**显示列数**算（见 `widen_block`），不能按字符数——CJK 注释行字少但更宽。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Span {
    pub range: Range<usize>,
    pub ink: Ink,
    pub row: usize,
    pub width_from: Option<Range<usize>>,
}

/// 该语言是否需要这一层。渲染侧据此免掉 canvas 与装饰集合的开销。
pub fn has_overlay(language: &str) -> bool {
    matches!(language, "batch" | "markdown" | "ini")
}

/// 计算某语言缓冲的全部着色段（升序、不重叠、逐行）。
pub fn spans(language: &str, text: &str) -> Vec<Span> {
    match language {
        "batch" => batch(text),
        "markdown" => markdown(text),
        "ini" => ini(text),
        _ => Vec::new(),
    }
}

/// 逐行遍历，给出 `(行号, 行首字节, 去掉行尾换行后的行末字节)`。
///
/// 是**迭代器**而不是 `Vec`：这份列表"每行一项"，2 MiB 的 .bat 约 4.7 万行，摊成
/// `Vec<(usize, usize, usize)>` 就是每次重算白分配 1.1 MB（三个语言层 markdown / ini /
/// batch 都只用顺序遍历，不需要随机访问）。行尾的 `\n` / `\r` 按字节判断，不走
/// `trim_end_matches` 的 char 模式机。
fn lines(text: &str) -> impl Iterator<Item = (usize, usize, usize)> + '_ {
    let mut offset = 0usize;
    text.split_inclusive('\n').enumerate().map(move |(row, line)| {
        let start = offset;
        offset += line.len();
        (row, start, start + pre_newline_len(line))
    })
}

/// 行尾换行（`\n` / `\r`）之前的字节长度。
fn pre_newline_len(line: &str) -> usize {
    let bytes = line.as_bytes();
    let mut end = bytes.len();
    while end > 0 && matches!(bytes[end - 1], b'\n' | b'\r') {
        end -= 1;
    }
    end
}

fn span(row: usize, start: usize, end: usize, ink: Ink) -> Span {
    Span { range: start..end, ink, row, width_from: None }
}

// ------------------------------------------------------------------ Markdown

/// Markdown `Code` 槽的底色（`styleLexMARKDOWN.c`：`fore:#00007F; back:#EBEBEB`）。
/// 前景 `#00007F` 由 tree-sitter 的 `text.code.span` / `text.literal` 负责，这里只管底色。
pub const MARKDOWN_CODE_BG: (u8, u8, u8) = (0xEB, 0xEB, 0xEB);

/// Markdown 六级标题的 `(前景, 条带底色)`，逐值取自 `styleLexMARKDOWN.c` 的
/// `SCE_MARKDOWN_HEADER1..6`。
///
/// Notepad3 把整行标题套同一个级别样式——它的 `EditLexer.c:261` 把
/// `lexer.markdown.header.eolfill` 设成 `1`，于是 `LexMarkdown.cxx` 走
/// `sc.SetState(SCE_MARKDOWN_HEADERn)` 那条分支，整行（含正文）都吃这个槽，
/// 底色 `eolfilled` 铺到行尾。所以这里给**整行**前景 + 底色，而不是只给 `#` 标记：
/// 只给标记的话正文会留在 tree-sitter 的 `@title`（`#336193`），H1 / H5 / H6
/// 的正文颜色就和 Notepad3 对不上了。
///
/// （`SCE_MARKDOWN_HDRTEXT` 那个槽在当前 Notepad3 里没有代码发它，是历史遗留；
/// tree-sitter 的 `@title` 正好给了同一个 `#336193`，所以正文不改也一致——除了
/// H1/H5/H6，见上。）
const MARKDOWN_HEADINGS: [((u8, u8, u8), (u8, u8, u8)); 6] = [
    ((0xFF, 0xFF, 0xE2), (0x8B, 0xA4, 0xED)), // H1
    (MARKDOWN_HDRTEXT, (0x9D, 0xCE, 0xFF)),   // H2
    (MARKDOWN_HDRTEXT, (0xD9, 0xEC, 0xFF)),   // H3
    (MARKDOWN_HDRTEXT, (0xFF, 0xFF, 0xFF)),   // H4（无 back:）
    ((0x3F, 0x77, 0xB6), (0xFF, 0xFF, 0xFF)), // H5
    ((0x5C, 0x8F, 0xC7), (0xFF, 0xFF, 0xFF)), // H6
];

/// Notepad3 的 `Header Text` 槽色（`fore:#336193`）。H2-H4 的级别前景就是它，
/// tree-sitter 的 `@title` 也正好取这个值（`syntax.rs` 里 Markdown 的 `Title`）。
const MARKDOWN_HDRTEXT: (u8, u8, u8) = (0x33, 0x61, 0x93);

/// Markdown 的槽：围栏代码块整块铺 `#EBEBEB`（**整块一个矩形**）；行内代码逐段
/// 铺同色底；ATX 标题按级别铺彩条。
///
/// 对齐 Notepad3 `LexMarkdown.cxx`：
/// - `` ``` `` / `~~~`（行首、≤3 空格缩进、≥3 个）开一个围栏块，直到同字符、
///   不短于开栏、其后只有空白的收栏行；块内一律按代码处理（标题不认）。
/// - 行内 `` ` `` / ``` `` ```（成对等长）之间的内容算行内代码。
/// - 行首 1-6 个 `#` 后跟空白或行尾是 ATX 标题；标题行按级别取色。
///
/// 围栏块用 [`widen_block`] 把块内每行的矩形宽度统一成**块里最宽那一行**的宽度，
/// 于是右缘对齐、逐行矩形无缝拼成一整块——这正是用户要的"一个矩形代码块区域"。
///
/// **有意保留的近似**（都在下面注释里）：Setext 标题（下划线式）不认；反斜杠
/// 转义的反引号不算开栏。
fn markdown(text: &str) -> Vec<Span> {
    let mut out = Vec::new();
    // 围栏状态：(围栏字符, 长度, 块里第一段在 `out` 的下标)。收栏时按它把整块拉齐。
    let mut fence: Option<(u8, usize, usize)> = None;
    for (row, start, end) in lines(text) {
        let body = &text[start..end];
        let indent = body.len() - body.trim_start_matches([' ', '\t']).len();
        let rest = &body[indent..];

        if fence.is_some() || indent <= 3 {
            if let Some((ch, len, first)) = fence {
                let run = rest.bytes().take_while(|&c| c == ch).count();
                out.push(span(row, start, end, Ink::bg(MARKDOWN_CODE_BG)));
                // 收栏：同字符、不短于开栏、其余只有空白。
                if run >= len && rest[run..].trim().is_empty() {
                    widen_block(&mut out, first, text);
                    fence = None;
                }
                continue;
            } else if indent <= 3 {
                if let Some((ch, len)) = fence_open(rest) {
                    out.push(span(row, start, end, Ink::bg(MARKDOWN_CODE_BG)));
                    fence = Some((ch, len, out.len() - 1));
                    continue;
                }
                if let Some(level) = atx_level(rest) {
                    let (fg, band) = MARKDOWN_HEADINGS[level - 1];
                    // 整行一个 span：级别前景 + 条带底色（`eolfilled` 由绘制侧推到行尾）。
                    out.push(span(
                        row,
                        start,
                        end,
                        Ink { fg: Some(fg), bg: Some(band), bold: true, eol: true },
                    ));
                    continue;
                }
            }
        }
        if fence.is_some() {
            out.push(span(row, start, end, Ink::bg(MARKDOWN_CODE_BG)));
            continue;
        }
        inline_code(row, text, start, end, &mut out);
    }
    // 收尾时仍未闭合的围栏：把已有的部分也拉成整块。
    if let Some((_, _, first)) = fence {
        widen_block(&mut out, first, text);
    }
    out
}

/// 把 `out[first..]` 这批围栏代码行统一成"整块一个矩形"：每段的 `width_from`
/// 都指向块内**内容最宽**的那一行。绘制侧据此把每行的矩形都按最宽行的像素宽度
/// 铺开，右缘对齐、上下连续，看上去就是一整块矩形（而不是参差不齐的逐行底色）。
///
/// **"最宽"按显示列数（`display_width`）算，不按字符数**：CJK / 全角字符占两列，
/// 一行 `x # 中文…` 的字符数可能少于纯 ASCII 行、像素却更宽。按 `chars().count()`
/// 挑会挑错行，那行就伸出矩形（用户报的"有注释时不是矩形"）。
fn widen_block(out: &mut [Span], first: usize, text: &str) {
    let widest = out[first..]
        .iter()
        .max_by_key(|span| display_width(&text[span.range.clone()]))
        .map(|span| span.range.clone());
    if let Some(widest) = widest {
        for span in &mut out[first..] {
            span.width_from = Some(widest.clone());
        }
    }
}

/// 一行文本的**显示列数**：ASCII / 半角记 1 列，CJK 与全角标点记 2 列。
///
/// 只用于在围栏块里挑"最宽那行"，不需要与渲染字体逐像素吻合——只要宽度序与
/// 等宽字体的实际排布一致即可（Maple Mono 对 CJK 走双宽回退）。控制字符与
/// 组合记号按 0 计，避免把不可见字符算进去。
fn display_width(line: &str) -> usize {
    line.chars().map(char_columns).sum()
}

/// 单个字符占的列数（等宽字体下的近似）。
fn char_columns(c: char) -> usize {
    match c as u32 {
        // 组合记号 / 零宽 / 控制字符：不占列。
        0x0000..=0x001F | 0x007F..=0x009F | 0x0300..=0x036F | 0x200B..=0x200F => 0,
        // CJK 统一表意文字、CJK 标点、全角形式、假名、谚文、全角符号等：两列。
        0x1100..=0x115F
        | 0x2E80..=0x303E
        | 0x3041..=0x33FF
        | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        | 0xA000..=0xA4CF
        | 0xAC00..=0xD7A3
        | 0xF900..=0xFAFF
        | 0xFE30..=0xFE4F
        | 0xFF00..=0xFF60
        | 0xFFE0..=0xFFE6
        | 0x1F300..=0x1FAFF
        | 0x20000..=0x3FFFD => 2,
        _ => 1,
    }
}

/// 行首的 ATX 标题级别（1-6）；`#` 后必须是空白或行尾。
fn atx_level(rest: &str) -> Option<usize> {
    let hashes = rest.bytes().take_while(|&c| c == b'#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    match rest.as_bytes().get(hashes) {
        None => Some(hashes),
        Some(&c) if c == b' ' || c == b'\t' => Some(hashes),
        _ => None,
    }
}

fn fence_open(rest: &str) -> Option<(u8, usize)> {
    let bytes = rest.as_bytes();
    let ch = *bytes.first()?;
    if ch != b'`' && ch != b'~' {
        return None;
    }
    let run = bytes.iter().take_while(|&&c| c == ch).count();
    if run < 3 {
        return None;
    }
    // 反引号围栏的信息串里不能再出现反引号（CommonMark）。
    if ch == b'`' && bytes[run..].contains(&b'`') {
        return None;
    }
    Some((ch, run))
}

fn inline_code(row: usize, text: &str, start: usize, end: usize, out: &mut Vec<Span>) {
    let line = &text[start..end];
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'`' && !is_escaped(bytes, i) {
            let run = bytes[i..].iter().take_while(|&&c| c == b'`').count();
            if let Some(close) = closing_run(bytes, i + run, run) {
                out.push(span(
                    row,
                    start + i,
                    start + close + run,
                    Ink::bg(MARKDOWN_CODE_BG),
                ));
                i = close + run;
                continue;
            }
            i += run;
            continue;
        }
        i += 1;
    }
}

fn closing_run(bytes: &[u8], from: usize, len: usize) -> Option<usize> {
    let mut j = from;
    while j < bytes.len() {
        if bytes[j] == b'`' {
            let run = bytes[j..].iter().take_while(|&&c| c == b'`').count();
            if run == len {
                return Some(j);
            }
            j += run;
        } else {
            j += 1;
        }
    }
    None
}

/// `bytes[i]` 是否被奇数个连续反斜杠转义。
fn is_escaped(bytes: &[u8], i: usize) -> bool {
    let mut backslashes = 0;
    let mut j = i;
    while j > 0 && bytes[j - 1] == b'\\' {
        backslashes += 1;
        j -= 1;
    }
    backslashes % 2 == 1
}

// ------------------------------------------------------------------ INI

/// INI 段落名 `[section]` 整行铺 `#FF8040`（`styleLexPROPS.c` 的 `Section`，`eolfilled`）。
const INI_SECTION_BG: (u8, u8, u8) = (0xFF, 0x80, 0x40);

fn ini(text: &str) -> Vec<Span> {
    let mut out = Vec::new();
    for (row, start, end) in lines(text) {
        let body = &text[start..end];
        if body.trim_start().starts_with('[') && body.contains(']') {
            out.push(span(row, start, end, Ink { fg: None, bg: Some(INI_SECTION_BG), bold: false, eol: true }));
        }
    }
    out
}

// ------------------------------------------------------------------ Batch

/// 批处理各槽的色值，逐值取自 `styleLexBAT.c`：
///
/// | 槽 | 值 |
/// | --- | --- |
/// | Comment | `fore:#008000` |
/// | Keyword（内部命令） | `bold; fore:#0A246A` |
/// | Identifier（变量） | `fore:#003CE6; back:#FFF1A8` |
/// | Operator | `fore:#B000B0` |
/// | Command（外部命令 / `@`） | `bold`（黑粗体） |
/// | Label | `fore:#C80000; back:#F4F4F4` |
/// | After Label | `fore:#00ACAC` |
const BAT_COMMENT: (u8, u8, u8) = (0x00, 0x80, 0x00);
const BAT_KEYWORD: (u8, u8, u8) = (0x0A, 0x24, 0x6A);
const BAT_IDENT: (u8, u8, u8) = (0x00, 0x3C, 0xE6);
const BAT_OPERATOR: (u8, u8, u8) = (0xB0, 0x00, 0xB0);
const BAT_LABEL: (u8, u8, u8) = (0xC8, 0x00, 0x00);
const BAT_AFTER_LABEL: (u8, u8, u8) = (0x00, 0xAC, 0xAC);
const BAT_IDENT_BG: (u8, u8, u8) = (0xFF, 0xF1, 0xA8);
const BAT_LABEL_BG: (u8, u8, u8) = (0xF4, 0xF4, 0xF4);
const BLACK: (u8, u8, u8) = (0x00, 0x00, 0x00);

/// `styleLexBAT.c` 的 `KeyWords_BAT`（内部命令 + 常用外部工具），全部小写。
const BAT_KEYWORDS: &[&str] = &[
    "arp", "assoc", "attrib", "bcdedit", "bootcfg", "break", "cacls", "call", "cd", "change",
    "chcp", "chdir", "chkdsk", "chkntfs", "choice", "cipher", "cleanmgr", "cls", "cmd", "cmdkey",
    "color", "com", "comp", "compact", "con", "convert", "copy", "country", "ctty", "date",
    "defined", "defrag", "del", "dir", "disabledelayedexpansion", "disableextensions", "diskcomp",
    "diskcopy", "diskpart", "do", "doskey", "driverquery", "echo", "echo.", "else",
    "enabledelayedexpansion", "enableextensions", "endlocal", "equ", "erase", "errorlevel",
    "exist", "exit", "expand", "fc", "find", "findstr", "for", "forfiles", "format", "fsutil",
    "ftp", "ftype", "geq", "goto", "goto:eof", "gpresult", "gpupdate", "graftabl", "gtr", "help",
    "icacls", "if", "in", "ipconfig", "kill", "label", "leq", "loadfix", "loadhigh", "logman",
    "logoff", "lpt", "lss", "md", "mem", "mkdir", "mklink", "mode", "more", "move", "msg",
    "msiexe", "nbtstat", "neq", "net", "netsh", "netstat", "not", "nslookup", "nul", "openfiles",
    "path", "pathping", "pause", "perfmon", "popd", "powercfg", "print", "prompt", "pushd", "rd",
    "recover", "reg", "regedit", "regsvr32", "rem", "ren", "rename", "replace", "rmdir",
    "robocopy", "route", "runas", "rundll32", "sc", "schtasks", "sclist", "set", "setlocal",
    "sfc", "shift", "shutdown", "sort", "start", "subst", "systeminfo", "taskkill", "tasklist",
    "time", "timeout", "title", "tracert", "tree", "type", "typeperf", "ver", "verify", "vol",
    "wmic", "xcopy",
];

/// `echo` / `goto` / `prompt` 之后的整行按原样文本处理（Notepad3 的 `continueProcessing`
/// 一关，后面的词不再判关键字，避免把 `echo set` 里的 `set` 也染成关键字）。
const BAT_PLAIN_TAIL: &[&str] = &["echo", "goto", "prompt"];

/// 这些关键字之后的下一个词落回"命令位置"（黑粗体），对应 Notepad3 里 `cmdLoc`
/// 被重置的那几个（`call` / `do` / `start` / `loadhigh` / `lh`）。其余关键字
/// （尤其 `if`）之后不重开命令位置——所以 `if 1==2` 里的 `1` 是普通文本。
const BAT_OPENS_COMMAND: &[&str] = &["call", "do", "start", "loadhigh", "lh"];

fn batch(text: &str) -> Vec<Span> {
    let mut out = Vec::new();
    for (row, start, end) in lines(text) {
        let body = &text[start..end];
        let indent = body.len() - body.trim_start_matches([' ', '\t']).len();
        let rest = &body[indent..];

        // `::` 是注释（假标签）；行首 `rem` 也是注释。整行染绿。
        if rest.starts_with("::") || starts_with_rem(rest) {
            if !rest.is_empty() {
                out.push(span(row, start + indent, end, Ink::fg(BAT_COMMENT)));
            }
            continue;
        }
        // 真标签：`:name`，直到标签终止符；其后文字是 "After Label"（青，eolfilled 灰底）。
        if rest.starts_with(':') {
            // 从**第 2 个字符**起找终止符：`:` 本身也在终止符集合里，从头找会得到空标签。
            let name_end = rest[1..]
                .find(|c: char| matches!(c, '\t' | ' ' | '&' | '+' | ':' | '<' | '>' | '|'))
                .map(|p| p + 1)
                .unwrap_or(rest.len());
            out.push(span(
                row,
                start + indent,
                start + indent + name_end,
                Ink { fg: Some(BAT_LABEL), bg: Some(BAT_LABEL_BG), bold: false, eol: name_end == rest.len() },
            ));
            if name_end < rest.len() {
                out.push(span(row, start + indent + name_end, end, Ink::fg(BAT_AFTER_LABEL)));
            }
            continue;
        }
        tokenize_line(row, text, start, indent, end, &mut out);
    }
    out
}

/// 行首是否是 `rem` 注释（`rem` 后必须是空白、行尾或 `.`；`remove` 不算）。
///
/// 按字节做大小写无关的前缀比较，**不**整行 `to_ascii_lowercase()`：那会给每一行分配
/// 一个 String，是 `spans()` 在大 .bat 上变贵的两个来源之一（见 [`lower_word`]）。
fn starts_with_rem(rest: &str) -> bool {
    let bytes = rest.as_bytes();
    // `bytes[..3]` 与 `rem` 相等 ⇒ 前三个字节都是 ASCII，于是 `rest[3..]` 落在字符边界上。
    if bytes.len() < 3 || !bytes[..3].eq_ignore_ascii_case(b"rem") {
        return false;
    }
    let tail = &rest[3..];
    tail.is_empty()
        || tail.starts_with(|c: char| c.is_ascii_whitespace())
        || tail.starts_with('.')
}

/// 逐词小写用的栈缓冲大小。
///
/// 比这更长的词不可能出现在关键字表里（表里最长的 `disabledelayedexpansion` 也才 23
/// 字节），所以超长直接按"不是关键字"处理，不必分配。
const WORD_BUFFER: usize = 64;

/// 把 `word` 按 ASCII 小写写进栈缓冲，返回供关键字表比较的切片。
///
/// `None` = 词长超过 [`WORD_BUFFER`]：不可能是关键字，调用侧按"不是关键字"处理即可。
/// 非 ASCII 字节原样拷贝——关键字表全是 ASCII，拷贝后仍是合法 UTF-8，比较必然不等，
/// 与旧实现（`to_ascii_lowercase()` 只降 ASCII、其余原样）的结果一致。
///
/// 为什么要这么绕：`tokenize_line` 对**每一个词**都要拿小写形态去查关键字表，原先
/// 每词一次 `to_ascii_lowercase()` 就是每词一次堆分配。P0 的批量文件里词最密，那一项
/// 把 `spans("batch", ..)` 推到 25 ms/MiB（实测见 AGENTS.md「补充层」）。
fn lower_word<'a>(word: &str, buffer: &'a mut [u8; WORD_BUFFER]) -> Option<&'a str> {
    let bytes = word.as_bytes();
    if bytes.len() > WORD_BUFFER {
        return None;
    }
    for (slot, byte) in buffer.iter_mut().zip(bytes) {
        *slot = byte.to_ascii_lowercase();
    }
    std::str::from_utf8(&buffer[..bytes.len()]).ok()
}

/// 把一行拆成变量 / 运算符 / 单词并判色。
fn tokenize_line(
    row: usize,
    text: &str,
    line_start: usize,
    from: usize,
    end: usize,
    out: &mut Vec<Span>,
) {
    let body = &text[line_start..end];
    let bytes = body.as_bytes();
    let mut i = from;
    // 逐词小写用的栈缓冲：整个循环共用一份，不再每个词分配一个 String。
    let mut word_buffer = [0u8; WORD_BUFFER];
    // 行首或分隔符之后是命令位置：未知词是 "Command"（黑粗体），已知词是关键字（蓝粗体）；
    // 命令行中间的未知词是普通文本（黑、不加粗）。
    let mut command_pos = true;
    // `echo` / `goto` / `prompt` 之后：只保留变量与运算符着色，单词一律普通文本。
    let mut plain = false;
    // Notepad3 的 `isNotAssigned`：只有 `set` 之后紧跟的那个 `=` 是赋值运算符，
    // 别处的单个 `=`（如 `echo a=b`）算普通文本；`==` 比较运算符不受此限。
    let mut assign_pending = false;

    while i < body.len() {
        let c = bytes[i];
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        // 变量：%NAME% / %n / %* / %%a / %~… / !NAME!
        if c == b'%' || c == b'!' {
            if let Some(len) = variable_len(bytes, i) {
                out.push(span(
                    row,
                    line_start + i,
                    line_start + i + len,
                    Ink { fg: Some(BAT_IDENT), bg: Some(BAT_IDENT_BG), bold: false, eol: false },
                ));
                i += len;
                continue;
            }
        }
        // `@`（隐藏回显）单独成段，走 "Command" 的黑色粗体（Notepad3 的
        // `SCE_BAT_HIDE` 与 `SCE_BAT_COMMAND` 同一个样式）。它之后仍是命令位置。
        if c == b'@' {
            out.push(span(
                row,
                line_start + i,
                line_start + i + 1,
                Ink { fg: Some(BLACK), bg: None, bold: true, eol: false },
            ));
            i += 1;
            continue;
        }
        // 运算符（Notepad3 的 `IsBOperator`）：`= + > < | ?` 一连串同色。
        // 单个 `=` 只有两种情形上运算符色——`==` 比较，或紧跟 `set` 的赋值；
        // 其余（`echo a=b`）按 Notepad3 的 `isNotAssigned` 判定算普通文本。
        if matches!(c, b'=' | b'+' | b'>' | b'<' | b'|' | b'?') {
            let run = bytes[i..]
                .iter()
                .take_while(|&&x| matches!(x, b'=' | b'+' | b'>' | b'<' | b'|' | b'?'))
                .count();
            let is_double_eq = c == b'=' && bytes[i..].starts_with(b"==");
            let is_assign = c == b'=' && assign_pending;
            if is_double_eq || is_assign {
                out.push(span(row, line_start + i, line_start + i + run, Ink::fg(BAT_OPERATOR)));
            } else if c == b'=' {
                out.push(span(row, line_start + i, line_start + i + run, Ink::fg(BLACK)));
            } else {
                out.push(span(row, line_start + i, line_start + i + run, Ink::fg(BAT_OPERATOR)));
            }
            assign_pending = false;
            i += run;
            command_pos = true; // 管道 / 重定向之后又到命令位置
            continue;
        }
        // 括号本身不着色，但 `(` / `)` 开启一个命令位置（`( echo hi )`）。
        if c == b'(' || c == b')' {
            command_pos = true;
            i += 1;
            continue;
        }
        // 单词：吃到下一个词边界。
        let word_start = i;
        while i < body.len() && !is_word_end(bytes[i]) {
            i += 1;
        }
        if i == word_start {
            i += 1;
            continue;
        }
        let word = lower_word(&body[word_start..i], &mut word_buffer).unwrap_or("");

        // `rem` 起头即注释：整行剩余部分染绿（Notepad3 在词级也会这么判）。
        if word == "rem" {
            out.push(span(row, line_start + word_start, end, Ink::fg(BAT_COMMENT)));
            break;
        }

        let ink = if plain {
            Ink::fg(BLACK)
        } else if is_bat_keyword(word) {
            Ink::bold(BAT_KEYWORD)
        } else if command_pos {
            Ink { fg: Some(BLACK), bg: None, bold: true, eol: false }
        } else {
            Ink::fg(BLACK)
        };
        out.push(span(row, line_start + word_start, line_start + i, ink));

        if BAT_PLAIN_TAIL.contains(&word) {
            plain = true;
        }
        // `set` 之后紧跟的那个 `=` 是赋值运算符（Notepad3 的 `isNotAssigned`）。
        // 这里**只置位、不因中间词清掉**：`set NAME=world` 里 `NAME` 之后仍是赋值位，
        // 直到那个 `=` 被消费掉（运算符分支清）。
        if word == "set" {
            assign_pending = true;
            plain = true; // `set` 也关掉后续的关键字判定（`set NAME=x` 的 `NAME` 是文本）
        }
        // 只有少数关键字（`start` / `call` / `do`）之后的下一个词可能是命令；
        // 其余关键字（含 `if`）之后回到普通文本——这正是 `if 1==2` 里的 `1` 不上色、
        // 而 `out.txt` 在 `if exist out.txt` 里也不上色的原因（Notepad3 的 cmdLoc）。
        command_pos = !plain && BAT_OPENS_COMMAND.contains(&word);
    }
}

/// `word`（已小写）是否在 `KeyWords_BAT` 里，即 Notepad3 的"内部命令 / 常用外部工具"。
///
/// 用**二分查找**而不是 `BAT_KEYWORDS.contains(..)`：表里有 151 个词，线性扫是每个词
/// 151 次比较，而 .bat 的词很密、每个词都要查三张表——这一项就是批处理层在大文件上
/// 的主要开销（实测 25 ms/MiB → 见 AGENTS.md「补充层」）。
///
/// 二分的前提是表按字节序升序：它本身就是逐值照抄 `KeyWords_BAT` 的顺序，
/// 由 `keyword_table_is_sorted_for_binary_search` 钉住。
fn is_bat_keyword(word: &str) -> bool {
    BAT_KEYWORDS.binary_search(&word).is_ok()
}

/// 变量 token 的字节长度；`i` 指向 `%` 或 `!`。照 `LexBatch` 的几种形态。
fn variable_len(bytes: &[u8], i: usize) -> Option<usize> {
    match bytes[i] {
        b'%' => {
            let next = *bytes.get(i + 1)?;
            if next == b'%' {
                // %%a（局部变量）或 %%~…（展开变量）。
                let mut j = i + 2;
                if bytes.get(j) == Some(&b'~') {
                    j += 1;
                    while j < bytes.len() && (bytes[j].is_ascii_alphabetic() || bytes[j] == b'$') {
                        j += 1;
                    }
                }
                return bytes
                    .get(j)
                    .is_some_and(|&c| !is_word_end(c))
                    .then(|| j + 1 - i);
            }
            if next.is_ascii_digit() || next == b'*' {
                return Some(2);
            }
            // %~dp0（展开参数）：`%~` + 路径运算符（字母）+ 一个数字。
            if next == b'~' {
                let mut j = i + 2;
                while j < bytes.len() && bytes[j].is_ascii_alphabetic() {
                    j += 1;
                }
                if bytes.get(j).is_some_and(|c| c.is_ascii_digit()) {
                    return Some(j + 1 - i);
                }
                return Some(j - i);
            }
            // %NAME% / %x:%y% —— 吃到下一个 %。
            bytes[i + 1..].iter().position(|&c| c == b'%').map(|p| p + 2)
        },
        b'!' => bytes[i + 1..].iter().position(|&c| c == b'!').map(|p| p + 2),
        _ => None,
    }
}

/// `LexBatch` 的 `IsBEndWord`：运算符 + 分隔符 + `%` `!` `@`，都结束一个词。
fn is_word_end(c: u8) -> bool {
    c.is_ascii_whitespace()
        || matches!(
            c,
            b'=' | b'+' | b'>' | b'<' | b'|' | b'?' | b'*' | b'&' | b'(' | b')' | b'\\' | b'.'
                | b';' | b'"' | b'\'' | b'/' | b'%' | b'!' | b'@'
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of<'a>(src: &'a str, spans: &[Span]) -> Vec<(&'a str, Ink)> {
        spans.iter().map(|s| (&src[s.range.clone()], s.ink)).collect()
    }

    fn find<'a>(src: &'a str, spans: &[Span], needle: &str) -> Ink {
        text_of(src, spans)
            .into_iter()
            .find(|(t, _)| *t == needle)
            .unwrap_or_else(|| panic!("no span {needle:?}"))
            .1
    }

    // ---- Markdown ----

    #[test]
    fn markdown_inline_code_gets_the_code_background() {
        let src = "text `code()` and more\n";
        let spans = markdown(src);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].range, 5..13);
        assert_eq!(spans[0].ink.bg, Some(MARKDOWN_CODE_BG));
        assert_eq!(spans[0].ink.fg, None);
    }

    #[test]
    fn markdown_double_backtick_pairs_correctly() {
        assert_eq!(markdown("a ``b ` c`` d\n")[0].range, 2..11);
    }

    #[test]
    fn markdown_unclosed_and_escaped_backticks_are_ignored() {
        assert!(markdown("a `b\n").is_empty());
        assert!(markdown("a \\`b\\` c\n").is_empty());
    }

    #[test]
    fn markdown_fenced_block_is_covered_line_by_line() {
        let src = "before\n```rust\nlet x = 1; `no`\n```\nafter\n";
        let spans = markdown(src);
        assert_eq!(spans.len(), 3);
        assert_eq!(&src[spans[0].range.clone()], "```rust");
        assert_eq!(&src[spans[1].range.clone()], "let x = 1; `no`");
        assert_eq!(&src[spans[2].range.clone()], "```");
        assert!(spans.iter().all(|s| !src[s.range.clone()].contains('\n')));
    }

    #[test]
    fn markdown_unclosed_fence_runs_to_the_end() {
        assert_eq!(markdown("```\nabc\n").len(), 2);
    }

    /// 围栏代码块的所有行要共享**同一个** `width_from`（= 块里最宽那行），这样绘制侧
    /// 把每行都铺成等宽、右缘对齐，拼成"一整块矩形"而不是参差不齐的逐行底色。
    #[test]
    fn markdown_fence_lines_share_one_width() {
        let src = "```rust\nfn a() {}\nlet longer = 1;\n```\n";
        let spans = markdown(src);
        let widths: Vec<_> = spans.iter().map(|s| s.width_from.clone()).collect();
        assert!(widths.iter().all(|w| w.is_some()), "围栏行都要有宽度来源");
        assert!(
            widths.windows(2).all(|w| w[0] == w[1]),
            "块内所有行要共享同一个宽度来源：{widths:?}"
        );
        // 宽度来源就是最宽那一行（`let longer = 1;`）。
        assert_eq!(widths[0].clone().unwrap(), src.find("let longer").unwrap()..src.find("let longer").unwrap() + "let longer = 1;".len());
    }

    /// 围栏块后面的行内代码不该被拉齐（它有自己的宽度）。
    #[test]
    fn markdown_inline_code_after_a_fence_keeps_its_own_width() {
        let src = "```\nx\n```\ntext `code`\n";
        let spans = markdown(src);
        let last = spans.last().unwrap();
        assert_eq!(&src[last.range.clone()], "`code`");
        assert!(last.width_from.is_none(), "行内代码不该被整块拉齐");
    }

    /// 含 CJK 的注释行字少但更宽，整块必须按**显示列数**选最宽行，否则那一行会
    /// 伸出矩形（用户报的"有注释时不是矩形"）。这里 CJK 行**列数更多、字符数更少**
    /// ——按 `chars().count()` 会挑错成 ASCII 行。
    #[test]
    fn markdown_fence_width_accounts_for_cjk_columns() {
        // 行1：20 个 ASCII = 20 列 / 20 字符。行2：`x # ` + 10 个 CJK = 24 列 / 14 字符。
        let src = "```\naaaaaaaaaaaaaaaaaaaa\nx # 中文中文中文中文中文\n```\n";
        let spans = markdown(src);
        let widths: Vec<_> = spans.iter().map(|s| s.width_from.clone()).collect();
        assert!(widths.iter().all(|w| w.is_some()));
        assert!(widths.windows(2).all(|w| w[0] == w[1]), "整块共享同一宽度");
        // 按显示列数挑出的是 CJK 那一行（24 列 > 20 列）；按字符数会挑到 20 字符的 ASCII 行。
        assert_eq!(&src[widths[0].clone().unwrap()], "x # 中文中文中文中文中文");
    }

    /// `display_width` 的列数规则：ASCII 1 列、CJK / 全角 2 列、组合记号 0 列。
    #[test]
    fn display_width_counts_full_width_as_two() {
        assert_eq!(display_width("abc"), 3);
        assert_eq!(display_width("中文"), 4);
        assert_eq!(display_width("a中b"), 4);
        assert_eq!(display_width("，。"), 4); // 全角标点两列
        assert_eq!(display_width("é"), 1);
    }

    #[test]
    fn markdown_atx_headings_are_level_colored() {
        let src = "# One\n## Two\n### Three\n";
        let spans = markdown(src);
        assert_eq!(spans.len(), 3);
        let line = |s: &str| {
            text_of(src, &spans)
                .into_iter()
                .find(|(t, _)| *t == s)
                .map(|(_, ink)| ink)
                .unwrap_or_else(|| panic!("no span for {s:?}"))
        };
        // 每级整行一个 span：级别前景 + 条带底色。
        assert_eq!(line("# One").bg, Some((0x8B, 0xA4, 0xED)));
        assert_eq!(line("# One").fg, Some((0xFF, 0xFF, 0xE2)));
        assert_eq!(line("## Two").bg, Some((0x9D, 0xCE, 0xFF)));
        assert_eq!(line("### Three").bg, Some((0xD9, 0xEC, 0xFF)));
        assert!(spans.iter().all(|s| s.ink.bold && s.ink.eol));
    }

    #[test]
    fn markdown_hash_inside_a_fence_is_code_not_a_heading() {
        let src = "```\n# not a heading\n```\n";
        let spans = markdown(src);
        assert_eq!(spans.len(), 3);
        assert!(spans.iter().all(|s| s.ink.bg == Some(MARKDOWN_CODE_BG)));
    }

    // ---- INI ----

    #[test]
    fn ini_section_lines_are_backgrounded() {
        let src = "[owner]\nname=me\n";
        let spans = ini(src);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].range, 0..7);
        assert_eq!(spans[0].ink.bg, Some(INI_SECTION_BG));
        assert!(spans[0].ink.eol);
    }

    // ---- Batch ----

    #[test]
    fn batch_keywords_commands_variables_and_labels() {
        let src = "@echo off\nset NAME=world\nif %COUNT% GTR 0 echo Hi\n:done\nrem a note\n";
        let spans = batch(src);
        assert_eq!(find(src, &spans, "@"), Ink { fg: Some(BLACK), bg: None, bold: true, eol: false });
        // 内部命令是蓝粗体关键字。
        assert_eq!(find(src, &spans, "echo"), Ink::bold(BAT_KEYWORD));
        // 变量是 Identifier 色 + 黄底。
        let count = find(src, &spans, "%COUNT%");
        assert_eq!(count.fg, Some(BAT_IDENT));
        assert_eq!(count.bg, Some(BAT_IDENT_BG));
        // 标签是红字 + 灰底。
        let done = find(src, &spans, ":done");
        assert_eq!(done.fg, Some(BAT_LABEL));
        assert_eq!(done.bg, Some(BAT_LABEL_BG));
        // `rem` 注释整行绿。
        assert_eq!(find(src, &spans, "rem a note"), Ink::fg(BAT_COMMENT));
        // GTR 是关键字（大小写不敏感）。
        assert_eq!(find(src, &spans, "GTR"), Ink::bold(BAT_KEYWORD));
    }

    #[test]
    fn batch_echo_switches_the_rest_of_the_line_to_plain_text() {
        // `echo set` 里的 `set` 是原样输出的文字，不该被当成关键字。
        let spans = batch("echo set\n");
        assert_eq!(find("echo set\n", &spans, "set"), Ink::fg(BLACK));
        // 但变量仍然上色。
        let spans = batch("echo %NAME%\n");
        assert_eq!(find("echo %NAME%\n", &spans, "%NAME%").bg, Some(BAT_IDENT_BG));
    }

    #[test]
    fn batch_double_colon_is_a_comment_not_a_label() {
        let spans = batch(":: comment\n:label\n");
        assert_eq!(find(":: comment\n:label\n", &spans, ":: comment"), Ink::fg(BAT_COMMENT));
        assert_eq!(find(":: comment\n:label\n", &spans, ":label").fg, Some(BAT_LABEL));
    }

    #[test]
    fn batch_operators_and_parens() {
        let src = "if 1==2 ( echo x )\n";
        let spans = batch(src);
        assert_eq!(find(src, &spans, "=="), Ink::fg(BAT_OPERATOR));
        // 括号本身不着色（Notepad3 明确跳过括号）。
        assert!(!text_of(src, &spans).iter().any(|(t, _)| *t == "(" || *t == ")"));
        // `if` 的操作数 `1` 是普通文本，不是命令（Notepad3 不重开命令位置）。
        assert_eq!(find(src, &spans, "1"), Ink::fg(BLACK));
    }

    #[test]
    fn batch_single_equals_is_assign_only_after_set() {
        // `set NAME=world` 的 `=` 是赋值运算符（洋红）。
        let spans = batch("set NAME=world\n");
        assert_eq!(find("set NAME=world\n", &spans, "="), Ink::fg(BAT_OPERATOR));
        // `echo a=b` 的 `=` 是普通文本（Notepad3 的 isNotAssigned）。
        let spans = batch("echo a=b\n");
        assert_eq!(find("echo a=b\n", &spans, "="), Ink::fg(BLACK));
    }

    #[test]
    fn batch_at_prefixed_rem_is_a_comment() {
        let spans = batch("@rem note\n");
        assert_eq!(find("@rem note\n", &spans, "rem note"), Ink::fg(BAT_COMMENT));
    }

    #[test]
    fn batch_rem_stays_case_insensitive_after_the_fast_path() {
        // `starts_with_rem` 改成按字节比较之后，大小写不敏感与尾部边界两条规则都要不变。
        assert_eq!(find("REM note\n", &batch("REM note\n"), "REM note"), Ink::fg(BAT_COMMENT));
        assert_eq!(find("ReM.note\n", &batch("ReM.note\n"), "ReM.note"), Ink::fg(BAT_COMMENT));
        // `remx`：`rem` 之后既不是空白、也不是行尾或 `.`，不算注释。
        let src = "remx\n";
        let spans = batch(src);
        assert!(!text_of(src, &spans).iter().any(|(_, ink)| *ink == Ink::fg(BAT_COMMENT)));
    }

    #[test]
    fn lower_word_handles_case_non_ascii_and_overlong_words() {
        let mut buffer = [0u8; WORD_BUFFER];
        assert_eq!(lower_word("SET", &mut buffer), Some("set"));
        // 非 ASCII 字节原样拷贝：仍是合法 UTF-8，只是永远不可能等于关键字表里的词。
        assert_eq!(lower_word("中文A", &mut buffer), Some("中文a"));
        let exact = "x".repeat(WORD_BUFFER);
        assert_eq!(lower_word(&exact, &mut buffer), Some(exact.as_str()));
        // 超一个字节就判"不是关键字"，不再分配。
        assert_eq!(lower_word(&"x".repeat(WORD_BUFFER + 1), &mut buffer), None);
    }

    #[test]
    fn batch_words_over_the_buffer_are_commands_not_keywords() {
        // 超长词走快速路径：仍然是词（有 span、是命令位置的黑粗体），但绝不能命中关键字色。
        let long = "a".repeat(WORD_BUFFER + 8);
        let src = format!("{long} tail\n");
        let spans = batch(&src);
        assert_eq!(
            find(&src, &spans, long.as_str()),
            Ink { fg: Some(BLACK), bg: None, bold: true, eol: false }
        );
        // 把关键字重复到超长同样不命中（旧实现先分配小写串再比对，结论一致）。
        let repeated = "if".repeat(40);
        let src = format!("{repeated}\n");
        let spans = batch(&src);
        assert_eq!(find(&src, &spans, repeated.as_str()).fg, Some(BLACK));
        assert_ne!(find(&src, &spans, repeated.as_str()), Ink::bold(BAT_KEYWORD));
    }

    #[test]
    fn batch_argument_forms() {
        let src = "call :greet %1 %%a %~dp0 !X!\n";
        let spans = batch(src);
        for token in ["%1", "%%a", "%~dp0", "!X!"] {
            assert_eq!(find(src, &spans, token).bg, Some(BAT_IDENT_BG), "{token}");
        }
    }

    #[test]
    fn batch_all_keyword_list_words_are_blue_bold() {
        // `enabledelayedexpansion` / `exist` / `gtr` 这些也都在 KeyWords_BAT 里。
        let src = "setlocal enabledelayedexpansion if exist x gtr y\n";
        let spans = batch(src);
        for token in ["setlocal", "enabledelayedexpansion", "if", "exist", "gtr"] {
            assert_eq!(find(src, &spans, token), Ink::bold(BAT_KEYWORD), "{token}");
        }
    }

    #[test]
    fn batch_label_span_excludes_space_after_name() {
        // `:done` 后若有文字，那部分归 "After Label"（青）。
        let src = ":done something\n";
        let spans = batch(src);
        assert_eq!(find(src, &spans, ":done").fg, Some(BAT_LABEL));
        assert_eq!(find(src, &spans, " something").fg, Some(BAT_AFTER_LABEL));
    }

    #[test]
    fn keyword_table_is_sorted_for_binary_search() {
        // `is_bat_keyword` 用二分查找，前提是这张表按字节序升序——它本身就是逐值照抄
        // `styleLexBAT.c` 的 `KeyWords_BAT` 的顺序，本用例保证"照抄进来的顺序"确实有序，
        // 免得以后往里插词插错位置之后关键字静默查不到。
        assert!(
            BAT_KEYWORDS.windows(2).all(|pair| pair[0] < pair[1]),
            "BAT_KEYWORDS 必须按字节序升序（`is_bat_keyword` 的二分前提）"
        );
        // 抽查首 / 中 / 末三个词都在表里能查到，并确认不存在的词确实查不到。
        for word in [
            BAT_KEYWORDS[0],
            BAT_KEYWORDS[BAT_KEYWORDS.len() / 2],
            BAT_KEYWORDS[BAT_KEYWORDS.len() - 1],
        ] {
            assert!(is_bat_keyword(word), "{word} 应在关键字表里");
        }
        assert!(!is_bat_keyword("notakeyword"));
        assert!(!is_bat_keyword(""), "空词不是关键字");
    }

    #[test]
    fn lines_reports_row_start_and_pre_newline_end() {
        // `lines` 改成迭代器之后，`(行号, 行首, 行尾)` 三个值仍要与旧实现一致：
        // 行尾不含 `\n` / `\r`，最后一行没有换行时按实际长度收尾。
        let text = "a\r\nbb\n\nccc";
        let got: Vec<_> = lines(text).collect();
        assert_eq!(got, vec![(0, 0, 1), (1, 3, 5), (2, 6, 6), (3, 7, 10)]);
    }

    #[test]
    fn only_known_languages_have_overlays() {
        assert!(has_overlay("batch"));
        assert!(has_overlay("markdown"));
        assert!(has_overlay("ini"));
        assert!(!has_overlay("rust"));
        assert!(!has_overlay("css"), "CSS 只有前景色（走语法层），没有补充层");
        assert!(spans("rust", "%X% `x`").is_empty());
    }
}
