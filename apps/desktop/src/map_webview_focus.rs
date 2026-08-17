//! Windows 父窗口焦点守卫；与地图会话状态机分离的平台适配细节。

#![cfg(windows)]
#![allow(
    unsafe_code,
    reason = "Windows subclass callback required to keep Slint keyboard focus instead of WebView2"
)]

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, GetClassLongPtrW, GCLP_WNDPROC, WM_ENTERSIZEMOVE, WM_SETFOCUS, WNDPROC,
};

const FOCUS_GUARD_SUBCLASS_ID: usize = 0x4d43_0001;

unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _uidsubclass: usize,
    dwrefdata: usize,
) -> LRESULT {
    if bypasses_wry(msg) {
        let original: WNDPROC = unsafe { std::mem::transmute(dwrefdata) };
        unsafe { CallWindowProcW(original, hwnd, msg, wparam, lparam) }
    } else {
        unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
    }
}

fn bypasses_wry(msg: u32) -> bool {
    msg == WM_SETFOCUS || msg == WM_ENTERSIZEMOVE
}

pub(crate) fn install(window: &impl HasWindowHandle) {
    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };
    let hwnd = HWND(handle.hwnd.get() as _);
    unsafe {
        let original = GetClassLongPtrW(hwnd, GCLP_WNDPROC);
        if original == 0 {
            log::debug!("map_webview: 焦点守卫未安装（原始窗口过程为空）");
            return;
        }
        log::debug!(
            "map_webview: 安装焦点守卫子类（原始窗口过程=0x{:x}）",
            original
        );
        let _ = SetWindowSubclass(hwnd, Some(subclass_proc), FOCUS_GUARD_SUBCLASS_ID, original);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::WindowsAndMessaging::{WM_MOVE, WM_SIZE};

    #[test]
    fn guard_bypasses_only_focus_messages() {
        assert!(bypasses_wry(WM_SETFOCUS));
        assert!(bypasses_wry(WM_ENTERSIZEMOVE));
        assert!(!bypasses_wry(WM_SIZE));
        assert!(!bypasses_wry(WM_MOVE));
    }
}
