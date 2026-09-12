# macOS window investigation

This investigation establishes the minimum AppKit and Objective-C runtime behavior needed to implement Titan's first native macOS window. It does not define a production API. The implementation used to gather the results was a temporary, standalone Rust program and is not retained in the repository.

## Result and scope

A handwritten Rust program created and showed a titled, closable, miniaturizable, resizable `NSWindow` on Apple Silicon. It retrieved AppKit events, inspected keyboard and pointer data, returned every retrieved event to AppKit for normal dispatch, received resize and close delegate callbacks, and released its owned Objective-C objects after the close callback stopped the event pump.

The experiment used Rust 2024 with no Cargo package or third-party dependency. It linked only the Objective-C runtime and AppKit explicitly. The tested configuration was:

- Apple Silicon (`arm64`)
- macOS 27.0
- macOS SDK 27.0
- Rust 1.98.1

The tested OS is evidence, not the deployment target. The selected deployment target is macOS 11.0 because that is the first macOS release for Apple Silicon. The AppKit and runtime calls used here are older: the latest documented availability among the exercised calls and callbacks is macOS 10.10, while runtime class construction and `NSEventMask` require macOS 10.5 and 10.6 respectively. No API in the experiment requires raising an Apple Silicon binary above macOS 11.0.

## Reproducible experiment

The temporary source consisted of one Rust file. It declared the Objective-C runtime functions, linked AppKit, represented Objective-C object and selector handles as opaque pointers, and used `#[repr(C)]` geometry types. It performed these steps on the process main thread:

1. Assert `pthread_main_np() == 1` and create an outer `NSAutoreleasePool`.
2. Get `NSApplication.sharedApplication` and set `NSApplicationActivationPolicyRegular`.
3. Create an `NSObject` subclass with `objc_allocateClassPair`, install `windowDidResize:` and `windowWillClose:` with `class_addMethod`, then register and instantiate it.
4. Allocate and initialize an `NSWindow` with a 720 by 480 point content rectangle and the titled, closable, miniaturizable, and resizable style bits. Use `NSBackingStoreBuffered`, set `releasedWhenClosed` to false, retain the delegate independently because the window's delegate property is weak, and enable mouse-moved events.
5. Call `finishLaunching`, `makeKeyAndOrderFront:`, and `activateIgnoringOtherApps:`.
6. In each pump iteration, create a local autorelease pool, ask for any event with a date 16 milliseconds in the future in `NSDefaultRunLoopMode`, inspect relevant input fields, call `sendEvent:` when an event was returned, call `updateWindows`, and drain the pool.
7. In `windowWillClose:`, set a process-owned atomic running flag to false. After `sendEvent:` returned and the loop ended, clear the window's delegate, release the delegate and window, and drain the outer pool.

The source was compiled directly rather than added to Titan's workspace:

```sh
MACOSX_DEPLOYMENT_TARGET=11.0 rustc --edition 2024 window_study.rs -o window-study
./window-study 2>&1 | tee observed.log
```

The source used these link declarations, so no build script or manual linker flags were necessary:

```rust
#[link(name = "objc")]
unsafe extern "C" { /* Objective-C runtime declarations */ }

#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {
    static NSDefaultRunLoopMode: Id;
}
```

Inspection with `file`, `otool -l`, and `otool -L` confirmed an arm64 Mach-O with `LC_BUILD_VERSION minos 11.0`, SDK 27.0, and direct loads of `/usr/lib/libobjc.A.dylib` and AppKit. A production Cargo target can use the same `#[link]` declarations; it does not need a third-party binding crate.

## Observed desktop behavior

The desktop exercise used a real AppKit window, not synthesized events or a headless test. The event log printed a readiness marker only after launch completion and ordering the window front. The following behavior was observed:

- A screenshot confirmed that the titled window was visible, and the human operator interacted with it directly.
- Moving into, across, and out of the window produced mouse-entered, mouse-moved, and mouse-exited events. Scrolling produced type 22 events.
- A left click and drag produced left-mouse-down, left-mouse-dragged, and left-mouse-up events (types 1, 6, and 2) with button 0 and changing window coordinates.
- Typing `a`, `s`, `d`, and `f` produced key-down and key-up events (types 10 and 11), virtual key codes 0 through 3, characters, and modifier flags. Modifier changes also arrived as type 12 events.
- Live resizing remained responsive and invoked `windowDidResize:` repeatedly. Logged full-frame sizes changed through values including 701 by 422, 528 by 448, and 543 by 496 points.
- Clicking the red close control invoked `windowWillClose:`, stopped the pump, printed the shutdown-complete marker, and exited with status 0. A fresh window query found no remaining window with the experiment's title.

