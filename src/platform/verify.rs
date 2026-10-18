//! Independent verification that a dll really landed in the target.
//!
//! `LoadLibraryW` returning non-zero means the target *accepted* the load. It
//! does not tell you anything about what the dll did afterwards — a payload
//! can map successfully and still crash the process a moment later.
//!
//! This module re-reads the target's module list from the outside, so you can
//! distinguish three very different outcomes:
//!
//! - **not present** — the injection never took effect
//! - **present, process alive** — the dll is loaded and stable
//! - **present, process dead** — the dll loaded and then killed the target
//!
//! The third case is the common one when a payload is built for the wrong
//! Minecraft version, so this is the check that tells you whether to debug the
//! injector or the payload.

use std::path::Path;

use log::info;

use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Module32FirstW, Module32NextW, MODULEENTRY32W, TH32CS_SNAPMODULE,
    TH32CS_SNAPMODULE32,
};

/// The outcome of checking a target's module list.
#[derive(Debug, PartialEq, Eq)]
pub enum ModuleCheck {
    /// The dll is mapped into the target.
    Loaded,
    /// The target is running but the dll is not in its module list.
    NotLoaded,
    /// The module list could not be read (process gone, or access denied).
    Unavailable,
}

/// Whether a dll named `file_name` is currently mapped into process `pid`.
///
/// `file_name` must be just the file name, e.g. `testlib.dll`.
///
/// # Safety
/// This reads another process's module list via `CreateToolhelp32Snapshot`,
/// which is read-only with respect to the target. It can only *observe*; it
/// cannot modify the target.
pub unsafe fn is_module_loaded(pid: u32, file_name: &str) -> ModuleCheck {
    // TH32CS_SNAPMODULE lists 64-bit modules, TH32CS_SNAPMODULE32 lists 32-bit
    // ones. Query both so the check works regardless of the target's bitness.
    for flags in [TH32CS_SNAPMODULE, TH32CS_SNAPMODULE32] {
        // SAFETY: a plain snapshot call; the handle is checked and closed below.
        let snapshot = unsafe { CreateToolhelp32Snapshot(flags, pid) };
        if snapshot == INVALID_HANDLE_VALUE || snapshot.is_null() {
            continue;
        }

        let mut entry: MODULEENTRY32W = unsafe { std::mem::zeroed() };
        // SAFETY: `snapshot` is a valid snapshot handle and `entry` is a
        // correctly sized, zeroed out-parameter.
        let mut ok = unsafe { Module32FirstW(snapshot, &mut entry) } != 0;

        while ok {
            // SAFETY: `szModule` is a fixed-size WCHAR array in `entry`.
            let name = unsafe { module_name(&entry) };
            if name.eq_ignore_ascii_case(file_name) {
                // SAFETY: the handle came from CreateToolhelp32Snapshot.
                unsafe { CloseHandle(snapshot) };
                info!("verified {file_name} is mapped into pid {pid}");
                return ModuleCheck::Loaded;
            }
            // SAFETY: as above; iteration ends when this returns 0.
            ok = unsafe { Module32NextW(snapshot, &mut entry) } != 0;
        }

        // SAFETY: closing the snapshot handle.
        unsafe { CloseHandle(snapshot) };
    }

    ModuleCheck::NotLoaded
}

/// Extracts the module name from an entry as a `String`.
///
/// # Safety
/// `entry` must be a populated `MODULEENTRY32W`.
unsafe fn module_name(entry: &MODULEENTRY32W) -> String {
    const MAX: usize = 256;
    let mut buf = [0u16; MAX];
    buf[..MAX - 1].copy_from_slice(&entry.szModule[..MAX - 1]);

    // `buf` is NUL-terminated within its bounds by construction above, and
    // `from_utf16_lossy` is a safe function that reads at most `MAX` units.
    String::from_utf16_lossy(&buf)
        .trim_end_matches('\0')
        .to_string()
}

/// Convenience wrapper: checks `lib_path` by file name.
pub fn verify_injected(pid: u32, lib_path: &Path) -> ModuleCheck {
    let Some(name) = lib_path.file_name().and_then(|n| n.to_str()) else {
        return ModuleCheck::Unavailable;
    };
    // SAFETY: see `is_module_loaded`.
    unsafe { is_module_loaded(pid, name) }
}
