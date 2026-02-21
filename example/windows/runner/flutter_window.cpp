#include "flutter_window.h"

#include <windows.h>
#include <powrprof.h>
#include <optional>
#include "flutter/generated_plugin_registrant.h"

#pragma comment(lib, "powrprof.lib")

// Load WTS functions dynamically to avoid linker issues
namespace {
  typedef BOOL(WINAPI* WTSRegisterFn)(HWND, DWORD);
  typedef BOOL(WINAPI* WTSUnregisterFn)(HWND);

  void RegisterWTSNotification(HWND hwnd) {
    HMODULE lib = LoadLibraryA("wtsapi32.dll");
    if (!lib) return;
    auto fn = (WTSRegisterFn)GetProcAddress(lib, "WTSRegisterSessionNotification");
    if (fn) fn(hwnd, 0); // 0 = NOTIFY_FOR_THIS_SESSION
    FreeLibrary(lib);
  }

  void UnregisterWTSNotification(HWND hwnd) {
    HMODULE lib = LoadLibraryA("wtsapi32.dll");
    if (!lib) return;
    auto fn = (WTSUnregisterFn)GetProcAddress(lib, "WTSUnregisterSessionNotification");
    if (fn) fn(hwnd);
    FreeLibrary(lib);
  }
}

FlutterWindow::FlutterWindow(const flutter::DartProject& project)
    : project_(project) {}

FlutterWindow::~FlutterWindow() {
  // Unregister display state notifications
  if (power_notification_handle_) {
    UnregisterPowerSettingNotification(power_notification_handle_);
    power_notification_handle_ = nullptr;
  }

  // Unregister sleep/resume callback
  if (suspend_resume_handle_) {
    PowerUnregisterSuspendResumeNotification(suspend_resume_handle_);
    suspend_resume_handle_ = nullptr;
  }
}

bool FlutterWindow::OnCreate() {
  if (!Win32Window::OnCreate()) {
    return false;
  }

  RECT frame = GetClientArea();

  flutter_controller_ = std::make_unique<flutter::FlutterViewController>(
      frame.right - frame.left, frame.bottom - frame.top, project_);

  if (!flutter_controller_->engine() || !flutter_controller_->view()) {
    return false;
  }

  RegisterPlugins(flutter_controller_->engine());
  SetChildContent(flutter_controller_->view()->GetNativeWindow());

  // Register for display on/off events (screen off without full sleep)
  power_notification_handle_ = RegisterPowerSettingNotification(
      GetHandle(), &GUID_CONSOLE_DISPLAY_STATE, DEVICE_NOTIFY_WINDOW_HANDLE);

  // Register for lock/unlock events
  RegisterWTSNotification(GetHandle());

  // Register for TRUE system sleep/resume via callback.
  // This fires on a system thread BEFORE the CPU suspends,
  // so it's guaranteed to work even when the message queue is too slow.
  // We PostMessage back to the window so the plugin's HandleWindowProc
  // receives it as a normal WM_POWERBROADCAST message.
  DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS params = {};
  params.Context = this;
  params.Callback = [](PVOID context, ULONG type, PVOID /*setting*/) -> ULONG {
    FlutterWindow* self = static_cast<FlutterWindow*>(context);
    HWND hwnd = self->GetHandle();
    if (!hwnd) return ERROR_SUCCESS;

    switch (type) {
      case PBT_APMSUSPEND:
        // System is going to sleep — post immediately before CPU halts
        PostMessage(hwnd, WM_POWERBROADCAST, PBT_APMSUSPEND, 0);
        break;
      case PBT_APMRESUMEAUTOMATIC:
        // System resumed from sleep
        PostMessage(hwnd, WM_POWERBROADCAST, PBT_APMRESUMEAUTOMATIC, 0);
        break;
    }
    return ERROR_SUCCESS;
  };

  PowerRegisterSuspendResumeNotification(
      DEVICE_NOTIFY_CALLBACK,
      &params,
      &suspend_resume_handle_);

  return true;
}

void FlutterWindow::OnDestroy() {
  UnregisterWTSNotification(GetHandle());

  if (flutter_controller_) {
    flutter_controller_ = nullptr;
  }
  Win32Window::OnDestroy();
}

LRESULT FlutterWindow::MessageHandler(HWND hwnd, UINT const message,
                                      WPARAM const wparam,
                                      LPARAM const lparam) noexcept {
  if (flutter_controller_) {
    std::optional<LRESULT> result =
        flutter_controller_->HandleTopLevelWindowProc(hwnd, message, wparam,
                                                      lparam);
    if (result) {
      return *result;
    }
  }

  switch (message) {
    case WM_FONTCHANGE:
      flutter_controller_->engine()->ReloadSystemFonts();
      break;
  }

  return Win32Window::MessageHandler(hwnd, message, wparam, lparam);
}