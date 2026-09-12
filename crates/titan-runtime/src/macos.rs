#![allow(non_snake_case, non_upper_case_globals)]
//! Handwritten AppKit and Objective-C runtime bindings for the macOS backend.
//!
//! `objc_msgSend` is declared once without a usable Rust signature and is
//! converted to one explicit function-pointer type per selector signature.
//! The public runtime never exposes these bindings. All AppKit calls and
//! callback state changes happen on the process main thread.

use core::marker::PhantomData;
use core::ptr::null_mut;
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicU8, Ordering};
use std::ffi::{CStr, CString, c_char, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::OnceLock;

use crate::{ExitReason, InitError, RunError, WindowConfig};

type Id = *mut c_void;
type Class = *mut c_void;
type Sel = *mut c_void;
type Imp = unsafe extern "C" fn();

type MsgId = unsafe extern "C" fn(Id, Sel) -> Id;
type MsgVoid = unsafe extern "C" fn(Id, Sel);
type MsgIdId = unsafe extern "C" fn(Id, Sel, Id) -> Id;
type MsgVoidId = unsafe extern "C" fn(Id, Sel, Id);
type MsgVoidBool = unsafe extern "C" fn(Id, Sel, bool);
type MsgVoidUsize = unsafe extern "C" fn(Id, Sel, usize);
type MsgBoolIsize = unsafe extern "C" fn(Id, Sel, isize) -> bool;
type MsgIsize = unsafe extern "C" fn(Id, Sel) -> isize;
type MsgIdIdSelId = unsafe extern "C" fn(Id, Sel, Id, Sel, Id) -> Id;
type MsgIdRectUsizeUsizeBool = unsafe extern "C" fn(Id, Sel, Rect, usize, usize, bool) -> Id;
type MsgIdU64IdIdBool = unsafe extern "C" fn(Id, Sel, u64, Id, Id, bool) -> Id;
type MsgIdF64 = unsafe extern "C" fn(Id, Sel, f64) -> Id;
type MsgIdCStr = unsafe extern "C" fn(Id, Sel, *const c_char) -> Id;

#[repr(C)]
#[derive(Clone, Copy)]
struct Point {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Size {
    width: f64,
    height: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Rect {
    origin: Point,
    size: Size,
}

const STYLE_TITLED: usize = 1 << 0;
const STYLE_CLOSABLE: usize = 1 << 1;
const STYLE_MINIATURIZABLE: usize = 1 << 2;
const STYLE_RESIZABLE: usize = 1 << 3;
const BACKING_STORE_BUFFERED: usize = 2;
const ACTIVATION_POLICY_REGULAR: isize = 0;
const EVENT_MASK_ANY: u64 = u64::MAX;
const EVENT_WAIT_SECONDS: f64 = 0.016;
const KEY_COMMAND: usize = 1 << 20;
const TERMINATE_CANCEL: usize = 0;
const TERMINATE_AFTER_LAST_WINDOW: bool = true;

const EXIT_NONE: u8 = 0;
const EXIT_WINDOW_CLOSED: u8 = 1;
const EXIT_APPLICATION_QUIT: u8 = 2;

static DELEGATE_CLASS: OnceLock<usize> = OnceLock::new();
static CALLBACK_STATE: AtomicPtr<CallbackState> = AtomicPtr::new(null_mut());

#[link(name = "objc")]
unsafe extern "C" {
    fn objc_msgSend();
    fn objc_getClass(name: *const c_char) -> Class;
    fn sel_registerName(name: *const c_char) -> Sel;
    fn objc_allocateClassPair(superclass: Class, name: *const c_char, extra_bytes: usize) -> Class;
    fn objc_disposeClassPair(class_: Class);
    fn class_addMethod(
        class_: Class,
        selector: Sel,
        implementation: Imp,
        types: *const c_char,
    ) -> bool;
    fn objc_registerClassPair(class_: Class);
}

#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {
    static NSDefaultRunLoopMode: Id;
}

#[link(name = "System")]
unsafe extern "C" {
    fn pthread_main_np() -> i32;
}

struct CallbackState {
    running: AtomicBool,
    exit_reason: AtomicU8,
    callback_failed: AtomicBool,
}

impl CallbackState {
    fn new() -> Self {
        Self {
            running: AtomicBool::new(true),
            exit_reason: AtomicU8::new(EXIT_NONE),
            callback_failed: AtomicBool::new(false),
        }
    }

    fn stop(&self, reason: u8) {
        let _ = self.exit_reason.compare_exchange(
            EXIT_NONE,
            reason,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        self.running.store(false, Ordering::Release);
    }
}

fn callback_state() -> Option<&'static CallbackState> {
    let pointer = CALLBACK_STATE.load(Ordering::Acquire);
    if pointer.is_null() {
        None
    } else {
        // The pointer is published only after native delegates are installed
        // and is cleared before either delegate or window ownership is released.
        Some(unsafe { &*pointer })
    }
}

fn mark_callback_failed() {
    if let Some(state) = callback_state() {
        state.callback_failed.store(true, Ordering::Release);
        state.stop(EXIT_APPLICATION_QUIT);
    }
}

fn callback_boundary(action: impl FnOnce()) {
    if let Err(payload) = catch_unwind(AssertUnwindSafe(action)) {
        // A panic payload can itself panic when dropped. Never drop it at an
        // FFI boundary; record failure and let the event loop shut down.
        core::mem::forget(payload);
        mark_callback_failed();
    }
}

fn callback_boundary_return<T>(fallback: T, action: impl FnOnce() -> T) -> T {
    match catch_unwind(AssertUnwindSafe(action)) {
        Ok(value) => value,
        Err(payload) => {
            core::mem::forget(payload);
            mark_callback_failed();
            fallback
        }
    }
}

unsafe extern "C" fn window_will_close(_: Id, _: Sel, _: Id) {
    callback_boundary(|| {
        if let Some(state) = callback_state() {
            state.stop(EXIT_WINDOW_CLOSED);
        }
    });
}

unsafe extern "C" fn application_should_terminate(_: Id, _: Sel, _: Id) -> usize {
    callback_boundary_return(TERMINATE_CANCEL, || {
        if let Some(state) = callback_state() {
            state.stop(EXIT_APPLICATION_QUIT);
        }
        // Keep AppKit from terminating the process before the Rust owner has
        // cleared delegates, menus, and retained native objects.
        TERMINATE_CANCEL
    })
}

unsafe extern "C" fn application_should_terminate_after_last_window_closed(
    _: Id,
    _: Sel,
    _: Id,
) -> bool {
    callback_boundary_return(false, || TERMINATE_AFTER_LAST_WINDOW)
}

unsafe extern "C" fn application_will_terminate(_: Id, _: Sel, _: Id) {
    callback_boundary(|| {
        if let Some(state) = callback_state() {
            state.stop(EXIT_APPLICATION_QUIT);
        }
    });
}

fn is_main_thread() -> bool {
    unsafe { pthread_main_np() != 0 }
}

fn selector(name: &'static CStr) -> Sel {
    unsafe { sel_registerName(name.as_ptr()) }
}

unsafe fn send_id(receiver: Id, command: Sel) -> Id {
    let send: MsgId = unsafe { core::mem::transmute(objc_msgSend as *const ()) };
    unsafe { send(receiver, command) }
}

unsafe fn send_void(receiver: Id, command: Sel) {
    let send: MsgVoid = unsafe { core::mem::transmute(objc_msgSend as *const ()) };
    unsafe { send(receiver, command) }
}

unsafe fn send_id_id(receiver: Id, command: Sel, argument: Id) -> Id {
    let send: MsgIdId = unsafe { core::mem::transmute(objc_msgSend as *const ()) };
    unsafe { send(receiver, command, argument) }
}

unsafe fn send_void_id(receiver: Id, command: Sel, argument: Id) {
    let send: MsgVoidId = unsafe { core::mem::transmute(objc_msgSend as *const ()) };
    unsafe { send(receiver, command, argument) }
}

unsafe fn send_void_bool(receiver: Id, command: Sel, argument: bool) {
    let send: MsgVoidBool = unsafe { core::mem::transmute(objc_msgSend as *const ()) };
    unsafe { send(receiver, command, argument) }
}

unsafe fn send_void_usize(receiver: Id, command: Sel, argument: usize) {
    let send: MsgVoidUsize = unsafe { core::mem::transmute(objc_msgSend as *const ()) };
    unsafe { send(receiver, command, argument) }
}

unsafe fn send_bool_isize(receiver: Id, command: Sel, argument: isize) -> bool {
    let send: MsgBoolIsize = unsafe { core::mem::transmute(objc_msgSend as *const ()) };
    unsafe { send(receiver, command, argument) }
}

unsafe fn activation_policy(application: Id) -> isize {
    let send: MsgIsize = unsafe { core::mem::transmute(objc_msgSend as *const ()) };
    unsafe { send(application, selector(c"activationPolicy")) }
}

unsafe fn send_id_id_sel_id(
    receiver: Id,
    command: Sel,
    title: Id,
    action: Sel,
    key_equivalent: Id,
) -> Id {
    let send: MsgIdIdSelId = unsafe { core::mem::transmute(objc_msgSend as *const ()) };
    unsafe { send(receiver, command, title, action, key_equivalent) }
}

unsafe fn send_id_rect_usize_usize_bool(
    receiver: Id,
    command: Sel,
    rect: Rect,
    style: usize,
    backing: usize,
    defer: bool,
) -> Id {
    let send: MsgIdRectUsizeUsizeBool = unsafe { core::mem::transmute(objc_msgSend as *const ()) };
    unsafe { send(receiver, command, rect, style, backing, defer) }
}

unsafe fn send_id_u64_id_id_bool(
    receiver: Id,
    command: Sel,
    mask: u64,
    date: Id,
    mode: Id,
    dequeue: bool,
) -> Id {
    let send: MsgIdU64IdIdBool = unsafe { core::mem::transmute(objc_msgSend as *const ()) };
    unsafe { send(receiver, command, mask, date, mode, dequeue) }
}

unsafe fn send_id_f64(receiver: Id, command: Sel, argument: f64) -> Id {
    let send: MsgIdF64 = unsafe { core::mem::transmute(objc_msgSend as *const ()) };
    unsafe { send(receiver, command, argument) }
}

unsafe fn send_id_cstr(receiver: Id, command: Sel, argument: *const c_char) -> Id {
    let send: MsgIdCStr = unsafe { core::mem::transmute(objc_msgSend as *const ()) };
    unsafe { send(receiver, command, argument) }
}

unsafe fn release(object: Id) {
    if !object.is_null() {
        unsafe { send_void(object, selector(c"release")) };
    }
}

struct AutoreleasePool {
    object: Id,
}

impl AutoreleasePool {
    fn new() -> Option<Self> {
        unsafe {
            let class = objc_getClass(c"NSAutoreleasePool".as_ptr());
            if class.is_null() {
                return None;
            }
            let allocated = send_id(class, selector(c"alloc"));
            if allocated.is_null() {
                return None;
            }
            let object = send_id(allocated, selector(c"init"));
            if object.is_null() {
                return None;
            }
            Some(Self { object })
        }
    }
}

impl Drop for AutoreleasePool {
    fn drop(&mut self) {
        unsafe { release(self.object) };
    }
}

unsafe fn register_delegate_class() -> Result<Class, InitError> {
    if let Some(class_) = DELEGATE_CLASS.get() {
        return Ok(*class_ as Class);
    }

    let superclass = unsafe { objc_getClass(c"NSObject".as_ptr()) };
    if superclass.is_null() {
        return Err(InitError::NSObjectUnavailable);
    }

    let class_ = unsafe { objc_allocateClassPair(superclass, c"TitanRuntimeDelegate".as_ptr(), 0) };
    if class_.is_null() {
        return Err(InitError::DelegateClassAllocationFailed);
    }

    let methods: [(&'static CStr, Imp, &'static CStr); 4] = [
        (
            c"windowWillClose:",
            unsafe {
                core::mem::transmute::<unsafe extern "C" fn(Id, Sel, Id), Imp>(window_will_close)
            },
            c"v@:@",
        ),
        (
            c"applicationShouldTerminate:",
            unsafe {
                core::mem::transmute::<unsafe extern "C" fn(Id, Sel, Id) -> usize, Imp>(
                    application_should_terminate,
                )
            },
            c"Q@:@",
        ),
        (
            c"applicationShouldTerminateAfterLastWindowClosed:",
            unsafe {
                core::mem::transmute::<unsafe extern "C" fn(Id, Sel, Id) -> bool, Imp>(
                    application_should_terminate_after_last_window_closed,
                )
            },
            c"B@:@",
        ),
        (
            c"applicationWillTerminate:",
            unsafe {
                core::mem::transmute::<unsafe extern "C" fn(Id, Sel, Id), Imp>(
                    application_will_terminate,
                )
            },
            c"v@:@",
        ),
    ];
    for (name, implementation, encoding) in methods {
        let installed = unsafe {
            class_addMethod(
                class_,
                selector(name),
                implementation,
                encoding.as_ptr().cast(),
            )
        };
        if !installed {
            unsafe { objc_disposeClassPair(class_) };
            return Err(InitError::DelegateMethodRegistrationFailed(
                name.to_str().unwrap_or("unknown"),
            ));
        }
    }

    unsafe { objc_registerClassPair(class_) };
    if DELEGATE_CLASS.set(class_ as usize).is_err() {
        return Err(InitError::AlreadyRunning);
    }
    Ok(class_)
}

fn install_quit_menu(application: Id) -> Result<(), InitError> {
    unsafe {
        let menu_class = objc_getClass(c"NSMenu".as_ptr());
        let item_class = objc_getClass(c"NSMenuItem".as_ptr());
        let string_class = objc_getClass(c"NSString".as_ptr());
        if menu_class.is_null() || item_class.is_null() || string_class.is_null() {
            return Err(InitError::MenuCreationFailed);
        }

        let app_title = send_id_cstr(
            string_class,
            selector(c"stringWithUTF8String:"),
            c"Titan".as_ptr(),
        );
        let quit_title = send_id_cstr(
            string_class,
            selector(c"stringWithUTF8String:"),
            c"Quit".as_ptr(),
        );
        let empty_key = send_id_cstr(
            string_class,
            selector(c"stringWithUTF8String:"),
            c"".as_ptr(),
        );
        let quit_key = send_id_cstr(
            string_class,
            selector(c"stringWithUTF8String:"),
            c"q".as_ptr(),
        );
        if app_title.is_null() || quit_title.is_null() || empty_key.is_null() || quit_key.is_null()
        {
            return Err(InitError::MenuCreationFailed);
        }

        let main_menu_allocated = send_id(menu_class, selector(c"alloc"));
        if main_menu_allocated.is_null() {
            return Err(InitError::MenuCreationFailed);
        }
        let main_menu = send_id_id(main_menu_allocated, selector(c"initWithTitle:"), app_title);
        if main_menu.is_null() {
            return Err(InitError::MenuCreationFailed);
        }

        let app_menu_allocated = send_id(menu_class, selector(c"alloc"));
        if app_menu_allocated.is_null() {
            release(main_menu);
            return Err(InitError::MenuCreationFailed);
        }
        let app_menu = send_id_id(app_menu_allocated, selector(c"initWithTitle:"), app_title);
        if app_menu.is_null() {
            release(main_menu);
            return Err(InitError::MenuCreationFailed);
        }

        let root_item_allocated = send_id(item_class, selector(c"alloc"));
        if root_item_allocated.is_null() {
            release(app_menu);
            release(main_menu);
            return Err(InitError::MenuCreationFailed);
        }
        let root_item = send_id_id_sel_id(
            root_item_allocated,
            selector(c"initWithTitle:action:keyEquivalent:"),
            app_title,
            null_mut(),
            empty_key,
        );
        if root_item.is_null() {
            release(app_menu);
            release(main_menu);
            return Err(InitError::MenuCreationFailed);
        }

        let quit_item_allocated = send_id(item_class, selector(c"alloc"));
        if quit_item_allocated.is_null() {
            release(root_item);
            release(app_menu);
            release(main_menu);
            return Err(InitError::MenuCreationFailed);
        }
        let quit_item = send_id_id_sel_id(
            quit_item_allocated,
            selector(c"initWithTitle:action:keyEquivalent:"),
            quit_title,
            selector(c"terminate:"),
            quit_key,
        );
        if quit_item.is_null() {
            release(root_item);
            release(app_menu);
            release(main_menu);
            return Err(InitError::MenuCreationFailed);
        }

        send_void_id(root_item, selector(c"setSubmenu:"), app_menu);
        send_void_id(main_menu, selector(c"addItem:"), root_item);
        send_void_id(app_menu, selector(c"addItem:"), quit_item);
        send_void_usize(
            quit_item,
            selector(c"setKeyEquivalentModifierMask:"),
            KEY_COMMAND,
        );
        send_void_id(application, selector(c"setMainMenu:"), main_menu);

        // NSApplication retains the main menu; each menu retains the items and
        // submenu. Release only the setup owners here.
        release(quit_item);
        release(root_item);
        release(app_menu);
        release(main_menu);
        Ok(())
    }
}

fn release_initialization_objects(application: Id, window: Id, delegate: Id) {
    unsafe {
        if !application.is_null() {
            send_void_id(application, selector(c"setMainMenu:"), null_mut());
            send_void_id(application, selector(c"setDelegate:"), null_mut());
        }
        if !window.is_null() {
            send_void_id(window, selector(c"setDelegate:"), null_mut());
            send_void(window, selector(c"close"));
            release(window);
        }
        release(delegate);
    }
}

pub(crate) struct Application {
    application: Id,
    window: Id,
    delegate: Id,
    state: Box<CallbackState>,
    // The native API is main-thread-only. This marker also prevents an
    // Application from being moved to a worker where Drop could run there.
    _main_thread: PhantomData<Rc<()>>,
}

impl Application {
    pub(crate) fn new(config: WindowConfig) -> Result<Self, InitError> {
        if !is_main_thread() {
            return Err(InitError::MainThreadRequired);
        }
        if !config.content_width.is_finite()
            || !config.content_height.is_finite()
            || config.content_width <= 0.0
            || config.content_height <= 0.0
        {
            return Err(InitError::InvalidContentSize);
        }
        let title = CString::new(config.title).map_err(|_| InitError::InvalidTitle)?;
        let _startup_pool = AutoreleasePool::new().ok_or(InitError::AutoreleasePoolUnavailable)?;
        if !CALLBACK_STATE.load(Ordering::Acquire).is_null() {
            return Err(InitError::AlreadyRunning);
        }

        unsafe {
            let application_class = objc_getClass(c"NSApplication".as_ptr());
            if application_class.is_null() {
                return Err(InitError::ApplicationUnavailable);
            }
            let application = send_id(application_class, selector(c"sharedApplication"));
            if application.is_null() {
                return Err(InitError::ApplicationUnavailable);
            }
            if activation_policy(application) != ACTIVATION_POLICY_REGULAR
                && !send_bool_isize(
                    application,
                    selector(c"setActivationPolicy:"),
                    ACTIVATION_POLICY_REGULAR,
                )
            {
                return Err(InitError::ActivationPolicyRejected);
            }
            let delegate_class = register_delegate_class()?;
            install_quit_menu(application)?;
            let delegate_allocated = send_id(delegate_class, selector(c"alloc"));
            if delegate_allocated.is_null() {
                release_initialization_objects(application, null_mut(), null_mut());
                return Err(InitError::DelegateCreationFailed);
            }
            let delegate = send_id(delegate_allocated, selector(c"init"));
            if delegate.is_null() {
                release_initialization_objects(application, null_mut(), null_mut());
                return Err(InitError::DelegateCreationFailed);
            }

            let window_class = objc_getClass(c"NSWindow".as_ptr());
            if window_class.is_null() {
                release_initialization_objects(application, null_mut(), delegate);
                return Err(InitError::WindowCreationFailed);
            }
            let window_allocated = send_id(window_class, selector(c"alloc"));
            if window_allocated.is_null() {
                release_initialization_objects(application, null_mut(), delegate);
                return Err(InitError::WindowCreationFailed);
            }
            let window = send_id_rect_usize_usize_bool(
                window_allocated,
                selector(c"initWithContentRect:styleMask:backing:defer:"),
                Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: config.content_width,
                        height: config.content_height,
                    },
                },
                STYLE_TITLED | STYLE_CLOSABLE | STYLE_MINIATURIZABLE | STYLE_RESIZABLE,
                BACKING_STORE_BUFFERED,
                false,
            );
            if window.is_null() {
                release_initialization_objects(application, null_mut(), delegate);
                return Err(InitError::WindowCreationFailed);
            }
            // This must be changed before any failure cleanup can close the
            // window, because AppKit otherwise may release it automatically.
            send_void_bool(window, selector(c"setReleasedWhenClosed:"), false);
            send_void_bool(window, selector(c"setAcceptsMouseMovedEvents:"), true);

            let title_class = objc_getClass(c"NSString".as_ptr());
            let title_object = if title_class.is_null() {
                null_mut()
            } else {
                send_id_cstr(
                    title_class,
                    selector(c"stringWithUTF8String:"),
                    title.as_ptr(),
                )
            };
            if title_object.is_null() {
                release_initialization_objects(application, window, delegate);
                return Err(InitError::WindowCreationFailed);
            }
            send_void_id(window, selector(c"setTitle:"), title_object);
            send_void_id(window, selector(c"setDelegate:"), delegate);
            send_void_id(application, selector(c"setDelegate:"), delegate);
            send_void(application, selector(c"finishLaunching"));

            let state = Box::new(CallbackState::new());
            let state_pointer = state.as_ref() as *const CallbackState as *mut CallbackState;
            if CALLBACK_STATE
                .compare_exchange(
                    null_mut(),
                    state_pointer,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_err()
            {
                release_initialization_objects(application, window, delegate);
                return Err(InitError::AlreadyRunning);
            }

            send_void_id(window, selector(c"makeKeyAndOrderFront:"), null_mut());
            send_void_bool(application, selector(c"activateIgnoringOtherApps:"), true);

            Ok(Self {
                application,
                window,
                delegate,
                state,
                _main_thread: PhantomData,
            })
        }
    }

    pub(crate) fn run(&mut self) -> Result<ExitReason, RunError> {
        if !is_main_thread() {
            return Err(RunError::MainThreadRequired);
        }

        while self.state.running.load(Ordering::Acquire) {
            let _event_pool = AutoreleasePool::new().ok_or(RunError::AutoreleasePoolUnavailable)?;
            let deadline = unsafe {
                let date_class = objc_getClass(c"NSDate".as_ptr());
                if date_class.is_null() {
                    null_mut()
                } else {
                    send_id_f64(
                        date_class,
                        selector(c"dateWithTimeIntervalSinceNow:"),
                        EVENT_WAIT_SECONDS,
                    )
                }
            };
            if deadline.is_null() {
                return Err(RunError::EventDeadlineUnavailable);
            }

            let event = unsafe {
                let mode = NSDefaultRunLoopMode;
                if mode.is_null() {
                    null_mut()
                } else {
                    send_id_u64_id_id_bool(
                        self.application,
                        selector(c"nextEventMatchingMask:untilDate:inMode:dequeue:"),
                        EVENT_MASK_ANY,
                        deadline,
                        mode,
                        true,
                    )
                }
            };
            if !event.is_null() {
                unsafe {
                    send_void_id(self.application, selector(c"sendEvent:"), event);
                }
            }
            unsafe {
                send_void(self.application, selector(c"updateWindows"));
            }
        }

        if self.state.callback_failed.load(Ordering::Acquire) {
            return Err(RunError::CallbackFailed);
        }
        match self.state.exit_reason.load(Ordering::Acquire) {
            EXIT_WINDOW_CLOSED => Ok(ExitReason::WindowClosed),
            _ => Ok(ExitReason::ApplicationQuit),
        }
    }
}

impl Drop for Application {
    fn drop(&mut self) {
        let _shutdown_pool = AutoreleasePool::new();
        self.state.running.store(false, Ordering::Release);
        unsafe {
            // Clear owned AppKit links before clearing callback state and
            // releasing the delegate and window.
            send_void_id(self.application, selector(c"setMainMenu:"), null_mut());
            send_void_id(self.window, selector(c"setDelegate:"), null_mut());
            send_void_id(self.application, selector(c"setDelegate:"), null_mut());
            let state_pointer = self.state.as_ref() as *const CallbackState as *mut CallbackState;
            let _ = CALLBACK_STATE.compare_exchange(
                state_pointer,
                null_mut(),
                Ordering::AcqRel,
                Ordering::Acquire,
            );
            send_void(self.window, selector(c"close"));
            release(self.window);
            release(self.delegate);
        }
    }
}
