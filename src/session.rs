//! 会话恢复：上次的标签集合与激活项、树根、侧栏开合与宽度、窗口位置尺寸。
//!
//! 落盘位置 `%APPDATA%\nebula-lite\session.json`。与 `settings.rs` 同一套约定：
//! 文件缺失 = "没有会话可用"（按命令行参数启动，不是错误）；解析失败 = 同样按命令行
//! 启动并在 stderr 留一行；字段缺失一律回落默认值；写失败只留一行 stderr，不打断使用。
//!
//! **有意不存的东西**：编辑缓冲的内容（那份只在内存里，未保存的改动不恢复）、
//! 撤销历史、查找框内容、每个标签的滚动位置。要恢复内容就得先有"草稿缓冲"这一层，
//! 是另一件事。
//!
//! 为什么"每次变化就写"而不是"退出时写一次"：退出路径不止一条（关窗口、任务管理器、
//! 断电），而这份 JSON 只有几百字节。窗口拖动 / 侧栏拖动是连续事件，那两路走节流
//! （见 `app.rs::persist_session_throttled`）。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// 恢复的标签数上限。
///
/// 挡的是"会话文件被手改成一万条"这种情况：每条都要读盘 + 起监听 + 建编辑器状态，
/// 不设上限等于让一个坏文件把启动拖死。
pub const MAX_RESTORED_TABS: usize = 64;

/// 会话文件名（`%APPDATA%\nebula-lite\session.json`）。
pub fn session_path() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(base).join("nebula-lite").join("session.json"))
}

/// 窗口位置与尺寸（逻辑像素）。
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowState {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// 最大化也要记：只记尺寸的话，还原一个"看着像最大化过但其实没有"的窗口很怪。
    pub maximized: bool,
}

impl Default for WindowState {
    fn default() -> Self {
        Self { x: 0.0, y: 0.0, width: 1280.0, height: 820.0, maximized: false }
    }
}

impl WindowState {
    /// 这份矩形是否像个真窗口。
    ///
    /// 手改 / 损坏的会话文件里可能出现 0 尺寸、NaN 或离谱的坐标，直接用会让窗口
    /// 打不开或跑到看不到的地方——所以启动时先过这一关（`main.rs::resolve_window`）。
    pub fn is_plausible(self) -> bool {
        const MAX_SIDE: f32 = 20000.0;
        let ok = |value: f32, min: f32| value.is_finite() && value >= min && value <= MAX_SIDE;
        ok(self.width, 200.0) && ok(self.height, 150.0) && ok(self.x.abs(), 0.0) && ok(self.y.abs(), 0.0)
    }
}

/// 一个标签要记住的东西（顺序即标签条的顺序）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabState {
    pub path: PathBuf,
    #[serde(default)]
    pub pinned: bool,
    /// 是否停在预览面（Markdown 的富文本 / 图片）。
    #[serde(default)]
    pub preview: bool,
    /// Markdown 的大纲面板是否展开。
    #[serde(default)]
    pub outline_open: bool,
}

/// 整份会话。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    /// 侧栏（文件树）的根目录。
    pub root: Option<PathBuf>,
    pub tabs: Vec<TabState>,
    /// 激活标签在 `tabs` 里的下标。
    pub active: usize,
    /// 侧栏是否展开。
    pub sidebar_open: bool,
    /// 侧栏宽度；`0.0` = 没存过（用默认宽度）。
    pub sidebar_width: f32,
    /// 窗口位置尺寸；`None` = 没存过（居中打开）。
    pub window: Option<WindowState>,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            root: None,
            tabs: Vec::new(),
            active: 0,
            // 侧栏默认是展开的：缺字段的旧会话文件不该让侧栏莫名收起。
            sidebar_open: true,
            sidebar_width: 0.0,
            window: None,
        }
    }
}

impl Session {
    /// 读会话。**没有文件（或读不动）就返回 `None`**。
    ///
    /// `None` 与"空会话"是两件事：前者是"从没跑过 / 文件坏了"，调用侧要按命令行参数
    /// 启动；后者是"上次确实是一个空工作区"，那就恢复成空工作区。
    pub fn load() -> Option<Self> {
        let path = session_path()?;
        let text = std::fs::read_to_string(&path).ok()?;
        match serde_json::from_str(&text) {
            Ok(session) => Some(session),
            Err(error) => {
                eprintln!(
                    "[nebula-lite] 会话文件无法解析，本次按命令行参数启动：{}（{error}）",
                    path.display()
                );
                None
            }
        }
    }

    /// 写会话。失败只留一行 stderr。
    pub fn save(&self) {
        let Some(path) = session_path() else {
            return;
        };
        if let Some(dir) = path.parent()
            && let Err(error) = std::fs::create_dir_all(dir)
        {
            eprintln!("[nebula-lite] 无法创建会话目录 {}：{error}", dir.display());
            return;
        }
        let json = match serde_json::to_string_pretty(self) {
            Ok(json) => json,
            Err(error) => {
                eprintln!("[nebula-lite] 会话无法序列化：{error}");
                return;
            }
        };
        if let Err(error) = std::fs::write(&path, json) {
            eprintln!("[nebula-lite] 会话写盘失败 {}：{error}", path.display());
        }
    }
}

