//! The `dlls` folder: quick access to dlls without a file dialog.
//!
//! VoidLatch keeps a drop-folder in the user's home directory called `dlls`
//! (matched case-insensitively, so `DLLS` or `Dlls` work too). Dropping files in
//! there makes them show up in the **Dll** tab, where they can be injected
//! straight away — no native file picker in the way.
//!
//! The folder can be pointed elsewhere with **Change**; the choice is written
//! to [`settings_file`] so it survives a restart.
//!
//! This module only *finds and lists* files. Nothing here injects anything;
//! the caller decides what to do with the list.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// Name of the drop-folder looked for inside the user's home directory.
const FOLDER_NAME: &str = "dlls";

/// The name an older VoidLatch used, still honoured if the folder is there.
const LEGACY_FOLDER_NAME: &str = "dll";

/// A dll discovered on disk, with the display fields the UI needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DllEntry {
    /// Full path to the file.
    pub path: PathBuf,
    /// File name, e.g. `vape.dll`.
    pub name: String,
    /// Pre-rendered size, e.g. `248.4 KB`.
    pub size: String,
}

impl DllEntry {
    /// Builds an entry for `path`, reading its size from disk.
    pub fn new(path: PathBuf) -> Self {
        let name = file_name(&path);
        let size = human_size(&path);
        Self { path, name, size }
    }

    /// Whether `filter` (case-insensitive) matches the file name.
    pub fn matches(&self, filter: &str) -> bool {
        let needle = filter.trim().to_lowercase();
        needle.is_empty() || self.name.to_lowercase().contains(&needle)
    }
}

/// The user's home directory, or `None` if the environment does not say.
pub fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE")
            .or_else(|| {
                std::env::var_os("HOMEDRIVE")
                    .zip(std::env::var_os("HOMEPATH"))
                    .map(|(d, p)| {
                        let mut s = d;
                        s.push(&p);
                        s
                    })
            })
            .map(PathBuf::from)
            .filter(|p| p.is_dir())
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_dir())
    }
}

/// The folder VoidLatch watches, whether or not it exists yet.
///
/// A folder chosen with **Change** wins. Otherwise the first entry of `home`
/// named `dlls` (or the legacy `dll`) in any casing is used, so a user who
/// made `DLLS` still gets their folder. Falls back to `home/dlls`, which the
/// GUI creates on demand.
pub fn dll_folder() -> Option<PathBuf> {
    if let Some(chosen) = saved_folder() {
        return Some(chosen);
    }
    let home = home_dir()?;
    Some(match_existing_dll_folder(&home).unwrap_or_else(|| home.join(FOLDER_NAME)))
}

/// Where the folder chosen with **Change** is written.
fn settings_file() -> Option<PathBuf> {
    Some(home_dir()?.join(".voidlatch-dll-folder"))
}

/// The folder recorded by a previous **Change**, if it still exists.
fn saved_folder() -> Option<PathBuf> {
    let path = settings_file()?;
    let saved = std::fs::read_to_string(path).ok()?;
    let saved = saved.trim();
    if saved.is_empty() {
        return None;
    }
    let path = PathBuf::from(saved);
    path.is_dir().then_some(path)
}

/// Records `folder` as the watched folder and returns it.
///
/// An empty path clears the choice and restores the default `home/dlls`.
pub fn set_dll_folder(folder: &Path) -> Option<PathBuf> {
    let settings = settings_file()?;
    if folder.as_os_str().is_empty() {
        let _ = std::fs::remove_file(&settings);
        return dll_folder();
    }
    if std::fs::write(&settings, folder.to_string_lossy().as_bytes()).is_err() {
        return None;
    }
    Some(folder.to_path_buf())
}

/// True when `name` is the drop-folder, ignoring case.
///
/// Both `dlls` and the legacy `dll` qualify, so an existing folder from an
/// older release is picked up rather than shadowed by a new empty one.
fn is_folder_name(name: &OsStr) -> bool {
    name.to_str().is_some_and(|s| {
        s.eq_ignore_ascii_case(FOLDER_NAME) || s.eq_ignore_ascii_case(LEGACY_FOLDER_NAME)
    })
}

