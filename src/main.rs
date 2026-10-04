//! nebula-lite —— 基于 Pebrel 的 GPUI 栈构建的本地文本编辑器。
//!
//! 复用面的取舍：主题（Paper 调色板）、字体（内嵌 Maple Mono）、代码编辑器
//! 控件、语言到 tree-sitter 的映射、以及"任何文件都当文本打开"的解码规则，
//! 全部取自 Pebrel 1.9.1。终端、SSH、Lua 配置、AI 钩子等与本编辑器无关的
//! 部分没有复制过来。

mod app;
mod brackets;
mod file_tree;
#[cfg(windows)]
mod folder_picker;
mod fonts;
mod http;
mod icons;
mod image_geom;
mod lang;
mod languages;
mod np3;
mod outline;
mod session;
mod settings;
mod shell;
mod syntax;
mod text_file;
mod theme;
mod urls;
mod watch;

use std::path::PathBuf;
use std::sync::Arc;

use gpui::{
    App, AppContext as _, Bounds, KeyBinding, Pixels, RenderImage, Size, WindowBounds,
    WindowOptions, point, px, size,
};
use gpui_component::{Root, TitleBar};
use image::Frame;

fn main() {
    // 没有参数时打开当前目录；传目录则打开它，传文件则打开该文件并在树里展开。
    let Launch { root: cli_root, file, prefer_preview, explicit } = resolve_target();
    // 会话：上次的标签 / 树根 / 侧栏 / 窗口位置。`None` = 第一次跑，或者会话文件坏了
    // ——那两种情况都完全按命令行参数启动（见 `session::Session::load`）。
    let session = session::Session::load();
    // 树根：命令行给了就用它（`启动 nebula-lite.cmd` 总会给一个目录），否则用上次的
    // （还得真的还是个目录），都没有才回落命令行算出来的那个（即当前工作目录）。
    let root = if explicit {
        cli_root
    } else {
        session
            .as_ref()
            .and_then(|saved| saved.root.clone())
            .filter(|path| path.is_dir())
            .unwrap_or(cli_root)
    };

    gpui_platform::application()
        .with_assets(gpui_component_assets::Assets)
        .with_quit_mode(gpui::QuitMode::LastWindowClosed)
        .run(move |cx: &mut App| {
            // 字体必须早于组件库与窗口：族名解析失败只会静默回落。
            fonts::register(cx);
            // Markdown 预览里的网络图片要能加载，先把 gpui 的图片管线接上 HTTP
            // 客户端（默认是 NullHttpClient，一切请求报错）。
            http::register(cx);
            gpui_component::init(cx);
            // 组件库的 tree-sitter feature 覆盖不到 XML / PowerShell / INI，这三个语法
            // 补在本仓库（见 `languages.rs`）。这里**不需要**显式注册：注册由
            // `lang::language_for_path` 用 `Once` 带起来，而那是语言 id 的唯一产地，
            // 注册与取用因此不可能错开。
            // 组件库自带的文案（查找面板的「替换 / 全部替换」、右键菜单的「剪切 /
            // 复制 / 粘贴」）走它自己的 i18n，默认回落英文。这里跟应用的中文界面
            // 对齐，否则一个中文界面里嵌着两行英文按钮。
            gpui_component::set_locale("zh-CN");
            // 外观基准是 macOS 观感的中性灰白 + 系统蓝（见 `theme` 模块顶部）。
            // 用户在设置面板里选过的字体会在 `Workspace::new` 里覆盖主题的字体字段。
            theme::apply_theme(cx);
            // `Ctrl+S` / 标签管理 / 字号是本应用的窗口级绑定；`Ctrl+F` / `Ctrl+H` 由
            // 组件库绑在 `"Input"` 上下文里，编辑器持有焦点时自然命中（`Workspace::activate`
            // 打开或切换文件即聚焦编辑器，见 `app.rs`）。
            // 字号三键对齐 Pebrel 的 `IncreaseFontSize` / `DecreaseFontSize`
            // 默认绑定（`config/bindings/defaults.rs` 的 `ctrl-=` / `ctrl--`）。
            cx.bind_keys([
                KeyBinding::new("ctrl-s", app::SaveDocument, None),
                KeyBinding::new("ctrl-w", app::CloseTab, None),
                KeyBinding::new("ctrl-tab", app::NextTab, None),
                KeyBinding::new("ctrl-shift-tab", app::PreviousTab, None),
                KeyBinding::new("ctrl-=", app::IncreaseFontSize, None),
                KeyBinding::new("ctrl-+", app::IncreaseFontSize, None),
                KeyBinding::new("ctrl--", app::DecreaseFontSize, None),
                KeyBinding::new("ctrl-0", app::ResetFontSize, None),
                // 打开工作区目录：对齐 VS Code 的 `Ctrl+K Ctrl+O` 太啰嗦，
                // 这里用单段 `Ctrl+Shift+O`（O = Open folder）。
                KeyBinding::new("ctrl-shift-o", app::OpenFolder, None),
            ]);

            // 窗口位置与尺寸：优先用上次那份（会话里），不可信或那块屏幕不在了就回落
            // 居中。尺寸的夹取与"位置还算不算在屏幕上"的判定都在 `resolve_window` /
            // `default_window_size` 里，两个都是纯函数、有单测。
            let display = cx.primary_display();
            let available = display.as_ref().map(|display| display.visible_bounds());
            let (bounds, maximized) =
                match resolve_window(session.as_ref().and_then(|saved| saved.window), available) {
                    Some((origin, size, maximized)) => (Bounds { origin, size }, maximized),
                    None => {
                        let size = default_window_size(available.map(|area| area.size));
                        (Bounds::centered(None, size, cx), false)
                    }
                };
            let window_bounds = if maximized {
                WindowBounds::Maximized(bounds)
            } else {
                WindowBounds::Windowed(bounds)
            };
            let root = root.clone();
            let file = file.clone();
            // 会话要 move 进下面的窗口闭包，这里给内层留一份。
            let session = session.clone();
            // 应用图标解码一次：标题栏左上角要显示它（点它开合侧栏），见 `app.rs`。
            let app_icon = load_app_icon();
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(window_bounds),
                    titlebar: Some(TitleBar::title_bar_options()),
                    app_id: Some("nebula-lite".to_owned()),
                    ..Default::default()
                },
                move |window, cx| {
                    // 初始标题：打开文件时给完整路径，否则给树根（与 `app.rs::set_title`
                    // 的规则一致；打开标签后由 `activate` 再确认一次）。
                    let shown = match file.as_deref() {
                        Some(path) => crate::shell::friendly_path(path),
                        None => crate::shell::friendly_path(&root),
                    };
                    window.set_window_title(&format!(
                        "{shown}\u{2009}—\u{2009}nebula-lite"
                    ));
                    let view = cx.new(|cx| {
                        app::Workspace::new(
                            root.clone(),
                            file.clone(),
                            prefer_preview,
                            session,
                            app_icon,
                            window,
                            cx,
                        )
                    });
                    cx.new(|cx| Root::new(view, window, cx))
                },
            )
            .expect("failed to open main window");
        });
}

