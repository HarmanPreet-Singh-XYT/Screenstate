import AppKit
import Cocoa
import CoreGraphics
import FlutterMacOS

public class DesktopScreenstatePlugin: NSObject, FlutterPlugin {
  public static func register(with registrar: FlutterPluginRegistrar) {
    let channel = FlutterMethodChannel(name: "screenstate", binaryMessenger: registrar.messenger)
    let instance = DesktopScreenstatePlugin(channel)
    registrar.addMethodCallDelegate(instance, channel: channel)
  }

  private let channel: FlutterMethodChannel
  private var isScreenLocked = false

  public init(_ channel: FlutterMethodChannel) {
    self.channel = channel
    super.init()

    let ws = NSWorkspace.shared.notificationCenter

    // True system sleep/wake (CPU suspended)
    ws.addObserver(self, selector: #selector(systemWillSleep),
        name: NSWorkspace.willSleepNotification, object: nil)
    ws.addObserver(self, selector: #selector(systemDidWake),
        name: NSWorkspace.didWakeNotification, object: nil)

    // Display on/off — screen turns off but system stays running
    // e.g. display timeout, manual screen off, lid close without sleep
    ws.addObserver(self, selector: #selector(screenDidTurnOff),
        name: NSWorkspace.screensDidSleepNotification, object: nil)
    ws.addObserver(self, selector: #selector(screenDidTurnOn),
        name: NSWorkspace.screensDidWakeNotification, object: nil)

    // Lock/unlock via distributed notifications
    let center = DistributedNotificationCenter.default()
    center.addObserver(self, selector: #selector(screenIsLocked),
        name: NSNotification.Name("com.apple.screenIsLocked"), object: nil)
    center.addObserver(self, selector: #selector(screenIsUnlocked),
        name: NSNotification.Name("com.apple.screenIsUnlocked"), object: nil)
  }

  deinit {
    NSWorkspace.shared.notificationCenter.removeObserver(self)
    DistributedNotificationCenter.default().removeObserver(self)
  }

  // MARK: - System Sleep / Wake

  @objc func systemWillSleep() {
    dispatchApplicationState(active: "sleep")
  }

  @objc func systemDidWake() {
    // Don't fire awaked if waking into a locked screen —
    // the unlock event will fire separately when user logs back in
    if !isScreenLocked {
      dispatchApplicationState(active: "awaked")
    }
  }

  // MARK: - Display On / Off (system still running)

  @objc func screenDidTurnOff() {
    dispatchApplicationState(active: "screenOff")
  }

  @objc func screenDidTurnOn() {
    // Don't fire screenOn if screen is locked —
    // unlock event will handle resuming tracking
    if !isScreenLocked {
      dispatchApplicationState(active: "screenOn")
    }
  }

  // MARK: - Lock / Unlock

  @objc func screenIsLocked() {
    isScreenLocked = true
    dispatchApplicationState(active: "locked")
  }

  @objc func screenIsUnlocked() {
    isScreenLocked = false
    dispatchApplicationState(active: "unlocked")
  }

  // MARK: - Dispatch

  private func dispatchApplicationState(active: String) {
    channel.invokeMethod("onScreenStateChange", arguments: active)
  }
}