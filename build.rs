//! 把 `windows/nebula-lite.rc`（应用图标 + 版本信息）编进可执行文件。
//!
//! 只用 `embed_resource` 编 .rc，不用它管清单：DPI 清单由 GPUI 自己嵌，这里再
//! 塞一份会让多份非默认清单的归属变得不确定（同 `.rc` 里的说明）。

fn main() {
    // build script 里的 `cfg(windows)` 看的是**宿主**平台。宿主与目标在这台机器上
    // 都是 x86_64-pc-windows-gnu，但显式再看一眼目标（`CARGO_CFG_WINDOWS` 只在
    // 目标为 Windows 时设置），免得以后交叉编译时往 ELF/Mach-O 里塞 .rc 资源。
    #[cfg(windows)]
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        // embed_resource 只为 .rc 发 rerun-if-changed，只改 .ico 不会重编。
        println!("cargo:rerun-if-changed=windows/nebula-lite.ico");
        println!("cargo:rerun-if-changed=windows/nebula-lite.rc");
        // `manifest_optional`：.rc 里没有清单是正常的（DPI 清单归 GPUI），但真正
        // 编不过（windres 报错）会在这里 panic，不会静默产出一个没图标的 exe。
        embed_resource::compile("./windows/nebula-lite.rc", embed_resource::NONE)
            .manifest_optional()
            .expect("嵌入 Windows 资源失败");
    }
}