/// The first child of `home` whose name is the drop-folder, ignoring case.
///
/// Exact `dlls` wins over the legacy `dll` when both are present.
fn match_existing_dll_folder(home: &Path) -> Option<PathBuf> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(home)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    entries.sort();

    entries
        .iter()
        .find(|p| {
            p.file_name().is_some_and(|n| {
                n.to_str()
                    .is_some_and(|s| s.eq_ignore_ascii_case(FOLDER_NAME))
            })
        })
        .or_else(|| {
            entries
                .iter()
                .find(|p| p.file_name().is_some_and(is_folder_name))
        })
        .cloned()
}

/// Every dll in `folder`, sorted by file name, ignoring case.
///
/// Non-files, unreadable entries and anything that is not a dll for this
/// platform are skipped, so the list is always injectable in principle.
pub fn list_dlls(folder: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };

    let ext = std::env::consts::DLL_EXTENSION;
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext)))
        .collect();

    found.sort_by_key(|p| {
        p.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
            .to_lowercase()
    });
    found
}

/// Short display name of a dll path, e.g. `vape.dll`.
pub fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// File size rendered for the library list, e.g. `248.4 KB`.
pub fn human_size(path: &Path) -> String {
    let Ok(bytes) = std::fs::metadata(path).map(|m| m.len()) else {
        return "—".to_string();
    };
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", bytes, UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Creates a throwaway directory with a unique-enough name.
    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("voidlatch-test-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn only_dlls_are_listed() {
        let dir = temp_dir("list");
        let ext = std::env::consts::DLL_EXTENSION;
        std::fs::write(dir.join(format!("a.{ext}")), b"").unwrap();
        std::fs::write(dir.join("b.txt"), b"").unwrap();
        std::fs::create_dir(dir.join(format!("nested.{ext}"))).unwrap();

        let found = list_dlls(&dir);
        assert_eq!(found.len(), 1);
        assert_eq!(file_name(&found[0]), format!("a.{ext}"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dlls_are_sorted_case_insensitively() {
        let dir = temp_dir("sort");
        let ext = std::env::consts::DLL_EXTENSION;
        for name in ["Zeta", "alpha", "Mid"] {
            std::fs::write(dir.join(format!("{name}.{ext}")), b"").unwrap();
        }

        let names: Vec<String> = list_dlls(&dir).iter().map(|p| file_name(p)).collect();
        assert_eq!(names.len(), 3);
        assert!(names[0].starts_with("alpha"));
        assert!(names[1].starts_with("Mid"));
        assert!(names[2].starts_with("Zeta"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_folder_lists_nothing() {
        assert!(list_dlls(Path::new("definitely/not/here")).is_empty());
    }

    #[test]
    fn the_drop_folder_is_found_whatever_its_casing() {
        let home = temp_dir("home");
        std::fs::create_dir(home.join("DLLS")).unwrap();

        let found = match_existing_dll_folder(&home).expect("folder");
        assert_eq!(found.file_name().unwrap(), "DLLS");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn the_folder_name_match_ignores_case() {
        assert!(is_folder_name(OsStr::new("dlls")));
        assert!(is_folder_name(OsStr::new("Dlls")));
        assert!(is_folder_name(OsStr::new("DLLS")));
        assert!(!is_folder_name(OsStr::new("dllss")));
    }

    #[test]
    fn a_legacy_dll_folder_is_still_recognised() {
        assert!(is_folder_name(OsStr::new("dll")));
        assert!(is_folder_name(OsStr::new("DLL")));

        let home = temp_dir("legacy");
        std::fs::create_dir(home.join("dll")).unwrap();
        let found = match_existing_dll_folder(&home).expect("folder");
        assert_eq!(found.file_name().unwrap(), "dll");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn dlls_wins_over_a_legacy_dll_folder() {
        let home = temp_dir("both");
        std::fs::create_dir(home.join("dll")).unwrap();
        std::fs::create_dir(home.join("dlls")).unwrap();

        let found = match_existing_dll_folder(&home).expect("folder");
        assert_eq!(found.file_name().unwrap(), "dlls");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_home_without_a_dll_folder_falls_back_to_the_default_path() {
        let home = temp_dir("empty");
        assert!(match_existing_dll_folder(&home).is_none());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn sizes_are_rendered_for_the_list() {
        let dir = temp_dir("size");
        let path = dir.join("blob.dll");
        std::fs::write(&path, vec![0u8; 2048]).unwrap();

        assert_eq!(human_size(&path), "2.0 KB");
        assert_eq!(human_size(Path::new("missing.dll")), "—");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
