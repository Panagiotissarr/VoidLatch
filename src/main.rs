mod app;
mod discovery;
mod dll;
mod gui;
mod platform;

use std::path::Path;

use log::LevelFilter;

use crate::discovery::find_minecraft_processes;
use crate::platform::verify::ModuleCheck;
use crate::platform::{validate_dll, DllInjector, InjectError, PlatformInjector};

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // `--list` only reads process info, so it needs no elevation.
    if args.iter().any(|a| a == "--list") {
        for proc in find_minecraft_processes() {
            println!("{}\t{}", proc.pid, proc.info);
        }
        return;
    }

    // Injection attaches to another process, so it needs elevation — for the
    // GUI and for headless mode alike. (`--list` above is exempt: it only
    // reads process info.)
    if !platform::is_elevated() {
        eprintln!("{}", elevation_hint());
        std::process::exit(1);
    }

    // `--inject <pid> <dll>` — headless injection, for scripting.
    if let Some((pid, dll)) = parse_inject_args(&args) {
        std::process::exit(run_headless_injection(pid, Path::new(&dll)));
    }

    // Log to the terminal so the injection steps are visible while learning.
    let _ = simplelog::TermLogger::init(
        LevelFilter::Debug,
        simplelog::Config::default(),
        simplelog::TerminalMode::Mixed,
        simplelog::ColorChoice::Auto,
    );

    if let Err(e) = gui::run() {
        eprintln!("Error: {e}");
    }
}

/// Parses `--inject <pid> <dll>`, if present.
fn parse_inject_args(args: &[String]) -> Option<(u32, String)> {
    let index = args.iter().position(|a| a == "--inject")?;
    let pid = args.get(index + 1)?.parse().ok()?;
    let dll = args.get(index + 2)?.clone();
    Some((pid, dll))
}

/// Injects without a UI; returns a process exit code.
///
/// Reports the independent module check too, so a headless run distinguishes
/// "the dll loaded" from "the dll loaded and then killed the target".
fn run_headless_injection(pid: u32, dll: &Path) -> i32 {
    match inject_dll(pid, dll) {
        Ok(()) => {
            println!("injected {} into process {pid}", dll.display());
            match crate::platform::verify::verify_injected(pid, dll) {
                ModuleCheck::Loaded => {
                    println!("verified: present in the target's module list");
                    0
                }
                ModuleCheck::NotLoaded => {
                    println!(
                        "WARNING: not in the module list — the target probably \
                         crashed while loading it; check the game log"
                    );
                    2
                }
                ModuleCheck::Unavailable => {
                    println!("could not read the target's module list");
                    0
                }
            }
        }
        Err(e) => {
            eprintln!("injection into process {pid} failed: {e}");
            1
        }
    }
}

/// Validates and performs the injection. Blocking — callers run it off the
/// UI thread.
fn inject_dll(pid: u32, dll: &Path) -> Result<(), InjectError> {
    validate_dll(dll)?;
    PlatformInjector.inject(pid, dll)
}

/// The hint shown when the process lacks the privileges injection needs.
fn elevation_hint() -> &'static str {
    #[cfg(windows)]
    {
        "This program must run as Administrator (right click → Run as administrator)."
    }
    #[cfg(not(windows))]
    {
        "This program must run with root privileges: sudo ./minecraft-injector"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{InjectionStatus, InjectorApp};

    #[test]
    fn headless_args_are_parsed() {
        let args: Vec<String> = ["prog", "--inject", "1234", "lib.dll"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            parse_inject_args(&args),
            Some((1234, "lib.dll".to_string()))
        );
    }

    #[test]
    fn headless_args_are_absent_by_default() {
        let args: Vec<String> = vec!["prog".to_string()];
        assert_eq!(parse_inject_args(&args), None);
    }

    #[test]
    fn a_missing_dll_is_rejected_before_injection() {
        let err = inject_dll(1, Path::new("definitely-not-here.dll")).unwrap_err();
        assert!(matches!(err, InjectError::NotAFile(_)));
    }

    #[test]
    fn an_idle_app_starts_with_nothing_selected() {
        let app = InjectorApp::new();
        assert!(app.processes().is_empty());
        assert!(app.selected_pid().is_none());
        assert!(app.dll().is_none());
        assert!(!app.is_busy());
    }

    #[test]
    fn injecting_without_a_selection_reports_an_error() {
        let mut app = InjectorApp::new();
        app.start_injection();
        assert!(matches!(app.status(), InjectionStatus::Failed(_)));
    }

    #[test]
    fn the_chosen_dll_is_recorded() {
        let mut app = InjectorApp::new();
        app.set_dll(Path::new("/tmp/libtest.dll").to_path_buf());
        assert_eq!(app.dll(), Some(Path::new("/tmp/libtest.dll")));
    }
}