/// 标题栏左上角的应用图标：把内嵌在 exe 里的那份 `.ico`（`windows/nebula-lite.ico`，
/// 与资源 ID 1 是同一个文件）解成 gpui 的 BGRA `RenderImage`。
///
/// gpui 的 `img` 元素**不认** `.ico`，也不认文件路径（只有 `Resource`/`Render`），
/// 所以必须自己解码并转成它要的格式——与图片预览那条路同源（见 `app.rs::decode_image`）。
/// 这里用 `include_bytes!` 直接嵌进二进制，省掉"exe 旁边找文件"的部署问题。
///
/// 先缩到 64px 再交出去：ICO 里最大的一帧是 256×256，缩到 64 后由 gpui 线性缩放到
/// 实际显示的 ~20px，边缘比直接拿 256 缩要干净（一次 `thumbnail` 便宜且只做一次）。
fn load_app_icon() -> Option<Arc<RenderImage>> {
    let bytes = include_bytes!("../windows/nebula-lite.ico");
    let decoded =
        image::load_from_memory_with_format(bytes, image::ImageFormat::Ico).ok()?.into_rgba8();
    let (width, height) = decoded.dimensions();
    let decoded = if width > 64 || height > 64 {
        image::imageops::thumbnail(&decoded, 64, 64)
    } else {
        decoded
    };
    let mut rgba = decoded;
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    Some(Arc::new(RenderImage::new([Frame::new(rgba)])))
}

