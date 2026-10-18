//! macOS — and any other non-Linux/Windows target — injection stub.
//!
//! Not implemented. The seam exists so that adding real macOS support later
//! (for example `task_for_pid` plus a remote thread, or `DYLD_INSERT_LIBRARIES`
//! for launch-time injection) means editing only this file.

use std::path::Path;

use super::{DllInjector, InjectError};

#[derive(Debug, Default)]
pub struct PlatformInjector;

impl DllInjector for PlatformInjector {
    fn inject(&self, _pid: u32, _lib_path: &Path) -> Result<(), InjectError> {
        Err(InjectError::Unsupported)
    }
}

/// Elevation is not checked on this platform, since injection is unsupported.
pub fn is_elevated() -> bool {
    false
}
