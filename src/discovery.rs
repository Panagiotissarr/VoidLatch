//! Discovery of running Minecraft (Java Edition) client processes.
//!
//! The scan is deliberately narrow: it only ever reports processes that look
//! like a Minecraft client. It is not a general-purpose process picker, and
//! the caller cannot inject into anything this module did not return.

use std::path::Path;

use sysinfo::System;

/// A Minecraft process the user may inject into.
#[derive(Debug, Clone)]
pub struct ProcessInfo {
    /// Operating-system process id.
    pub pid: u32,
    /// Human-readable label — the game version when the command line carries
    /// one, otherwise a generic description.
    pub info: String,
}

/// Scans running processes and returns every Java process that looks like a
/// Minecraft client, sorted by pid.
pub fn find_minecraft_processes() -> Vec<ProcessInfo> {
    let mut sys = System::new_all();
    sys.refresh_all();

    let mut found: Vec<ProcessInfo> = sys
        .processes()
        .iter()
        .filter_map(|(pid, process)| {
            let exe = process.name().to_string_lossy();
            let args: Vec<String> = process
                .cmd()
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            classify(&exe, &args).map(|info| ProcessInfo {
                pid: pid.as_u32(),
                info,
            })
        })
        .collect();

    found.sort_by_key(|p| p.pid);
    found
}

/// Pure classification: given a process executable name and its command-line
/// arguments, decide whether this is a Minecraft client and, if so, produce a
/// display label. Free of `sysinfo` types so it stays unit-testable.
fn classify(exe_name: &str, args: &[String]) -> Option<String> {
    if !is_java_executable(exe_name) {
        return None;
    }
    if !args.join(" ").to_lowercase().contains("minecraft") {
        return None;
    }
    Some(extract_version(args).unwrap_or_else(|| "Minecraft instance".to_string()))
}

/// True if `exe_name` is a Java launcher binary, ignoring case and any
/// platform extension (`java`, `javaw`, `java.exe`, `javaw.exe`).
fn is_java_executable(exe_name: &str) -> bool {
    let stem = Path::new(exe_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(exe_name);
    stem.eq_ignore_ascii_case("java") || stem.eq_ignore_ascii_case("javaw")
}

/// Pulls the value following a `--version` argument, if present.
fn extract_version(args: &[String]) -> Option<String> {
    let idx = args.iter().position(|a| a == "--version")?;
    args.get(idx + 1).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_plain_java_minecraft_process_is_detected() {
        assert!(classify(
            "java",
            &args(&["-cp", "minecraft.jar", "net.minecraft.client.main.Main"]),
        )
        .is_some());
    }

    #[test]
    fn the_windows_javaw_exe_is_detected() {
        assert!(classify(
            "javaw.exe",
            &args(&["-Xmx2G", "minecraft", "--gameDir", "."])
        )
        .is_some());
    }

    #[test]
    fn the_java_binary_match_is_case_insensitive() {
        assert!(classify("JavaW", &args(&["net.minecraft.client"])).is_some());
    }

    #[test]
    fn the_minecraft_keyword_match_is_case_insensitive() {
        assert!(classify("java", &args(&["-jar", "MINECRAFT.jar"])).is_some());
    }

    #[test]
    fn a_non_java_process_is_ignored() {
        assert!(classify("python", &args(&["minecraft_server.py"])).is_none());
    }

    #[test]
    fn a_java_process_without_minecraft_is_ignored() {
        assert!(classify("java", &args(&["-jar", "build-tools.jar"])).is_none());
    }

    #[test]
    fn the_version_is_extracted_when_the_flag_is_present() {
        let label = classify(
            "java",
            &args(&["--version", "1.21.4", "-cp", "minecraft.jar"]),
        );
        assert_eq!(label, Some("1.21.4".to_string()));
    }

    #[test]
    fn a_generic_label_is_used_without_a_version_flag() {
        let label = classify("java", &args(&["-cp", "minecraft.jar"]));
        assert_eq!(label, Some("Minecraft instance".to_string()));
    }
}
