//! UI-agnostic core: the state machine behind the GUI.
//!
//! Holds the scan results, the chosen dll path, and the status of the
//! current injection. Injection runs on a worker thread so the UI never
//! blocks.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use crate::discovery::{find_minecraft_processes, ProcessInfo};
use crate::platform::{
    validate_dll, verify::ModuleCheck, DllInjector, InjectError, PlatformInjector,
};

/// The worker's result: did the injection succeed, and did an independent
/// module check confirm the dll is actually mapped?
type InjectionResult = (Result<(), InjectError>, ModuleCheck);

/// Where an injection attempt currently stands.
#[derive(Debug, Clone)]
pub enum InjectionStatus {
    /// Idle and ready.
    Idle,
    /// A process scan is running.
    Scanning,
    /// An injection into the given pid is in progress.
    Injecting(u32),
    /// The dll was injected and independently verified in the target.
    Done(u32),
    /// The dll loaded, but could not be confirmed in the target's module
    /// list — often because the target died right after loading it.
    Unverified(u32),
    /// The last action failed, with a human-readable reason.
    Failed(String),
}

impl InjectionStatus {
    /// Short human-readable line suitable for a status bar.
    pub fn message(&self) -> String {
        match self {
            InjectionStatus::Idle => "Ready.".to_string(),
            InjectionStatus::Scanning => "Scanning for Minecraft…".to_string(),
            InjectionStatus::Injecting(pid) => format!("Injecting into process {pid}…"),
            InjectionStatus::Done(pid) => {
                format!("Injected into process {pid} — verified in module list.")
            }
            InjectionStatus::Unverified(pid) => format!(
                "Injected into process {pid}, but it is NOT in the module list — \
                 the target may have crashed on load."
            ),
            InjectionStatus::Failed(reason) => format!("Error: {reason}"),
        }
    }

    /// Whether an operation is currently in flight.
    #[allow(dead_code)]
    pub fn is_busy(&self) -> bool {
        matches!(
            self,
            InjectionStatus::Scanning | InjectionStatus::Injecting(_)
        )
    }
}

/// The injector's state, independent of how it is drawn.
pub struct InjectorApp {
    processes: Vec<ProcessInfo>,
    selected_pid: Option<u32>,
    dll: Option<PathBuf>,
    status: InjectionStatus,
    /// Result channel of an in-flight injection, with its target pid.
    pending: Option<(u32, Receiver<InjectionResult>)>,
}

impl InjectorApp {
    /// Creates an idle app with no scan results and no dll chosen.
    pub fn new() -> Self {
        Self {
            processes: Vec::new(),
            selected_pid: None,
            dll: None,
            status: InjectionStatus::Idle,
            pending: None,
        }
    }

    /// The processes found by the last [`scan`](Self::scan).
    pub fn processes(&self) -> &[ProcessInfo] {
        &self.processes
    }

    /// The currently selected process id, if any.
    pub fn selected_pid(&self) -> Option<u32> {
        self.selected_pid
    }

    /// The dll the user picked, if any.
    pub fn dll(&self) -> Option<&Path> {
        self.dll.as_deref()
    }

    /// The current status.
    pub fn status(&self) -> &InjectionStatus {
        &self.status
    }

    /// Whether an operation is currently running.
    #[allow(dead_code)]
    pub fn is_busy(&self) -> bool {
        self.pending.is_some()
    }

    /// Records the dll the user chose through the file picker.
    pub fn set_dll(&mut self, path: PathBuf) {
        self.dll = Some(path);
    }

    /// Selects a process by pid, if it is in the current list.
    pub fn select(&mut self, pid: u32) {
        if self.processes.iter().any(|p| p.pid == pid) {
            self.selected_pid = Some(pid);
        }
    }

    /// Rescans for Minecraft processes. Keeps the current selection if it is
    /// still present, otherwise selects the first result.
    pub fn scan(&mut self) {
        if self.is_busy() {
            return;
        }
        self.status = InjectionStatus::Scanning;
        self.processes = find_minecraft_processes();

        let kept = self
            .selected_pid
            .is_some_and(|pid| self.processes.iter().any(|p| p.pid == pid));
        if !kept {
            self.selected_pid = self.processes.first().map(|p| p.pid);
        }
        self.status = InjectionStatus::Idle;
    }

    /// Starts injecting the chosen dll into the selected process on a
    /// worker thread. A no-op when nothing is selected, nothing is chosen, or
    /// an injection is already running.
    pub fn start_injection(&mut self) {
        if self.is_busy() {
            return;
        }
        let Some(pid) = self.selected_pid else {
            self.status = InjectionStatus::Failed("select a Minecraft process first".to_string());
            return;
        };
        let Some(dll) = self.dll.clone() else {
            self.status = InjectionStatus::Failed("choose a dll file first".to_string());
            return;
        };

        // Fail fast with a clear message rather than an opaque backend error.
        if let Err(e) = validate_dll(&dll) {
            self.status = InjectionStatus::Failed(e.to_string());
            return;
        }

        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let result = PlatformInjector.inject(pid, &dll);
            // Re-read the target's module list from the outside. This tells us
            // whether the dll really landed, independently of whatever the
            // payload did next.
            let check = if result.is_ok() {
                crate::platform::verify::verify_injected(pid, &dll)
            } else {
                ModuleCheck::Unavailable
            };
            let _ = tx.send((result, check));
        });
        self.pending = Some((pid, rx));
        self.status = InjectionStatus::Injecting(pid);
    }

    /// Polls the injection worker. The GUI calls this once per frame;
    /// returns `true` when the status changed so it knows to repaint.
    pub fn poll(&mut self) -> bool {
        let Some((pid, rx)) = &self.pending else {
            return false;
        };
        let pid = *pid;
        match rx.try_recv() {
            Ok((result, check)) => {
                self.status = match (result, check) {
                    (Ok(()), ModuleCheck::Loaded) => InjectionStatus::Done(pid),
                    // Loaded but unconfirmed — the usual sign that the payload
                    // crashed the target immediately after mapping.
                    (Ok(()), ModuleCheck::NotLoaded | ModuleCheck::Unavailable) => {
                        InjectionStatus::Unverified(pid)
                    }
                    (Err(e), _) => InjectionStatus::Failed(e.to_string()),
                };
                self.pending = None;
                true
            }
            Err(TryRecvError::Empty) => false,
            Err(TryRecvError::Disconnected) => {
                self.status =
                    InjectionStatus::Failed("injection worker stopped unexpectedly".to_string());
                self.pending = None;
                true
            }
        }
    }
}

impl Default for InjectorApp {
    fn default() -> Self {
        Self::new()
    }
}