The resize log reports the full window frame, so its height includes the title bar. The initial request is a 720 by 480 point **content** rectangle; comparing it directly with a captured frame in pixels is not a valid scale or title-bar measurement.

## Objective-C and Rust ABI

`objc/message.h` declares `objc_msgSend` without a useful static signature and requires callers to cast it to an appropriate function-pointer type before calling. The Rust binding must do the same for every distinct method signature. A single variadic declaration, an untyped call, or a generic transmute exposed to safe code is not sufficient.

For the investigated arm64 target, the SDK types map as follows:

| SDK type | Rust representation | Evidence |
| --- | --- | --- |
| `id`, `Class`, `SEL` | opaque pointer | Objective-C runtime headers |
| `BOOL` | `bool` | the arm64 macOS target defines `__OBJC_BOOL_IS_BOOL` as `1`; `objc.h` checks this before its older `TARGET_OS_OSX` fallback |
| `NSInteger` | `isize` | `NSObjCRuntime.h` defines it as `long` under `__LP64__` |
| `NSUInteger`, `NSEventType` | `usize` | `NSObjCRuntime.h` defines `NSUInteger` as `unsigned long` under `__LP64__` |
| `NSEventMask` | `u64` | `NSEvent.h` defines it as `unsigned long long` |
| `CGFloat` | `f64` | 64-bit Core Graphics definition |
| `NSPoint` | `#[repr(C)] { x: f64, y: f64 }` | `NSGeometry.h` aliases it to `CGPoint` on 64-bit |
| `NSSize` | `#[repr(C)] { width: f64, height: f64 }` | `NSGeometry.h` aliases it to `CGSize` on 64-bit |
| `NSRect` | `#[repr(C)] { origin: NSPoint, size: NSSize }` | `NSGeometry.h` aliases it to `CGRect` on 64-bit |

`BOOL` is target-dependent. Do not infer its Rust type only from `TARGET_OS_OSX`; verify the target's `__OBJC_BOOL_IS_BOOL` definition. The investigated `arm64-apple-macos` target uses C `bool`, which matches Rust `bool` at this FFI boundary.

On arm64, call `objc_msgSend` for structure arguments and structure returns as well as scalar calls. The installed runtime header marks `objc_msgSend_stret` unavailable on arm64. The function pointer still has to include the exact `NSPoint` or `NSRect` type so the compiler applies the arm64 aggregate calling convention, including the indirect return convention for a 32-byte `NSRect`.

These are the distinct message signatures exercised. `Id` and `Sel` below are opaque pointers; `Rect` and `Point` are the `#[repr(C)]` types above.

```rust
unsafe extern "C" fn(Id, Sel) -> Id
unsafe extern "C" fn(Id, Sel) -> ()
unsafe extern "C" fn(Id, Sel, Id) -> Id
unsafe extern "C" fn(Id, Sel, Id) -> ()
unsafe extern "C" fn(Id, Sel, bool) -> ()
unsafe extern "C" fn(Id, Sel, isize) -> bool
unsafe extern "C" fn(Id, Sel, Rect, usize, usize, bool) -> Id
unsafe extern "C" fn(Id, Sel, u64, Id, Id, bool) -> Id
unsafe extern "C" fn(Id, Sel, f64) -> Id
unsafe extern "C" fn(Id, Sel, *const c_char) -> Id
unsafe extern "C" fn(Id, Sel) -> usize
unsafe extern "C" fn(Id, Sel) -> isize
unsafe extern "C" fn(Id, Sel) -> u16
unsafe extern "C" fn(Id, Sel) -> Point
unsafe extern "C" fn(Id, Sel) -> Rect
unsafe extern "C" fn(Id, Sel) -> *const c_char
```

