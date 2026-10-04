//! 交给系统的两个动作：「在资源管理器中显示」与「用默认程序打开」。
//!
//! Windows 上 `explorer` 自己解析命令行、**不**遵守 `CommandLineToArgvW`：
//! `/select,<path>` 整体被标准 quoting 包成 `"/select,<path>"` 后，它会在路径的
//! 第一个空格处截断，目标不存在就回退到仍然存在的祖先目录（实测
//! `D:\…\My Projects\…\file` 打开了 `D:\Documents`）。引号必须只包住路径，
//! 即 `/select,"<path>"`，所以用 `raw_arg` 绕过标准 quoting。Windows 路径本身
//! 不能含 `"`，拼接是安全的。
//!
//! 命令构造与 Pebrel 的 `nebula_app/src/platform/file_manager.rs` 逐字一致。

use std::path::Path;
use std::process::{Command, Stdio};

/// 在系统文件管理器中选中该路径。
///
/// 失败返回错误串而不是静默吞掉：`explorer` 一旦没弹窗（复用后台窗口、路径
/// 不存在、拒绝启动），界面上零反馈，只能靠猜——"点了没反应"的报障就是这么来的。
pub fn reveal_in_file_manager(path: &Path) -> Result<(), String> {
    reveal_command(path).spawn().map(|_| ()).map_err(|error| error.to_string())
}

/// 用系统默认程序打开该路径。
pub fn open_with_default_app(path: &Path) -> Result<(), String> {
    command("explorer.exe").arg(path).spawn().map(|_| ()).map_err(|error| error.to_string())
}

/// 无控制台、无继承标准流的 Command（避免子进程弹窗或继承我们的 IO）。
fn command(program: &str) -> Command {
    let mut command = Command::new(program);
    command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    command
}

/// Windows 走 `raw_arg`（原样写入命令行的 select 串）；其它平台用普通 `arg`，
/// 让 argv 内容与 Windows 上完全一致，测试因此不必按平台分叉。
#[cfg(windows)]
fn reveal_command(path: &Path) -> Command {
    use std::os::windows::process::CommandExt as _;
    let mut command = command("explorer.exe");
    command.raw_arg(select_arg(path));
    command
}

#[cfg(not(windows))]
fn reveal_command(path: &Path) -> Command {
    let mut command = command("explorer.exe");
    command.arg(select_arg(path));
    command
}

/// `/select,"<path>"`——引号只包路径，见模块头注释。
fn select_arg(path: &Path) -> std::ffi::OsString {
    let mut select = std::ffi::OsString::from("/select,\"");
    select.push(path.as_os_str());
    select.push(std::ffi::OsStr::new("\""));
    select
}

/// 去掉 Windows verbatim 前缀（`\\?\`），**只用于显示与剪贴板**。
///
/// `std::fs::canonicalize` 在 Windows 上返回 `\\?\C:\…` 形式的 verbatim 路径
/// （它绕过 MAX_PATH 限制与路径规范化）。这个前缀对文件系统 API 有意义，但摆给
/// 人看、或粘进"另存为"对话框，就只是噪音——`\\?\C:\a\b` 与 `C:\a\b` 指向同一处。
/// 传给 `explorer` 之类的命令也走这个（它认不出 verbatim 形式）。
///
/// UNC 的 verbatim 形式 `\\?\UNC\server\share` 折回常规的 `\\server\share`。
/// 非 verbatim 路径原样返回。
pub fn friendly_path(path: &Path) -> String {
    let text = path.display().to_string();
    strip_verbatim(&text).unwrap_or(text)
}

fn strip_verbatim(text: &str) -> Option<String> {
    let rest = text.strip_prefix(r"\\?\")?;
    // 用 `get` 而不是切片：`rest[..4]` 在非 ASCII 边界上会 panic。
    if rest.get(..4).is_some_and(|head| head.eq_ignore_ascii_case("unc\\")) {
        return Some(format!(r"\\{}", &rest[4..]));
    }
    Some(rest.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 引号必须只包住路径：`/select,"C:\a b\c.txt"`。包住整个 `/select,…` 会让
    /// explorer 在第一个空格处截断（模块头记的那次实测）。
    #[test]
    fn select_argument_quotes_only_the_path() {
        let arg = select_arg(Path::new(r"C:\my projects\a b.txt"));
        assert_eq!(arg.to_string_lossy(), r#"/select,"C:\my projects\a b.txt""#);
    }

    /// `\\?\` 前缀要去掉；UNC 的 verbatim 形式折回 `\\server\share`。
    #[test]
    fn friendly_path_strips_the_verbatim_prefix() {
        let cases = [
            (r"\\?\C:\a\b\c.txt", r"C:\a\b\c.txt"),
            (r"\\?\UNC\server\share\f.txt", r"\\server\share\f.txt"),
            // 已经是常规形式就原样返回（含中文路径，验证 `get(..4)` 的边界安全）。
            (r"C:\项目\文件.txt", r"C:\项目\文件.txt"),
            (r"\\server\share\f.txt", r"\\server\share\f.txt"),
        ];
        for (input, expected) in cases {
            assert_eq!(friendly_path(Path::new(input)), expected, "输入 {input}");
        }
    }
}
