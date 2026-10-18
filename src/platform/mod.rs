//! Platform abstraction for shared-dll injection.
//!
//! Each OS provides a [`DllInjector`] implementation. Windows uses
//! `dll-syringe` (remote-thread `LoadLibraryW`); Linux uses `ptrace` to map
//! an `.so` into the target. macOS is a stub.
//!
//! This is the whole mechanism, kept in one place so it can be read and
//! understood on its own.

#[cfg(target_os = "linux")]
#[path = "linux.rs"]
mod imp;
#[cfg(target_os = "windows")]
#[path = "windows.rs"]
mod imp;
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
#[path = "macos.rs"]
mod imp;

#[cfg(target_os = "windows")]
#[path = "verify.rs"]
pub mod verify;

#[cfg(not(target_os = "windows"))]
pub mod verify {
    //! Module verification is implemented only on Windows, where the
    //! ToolHelp snapshot API is available. Elsewhere this is a no-op.

    use std::path::Path;

    /// The outcome of checking a target's module list.
    #[derive(Debug, PartialEq, Eq)]
    pub enum ModuleCheck {
        /// The dll is mapped into the target.
        Loaded,
        /// The target is running but the dll is not in its module list.
        NotLoaded,
        /// The module list could not be read.
        Unavailable,
    }

    /// Always reports [`ModuleCheck::Unavailable`] on non-Windows targets.
    pub fn verify_injected(_pid: u32, _lib_path: &Path) -> ModuleCheck {
        ModuleCheck::Unavailable
    }
}

pub use imp::PlatformInjector;

use std::error::Error;
use std::path::Path;

use thiserror::Error;

/// A boxed, thread-safe source error, so each platform backend can funnel its
/// own error type into [`InjectError`] without the abstraction depending on it.
pub type BoxError = Box<dyn Error + Send + Sync>;

/// Everything that can go wrong while injecting. `Send + Sync` so it can be
/// returned from a worker thread.
// Some variants are only constructed by one platform's backend (`Attach` on
// Linux, `Unsupported` on macOS), so they read as dead code elsewhere.
#[allow(dead_code)]
#[derive(Debug, Error)]
pub enum InjectError {
    /// The target process disappeared before injection could start.
    #[error("process {0} is no longer running")]
    ProcessGone(u32),
    /// Attaching to the target process failed.
    #[error("could not attach to process {pid}: {source}")]
    Attach { pid: u32, source: BoxError },
    /// The platform backend failed to map the dll into the target.
    #[error("injection failed: {0}")]
    Inject(BoxError),
    /// The chosen path does not point at a readable file.
    #[error("not a readable file: {0}")]
    NotAFile(String),
    /// The chosen file does not look like a dll.
    #[error("{name} does not look like a dll (expected .{expected})")]
    WrongExtension { name: String, expected: String },
    /// Injection is not implemented for the host platform.
    #[error("injection is not supported on this platform yet")]
    Unsupported,
}

/// Platform-specific dll injection. One implementation per OS.
pub trait DllInjector {
    /// Injects the dll at `lib_path` into the process `pid`.
    fn inject(&self, pid: u32, lib_path: &Path) -> Result<(), InjectError>;
}

/// Whether the current process holds the privileges injection requires.
///
/// Injection attaches to another process, so it needs Administrator on Windows
/// or root on Linux. This *reports* the requirement; it never acquires it.
pub fn is_elevated() -> bool {
    imp::is_elevated()
}

/// Validates that `lib_path` is usable before attempting an injection, so the
/// user gets a clear message instead of an opaque backend failure.
pub fn validate_dll(lib_path: &Path) -> Result<(), InjectError> {
    if !lib_path.is_file() {
        return Err(InjectError::NotAFile(lib_path.display().to_string()));
    }

    let expected = std::env::consts::DLL_EXTENSION;
    let matches = lib_path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case(expected));

    if !matches {
        return Err(InjectError::WrongExtension {
            name: lib_path.display().to_string(),
            expected: expected.to_string(),
        });
    }

    Ok(())
}
