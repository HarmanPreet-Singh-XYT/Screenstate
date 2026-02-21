#include "include/desktop_screenstate/desktop_screenstate_plugin.h"

#include <windows.h>
#include <powrprof.h>
#include <optional>
#include <map>
#include <memory>
#include <sstream>

#include <flutter/event_channel.h>
#include <flutter/event_sink.h>
#include <flutter/event_stream_handler_functions.h>
#include <flutter/method_channel.h>
#include <flutter/plugin_registrar_windows.h>
#include <flutter/standard_method_codec.h>

namespace {

class DesktopScreenstatePlugin : public flutter::Plugin {
 public:
  static void RegisterWithRegistrar(flutter::PluginRegistrarWindows *registrar);

  explicit DesktopScreenstatePlugin(
      flutter::PluginRegistrarWindows *registrar,
      std::unique_ptr<flutter::MethodChannel<flutter::EncodableValue>> channel);

  ~DesktopScreenstatePlugin() override;

 private:
  bool isScreenLocked = false;

  flutter::PluginRegistrarWindows *registrar_;
  int proc_id_;
  std::unique_ptr<flutter::MethodChannel<flutter::EncodableValue>> channel_;

  std::optional<HRESULT> HandleWindowProc(HWND hwnd, UINT message, WPARAM wparam, LPARAM lparam);
};

// static
void DesktopScreenstatePlugin::RegisterWithRegistrar(
    flutter::PluginRegistrarWindows *registrar) {
  auto channel = std::make_unique<flutter::MethodChannel<flutter::EncodableValue>>(
      registrar->messenger(), "screenstate",
      &flutter::StandardMethodCodec::GetInstance());

  HWND hwnd = nullptr;
  if (registrar->GetView()) {
    hwnd = registrar->GetView()->GetNativeWindow();
  }
  if (!hwnd) {
    std::cerr << "DesktopScreenstatePlugin: no flutter window." << std::endl;
    return;
  }

  auto plugin = std::make_unique<DesktopScreenstatePlugin>(registrar, std::move(channel));
  registrar->AddPlugin(std::move(plugin));
}

DesktopScreenstatePlugin::DesktopScreenstatePlugin(
    flutter::PluginRegistrarWindows *registrar,
    std::unique_ptr<flutter::MethodChannel<flutter::EncodableValue>> channel)
    : registrar_(registrar), channel_(std::move(channel)) {

  proc_id_ = registrar_->RegisterTopLevelWindowProcDelegate(
      [this](HWND hwnd, UINT message, WPARAM wparam, LPARAM lparam) {
        return this->HandleWindowProc(hwnd, message, wparam, lparam);
      });
}

DesktopScreenstatePlugin::~DesktopScreenstatePlugin() {
  registrar_->UnregisterTopLevelWindowProcDelegate(proc_id_);
}

std::optional<HRESULT> DesktopScreenstatePlugin::HandleWindowProc(
    HWND hwnd, UINT message, WPARAM wparam, LPARAM lparam) {

  switch (message) {

    case WM_POWERBROADCAST:
      switch (wparam) {

        // ----------------------------------------------------------------
        // Real system sleep (S3/hibernate) — CPU suspended
        // Fired just before the system actually sleeps.
        // No registration needed; delivered to all windows automatically.
        // ----------------------------------------------------------------
        case PBT_APMSUSPEND:
          channel_->InvokeMethod(
              "onScreenStateChange",
              std::make_unique<flutter::EncodableValue>("sleep"));
          break;

        // ----------------------------------------------------------------
        // System resumed from sleep/hibernate.
        // PBT_APMRESUMEAUTOMATIC fires first (always),
        // PBT_APMRESUMESUSPEND fires after if user interaction caused it.
        // We use AUTOMATIC so we catch all resume paths.
        // ----------------------------------------------------------------
        case PBT_APMRESUMEAUTOMATIC:
          // Only fire "awaked" if screen isn't locked after resume.
          // If locked, the unlock event will fire separately via WTS.
          if (!isScreenLocked) {
            channel_->InvokeMethod(
                "onScreenStateChange",
                std::make_unique<flutter::EncodableValue>("awaked"));
          }
          break;

        // ----------------------------------------------------------------
        // Display state change (requires RegisterPowerSettingNotification
        // with GUID_CONSOLE_DISPLAY_STATE in flutter_window.cpp).
        // Values: 0 = off, 1 = on, 2 = dimmed
        // This catches: monitor timeout, manual display off, lid close
        // that only turns off the display without full system sleep.
        // ----------------------------------------------------------------
        case PBT_POWERSETTINGCHANGE: {
          POWERBROADCAST_SETTING *setting = (POWERBROADCAST_SETTING *)lparam;
          if (IsEqualGUID(setting->PowerSetting, GUID_CONSOLE_DISPLAY_STATE)) {
            DWORD displayState = *(DWORD *)setting->Data;
            if (displayState == 0) {
              // Screen turned off but system still running — NOT true sleep
              channel_->InvokeMethod(
                  "onScreenStateChange",
                  std::make_unique<flutter::EncodableValue>("screenOff"));
            } else if (displayState == 1) {
              // Screen turned back on
              if (!isScreenLocked) {
                channel_->InvokeMethod(
                    "onScreenStateChange",
                    std::make_unique<flutter::EncodableValue>("screenOn"));
              }
            }
            // displayState == 2 is dimmed; ignored intentionally
          }
          break;
        }
      }
      break;

    // --------------------------------------------------------------------
    // Session lock/unlock (requires WTSRegisterSessionNotification
    // in flutter_window.cpp).
    // --------------------------------------------------------------------
    case WM_WTSSESSION_CHANGE:
      switch (wparam) {
        case WTS_SESSION_LOCK:
          isScreenLocked = true;
          channel_->InvokeMethod(
              "onScreenStateChange",
              std::make_unique<flutter::EncodableValue>("locked"));
          break;
        case WTS_SESSION_UNLOCK:
          isScreenLocked = false;
          channel_->InvokeMethod(
              "onScreenStateChange",
              std::make_unique<flutter::EncodableValue>("unlocked"));
          break;
      }
      break;
  }

  // Return nullopt to let the default window proc also handle the message
  return std::nullopt;
}

}  // namespace

void DesktopScreenstatePluginRegisterWithRegistrar(
    FlutterDesktopPluginRegistrarRef registrar) {
  DesktopScreenstatePlugin::RegisterWithRegistrar(
      flutter::PluginRegistrarManager::GetInstance()
          ->GetRegistrar<flutter::PluginRegistrarWindows>(registrar));
}