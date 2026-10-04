//! 文本快照：把任意字节解码成可显示的字符，并决定它是否可写回。
//!
//! 移植自 Pebrel 的 `nebula_app/src/text_document.rs`。语义照抄，因为那套裁定
//! 正是"像 VS Code 一样什么文件都当文本打开"的关键：
//! - 不合法 UTF-8 或含 NUL 的字节用**有损解码**照样出字符，只是标记
//!   `invalid_encoding` 并转只读——所以二进制文件打开看到的是乱码而不是报错，
//!   也不会因为一次误保存把原文件写坏；
//! - 超过上限的内容截断显示并转只读；
//! - BOM 与 CRLF 记录下来，保存时原样写回，不改动用户文件的既有约定。

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// 一次性读入的上限。超过就截断（行级虚拟化只解决渲染成本，解码与塑形仍随
/// 内容量增长）。
pub const MAX_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct TextSnapshot {
    pub text: String,
    pub bytes: Arc<[u8]>,
    pub bom: bool,
    pub crlf: bool,
    pub truncated: bool,
    pub invalid_encoding: bool,
    pub read_only: bool,
}

#[derive(Debug)]
pub enum SaveError {
    Changed,
    ReadOnly,
    Io(io::Error),
}

impl From<io::Error> for SaveError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Changed => formatter.write_str("文件在打开之后被外部修改过"),
            Self::ReadOnly => formatter.write_str("该文档是只读的"),
            Self::Io(error) => error.fmt(formatter),
        }
    }
}

/// 编辑缓冲（rope）与磁盘基线是否**逐字节相同**。
///
/// 这是"未保存标记"的判据。从前调用侧写的是 `value() != snapshot.text`，而 `value()`
/// 每次都会把整条 rope 拷成一份新 String（2 MiB 时实测 1.14 ms，且随文件线性增长）。
/// 改成按块比对之后：调用侧先比长度——插入 / 删除是最常见的编辑，长度一变就有结论，
/// 那是 O(1)；只有"等长改写"才走到这里逐块 memcmp。
///
/// 参数是**块迭代器**（`Rope::chunks()`）而不是 rope 本身：这一层不必为此多一个 ropey
/// 直接依赖，也就不会踩到"同名 crate 解析成两套不兼容类型"那个坑（见 Cargo.toml 顶部）。
pub fn same_text<'a>(chunks: impl Iterator<Item = &'a str>, baseline: &str) -> bool {
    let mut rest = baseline.as_bytes();
    for chunk in chunks {
        let bytes = chunk.as_bytes();
        if !rest.starts_with(bytes) {
            return false;
        }
        rest = &rest[bytes.len()..];
    }
    rest.is_empty()
}

impl std::error::Error for SaveError {}

impl TextSnapshot {
    pub fn decode(mut bytes: Vec<u8>, read_only: bool) -> Self {
        let truncated = bytes.len() > MAX_BYTES;
        bytes.truncate(MAX_BYTES);
        let bom = bytes.starts_with(b"\xef\xbb\xbf");
        let body = if bom { &bytes[3..] } else { &bytes };
        let invalid_encoding = std::str::from_utf8(body).is_err() || body.contains(&0);
        let text = String::from_utf8_lossy(body);
        let crlf = text.contains("\r\n") && !text.replace("\r\n", "").contains('\n');
        let text = text.replace("\r\n", "\n");
        Self {
            text,
            bytes: bytes.into(),
            bom,
            crlf,
            truncated,
            invalid_encoding,
            read_only: read_only || truncated || invalid_encoding,
        }
    }

    /// 按原始约定编码回字节：BOM 与 CRLF 都会还原。
    pub fn encode(&self, text: &str) -> Result<Vec<u8>, SaveError> {
        if self.read_only {
            return Err(SaveError::ReadOnly);
        }
        let mut bytes = if self.bom { b"\xef\xbb\xbf".to_vec() } else { Vec::new() };
        let encoded = if self.crlf { text.replace('\n', "\r\n") } else { text.to_owned() };
        bytes.extend_from_slice(encoded.as_bytes());
        if bytes.len() > MAX_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "可编辑文本上限为 8 MiB",
            )
            .into());
        }
        Ok(bytes)
    }

    /// 保存前确认磁盘内容仍是打开时那份，避免覆盖外部改动。
    pub fn verify(&self, current: &[u8]) -> Result<(), SaveError> {
        if current == self.bytes.as_ref() { Ok(()) } else { Err(SaveError::Changed) }
    }

    /// 保存成功后把当前文本设为新的比对基线。
    ///
    /// 只换 `bytes`，保留 `bom` / `crlf` / 编解码标记：一次成功保存不该把文件
    /// 原本的换行风格或 BOM 约定丢掉，否则第二次保存就会改写用户的文件格式。
    pub fn rebase(&mut self, text: &str) -> Result<(), SaveError> {
        self.bytes = self.encode(text)?.into();
        Ok(())
    }
}

