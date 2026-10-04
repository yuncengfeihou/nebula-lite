//! 原生「选择文件夹」对话框（`IFileOpenDialog` + `FOS_PICKFOLDERS`）。
//!
//! 移植自 Pebrel 的 `nebula_app/src/display/file_dialog/folder.rs`，去掉旧壳
//! (winit) 入口与 WSL 发行版侧栏——本编辑器只需要"选一个目录"这一件事。
//!
//! windows-sys 只提供原始 COM 函数、不生成接口包装，所以 `IFileDialog` 的
//! vtable 顺序必须自己精确排布：下面 `IFileDialogVTable` / `IFileOpenDialogVTable`
//! 里每个 `usize` 占位都对应真实 vtable 的一个方法槽，注释标了槽名。**顺序错一位
//! 就会调到别的方法上**，所以这张表要整段照抄而不是"按需拼"。
//!
//! 对话框是**模态**的，自带消息泵；在 GPUI 的 `update` 借用里直接跑它会重入
//! wndproc、造成 `AppCell` 二次可变借用。所以 [`pick_folder_async`] 先在 UI 线程
//! 取出 HWND，再让**专用线程**跑对话框，结果经 `async_channel` 送回前台任务。

use std::ffi::{OsString, c_void};
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;

use gpui::Window;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows_sys::Win32::Foundation::{HWND, RPC_E_CHANGED_MODE};
use windows_sys::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance,
    CoInitializeEx, CoTaskMemFree, CoUninitialize,
};
use windows_sys::Win32::UI::Shell::{
    FILEOPENDIALOGOPTIONS, FOS_FORCEFILESYSTEM, FOS_NOCHANGEDIR, FOS_PATHMUSTEXIST,
    FOS_PICKFOLDERS, FileOpenDialog, SIGDN, SIGDN_FILESYSPATH,
};
use windows_sys::core::{GUID, HRESULT, PCWSTR, PWSTR};

const FILE_OPEN_DIALOG_IID: GUID = GUID::from_u128(0xd57c7288_d4ad_4768_be02_9d969532d960);
/// `HRESULT_FROM_WIN32(ERROR_CANCELLED)`：用户点了取消，不是错误。
const HRESULT_CANCELLED: HRESULT = 0x800704c7_u32 as HRESULT;

#[repr(C)]
struct Interface<T> {
    vtable: *const T,
}

#[repr(C)]
struct IUnknownVTable {
    _query_interface: usize, // QueryInterface
    _add_ref: usize,         // AddRef
    release: unsafe extern "system" fn(this: *mut c_void) -> u32,
}

#[repr(C)]
struct IModalWindowVTable {
    base: IUnknownVTable,
    show: unsafe extern "system" fn(this: *mut c_void, owner: HWND) -> HRESULT,
}

// IFileDialog 的精确 vtable（继承自 IModalWindow）。
#[repr(C)]
struct IFileDialogVTable {
    base: IModalWindowVTable,
    _set_file_types: usize,
    _set_file_type_index: usize,
    _get_file_type_index: usize,
    _advise: usize,
    _unadvise: usize,
    set_options:
        unsafe extern "system" fn(this: *mut c_void, options: FILEOPENDIALOGOPTIONS) -> HRESULT,
    get_options: unsafe extern "system" fn(
        this: *mut c_void,
        options: *mut FILEOPENDIALOGOPTIONS,
    ) -> HRESULT,
    _set_default_folder: usize,
    _set_folder: usize,
    _get_folder: usize,
    _get_current_selection: usize,
    _set_file_name: usize,
    _get_file_name: usize,
    set_title: unsafe extern "system" fn(this: *mut c_void, title: PCWSTR) -> HRESULT,
    _set_ok_button_label: usize,
    _set_file_name_label: usize,
    get_result: unsafe extern "system" fn(this: *mut c_void, item: *mut *mut c_void) -> HRESULT,
    _add_place: usize,
    _set_default_extension: usize,
    _close: usize,
    _set_client_guid: usize,
    _clear_client_data: usize,
    _set_filter: usize,
}

#[repr(C)]
struct IFileOpenDialogVTable {
    base: IFileDialogVTable,
    _get_results: usize,
    _get_selected_items: usize,
}

#[repr(C)]
struct IShellItemVTable {
    base: IUnknownVTable,
    _bind_to_handler: usize,
    _get_parent: usize,
    get_display_name:
        unsafe extern "system" fn(this: *mut c_void, name_kind: SIGDN, name: *mut PWSTR) -> HRESULT,
    _get_attributes: usize,
    _compare: usize,
}

struct ComApartment {
    should_uninitialize: bool,
}

impl ComApartment {
    fn initialize() -> Option<Self> {
        let result = unsafe {
            CoInitializeEx(
                std::ptr::null(),
                (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
            )
        };
        if result >= 0 {
            return Some(Self { should_uninitialize: true });
        }
        // 线程已经是别的 apartment 时不能配对反初始化，但仍可沿用宿主环境尝试。
        if result == RPC_E_CHANGED_MODE {
            return Some(Self { should_uninitialize: false });
        }
        None
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.should_uninitialize {
            unsafe { CoUninitialize() };
        }
    }
}

struct ComPtr(*mut c_void);

impl ComPtr {
    fn from_raw(pointer: *mut c_void) -> Option<Self> {
        (!pointer.is_null()).then_some(Self(pointer))
    }

