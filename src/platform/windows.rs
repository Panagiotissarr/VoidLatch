//! Windows dll injection via a remote `LoadLibraryW` thread.
//!
//! This is the textbook technique, implemented directly against the Win32 API
//! so the mechanism is visible rather than hidden behind a crate:
//!
//! 1. `OpenProcess` — ask for a handle to the target with the rights needed to
//!    create a thread and write to its memory. Cross-process access like this is
//!    why Administrator is required.
//! 2. `VirtualAllocEx` — reserve and commit a page of *read-write* memory
//!    **inside the target**, just big enough for the dll path.
//! 3. `WriteProcessMemory` — copy the UTF-16 path of the dll into that page.
//! 4. `GetModuleHandleW` / `GetProcAddress` — resolve the address of
//!    `LoadLibraryW` **in this** process. Because `kernel32.dll` is loaded at the
//!    same address in every process of the same bitness, that address is also
//!    valid inside the target.
//! 5. `CreateRemoteThread` — start a thread in the target at that address,
//!    passing the path as its single argument.
//! 6. Wait for it, read the exit code (which is `LoadLibraryW`'s `HMODULE`),
//!    then free the scratch page and close handles.
//!
//! Note on step 4: the shared-address trick only holds between processes of the
//! same CPU architecture. A 32-bit dll cannot be loaded into a 64-bit
//! process this way, which is the most common cause of failure.
//!
//! SAFETY: this module performs deliberate cross-process memory writes. It is
//! only ever pointed at a Minecraft process chosen by the user.

use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr;

use log::{info, warn};

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::Diagnostics::Debug::WriteProcessMemory;
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::Win32::System::Memory::{
    VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE,
};
use windows_sys::Win32::System::Threading::{
    CreateRemoteThread, GetExitCodeThread, OpenProcess, WaitForSingleObject, INFINITE,
    PROCESS_CREATE_THREAD, PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_WRITE,
};

use super::{DllInjector, InjectError};

/// Injects DLLs into a running process via a remote `LoadLibraryW` thread.
#[derive(Debug, Default)]
pub struct PlatformInjector;

impl DllInjector for PlatformInjector {
    fn inject(&self, pid: u32, lib_path: &Path) -> Result<(), InjectError> {
        inject_load_library(pid, lib_path)
    }
}

/// Guards a `HANDLE` so it is closed exactly once, on every exit path.
struct OwnedHandle(HANDLE);

impl OwnedHandle {
    /// Takes ownership of a handle, treating `NULL` and `INVALID_HANDLE_VALUE`
    /// as failure.
    fn new(handle: HANDLE) -> Option<Self> {
        if handle.is_null() || handle as isize == -1 {
            None
        } else {
            Some(Self(handle))
        }
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: the handle is valid and owned by this value; it is not closed
        // anywhere else.
        unsafe { CloseHandle(self.0) };
    }
}

