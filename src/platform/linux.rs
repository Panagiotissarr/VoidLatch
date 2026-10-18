//! Linux dll injection via `ptrace`.
//!
//! `ptrace-inject` attaches to the target, maps the `.so` into its address
//! space with `mmap`, then forces the dynamic linker to load it. Attaching to
//! another process requires root (or `CAP_SYS_PTRACE`).

use std::path::Path;

use log::info;
use ptrace_inject::{Injector, Process};

use super::{DllInjector, InjectError};

/// Injects shared dll into a running process with `ptrace`.
#[derive(Debug, Default)]
pub struct PlatformInjector;

impl DllInjector for PlatformInjector {
    fn inject(&self, pid: u32, lib_path: &Path) -> Result<(), InjectError> {
        if !Path::new(&format!("/proc/{pid}")).exists() {
            return Err(InjectError::ProcessGone(pid));
        }

        let process = Process::get(pid).map_err(|e| InjectError::Attach {
            pid,
            source: e.to_string().into(),
        })?;
        let mut injector = Injector::attach(process).map_err(|e| InjectError::Attach {
            pid,
            source: e.to_string().into(),
        })?;
        injector
            .inject(lib_path)
            .map_err(|e| InjectError::Inject(e.to_string().into()))?;

        info!("dll injected into pid {pid}");
        Ok(())
    }
}

/// Whether the process runs as root.
pub fn is_elevated() -> bool {
    // SAFETY: `geteuid` takes no arguments and never fails.
    unsafe { libc::geteuid() == 0 }
}