/// 从会话里挑出**现在还能打开**的标签，并给出激活项在新列表里的下标。
///
/// - 文件已被删 / 改名 / 变成目录的标签直接丢掉（否则启动时会弹一串错误）；
/// - 激活项按"原来激活的那个文件还在不在"决定：不在了就落到**它前面最近的那个**
///   还在的标签上（都在它后面就取 0），全没了就是 0；
/// - 条数截到 [`MAX_RESTORED_TABS`]。
pub fn restorable(session: &Session) -> (Vec<TabState>, usize) {
    let mut kept: Vec<TabState> = Vec::new();
    let mut active = 0usize;
    for (index, tab) in session.tabs.iter().enumerate() {
        if kept.len() >= MAX_RESTORED_TABS {
            break;
        }
        if !tab.path.is_file() {
            continue;
        }
        if index <= session.active {
            // 循环走完时它落在"激活项或它前面最近的那个"上。
            active = kept.len();
        }
        kept.push(tab.clone());
    }
    if kept.is_empty() {
        active = 0;
    }
    (kept, active)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(path: &str) -> TabState {
        TabState {
            path: PathBuf::from(path),
            pinned: false,
            preview: false,
            outline_open: false,
        }
    }

    /// 缺字段、多字段、空对象都要能读——旧版本写的文件不该让编辑器起不来。
    #[test]
    fn missing_and_unknown_fields_are_tolerated() {
        let session: Session = serde_json::from_str("{}").unwrap();
        assert_eq!(session, Session::default());
        assert!(session.sidebar_open, "缺字段时侧栏保持展开");
        let session: Session =
            serde_json::from_str(r#"{"active":2,"将来才有的字段":true}"#).unwrap();
        assert_eq!(session.active, 2);
        assert!(session.tabs.is_empty());
    }

    /// 存-读一轮必须逐字段相等。
    #[test]
    fn round_trip_keeps_every_field() {
        let session = Session {
            root: Some(PathBuf::from("C:\\工作区")),
            tabs: vec![TabState {
                path: PathBuf::from("C:\\工作区\\a.md"),
                pinned: true,
                preview: true,
                outline_open: true,
            }],
            active: 3,
            sidebar_open: false,
            sidebar_width: 412.0,
            window: Some(WindowState { x: -1200.0, y: 40.0, width: 900.0, height: 640.0, maximized: true }),
        };
        let json = serde_json::to_string(&session).unwrap();
        assert_eq!(serde_json::from_str::<Session>(&json).unwrap(), session);
    }

    /// 尺寸不合理的窗口矩形要被挡掉（手改 / 损坏的会话文件）。
    #[test]
    fn implausible_window_states_are_rejected() {
        assert!(WindowState { width: 1280.0, height: 820.0, ..Default::default() }.is_plausible());
        assert!(!WindowState { width: 0.0, height: 820.0, ..Default::default() }.is_plausible());
        assert!(!WindowState { width: 1280.0, height: 10.0, ..Default::default() }.is_plausible());
        assert!(!WindowState { width: f32::NAN, height: 820.0, ..Default::default() }.is_plausible());
        assert!(!WindowState { width: 99999.0, height: 820.0, ..Default::default() }.is_plausible());
    }

    /// 只剩还在磁盘上的标签被恢复，其余丢掉（本用例用临时目录造真实文件路径）。
    #[test]
    fn restorable_keeps_only_files_that_still_exist() {
        let dir = std::env::temp_dir().join(format!("nebula-session-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let good_a = dir.join("a.md");
        let good_b = dir.join("b.rs");
        std::fs::write(&good_a, "a").unwrap();
        std::fs::write(&good_b, "b").unwrap();
        let missing = dir.join("gone.md");

        let session = Session {
            root: None,
            tabs: vec![
                tab(&missing.to_string_lossy()),
                tab(&good_a.to_string_lossy()),
                tab(&good_b.to_string_lossy()),
            ],
            // 激活项原本指向最后一个（还在）——恢复后应落在它身上。
            active: 2,
            ..Default::default()
        };
        let (kept, active) = restorable(&session);
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].path, good_a);
        assert_eq!(kept[1].path, good_b);
        assert_eq!(active, 1, "激活项跟着它原来那个文件走");

        // 激活项指向的那个文件没了：落到它前面最近的那个还在的标签。
        let session = Session {
            tabs: vec![tab(&good_a.to_string_lossy()), tab(&missing.to_string_lossy())],
            active: 1,
            ..Default::default()
        };
        let (kept, active) = restorable(&session);
        assert_eq!(kept.len(), 1);
        assert_eq!(active, 0);

        // 一个都不在了：空列表 + 下标 0（调用侧据此保持"从左侧选一个文件"）。
        let session = Session { tabs: vec![tab(&missing.to_string_lossy())], active: 0, ..Default::default() };
        let (kept, active) = restorable(&session);
        assert!(kept.is_empty());
        assert_eq!(active, 0);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 标签数上限：手改出来的超长会话文件不能把启动拖死。
    #[test]
    fn restorable_caps_the_tab_count() {
        let dir = std::env::temp_dir().join(format!("nebula-session-cap-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("many.md");
        std::fs::write(&path, "x").unwrap();
        let tabs = vec![tab(&path.to_string_lossy()); MAX_RESTORED_TABS + 5];
        let (kept, _) = restorable(&Session { tabs, ..Default::default() });
        assert_eq!(kept.len(), MAX_RESTORED_TABS);
        std::fs::remove_dir_all(&dir).ok();
    }
}