/// 读盘并解码。只读判定只看文件系统属性，解码层的只读另行叠加。
pub fn load(path: &Path) -> io::Result<TextSnapshot> {
    let bytes = std::fs::read(path)?;
    Ok(load_bytes(path, bytes))
}

/// 用调用方已经读到的那份内容建快照。
///
/// 外部改动那条路已经为"文件是否变了"读过一次盘；若再调 [`load`] 读第二遍，
/// 快照里的字节就不是刚才据以裁定的那一份了（两次读之间文件还可能再变）。
pub fn load_bytes(path: &Path, bytes: Vec<u8>) -> TextSnapshot {
    let fs_read_only = std::fs::metadata(path).map(|m| m.permissions().readonly()).unwrap_or(false);
    TextSnapshot::decode(bytes, fs_read_only)
}

/// 写回：先校验未被外部改动，再按原编码与换行风格写出。
pub fn save(path: &Path, snapshot: &TextSnapshot, text: &str) -> Result<(), SaveError> {
    if let Ok(current) = std::fs::read(path) {
        snapshot.verify(&current)?;
    }
    write(path, snapshot, text)
}

/// 用本地内容覆盖磁盘上的外部改动：跳过基线校验。
///
/// 与 [`save`] 分成两个函数、而不是给 `save` 加个 bool 参数：绕过冲突检测是必须
/// 在调用点被看见的决定，不该藏在一个默认参数里。
pub fn save_over(path: &Path, snapshot: &TextSnapshot, text: &str) -> Result<(), SaveError> {
    write(path, snapshot, text)
}

fn write(path: &Path, snapshot: &TextSnapshot, text: &str) -> Result<(), SaveError> {
    let bytes = snapshot.encode(text)?;
    std::fs::write(path, bytes)?;
    Ok(())
}

/// 文件类型分流：决定右上角是否有"源码/预览"切换，以及预览用什么渲染。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    /// Markdown：源码 / 富文本预览。
    Markdown,
    /// 图片：源码（字节）/ 图像预览。
    Image,
    /// 其余一律当文本打开。
    Text,
}

impl FileKind {
    pub fn of(path: &Path) -> Self {
        let ext = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.to_ascii_lowercase());
        match ext.as_deref() {
            Some("md" | "markdown") => Self::Markdown,
            Some("png" | "jpg" | "jpeg" | "webp" | "bmp" | "gif") => Self::Image,
            _ => Self::Text,
        }
    }

    /// 是否有可切换的预览面。
    pub fn has_preview(self) -> bool {
        matches!(self, Self::Markdown | Self::Image)
    }
}

