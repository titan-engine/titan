//! Native application lifecycle for Titan.
//!
//! The first runtime backend is intentionally macOS-only. It owns one AppKit
//! application and one native window, runs AppKit's ordinary event dispatch on
//! the process main thread, and returns when the window or application quits.

use core::error::Error;
use core::fmt;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod macos;
#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
mod unsupported;

/// The initial native window configuration.
#[derive(Clone, Debug)]
pub struct WindowConfig {
    /// Text shown in the window title bar.
    pub title: String,
    /// Requested content width in points.
    pub content_width: f64,
    /// Requested content height in points.
    pub content_height: f64,
}

impl WindowConfig {
    /// Creates a window configuration from a title and content size.
    pub fn new(title: impl Into<String>, content_width: f64, content_height: f64) -> Self {
        Self {
            title: title.into(),
            content_width,
            content_height,
        }
    }
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self::new("Titan", 720.0, 480.0)
    }
}

/// Why [`Application::run`] stopped dispatching events.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExitReason {
    /// The native window's close control was used.
    WindowClosed,
    /// macOS requested application termination, for example through Dock
    /// Quit or another standard application quit action.
    ApplicationQuit,
}

impl fmt::Display for ExitReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WindowClosed => formatter.write_str("window closed"),
            Self::ApplicationQuit => formatter.write_str("application quit"),
        }
    }
}

/// Failure while creating the native application and window.
#[derive(Debug)]
pub enum InitError {
    /// AppKit objects must be created on the process main thread.
    MainThreadRequired,
    /// The title contains an interior NUL byte and cannot be passed to
    /// `NSString.stringWithUTF8String:`.
    InvalidTitle,
    /// The requested content size must be finite and greater than zero.
    InvalidContentSize,
    /// The Objective-C runtime could not find `NSObject`.
    NSObjectUnavailable,
    /// The Objective-C runtime could not allocate Titan's delegate class.
    DelegateClassAllocationFailed,
    /// The runtime could not install one of the required delegate methods.
    DelegateMethodRegistrationFailed(&'static str),
    /// `NSApplication.sharedApplication` returned null.
    ApplicationUnavailable,
    /// AppKit rejected the regular activation policy.
    ActivationPolicyRejected,
    /// AppKit could not construct the minimal application menu.
    MenuCreationFailed,
    /// AppKit could not allocate or initialize the application delegate.
    DelegateCreationFailed,
    /// AppKit could not allocate or initialize the window.
    WindowCreationFailed,
    /// AppKit could not create an autorelease pool.
    AutoreleasePoolUnavailable,
    /// A Titan runtime application is already active in this process.
    AlreadyRunning,
    /// This target does not have a native Titan runtime backend yet.
    UnsupportedPlatform,
}

impl fmt::Display for InitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MainThreadRequired => {
                formatter.write_str("titan-runtime must be initialized on the macOS main thread")
            }
            Self::InvalidTitle => {
                formatter.write_str("window title cannot contain an interior NUL byte")
            }
            Self::InvalidContentSize => formatter
                .write_str("window content width and height must be finite and greater than zero"),
            Self::NSObjectUnavailable => {
                formatter.write_str("Objective-C NSObject class is unavailable")
            }
            Self::DelegateClassAllocationFailed => {
                formatter.write_str("could not allocate the Titan Objective-C delegate class")
            }
            Self::DelegateMethodRegistrationFailed(method) => {
                write!(
                    formatter,
                    "could not register Objective-C delegate method {method}"
                )
            }
            Self::ApplicationUnavailable => {
                formatter.write_str("NSApplication.sharedApplication returned null")
            }
            Self::ActivationPolicyRejected => {
                formatter.write_str("NSApplication rejected the regular activation policy")
            }
            Self::MenuCreationFailed => {
                formatter.write_str("could not construct the native application menu")
            }
            Self::DelegateCreationFailed => {
                formatter.write_str("could not allocate or initialize the application delegate")
            }
            Self::WindowCreationFailed => {
                formatter.write_str("could not allocate or initialize the native window")
            }
            Self::AutoreleasePoolUnavailable => {
                formatter.write_str("could not allocate or initialize an autorelease pool")
            }
            Self::AlreadyRunning => {
                formatter.write_str("a Titan runtime application is already active")
            }
            Self::UnsupportedPlatform => {
                formatter.write_str("titan-runtime currently supports macOS only")
            }
        }
    }
}

impl Error for InitError {}

/// Failure while dispatching the native event loop.
#[derive(Debug)]
pub enum RunError {
    /// AppKit operations must remain on the process main thread.
    MainThreadRequired,
    /// AppKit could not create an autorelease pool for an event iteration.
    AutoreleasePoolUnavailable,
    /// AppKit could not create the deadline object used for event retrieval.
    EventDeadlineUnavailable,
    /// A Rust callback failure was caught before it could unwind through
    /// Objective-C. The application is shutting down.
    CallbackFailed,
    /// This target does not have a native Titan runtime backend yet.
    UnsupportedPlatform,
}

impl fmt::Display for RunError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MainThreadRequired => formatter
                .write_str("titan-runtime event dispatch must run on the macOS main thread"),
            Self::AutoreleasePoolUnavailable => {
                formatter.write_str("could not create an autorelease pool for event dispatch")
            }
            Self::EventDeadlineUnavailable => {
                formatter.write_str("could not create an event deadline for AppKit dispatch")
            }
            Self::CallbackFailed => {
                formatter.write_str("a native callback failed; the application was stopped safely")
            }
            Self::UnsupportedPlatform => {
                formatter.write_str("titan-runtime currently supports macOS only")
            }
        }
    }
}

impl Error for RunError {}

/// An owned native application and window lifecycle.
///
/// A value must be created and run on the process main thread. The runtime
/// owns the native window and delegate until this value is dropped. Dropping
/// it clears weak Objective-C delegate links before releasing either owned
/// object.
pub struct Application {
    #[cfg(target_os = "macos")]
    inner: macos::Application,
    #[cfg(not(target_os = "macos"))]
    inner: unsupported::Application,
}

impl Application {
    /// Creates and shows a responsive native window.
    pub fn new(config: WindowConfig) -> Result<Self, InitError> {
        #[cfg(target_os = "macos")]
        {
            Ok(Self {
                inner: macos::Application::new(config)?,
            })
        }
        #[cfg(not(target_os = "macos"))]
        {
            Ok(Self {
                inner: unsupported::Application::new(config)?,
            })
        }
    }

    /// Dispatches normal AppKit events until the window or application quits.
    pub fn run(&mut self) -> Result<ExitReason, RunError> {
        #[cfg(target_os = "macos")]
        {
            self.inner.run()
        }
        #[cfg(not(target_os = "macos"))]
        {
            self.inner.run()
        }
    }
}
