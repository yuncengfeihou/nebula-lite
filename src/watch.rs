//! 外部改动监听：发现文件被别的程序改写，并在不吞掉用户输入的前提下跟上它。
//!
//! 监听的是**目标文件所在目录**（非递归），不是文件本身的路径：编辑器保存普遍
//! 走"写临时文件 + 改名"（VS Code、`vim` 的 backupcopy、各种原子写），直接盯住
//! 原路径会在改名那一刻失去目标；目录级监听同时覆盖原地写入与原子替换，事件里
//! 再按路径筛出我们关心的那一个。
//!
//! 决定"发现改动之后怎么处置"的 [`decide`] 是纯函数：它不碰文件系统、不碰 gpui，
//! 所以每条分支都能直接测，不用起监听也不用等异步。

use std::io;
use std::path::Path;
use std::time::Duration;

use notify::{Event, EventKind, RecursiveMode, Watcher};

/// 事件合并窗口。
///
/// 一次保存通常产生多条事件（内容、时间戳、改名），而且写入方可能还没写完。
/// 收到第一条后先等这么久再重读：既躲开半截内容，也把一次保存收敛成一次重载。
pub const SETTLE: Duration = Duration::from_millis(120);

/// 监听器上报的信号。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signal {
    /// 目录里有动静，但目标文件之外的东西变了。
    ///
    /// 文件树每次渲染都重读磁盘，所以这类事件只需要触发一次重渲染。
    Structure,
    /// 目标文件自身被写入 / 新建 / 改名 / 删除。
    Target,
}

/// 发现外部改动之后的处置。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// 磁盘内容与打开时的基线一致——多半是我们自己刚保存触发的，缓冲无需改动。
    Unchanged,
    /// 缓冲没有未保存改动：直接采用磁盘内容。
    Reload,
    /// 缓冲有未保存改动：不能覆盖用户敲进去的字，交给用户裁决。
    Conflict,
    /// 文件在外部被删除或改名。
    Missing,
}

/// 按「磁盘现状 + 缓冲是否脏」裁定处置方式。
///
/// `baseline` 是打开时（或上次保存后）记录的磁盘内容；`disk` 是现在读到的内容，
/// `None` 表示文件已经不在。判定顺序是有意的：
/// - 先比内容：一致就什么都不做。这同时挡掉了"我们自己保存触发监听"的自激，
///   以及外部进程原样重写同一份内容的情况。
/// - 再比脏标记：脏缓冲一律不覆盖，只挂冲突。
pub fn decide(dirty: bool, disk: Option<&[u8]>, baseline: &[u8]) -> Decision {
    match disk {
        None => Decision::Missing,
        Some(bytes) if bytes == baseline => Decision::Unchanged,
        Some(_) if dirty => Decision::Conflict,
        Some(_) => Decision::Reload,
    }
}

/// 事件是否落在目标文件上。
///
/// `Access` 事件要丢掉：读一个文件也会产生它，若当成改动就会变成
/// "重载 → 读盘 → 又触发重载"的自激循环。
pub fn event_touches(event: &Event, target: &Path) -> bool {
    if matches!(event.kind, EventKind::Access(_)) {
        return false;
    }
    event.paths.iter().any(|path| same_file(path, target))
}

/// 路径是否指向同一个文件。
///
/// 先按字面比，退回"同目录 + 同文件名"：后端回给我们的路径拼法（前缀、大小写）
/// 未必与手里那条完全一致。监听是单目录非递归的，同一目录里不可能有两个同名
/// 文件，所以这个退路不会认错。比对失败只会让自动重载漏掉一次，不会误伤。
fn same_file(candidate: &Path, target: &Path) -> bool {
    candidate == target
        || (candidate.parent() == target.parent()
            && candidate.file_name().is_some()
            && candidate.file_name() == target.file_name())
}

/// 目标文件的目录级监听器。
///
/// 它没有可读的状态——价值全在"被持有"这件事上：Drop 掉它，发送端随之消失、
/// 通道关闭，前台重载任务自己退出，不需要额外的取消逻辑；同时 `notify` 的
/// watcher 被 Drop 也会停掉系统级监听。
pub struct FileWatch {
    _watcher: notify::RecommendedWatcher,
    _sender: async_channel::Sender<Signal>,
}

