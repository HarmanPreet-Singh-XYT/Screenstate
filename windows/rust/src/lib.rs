//! desktop_screenstate — pure Rust Windows implementation.
//!
//! This DLL is loaded by Dart via `dart:ffi`. It creates a hidden
//! message-only window on a dedicated thread, registers for Win32 power
//! and session notifications, and invokes a Dart callback whenever the
//! screen state changes.
//!
//! Exported surface (all `extern "C"`):
//!   screenstate_start(callback)  — start monitoring; call once
//!   screenstate_stop()           — stop monitoring and clean up

#![allow(non_snake_case)]

use std::sync::atomic::{AtomicPtr, Ordering};
use std::thread;

use windows_sys::Win32::Foundation::{
    FreeLibrary, HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM,
};
use windows_sys::Win32::System::LibraryLoader::{
    GetModuleHandleW, GetProcAddress, LoadLibraryA,
};
use windows_sys::Win32::System::Power::{
    DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS, HPOWERNOTIFY,
    PowerRegisterSuspendResumeNotification, PowerUnregisterSuspendResumeNotification,
    RegisterPowerSettingNotification, UnregisterPowerSettingNotification,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, PostMessageW,
    RegisterClassExW, TranslateMessage, CS_HREDRAW, CS_VREDRAW, DEVICE_NOTIFY_CALLBACK,
    DEVICE_NOTIFY_WINDOW_HANDLE, HWND_MESSAGE, MSG, PBT_APMRESUMEAUTOMATIC, PBT_APMSUSPEND,
    PBT_POWERSETTINGCHANGE, REGISTER_NOTIFICATION_FLAGS, WM_DESTROY, WM_POWERBROADCAST, WM_QUIT,
    WM_USER, WM_WTSSESSION_CHANGE, WNDCLASSEXW, WS_OVERLAPPEDWINDOW, WTS_SESSION_LOCK,
    WTS_SESSION_UNLOCK,
};

use windows_sys::core::GUID;

// GUID_CONSOLE_DISPLAY_STATE = {6FE69556-704A-47A0-8F24-C28D936FDA47}
// from powrprof.h — not exposed by windows-sys 0.52
const GUID_CONSOLE_DISPLAY_STATE: GUID = GUID {
    data1: 0x6FE6_9556,
    data2: 0x704A,
    data3: 0x47A0,
    data4: [0x8F, 0x24, 0xC2, 0x8D, 0x93, 0x6F, 0xDA, 0x47],
};

// WM_USER+1: posted by the suspend/resume system callback to the message thread
const WM_SCREENSTATE_SUSPEND: u32 = WM_USER + 1;
const WM_SCREENSTATE_RESUME: u32 = WM_USER + 2;

// POWERBROADCAST_SETTING is in windows-sys but its Data field is [u8; 1]
// representing a flexible array. We define a minimal mirror for casting.
#[repr(C)]
struct PowerbroadcastSetting {
    power_setting: GUID,
    data_length: u32,
    data: [u8; 1],
}

// ── WTS dynamic loading ───────────────────────────────────────────────────────
type WTSRegisterFn = unsafe extern "system" fn(hwnd: HWND, flags: u32) -> i32;
type WTSUnregisterFn = unsafe extern "system" fn(hwnd: HWND) -> i32;

// ── Dart callback ─────────────────────────────────────────────────────────────
// Dart NativeCallable.listener passes a *const u8 (null-terminated UTF-8).
type DartCallback = unsafe extern "C" fn(event: *const u8);

static CALLBACK: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static MSG_HWND: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

// ── Exported FFI ─────────────────────────────────────────────────────────────

/// Start monitoring screen state. `callback` receives null-terminated UTF-8
/// strings: "sleep", "awaked", "screenOff", "screenOn", "locked", "unlocked".
/// Must be called exactly once. Safe to call from any thread/isolate.
#[no_mangle]
pub extern "C" fn screenstate_start(callback: Option<DartCallback>) {
    if let Some(cb) = callback {
        CALLBACK.store(cb as *mut (), Ordering::Release);
        thread::Builder::new()
            .name("screenstate-msg".into())
            .spawn(message_loop)
            .expect("failed to spawn screenstate thread");
    }
}

/// Stop monitoring. Posts WM_QUIT to the message thread. Safe to call from any
/// thread. No further callbacks will fire after this returns.
#[no_mangle]
pub extern "C" fn screenstate_stop() {
    CALLBACK.store(std::ptr::null_mut(), Ordering::Release);
    let hwnd = MSG_HWND.load(Ordering::Acquire) as HWND;
    if hwnd != 0 {
        unsafe { PostMessageW(hwnd, WM_QUIT, 0, 0) };
    }
}

// ── Message loop (dedicated thread) ──────────────────────────────────────────