/// 路径显示用的简写：优先文件名，退回完整路径。
pub fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// 目录项排序：目录在前，然后按名字不区分大小写。
pub fn sort_entries(entries: &mut [(PathBuf, bool)]) {
    entries.sort_by(|(left_path, left_dir), (right_path, right_dir)| {
        right_dir
            .cmp(left_dir)
            .then_with(|| {
                let left = display_name(left_path).to_lowercase();
                let right = display_name(right_path).to_lowercase();
                left.cmp(&right)
            })
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 用户的核心要求：任何文件都要能打开并显示字符，而不是交给默认应用。
    /// 含 NUL 的二进制必须仍然解出文本，只是转只读。
    #[test]
    fn binary_files_still_decode_to_text_but_stay_read_only() {
        let snapshot = TextSnapshot::decode(vec![b'a', 0, b'b', 0xff], false);
        assert!(snapshot.invalid_encoding);
        assert!(snapshot.read_only);
        // 有损解码保证一定有字符可显示，不会因为非法字节而报错。
        assert!(!snapshot.text.is_empty());
        assert!(matches!(snapshot.encode("replacement"), Err(SaveError::ReadOnly)));
    }

    #[test]
    fn valid_utf8_without_nul_is_editable() {
        let snapshot = TextSnapshot::decode("你好 hello\n".as_bytes().to_vec(), false);
        assert!(!snapshot.invalid_encoding);
        assert!(!snapshot.read_only);
        assert_eq!(snapshot.text, "你好 hello\n");
    }

    /// BOM 与 CRLF 必须原样还原，不能因为一次保存改写用户文件的既有约定。
    #[test]
    fn bom_and_crlf_round_trip() {
        let snapshot =
            TextSnapshot::decode("\u{feff}标题\r\n内容\r\n".as_bytes().to_vec(), false);
        assert!(snapshot.bom && snapshot.crlf);
        assert_eq!(snapshot.text, "标题\n内容\n");
        assert_eq!(snapshot.encode("新标题\n").unwrap(), "\u{feff}新标题\r\n".as_bytes());
    }

    /// 保存前的冲突检测：磁盘内容变了就拒绝覆盖。
    #[test]
    fn external_change_is_detected() {
        let snapshot = TextSnapshot::decode(b"first".to_vec(), false);
        assert!(snapshot.verify(b"first").is_ok());
        assert!(matches!(snapshot.verify(b"other"), Err(SaveError::Changed)));
    }

    /// 冲突之后用户得有出路：显式的覆盖保存能写过去，而普通保存照旧拒绝。
    /// 两条路都要有，否则冲突提示会变成死胡同。
    #[test]
    fn save_refuses_but_save_over_replaces_external_content() {
        let dir = std::env::temp_dir().join(format!("nebula-lite-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("conflict.txt");
        std::fs::write(&path, b"opened").unwrap();

        let snapshot = load(&path).unwrap();
        std::fs::write(&path, b"external").unwrap();
        assert!(matches!(save(&path, &snapshot, "mine"), Err(SaveError::Changed)));
        assert_eq!(std::fs::read(&path).unwrap(), b"external", "拒绝即不得动磁盘");

        save_over(&path, &snapshot, "mine").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"mine");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `load_bytes` 用的是调用方读到的那份字节：它必须成为新的比对基线，
    /// 而不是再从磁盘读一遍。
    #[test]
    fn load_bytes_uses_the_bytes_it_was_given() {
        let dir = std::env::temp_dir().join(format!("nebula-lite-bytes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bytes.txt");
        std::fs::write(&path, b"on disk").unwrap();

        let snapshot = load_bytes(&path, b"from watcher".to_vec());
        assert_eq!(snapshot.text, "from watcher");
        assert!(snapshot.verify(b"from watcher").is_ok());
        assert!(snapshot.verify(b"on disk").is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn oversized_input_is_truncated_and_locked() {
        let snapshot = TextSnapshot::decode(vec![b'x'; MAX_BYTES + 1], false);
        assert!(snapshot.truncated && snapshot.read_only);
        assert_eq!(snapshot.text.len(), MAX_BYTES);
    }

    #[test]
    fn file_kind_routes_preview_faces() {
        assert_eq!(FileKind::of(Path::new("a.md")), FileKind::Markdown);
        assert_eq!(FileKind::of(Path::new("a.PNG")), FileKind::Image);
        assert_eq!(FileKind::of(Path::new("a.zzz")), FileKind::Text);
        assert!(FileKind::Markdown.has_preview());
        assert!(FileKind::Image.has_preview());
        // 普通文件没有预览面，只有编辑器。
        assert!(!FileKind::Text.has_preview());
    }

    #[test]
    fn same_text_compares_chunk_wise_against_the_baseline() {
        // 缓冲侧给的是"按块"的切片（`Rope::chunks()` 的形状），基线是一整条 &str。
        assert!(same_text(["hello", " world"].into_iter(), "hello world"));
        assert!(same_text(["hello world"].into_iter(), "hello world"));
        assert!(same_text(std::iter::empty(), ""), "空缓冲 + 空基线 = 相同");
        assert!(!same_text(["hello", " world"].into_iter(), "hello worlD"));
        assert!(!same_text(["hello"].into_iter(), "hello world"), "短了不算相同");
        assert!(!same_text(["hello world!"].into_iter(), "hello world"), "长了也不算");
        assert!(!same_text(std::iter::empty(), "x"));
    }
}
