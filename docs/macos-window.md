# Titan native macOS window

`titan-runtime` owns the first native Titan application lifecycle. It currently
supports one AppKit application and one window on macOS. The runtime does not
provide Metal rendering, game input events, or a cross-platform window trait.

## Requirements

The supported build is an arm64 macOS binary with:

- macOS 11.0 or newer;
- Rust 1.98.0 or newer; and
- Apple's Command Line Tools, including the macOS SDK and linker.

The crate has no Cargo dependencies. It links AppKit, the Objective-C runtime,
and `libSystem` through handwritten declarations. Set the deployment target to
macOS 11.0 with `MACOSX_DEPLOYMENT_TARGET` as shown below. The investigation in
[`macos-window-investigation.md`](macos-window-investigation.md) records the
SDK ABI evidence and the desktop behavior used by this implementation.

On a target other than arm64 macOS, constructing an application returns
`InitError::UnsupportedPlatform`; this is a clear platform error, not a
portable window implementation.

## Running the example

```sh
MACOSX_DEPLOYMENT_TARGET=11.0 cargo run --locked -p titan-runtime --example window
```

The example reports initialization failures to standard error and exits with a
nonzero status. On success it opens a titled, closable, miniaturizable, and
resizable 720 by 480 point window. Drag the resize control to exercise live
resizing. The red close control ends the example and prints `window closed`.

The normal application quit path is native AppKit termination. The runtime
installs a minimal application menu with **Quit** bound to Command-Q, and its
delegate implements `applicationShouldTerminate:` by recording the request,
returning `NSTerminateCancel`, and letting Rust perform orderly shutdown. Dock
**Quit** follows the same delegate path. Its
`applicationShouldTerminateAfterLastWindowClosed:` delegate callback returns
YES, so closing the last window requests application termination as well. The
example reports an application termination as `application quit`; closing the
window control remains `window closed`.

## Lifecycle and safety boundaries

`Application::new` must be called on the process main thread, and
`Application::run` must continue on that thread. The value is deliberately not
sendable to a worker. AppKit creation, delegate installation, event dispatch,
window mutation, and shutdown all remain on the main thread.

The macOS module keeps all unsafe code in one private module. It represents
`id`, `Class`, and `SEL` as opaque pointers; uses the arm64 SDK mappings from
the investigation (`BOOL` as `bool`, `NSInteger` as `isize`, `NSUInteger` as
`usize`, `NSEventMask` as `u64`, and 64-bit `NSRect` as four `f64` values); and
casts `objc_msgSend` separately for every selector signature. Structure
arguments and returns use their exact `#[repr(C)]` Rust types. The public API
never exposes an untyped or caller-selected message-send function.

Titan creates one registered Objective-C delegate class. The class installs
`windowWillClose:`, `applicationShouldTerminate:`,
`applicationShouldTerminateAfterLastWindowClosed:`, and
`applicationWillTerminate:`. The close and termination notifications use the
investigated `v@:@` encoding; the termination reply uses `Q@:@` (`NSUInteger`
on arm64), and the last-window reply uses `B@:@` (`BOOL` on that target).
Callbacks only update process-owned atomic state or return the native
termination decision. Each callback has a `catch_unwind` boundary, so a Rust
panic cannot unwind through Objective-C; a caught callback failure stops the
pump and is reported as `RunError::CallbackFailed`.

The runtime uses manual reference counting:

- the allocated and initialized window and delegate each have one explicit
  balancing `release`;
- the window sets `releasedWhenClosed` to false because Titan owns it across
  close processing;
- AppKit's application, title string, deadlines, events, and notifications
  are borrowed or autoreleased values and are not released by Titan;
- the window and application delegate links are cleared before their owned
  objects are released; and
- an outer autorelease pool covers startup and shutdown, while each custom
  event-pump iteration has its own pool.

The event pump retrieves at most one event with a 16 ms deadline, sends every
non-null event back through `NSApplication.sendEvent:`, calls
`updateWindows`, and then drains that iteration's pool. A null event at the
deadline is normal. Returning events to AppKit is what preserves ordinary
mouse, keyboard, live-resize, and window-control behavior.

This first lifecycle intentionally supports one runtime application/window per
process. Modal panels, sheets, custom menus, Metal, game input translation,
multiple windows, and other run-loop modes are outside issue #14.

## Verification

The production example was exercised on Apple Silicon macOS 27.0 with the
macOS 27.0 SDK. A visible window and live resize were confirmed on the desktop.
The red close control and Command-Q each returned status 0 after the example
reported shutdown complete. Dock Quit uses the same termination delegate but
was not separately verified. macOS 11.0 is the deployment target, not an OS
version exercised in this verification.

An independent Rust consumer exercised invalid dimensions and title, worker
thread rejection, rejection of a second active owner, and dropping then
recreating the application on both stable Rust and Rust 1.98.0. Recreating
the owner reuses an already-regular activation policy instead of asking AppKit
to apply it again.