impl FileWatch {
    /// 开始监听 `path` 所在目录，返回监听器与信号接收端。
    ///
    /// 建不起来不算致命：调用方让文件照常打开即可，只是没有自动重载。
    pub fn start(path: &Path) -> io::Result<(Self, async_channel::Receiver<Signal>)> {
        let (sender, receiver) = async_channel::unbounded();
        let target = path.to_path_buf();
        let signal = sender.clone();
        let watcher = notify::recommended_watcher(move |result: notify::Result<Event>| {
            let Ok(event) = result else { return };
            if matches!(event.kind, EventKind::Access(_)) {
                return;
            }
            let kind = if event_touches(&event, &target) { Signal::Target } else { Signal::Structure };
            let _ = signal.try_send(kind);
        })
        .map_err(io::Error::other)?;

        // 裸文件名的 `parent()` 是空路径，不能当目录用（同 `main.rs` 的坑）。
        let directory = match path.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent,
            _ => Path::new("."),
        };
        let mut watcher = watcher;
        watcher.watch(directory, RecursiveMode::NonRecursive).map_err(io::Error::other)?;
        Ok((Self { _watcher: watcher, _sender: sender }, receiver))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use notify::event::{AccessKind, AccessMode, CreateKind, ModifyKind};

    /// 磁盘内容与基线一致时什么都不做：这条同时挡住了"我们自己保存触发监听"
    /// 造成的自激重载。
    #[test]
    fn identical_content_needs_no_action() {
        assert_eq!(decide(false, Some(b"same"), b"same"), Decision::Unchanged);
        assert_eq!(decide(true, Some(b"same"), b"same"), Decision::Unchanged);
    }

    /// 缓冲干净就直接采用磁盘内容——这就是"自动重载"。
    #[test]
    fn clean_buffer_adopts_external_content() {
        assert_eq!(decide(false, Some(b"external"), b"opened"), Decision::Reload);
    }

    /// 缓冲脏就只能提示冲突：自动重载绝不能吞掉用户刚敲进去的字。
    #[test]
    fn dirty_buffer_never_gets_overwritten() {
        assert_eq!(decide(true, Some(b"external"), b"opened"), Decision::Conflict);
    }

    #[test]
    fn missing_file_is_reported_as_missing() {
        assert_eq!(decide(false, None, b"opened"), Decision::Missing);
        assert_eq!(decide(true, None, b"opened"), Decision::Missing);
    }

    /// 读文件会产生 Access 事件；把它当改动就会自激。
    #[test]
    fn access_events_are_ignored() {
        let target = Path::new("C:/work/notes.md");
        let read = Event::new(EventKind::Access(AccessKind::Close(AccessMode::Read)))
            .add_path(target.to_path_buf());
        assert!(!event_touches(&read, target));

        let written =
            Event::new(EventKind::Modify(ModifyKind::Any)).add_path(target.to_path_buf());
        assert!(event_touches(&written, target));
    }

    /// 同目录里的其他文件不算目标文件，但仍要触发文件树重读。
    #[test]
    fn sibling_paths_are_structure_only() {
        let target = Path::new("C:/work/notes.md");
        let sibling =
            Event::new(EventKind::Create(CreateKind::File)).add_path(PathBuf::from("C:/work/other.md"));
        assert!(!event_touches(&sibling, target));
    }

    /// 监听要真的能在本机起来并报出改动：这是整条链路里最依赖平台的部分，
    /// 纯函数测试覆盖不到。
    #[test]
    fn watching_a_file_reports_its_writes() {
        let dir = std::env::temp_dir().join(format!("nebula-lite-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("watched.txt");
        std::fs::write(&file, b"first").unwrap();

        let (watch, events) = FileWatch::start(&file).expect("watcher should start");
        std::fs::write(&file, b"second").unwrap();

        // 事件是异步从系统队列过来的，给它足够时间；超时即失败。
        let mut saw_target = false;
        for _ in 0..100 {
            match events.try_recv() {
                Ok(Signal::Target) => {
                    saw_target = true;
                    break;
                },
                Ok(Signal::Structure) => {},
                Err(_) => std::thread::sleep(Duration::from_millis(50)),
            }
        }
        assert!(saw_target, "写入目标文件后应上报 Signal::Target");

        // 监听器被丢弃即停止上报：通道关闭是前台任务退出的依据。
        drop(watch);
        for _ in 0..100 {
            if events.is_closed() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(events.is_closed(), "丢弃监听器后通道应关闭，重载任务才会退出");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