fn message_loop() {
    unsafe {
        let hinstance = GetModuleHandleW(std::ptr::null()) as HINSTANCE;

        let class_name = wide_null("ScreenstateWnd");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance,
            hIcon: 0,
            hCursor: 0,
            hbrBackground: 0,
            lpszMenuName: std::ptr::null(),
            lpszClassName: class_name.as_ptr(),
            hIconSm: 0,
        };
        RegisterClassExW(&wc);

        let window_name = wide_null("screenstate");
        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            window_name.as_ptr(),
            WS_OVERLAPPEDWINDOW,
            0, 0, 0, 0,
            HWND_MESSAGE, // message-only window — no painting, no taskbar
            0,
            hinstance,
            std::ptr::null(),
        );

        if hwnd == 0 {
            return;
        }

        MSG_HWND.store(hwnd as *mut (), Ordering::Release);

        // Register for display on/off (screen off without full sleep).
        let power_handle: HPOWERNOTIFY = RegisterPowerSettingNotification(
            hwnd as HANDLE,
            &GUID_CONSOLE_DISPLAY_STATE,
            DEVICE_NOTIFY_WINDOW_HANDLE as u32,
        );

        // Register for true system sleep/resume via a callback that fires on a
        // system thread *before* the CPU halts. We post to our window so the
        // actual handling stays on this thread.
        let hwnd_for_cb = hwnd;
        let mut suspend_params = DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS {
            Callback: Some(suspend_resume_callback),
            Context: hwnd_for_cb as *mut core::ffi::c_void,
        };
        let mut suspend_handle: *mut core::ffi::c_void = std::ptr::null_mut();
        PowerRegisterSuspendResumeNotification(
            DEVICE_NOTIFY_CALLBACK as REGISTER_NOTIFICATION_FLAGS,
            &mut suspend_params as *mut _ as HANDLE,
            &mut suspend_handle,
        );

        // Register for session lock/unlock.
        wts_register(hwnd);

        // Run the message loop.
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, 0, 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        // Cleanup on exit.
        if power_handle != 0 {
            UnregisterPowerSettingNotification(power_handle);
        }
        if !suspend_handle.is_null() {
            PowerUnregisterSuspendResumeNotification(suspend_handle as HPOWERNOTIFY);
        }
        wts_unregister(hwnd);
        DestroyWindow(hwnd);
        MSG_HWND.store(std::ptr::null_mut(), Ordering::Release);
    }
}

// ── Window procedure ──────────────────────────────────────────────────────────

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    thread_local! {
        static IS_LOCKED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }

    match msg {
        // Posted by suspend_resume_callback from the system thread.
        WM_SCREENSTATE_SUSPEND => fire(b"sleep\0"),
        WM_SCREENSTATE_RESUME => {
            if !IS_LOCKED.with(|l| l.get()) {
                fire(b"awaked\0");
            }
        }

        WM_POWERBROADCAST => match wparam as u32 {
            PBT_POWERSETTINGCHANGE => {
                let setting = &*(lparam as *const PowerbroadcastSetting);
                if guids_equal(&setting.power_setting, &GUID_CONSOLE_DISPLAY_STATE) {
                    match *setting.data.as_ptr() {
                        0 => fire(b"screenOff\0"),
                        1 => {
                            if !IS_LOCKED.with(|l| l.get()) {
                                fire(b"screenOn\0");
                            }
                        }
                        _ => {} // 2 = dimmed, ignored
                    }
                }
            }
            // Fallback path in case the callback approach is unavailable.
            PBT_APMSUSPEND => fire(b"sleep\0"),
            PBT_APMRESUMEAUTOMATIC => {
                if !IS_LOCKED.with(|l| l.get()) {
                    fire(b"awaked\0");
                }
            }
            _ => {}
        },

        WM_WTSSESSION_CHANGE => match wparam as u32 {
            WTS_SESSION_LOCK => {
                IS_LOCKED.with(|l| l.set(true));
                fire(b"locked\0");
            }
            WTS_SESSION_UNLOCK => {
                IS_LOCKED.with(|l| l.set(false));
                fire(b"unlocked\0");
            }
            _ => {}
        },

        WM_DESTROY => {
            PostMessageW(hwnd, WM_QUIT, 0, 0);
        }

        _ => {}
    }

    DefWindowProcW(hwnd, msg, wparam, lparam)
}

// ── Suspend/resume system callback ───────────────────────────────────────────
// Runs on a Windows system thread — must not block. Posts to the message window.

unsafe extern "system" fn suspend_resume_callback(
    context: *const core::ffi::c_void,
    event_type: u32,
    _setting: *const core::ffi::c_void,
) -> u32 {
    let hwnd = context as HWND;
    match event_type {
        PBT_APMSUSPEND => {
            PostMessageW(hwnd, WM_SCREENSTATE_SUSPEND, 0, 0);
        }
        PBT_APMRESUMEAUTOMATIC => {
            PostMessageW(hwnd, WM_SCREENSTATE_RESUME, 0, 0);
        }
        _ => {}
    }
    0 // ERROR_SUCCESS
}

// ── WTS session notification (dynamic load to avoid hard linker dependency) ───

unsafe fn wts_register(hwnd: HWND) {
    let lib = LoadLibraryA(b"wtsapi32.dll\0".as_ptr());
    if lib == 0 {
        return;
    }
    let proc = GetProcAddress(lib, b"WTSRegisterSessionNotification\0".as_ptr());
    if let Some(f) = proc {
        let f: WTSRegisterFn = std::mem::transmute(f);
        f(hwnd, 0); // NOTIFY_FOR_THIS_SESSION = 0
    }
    FreeLibrary(lib);
}

unsafe fn wts_unregister(hwnd: HWND) {
    let lib = LoadLibraryA(b"wtsapi32.dll\0".as_ptr());
    if lib == 0 {
        return;
    }
    let proc = GetProcAddress(lib, b"WTSUnregisterSessionNotification\0".as_ptr());
    if let Some(f) = proc {
        let f: WTSUnregisterFn = std::mem::transmute(f);
        f(hwnd);
    }
    FreeLibrary(lib);
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn fire(event: &'static [u8]) {
    let ptr = CALLBACK.load(Ordering::Acquire);
    if ptr.is_null() {
        return;
    }
    let cb: DartCallback = unsafe { std::mem::transmute(ptr) };
    unsafe { cb(event.as_ptr()) };
}

fn guids_equal(a: &GUID, b: &GUID) -> bool {
    a.data1 == b.data1 && a.data2 == b.data2 && a.data3 == b.data3 && a.data4 == b.data4
}

fn wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