They cover allocation and initialization, application policy, the window initializer, setters, event retrieval and dispatch, `NSDate.dateWithTimeIntervalSinceNow:`, `NSString.stringWithUTF8String:`, event properties, `NSNotification.object`, and window geometry. Production code should give these signatures descriptive wrappers tied to their selectors rather than provide a caller-selectable return type.

The runtime declarations needed to construct the delegate are:

```rust
unsafe extern "C" fn objc_getClass(*const c_char) -> Id;
unsafe extern "C" fn sel_registerName(*const c_char) -> Sel;
unsafe extern "C" fn objc_allocateClassPair(Class, *const c_char, usize) -> Class;
unsafe extern "C" fn class_addMethod(Class, Sel, Imp, *const c_char) -> bool;
unsafe extern "C" fn objc_registerClassPair(Class);
```

Both installed methods have Objective-C type encoding `v@:@`: void return, object receiver, selector, and one object argument. Their Rust implementation signature is:

```rust
unsafe extern "C" fn(delegate: Id, command: Sel, notification: Id)
```

The `notification` parameter is not the window. Send it `object` to obtain the `NSWindow`. Callback functions must never unwind into Objective-C. Production callbacks must keep their direct bodies panic-free and place `catch_unwind` at the FFI boundary around any Rust call that could panic, translating failure into process-owned state or a controlled abort.

## Event and callback behavior

`nextEventMatchingMask:untilDate:inMode:dequeue:` can return null at the deadline. A null result is normal and still gives the engine an opportunity to update. Passing `NSEventMaskAny` (`u64::MAX`) and `dequeue: true` removes one event from the application queue. Retrieval does not perform normal AppKit behavior: every non-null event must subsequently be passed to `NSApplication.sendEvent:`. The experiment called `updateWindows` after dispatch.

The following event properties were valid for the categories the experiment logged:

- `type -> NSEventType` and `modifierFlags -> NSEventModifierFlags` for all events.
- `keyCode -> unsigned short` and `characters -> NSString *` only for key-up and key-down events. The SDK warns that `keyCode` raises an exception for a non-key event.
- `locationInWindow -> NSPoint` only for mouse-related events. A mouse-moved event can have a null window, in which case this point is in screen coordinates.
- `buttonNumber -> NSInteger` for mouse button and drag events.

`NSWindow.acceptsMouseMovedEvents` defaults to false and must be enabled to receive ordinary pointer-motion events. Button, drag, scroll, key, and modifier event constants come from `NSEvent.h`; code should use the SDK numeric definitions rather than a Rust enum whose invalid values could cause undefined behavior.

`windowDidResize:` and `windowWillClose:` are optional `NSWindowDelegate` callbacks. AppKit passes an `NSNotification *`, and the notification's `object` is the affected window. The callbacks are main-actor APIs in current Apple documentation. In the experiment they ran synchronously while the main thread dispatched AppKit events. The close callback changed only process-owned state; Objective-C releases happened after callback return.

Live title-bar resize remains responsive because retrieved events are returned to AppKit. AppKit may run its own nested event-tracking work during `sendEvent:`. If Titan later performs its own modal pointer tracking instead of ordinary dispatch, Apple's event-retrieval documentation says to request the relevant drag and mouse-up masks in `NSEventTrackingRunLoopMode`, not the default mode used by this ordinary pump.

## Ownership and lifetime

The experiment used manual reference counting because handwritten message dispatch does not enable Objective-C ARC:

- Objects returned from `alloc` followed by `init` are owned and receive one balancing `release`.
- `NSApplication.sharedApplication`, `NSString.stringWithUTF8String:`, `NSDate.dateWithTimeIntervalSinceNow:`, retrieved events, notification objects, and `NSNotification.object` are borrowed or autoreleased and are not released by the caller.
- The window's `delegate` property is weak. Keep an independently owned delegate alive for at least as long as callbacks are possible, clear `delegate` before releasing it, then release it.
- Set `releasedWhenClosed` to false when retaining the window explicitly. Apple specifically warns ARC clients about this close behavior, and an explicit setting also makes manual ownership predictable.
- Use an outer pool around startup and shutdown and a fresh pool for each custom event-loop iteration. `NSApplication.run` normally supplies per-iteration pools, but a replacement pump must do so itself.
- Do not use an event or deadline object after draining the pool that contains it.
- A registered Objective-C class is process-global. Register it once. Do not dispose of the class while any instance can exist.