/// 启动参数解析结果。
struct Launch {
    /// 文件树的根目录。
    root: PathBuf,
    /// 启动时直接打开的文件。
    file: Option<PathBuf>,
    /// `--preview`：有预览面的文件以预览打开。
    prefer_preview: bool,
    /// 命令行是否**显式**给了位置参数。
    ///
    /// 用来决定"树根听谁的"：显式给了就听命令行的（启动器总会给一个目录），否则听
    /// 会话里上次那个。标签恢复不受这个影响——两种情况都会恢复。
    explicit: bool,
}

/// 决定文件树根与启动时要打开的文件：命令行位置参数优先，否则用当前工作目录。
fn resolve_target() -> Launch {
    let mut positional: Option<PathBuf> = None;
    let mut prefer_preview = false;
    for argument in std::env::args_os().skip(1) {
        if argument == "--preview" {
            prefer_preview = true;
            continue;
        }
        if positional.is_none() {
            positional = Some(PathBuf::from(argument));
        }
    }

    let explicit = positional.is_some();

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let (candidate, file) = split_target(positional, cwd);
    // 绝对化，避免文件树里出现相对路径拼出来的怪名字。
    let root = std::fs::canonicalize(&candidate).unwrap_or(candidate);
    let file = file.map(|path| std::fs::canonicalize(&path).unwrap_or(path));
    Launch { root, file, prefer_preview, explicit }
}

/// 把位置参数拆成「树根 + 要打开的文件」。
///
/// 传文件时树根取它所在目录：裸文件名（`nebula-lite sample.md`）的
/// `Path::parent()` 返回的是**空路径**而不是 `None`，直接用会让树根变成空串、
/// 文件树整棵空掉，所以空路径要当当前目录处理。
fn split_target(positional: Option<PathBuf>, cwd: PathBuf) -> (PathBuf, Option<PathBuf>) {
    match positional {
        Some(path) if path.is_file() => (parent_or_current(&path, &cwd), Some(path)),
        Some(path) => (path, None),
        None => (cwd, None),
    }
}

fn parent_or_current(path: &std::path::Path, cwd: &std::path::Path) -> PathBuf {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => cwd.to_path_buf(),
    }
}

/// 默认窗口尺寸：1280×820 夹进显示器可用区。
///
/// 不能无条件用 1280×820：高缩放比（本机 125%）下逻辑尺寸换算成物理像素后可能超过
/// 物理屏，窗口右侧会被推到屏幕外——头部右上角的「源码/预览」按钮就永远点不到。
fn default_window_size(available: Option<Size<Pixels>>) -> Size<Pixels> {
    let preferred = size(px(1280.), px(820.));
    match available {
        Some(area) => size(
            preferred.width.min(area.width - px(96.)).max(px(720.)),
            preferred.height.min(area.height - px(96.)).max(px(480.)),
        ),
        None => preferred,
    }
}