    unsafe fn vtable<T>(&self) -> &T {
        let interface = unsafe { &*self.0.cast::<Interface<T>>() };
        unsafe { &*interface.vtable }
    }
}

impl Drop for ComPtr {
    fn drop(&mut self) {
        unsafe {
            let vtable = self.vtable::<IUnknownVTable>();
            (vtable.release)(self.0);
        }
    }
}

struct TaskMemWide(PWSTR);

impl Drop for TaskMemWide {
    fn drop(&mut self) {
        unsafe { CoTaskMemFree(self.0.cast()) };
    }
}

/// 在**专用线程**上弹出目录选择器，结果经通道送回。
///
/// 先在调用方（UI 线程）取出 HWND——`Window`/`HWND` 都不跨线程，所以只把地址
/// 以 `isize` 传过去。无窗口（HWND 为空）时退化为"无主窗口"的对话框。
pub fn pick_folder_async(
    window: &Window,
    title: &'static str,
) -> async_channel::Receiver<Option<PathBuf>> {
    // 只把 HWND 的整数值传进线程：`HWND`（`*mut c_void`）不是 `Send`，而裸地址
    // 在对话框存活期间有效（它由窗口持有，窗口在对话框返回前不会销毁）。
    let owner = owner_hwnd(window);
    let (sender, receiver) = async_channel::unbounded();
    std::thread::spawn(move || {
        let selected = pick_folder(owner, title);
        // 前台任务可能已经退出（窗口关了），发送失败直接忽略。
        let _ = sender.send_blocking(selected);
    });
    receiver
}

/// 取窗口的原生 HWND 并转成 `isize`（可跨线程传的裸值；`HWND` 本身不是 `Send`）。
///
/// 必须显式走 `HasWindowHandle::window_handle`：`Window` 上另有一个同名的固有方法
/// 返回 gpui 自己的 `AnyWindowHandle`，不限定 trait 会调到那一个。
fn owner_hwnd(window: &Window) -> isize {
    match <Window as HasWindowHandle>::window_handle(window).map(|handle| handle.as_raw()) {
        Ok(RawWindowHandle::Win32(handle)) => handle.hwnd.get() as isize,
        _ => 0,
    }
}

fn pick_folder(owner: isize, title: &str) -> Option<PathBuf> {
    let owner = owner as HWND;
    let _apartment = ComApartment::initialize()?;

    let mut dialog_pointer = std::ptr::null_mut();
    let result = unsafe {
        CoCreateInstance(
            &FileOpenDialog,
            std::ptr::null_mut(),
            CLSCTX_INPROC_SERVER,
            &FILE_OPEN_DIALOG_IID,
            &mut dialog_pointer,
        )
    };
    if result < 0 {
        return None;
    }
    let dialog = ComPtr::from_raw(dialog_pointer)?;
    let dialog_vtable = unsafe { dialog.vtable::<IFileOpenDialogVTable>() };

    let mut options = 0;
    let result = unsafe { (dialog_vtable.base.get_options)(dialog.0, &mut options) };
    if result < 0 {
        return None;
    }
    // 只让用户选目录，且必须是文件系统里真实存在的路径。
    let options =
        options | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST | FOS_NOCHANGEDIR;
    let result = unsafe { (dialog_vtable.base.set_options)(dialog.0, options) };
    if result < 0 {
        return None;
    }

    let title = wide(title);
    let result = unsafe { (dialog_vtable.base.set_title)(dialog.0, title.as_ptr()) };
    if result < 0 {
        return None;
    }

    let result = unsafe { (dialog_vtable.base.base.show)(dialog.0, owner) };
    if result == HRESULT_CANCELLED {
        return None;
    }
    if result < 0 {
        return None;
    }

    let mut item_pointer = std::ptr::null_mut();
    let result = unsafe { (dialog_vtable.base.get_result)(dialog.0, &mut item_pointer) };
    if result < 0 {
        return None;
    }
    let item = ComPtr::from_raw(item_pointer)?;
    let item_vtable = unsafe { item.vtable::<IShellItemVTable>() };

    let mut path_pointer = std::ptr::null_mut();
    let result =
        unsafe { (item_vtable.get_display_name)(item.0, SIGDN_FILESYSPATH, &mut path_pointer) };
    if result < 0 || path_pointer.is_null() {
        return None;
    }
    // Shell 返回的是它自己的分配，必须用 CoTaskMemFree 释放（不是 Rust 的堆）。
    let path_pointer = TaskMemWide(path_pointer);
    unsafe { path_from_nul_terminated_wide(path_pointer.0) }
}

/// NUL 结尾的 UTF-16 → `Vec<u16>`（含结尾 0）。
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe fn path_from_nul_terminated_wide(pointer: *const u16) -> Option<PathBuf> {
    if pointer.is_null() {
        return None;
    }
    let mut length = 0;
    while unsafe { *pointer.add(length) } != 0 {
        length += 1;
    }
    let units = unsafe { std::slice::from_raw_parts(pointer, length) };
    path_from_wide_units(units)
}

fn path_from_wide_units(units: &[u16]) -> Option<PathBuf> {
    (!units.is_empty()).then(|| PathBuf::from(OsString::from_wide(units)))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::path_from_wide_units;

    /// 中文本地路径要原样还原（本工作区路径含中文，这条是回归护栏）。
    #[test]
    fn keeps_local_unicode_path() {
        let path = r"D:\普通目录\项目";
        let wide = path.encode_utf16().collect::<Vec<_>>();
        assert_eq!(path_from_wide_units(&wide), Some(PathBuf::from(path)));
    }

    /// 空串当"没有选择"处理，而不是给出空路径。
    #[test]
    fn empty_units_are_no_selection() {
        assert_eq!(path_from_wide_units(&[]), None);
    }
}