/// NUL-terminated UTF-16 encoding of a string, for the Win32 `W` functions.
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Injects `lib_path` into the process `pid`.
fn inject_load_library(pid: u32, lib_path: &Path) -> Result<(), InjectError> {
    // `LoadLibraryW` takes a NUL-terminated UTF-16 string.
    let path_wide: Vec<u16> = lib_path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let byte_len = path_wide.len() * std::mem::size_of::<u16>();

    // 1. Open the target process.
    //
    // SAFETY: a plain Win32 call with constant arguments; the result is checked.
    let process = OwnedHandle::new(unsafe {
        OpenProcess(
            PROCESS_CREATE_THREAD
                | PROCESS_QUERY_INFORMATION
                | PROCESS_VM_OPERATION
                | PROCESS_VM_WRITE,
            0,
            pid,
        )
    })
    .ok_or(InjectError::ProcessGone(pid))?;

    // 2. Allocate scratch memory in the target to hold the path.
    //
    // SAFETY: `process` is open with PROCESS_VM_OPERATION; a NULL address lets
    // the OS choose.
    let remote = OwnedHandle::new(unsafe {
        VirtualAllocEx(
            process.0,
            ptr::null(),
            byte_len,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        )
    })
    .ok_or_else(|| InjectError::Inject("could not allocate memory in the target process".into()))?;

    // 3. Write the path into it.
    //
    // SAFETY: `remote` is a writable allocation of at least `byte_len` bytes in
    // the target, and `path_wide` is a live buffer of exactly that size.
    let written = unsafe {
        WriteProcessMemory(
            process.0,
            remote.0.cast(),
            path_wide.as_ptr().cast(),
            byte_len,
            ptr::null_mut(),
        )
    };
    if written == 0 {
        // SAFETY: `remote` is a valid allocation in the target.
        unsafe { VirtualFreeEx(process.0, remote.0, 0, MEM_RELEASE) };
        return Err(InjectError::Inject(
            "could not write the dll path into the target process".into(),
        ));
    }

    // 4. Resolve `LoadLibraryW`. kernel32 sits at the same address in every
    //    process of the same bitness, so this address is valid in the target too.
    //
    // SAFETY: constant argument; the returned handle is only used to resolve a
    // well-known export.
    let kernel32 = unsafe { GetModuleHandleW(wide("kernel32.dll").as_ptr()) };
    if kernel32.is_null() {
        // SAFETY: see above.
        unsafe { VirtualFreeEx(process.0, remote.0, 0, MEM_RELEASE) };
        return Err(InjectError::Inject("kernel32.dll was not found".into()));
    }

    // `GetProcAddress` takes an ASCII name and returns `FARPROC`, which is an
    // `Option<fn>` — so absence is `None`, not a null pointer.
    //
    // SAFETY: `kernel32` is a valid module handle and `LoadLibraryW` is a
    // stable kernel32 export.
    // `.cast()` because `PCSTR` is `*const u8` while `c_char` is signed.
    let load_dll = unsafe { GetProcAddress(kernel32, c"LoadLibraryW".as_ptr().cast()) };
    let Some(load_dll) = load_dll else {
        // SAFETY: see above.
        unsafe { VirtualFreeEx(process.0, remote.0, 0, MEM_RELEASE) };
        return Err(InjectError::Inject("LoadLibraryW was not found".into()));
    };

    // 5. Start the remote thread. The start address is the `LoadLibraryW` we
    //    just resolved, and the parameter is the path we wrote in step 3.
    //
    // `CreateRemoteThread` declares the thread routine as taking a `POINT` and
    // returning a `DWORD`, whereas `LoadLibraryW` takes an `LPCWSTR` and
    // returns an `HMODULE`. Both are pointer-sized, so calling through the
    // required shape is what the OS will do anyway; and since
    // `GetExitCodeThread` reports only the low DWORD of the result either way,
    // the value we check for non-null is unaffected.
    //
    // SAFETY: the resolved address really is `LoadLibraryW` in kernel32, which
    // takes exactly one pointer argument. The cast changes only the declared
    // return width, which nothing depends on.
    let thread_start: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32 =
        unsafe { std::mem::transmute(load_dll) };

    // SAFETY: `process` holds PROCESS_CREATE_THREAD, `remote` points at a
    // readable region of `byte_len` bytes, and `thread_start` is a valid
    // function pointer inside kernel32.
    let thread = OwnedHandle::new(unsafe {
        CreateRemoteThread(
            process.0,
            ptr::null(),
            0,
            Some(thread_start),
            remote.0.cast(),
            0,
            ptr::null_mut(),
        )
    })
    .ok_or_else(|| InjectError::Inject("could not start the remote thread".into()))?;

    // 6. Wait for the thread to finish and read the result.
    //
    // SAFETY: `thread` is a valid thread handle owned by this scope.
    unsafe { WaitForSingleObject(thread.0, INFINITE) };

    let mut module: u32 = 0;
    // SAFETY: the thread has exited, and `module` is a valid out-pointer.
    let ok = unsafe { GetExitCodeThread(thread.0, &mut module) } != 0;

    // Release the scratch page now that the thread has consumed the path.
    // SAFETY: the thread has exited, so nothing references the region.
    unsafe { VirtualFreeEx(process.0, remote.0, 0, MEM_RELEASE) };

    if !ok || module == 0 {
        warn!("remote LoadLibraryW returned a null module for pid {pid}");
        return Err(InjectError::Inject(
            format!(
                "the target failed to load {} (an architecture mismatch is the usual cause)",
                lib_path.display()
            )
            .into(),
        ));
    }

    info!("dll injected into pid {pid} at base {module:#x}");
    Ok(())
}

/// Whether the process runs with Administrator privileges.
pub fn is_elevated() -> bool {
    is_elevated::is_elevated()
}