/// 由上次的窗口矩形算出这次的窗口位置与尺寸；`None` = 用默认的居中窗口。
///
/// 两种情况要把**位置**丢掉、回到居中：
/// 1. 存下来的尺寸不像个真窗口（手改 / 损坏的会话文件，见 `WindowState::is_plausible`）；
/// 2. 那块区域已经不在当前显示器的可用区里——拔掉外接屏之后最常见。
///
/// 尺寸无论如何都会夹进可用区：换了缩放比或分辨率之后，旧尺寸可能整块超出屏幕。
fn resolve_window(
    saved: Option<session::WindowState>,
    available: Option<Bounds<Pixels>>,
) -> Option<(gpui::Point<Pixels>, Size<Pixels>, bool)> {
    let saved = saved?;
    if !saved.is_plausible() {
        return None;
    }
    let mut window_size = size(px(saved.width), px(saved.height));
    if let Some(area) = available {
        window_size = size(
            window_size.width.min(area.size.width - px(96.)).max(px(720.)),
            window_size.height.min(area.size.height - px(96.)).max(px(480.)),
        );
    }
    let rect = Bounds { origin: point(px(saved.x), px(saved.y)), size: window_size };
    if let Some(area) = available {
        // 至少要露出一块抓得住的标题栏（80×40），否则用户拖不回来。
        let visible = rect.intersect(&area);
        if visible.size.width < px(80.) || visible.size.height < px(40.) {
            return None;
        }
    }
    Some((rect.origin, rect.size, saved.maximized))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    /// 裸文件名（`nebula-lite notes.md`）的 `parent()` 是空路径。这正是
    /// 文件树整棵空掉的那个 bug：空路径不能当目录用，要回落到当前目录。
    #[test]
    fn empty_parent_falls_back_to_the_current_directory() {
        let cwd = PathBuf::from("C:/work");
        assert_eq!(parent_or_current(Path::new("a.md"), &cwd), cwd);
    }

    #[test]
    fn non_empty_parent_is_kept() {
        assert_eq!(
            parent_or_current(Path::new("src/a.md"), Path::new("C:/work")),
            PathBuf::from("src")
        );
        assert_eq!(
            parent_or_current(Path::new("C:/proj/src/a.md"), Path::new("C:/work")),
            PathBuf::from("C:/proj/src")
        );
    }

    #[test]
    fn no_argument_falls_back_to_the_current_directory() {
        let (root, file) = split_target(None, PathBuf::from("C:/work"));
        assert_eq!(root, PathBuf::from("C:/work"));
        assert_eq!(file, None);
    }

    /// 传一个存在的文件：树根取它的目录，并把它作为要打开的文件。
    #[test]
    fn file_argument_yields_its_directory_and_the_file() {
        let dir = std::env::temp_dir();
        let file = dir.join("nebula-lite-split-target-test.txt");
        std::fs::write(&file, b"x").unwrap();
        let (root, opened) = split_target(Some(file.clone()), PathBuf::from("C:/work"));
        assert_eq!(root, dir);
        assert_eq!(opened, Some(file.clone()));
        let _ = std::fs::remove_file(&file);
    }

    /// 传目录（或尚不存在的路径）：当树根，不打开任何文件。
    #[test]
    fn directory_argument_becomes_the_root() {
        let (root, file) = split_target(Some(PathBuf::from("C:/work")), PathBuf::from("C:/other"));
        assert_eq!(root, PathBuf::from("C:/work"));
        assert_eq!(file, None);
    }

    /// 会话里存的窗口矩形：能用就用、越界就夹、不可信就丢掉（回落居中）。
    #[test]
    fn saved_window_bounds_are_restored_clamped_or_dropped() {
        let area = Bounds { origin: point(px(0.), px(0.)), size: size(px(1920.), px(1080.)) };

        // 正常存过：位置与尺寸照用（最大化标志也带回来）。
        let saved = session::WindowState {
            x: 120.0,
            y: 60.0,
            width: 1000.0,
            height: 700.0,
            maximized: false,
        };
        let (origin, window_size, maximized) = resolve_window(Some(saved), Some(area)).unwrap();
        assert_eq!((origin.x, origin.y), (px(120.), px(60.)));
        assert_eq!((window_size.width, window_size.height), (px(1000.), px(700.)));
        assert!(!maximized);

        // 尺寸超出可用区：夹到"可用区 - 96px"。
        let saved = session::WindowState {
            x: 0.0,
            y: 0.0,
            width: 5000.0,
            height: 4000.0,
            maximized: true,
        };
        let (_, window_size, maximized) = resolve_window(Some(saved), Some(area)).unwrap();
        assert_eq!((window_size.width, window_size.height), (px(1824.), px(984.)));
        assert!(maximized);

        // 位置整块在屏幕外（拔掉外接屏）：连位置一起丢掉，调用侧会居中打开。
        let saved = session::WindowState {
            x: -5000.0,
            y: -5000.0,
            width: 1000.0,
            height: 700.0,
            maximized: false,
        };
        assert!(resolve_window(Some(saved), Some(area)).is_none());

        // 尺寸不可信（0×0）：丢掉。
        let saved = session::WindowState { x: 10.0, y: 10.0, width: 0.0, height: 0.0, maximized: false };
        assert!(resolve_window(Some(saved), Some(area)).is_none());

        // 没存过：也走居中那条路。
        assert!(resolve_window(None, Some(area)).is_none());
    }
}