All AppKit creation, dispatch, window mutation, and teardown should remain on the process main thread. Apple's threading guidance makes the main thread responsible for event handling and warns that involving other threads in the event path can reorder operations. Engine work may use other threads, but it must hand UI work back to the main thread.

## Guidance for the production window

Issue #14 can use this lifecycle without retaining the experiment:

1. Put opaque runtime calls and typed `objc_msgSend` wrappers in one macOS-only unsafe module.
2. Assert or otherwise enforce creation on the main thread before calling AppKit.
3. Register one delegate class, keep its instance alive independently, and use callbacks only to record state or call panic-free Rust entry points.
4. Keep the `NSWindow` owned until close processing and callback return complete. Clear weak links before releasing owners.
5. Retrieve at most one event per pump call, return non-null events to `sendEvent:`, and let the engine continue on a null deadline result.
6. Drain an autorelease pool on every custom pump iteration.
7. Turn initialization failures such as null class, failed `class_addMethod`, failed activation policy, or null window initialization into explicit Rust errors before entering the pump.

Do not copy the experiment as a parallel executable or public API. Recreate only the minimal typed bindings needed by the runtime crate and keep the deployment target explicit in the macOS build configuration.

## Unresolved questions

The investigation deliberately leaves these decisions visible:

- It verified one window. Delegate ownership and application shutdown with multiple simultaneous windows remain untested.
- It verified normal close through the window control. A bundled application's main menu, Command-Q action, Dock quit action, and `NSApplicationDelegate` termination policy were not part of the standalone program; issue #14 must choose and exercise its quit path.
- It did not test an Objective-C exception crossing Rust. Production code must validate inputs and treat native exceptions as outside the Rust unwind model.
- It did not compare the custom pump with a long-running game loop under modal panels, sheets, input methods, or accessibility features. The ordinary event path tested here is sufficient for the first window, but those integrations may require additional run-loop modes later.
- The exact `BOOL` representation was established only for the arm64 macOS target. A future Intel or other Apple target must derive it from that target's SDK/compiler definitions.

## Primary sources

- Apple, [`objc_msgSend`](https://developer.apple.com/documentation/objectivec/objc_msgsend)
- Apple, [`NSApplication`](https://developer.apple.com/documentation/appkit/nsapplication)
- Apple, [`nextEvent(matching:until:inMode:dequeue:)`](https://developer.apple.com/documentation/appkit/nsapplication/nextevent(matching:until:inmode:dequeue:))
- Apple, [`NSWindow.init(contentRect:styleMask:backing:defer:)`](https://developer.apple.com/documentation/appkit/nswindow/init(contentrect:stylemask:backing:defer:))
- Apple, [`NSWindowDelegate`](https://developer.apple.com/documentation/appkit/nswindowdelegate)
- Apple, [`windowDidResize(_:)`](https://developer.apple.com/documentation/appkit/nswindowdelegate/windowdidresize(_:)) and [`windowWillClose(_:)`](https://developer.apple.com/documentation/appkit/nswindowdelegate/windowwillclose(_:))
- Apple, [`NSWindow.delegate`](https://developer.apple.com/documentation/appkit/nswindow/delegate) and [`acceptsMouseMovedEvents`](https://developer.apple.com/documentation/appkit/nswindow/acceptsmousemovedevents)
- Apple, [`NSEvent.locationInWindow`](https://developer.apple.com/documentation/appkit/nsevent/locationinwindow) and [`keyCode`](https://developer.apple.com/documentation/appkit/nsevent/keycode)
- Apple, [Thread Safety Summary](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/Multithreading/ThreadSafetySummary/ThreadSafetySummary.html)
- Apple, [Memory Management Policy](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/MemoryMgmt/Articles/mmRules.html) and [Using Autorelease Pool Blocks](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/MemoryMgmt/Articles/mmAutoreleasePools.html)

The SDK declarations were checked directly under `$SDKROOT` in `usr/include/objc/{objc.h,message.h,runtime.h}`, `System/Library/Frameworks/Foundation.framework/Headers/{NSObjCRuntime.h,NSGeometry.h}`, and `System/Library/Frameworks/AppKit.framework/Headers/{NSApplication.h,NSWindow.h,NSEvent.h}`.
