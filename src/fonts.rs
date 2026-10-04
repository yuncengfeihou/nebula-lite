//! 内嵌字体注册：Maple Mono Normal NF CN。
//!
//! 与 Pebrel 同源同族（`nebula_app/src/font_install.rs` 的
//! `REQUIRED_FONT_FAMILY`）。字体必须在内嵌到二进制里而不是要求系统安装：
//! 系统等宽字体没有 Nerd Font 图标码点，缺了会出方框；内嵌也保证换台机器
//! 打开是同一副字形。

use std::borrow::Cow;

use gpui::App;

/// 与 Pebrel 完全一致的字体族名。主题与编辑器都按这个名字解析。
pub const REQUIRED_FONT_FAMILY: &str = "Maple Mono Normal NF CN";

/// 普通连字版本，Pebrel 的显示栅格器与设置页用它。
static FONT_NORMAL: &[u8] =
    include_bytes!("../assets/fonts/MapleMonoNormal-NF-CN-Regular.ttf");

/// 带 NF 图标的版本，Pebrel 的 GPUI 壳用它。
static FONT_NF: &[u8] = include_bytes!("../assets/fonts/MapleMono-NF-CN-Regular.ttf");

/// 在解析任何字体之前注册内嵌字体。
///
/// 必须早于 `gpui_component::init` 与首个窗口：GPUI 先在系统字体集里找族名，
/// 找不到会静默回落，于是"字体看起来不对"不会有任何报错。
pub fn register(cx: &App) {
    if let Err(error) = cx
        .text_system()
        .add_fonts(vec![Cow::Borrowed(FONT_NORMAL), Cow::Borrowed(FONT_NF)])
    {
        eprintln!("[nebula-lite] 注册内嵌 Maple 字体失败: {error}");
    }
}
