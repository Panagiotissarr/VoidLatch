//! egui front-end.
//!
//! Two tabs share one window:
//!
//! * **Targets** — scan for Minecraft processes and pick one.
//! * **Dll** — every `.dll` in the user's home `dll` folder, injectable
//!   straight from the list without opening a file dialog.
//!
//! A native file picker is still available on the Targets tab for dlls
//! that live anywhere else. Injection logic itself lives in
//! [`InjectorApp`] and `platform`; this module only draws and forwards clicks.

mod theme;

use std::path::{Path, PathBuf};

use eframe::NativeOptions;
use egui::{Align, Context, Layout, RichText, ScrollArea, Stroke, Ui, ViewportBuilder};

use crate::app::{InjectionStatus, InjectorApp};
use crate::dll::{self, DllEntry};
use theme::Palette;

/// Launches the GUI. Blocks until the window is closed.
pub fn run() -> Result<(), eframe::Error> {
    let native_options = NativeOptions {
        viewport: ViewportBuilder::default()
            .with_inner_size([620.0, 720.0])
            .with_min_inner_size([520.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "VoidLatch",
        native_options,
        Box::new(|cc| {
            theme::apply(&cc.egui_ctx);
            let mut app = InjectorApp::new();
            app.scan();
            Ok(Box::new(InjectorGui {
                app,
                tab: Tab::Targets,
                dll: DllPanel::new(),
            }))
        }),
    )
}

/// The two views of the window.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Targets,
    Dll,
}

/// State for the dll tab: the watched folder, its contents, and the entry the
/// user highlighted.
struct DllPanel {
    folder: Option<PathBuf>,
    entries: Vec<DllEntry>,
    /// Index into `entries` of the highlighted dll, if any.
    selected: Option<usize>,
    /// Text in the filter box; entries whose name does not contain it are hidden.
    filter: String,
    /// Set when the folder could not be listed, to explain the empty list.
    error: Option<String>,
}

impl DllPanel {
    fn new() -> Self {
        let mut panel = Self {
            folder: dll::dll_folder(),
            entries: Vec::new(),
            selected: None,
            filter: String::new(),
            error: None,
        };
        panel.reload();
        panel
    }

    /// Re-reads the watched folder from disk.
    ///
    /// A folder the user picked with **Change** already exists, so it is used
    /// as-is. The default `home/dlls` is created when missing, so the tab is
    /// never a dead end — dlls can be dropped straight into it.
    fn reload(&mut self) {
        self.folder = match dll::dll_folder() {
            Some(folder) if folder.is_dir() => Some(folder),
            Some(folder) => {
                let _ = std::fs::create_dir_all(&folder);
                Some(folder)
            }
            None => None,
        };

        let Some(folder) = self.folder.clone() else {
            self.entries.clear();
            self.error = Some("could not locate the home directory".to_string());
            return;
        };

        if !folder.is_dir() {
            self.entries.clear();
            self.error = Some("could not create the dlls folder".to_string());
            return;
        }

        self.error = None;
        // The shared helper already filters to dll and sorts by name.
        self.entries = dll::list_dlls(&folder)
            .into_iter()
            .map(DllEntry::new)
            .collect();

        if self.selected.is_some_and(|i| i >= self.entries.len()) {
            self.selected = None;
        }
    }

    /// Entries matching the current filter, paired with their index.
    fn visible(&self) -> Vec<(usize, &DllEntry)> {
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.matches(&self.filter))
            .collect()
    }

    /// The highlighted dll, if any.
    fn selected_entry(&self) -> Option<&DllEntry> {
        self.selected.and_then(|i| self.entries.get(i))
    }
}

/// The eframe application — owns the [`InjectorApp`] and renders it.
struct InjectorGui {
    app: InjectorApp,
    tab: Tab,
    dll: DllPanel,
}

impl eframe::App for InjectorGui {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.app.poll();

        egui::TopBottomPanel::top("header")
            .frame(theme::header_frame())
            .show(ctx, theme::header);

        egui::TopBottomPanel::bottom("footer")
            .frame(theme::footer_frame())
            .show(ctx, |ui| self.footer(ui));

        egui::CentralPanel::default().show(ctx, |ui| {
            self.tab_bar(ui);
            ui.add_space(12.0);
            match self.tab {
                Tab::Targets => self.targets_tab(ui),
                Tab::Dll => self.dll_tab(ui),
            }
        });

        // Keep repainting while a worker thread runs so the status stays live.
        if self.app.is_busy() {
            ctx.request_repaint();
        }
    }
}

impl InjectorGui {
    /// The row of tab buttons under the header.
    fn tab_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if theme::tab(ui, "Targets", self.tab == Tab::Targets) {
                self.tab = Tab::Targets;
            }
            if theme::tab(ui, "Dll", self.tab == Tab::Dll) {
                self.tab = Tab::Dll;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let count = self.app.processes().len();
                ui.label(
                    RichText::new(if count == 1 {
                        "1 target".to_string()
                    } else {
                        format!("{count} targets")
                    })
                    .color(Palette::TEXT_DIM)
                    .small(),
                );
            });
        });
    }

    /// Targets tab: the dll picker, then the process list.
    fn targets_tab(&mut self, ui: &mut Ui) {
        dll_picker(ui, &mut self.app);
        ui.add_space(14.0);
        process_list(ui, &mut self.app);
    }

    /// Dll tab: the home `dll` folder, its dlls, and a fast inject
    /// path that needs no file dialog.
    fn dll_tab(&mut self, ui: &mut Ui) {
        folder_header(ui, &mut self.dll);

        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("🔎").color(Palette::TEXT_DIM));
            ui.add(
                egui::TextEdit::singleline(&mut self.dll.filter)
                    .hint_text("Filter dlls…")
                    .desired_width(f32::INFINITY),
            );
        });

        ui.add_space(10.0);

        if self.dll.entries.is_empty() {
            match &self.dll.error {
                Some(e) => theme::empty_state(ui, "Could not read the folder", e),
                None => theme::empty_state(
                    ui,
                    "No dll yet",
                    "Drop .dll files into the folder above and press Rescan.",
                ),
            }
            return;
        }

        if self.dll.visible().is_empty() {
            theme::empty_state(ui, "Nothing matches that filter", "Try a different name.");
            return;
        }

        let busy = self.app.is_busy();
        let mut pending: Option<usize> = None;
        let mut double_clicked: Option<PathBuf> = None;
        let selected = self.dll.selected;

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .max_height(ui.available_height() - 58.0)
            .show(ui, |ui| {
                for (index, entry) in self.dll.visible() {
                    let resp = dll_row(ui, entry, selected == Some(index));
                    if resp.double_clicked() {
                        double_clicked = Some(entry.path.clone());
                    } else if resp.clicked() {
                        pending = Some(index);
                    }
                }
            });

        if let Some(index) = pending {
            self.dll.selected = Some(index);
        }

        ui.add_space(8.0);
        self.fast_inject_row(ui, busy);

        // Double-clicking a dll injects it right away.
        if let Some(path) = double_clicked {
            self.inject_now(&path);
        }
    }

    /// The primary button of the dll tab: injects the highlighted dll
    /// with no dialog and no intermediate selection step.
    fn fast_inject_row(&mut self, ui: &mut Ui, busy: bool) {
        let entry = self.dll.selected_entry();
        let pid = self.app.selected_pid();

        let (label, ready) = match (entry, pid) {
            (Some(e), Some(pid)) => (format!("⬤  Inject {} into {}", e.name, pid), !busy),
            (Some(e), None) => (format!("⬤  Inject {}", e.name), false),
            (None, _) => ("⬤  Inject".to_string(), false),
        };

        let hint = match (entry, pid) {
            (None, _) => "Select a dll above, or scan for targets first.",
            (Some(_), None) => "No target selected — pick one on the Targets tab.",
            _ => "Injects the highlighted dll immediately.",
        };

        ui.vertical(|ui| {
            let resp = ui.add_enabled(
                ready,
                egui::Button::new(RichText::new(label).strong())
                    .fill(Palette::ACCENT)
                    .stroke(Stroke::NONE)
                    .min_size(egui::vec2(ui.available_width(), 38.0)),
            );
            if resp.clicked() {
                if let Some(e) = self.dll.selected_entry() {
                    let path = e.path.clone();
                    self.inject_now(&path);
                }
            }
            ui.label(RichText::new(hint).color(Palette::TEXT_DIM).small());
        });
    }

    /// Points the app at `path` and starts injection immediately. This is what
    /// makes the dll tab fast: no file dialog, no second confirm step.
    fn inject_now(&mut self, path: &Path) {
        if self.app.is_busy() {
            return;
        }
        self.app.set_dll(path.to_path_buf());
        self.app.start_injection();
    }

    /// The bottom bar: current status on the left, main action on the right.
    fn footer(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            status_banner(ui, self.app.status());

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let ready = !self.app.is_busy()
                    && self.app.selected_pid().is_some()
                    && self.app.dll().is_some();
                if ui
                    .add_enabled(
                        ready,
                        egui::Button::new(RichText::new("⬤  Inject").strong())
                            .fill(Palette::ACCENT)
                            .stroke(Stroke::NONE)
                            .min_size(egui::vec2(140.0, 38.0)),
                    )
                    .clicked()
                {
                    self.app.start_injection();
                }

                let chosen = self
                    .app
                    .dll()
                    .map(dll::file_name)
                    .unwrap_or_else(|| "no dll".to_string());
                let target = self
                    .app
                    .selected_pid()
                    .map(|pid| pid.to_string())
                    .unwrap_or_else(|| "—".to_string());
                ui.label(
                    RichText::new(format!("Target: {target}"))
                        .color(Palette::TEXT_DIM)
                        .small(),
                );
                ui.label(
                    RichText::new(format!("dll: {chosen}"))
                        .color(Palette::TEXT_DIM)
                        .small(),
                );
            });
        });
    }
}

/// The folder path shown at the top of the dll tab, with **Change** to point
/// the tab at a different folder and **Rescan** to re-read the current one.
fn folder_header(ui: &mut Ui, panel: &mut DllPanel) {
    theme::section_title(ui, "Dlls folder");

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("📁").color(Palette::TEXT_DIM));
        let path = panel
            .folder
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "not found".to_string());
        ui.label(RichText::new(path).color(Palette::TEXT).monospace().small());

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .add(egui::Button::new("Change…"))
                .on_hover_text("Pick a different folder of dlls")
                .clicked()
            {
                if let Some(folder) = rfd::FileDialog::new()
                    .set_title("Choose a folder of dlls")
                    .set_directory(
                        panel
                            .folder
                            .as_ref()
                            .map(|p| p.to_string_lossy().into_owned())
                            .filter(|p| !p.is_empty())
                            .unwrap_or_default()
                            .as_str(),
                    )
                    .pick_folder()
                {
                    if let Some(saved) = dll::set_dll_folder(&folder) {
                        panel.folder = Some(saved);
                        panel.selected = None;
                        panel.reload();
                    }
                }
            }
        });
    });

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui
            .add(egui::Button::new("🔄  Rescan"))
            .on_hover_text("Re-read the folder from disk")
            .clicked()
        {
            panel.reload();
        }
        ui.label(
            RichText::new(format!("{} found", panel.entries.len()))
                .color(Palette::TEXT_DIM)
                .small(),
        );
    });
}

/// The dll row on the Targets tab: shows the current choice and opens the
/// native file picker. This stays available for dll outside the home
/// folder.
fn dll_picker(ui: &mut Ui, app: &mut InjectorApp) {
    theme::section_title(ui, "Dll");
    ui.add_space(6.0);

    theme::card(Palette::CARD, None).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                let chosen = app.dll().map(dll::file_name);
                match chosen {
                    Some(name) => {
                        ui.label(RichText::new(name).strong().color(Palette::TEXT));
                        let full = app
                            .dll()
                            .map(|p| p.display().to_string())
                            .unwrap_or_default();
                        ui.label(RichText::new(full).color(Palette::TEXT_DIM).small());
                    }
                    None => {
                        ui.label(
                            RichText::new("No DLL selected")
                                .color(Palette::TEXT_DIM)
                                .strong(),
                        );
                        ui.label(
                            RichText::new("Browse for one, or use the dll tab.")
                                .color(Palette::TEXT_DIM)
                                .small(),
                        );
                    }
                }
            });

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add_enabled(!app.is_busy(), egui::Button::new("Browse…"))
                    .on_hover_text("Select a dll from anywhere on disk")
                    .clicked()
                {
                    open_file_dialog(app);
                }
            });
        });
    });
}

/// Opens the native OS file picker and records the chosen path.
fn open_file_dialog(app: &mut InjectorApp) {
    let result = rfd::FileDialog::new()
        .set_title("Select a dll to inject")
        .add_filter("dll", &[std::env::consts::DLL_EXTENSION])
        .pick_file();

    if let Some(path) = result {
        app.set_dll(path);
    }
}

/// The scan row and the scrollable list of process cards.
fn process_list(ui: &mut Ui, app: &mut InjectorApp) {
    ui.horizontal(|ui| {
        theme::section_title(ui, "Targets");
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .add_enabled(!app.is_busy(), egui::Button::new("🔄  Scan"))
                .on_hover_text("Look for running Minecraft processes")
                .clicked()
            {
                app.scan();
            }
        });
    });

    ui.add_space(8.0);

    if app.processes().is_empty() {
        theme::empty_state(
            ui,
            "No Minecraft processes found.",
            "Launch Minecraft, then press Scan.",
        );
        return;
    }

    let selected = app.selected_pid();
    let mut clicked = None;
    ScrollArea::vertical()
        .auto_shrink([false, false])
        .max_height(ui.available_height() - 4.0)
        .show(ui, |ui| {
            for proc in app.processes() {
                let is_selected = selected == Some(proc.pid);
                if process_card(ui, proc, is_selected).clicked() {
                    clicked = Some(proc.pid);
                }
                ui.add_space(6.0);
            }
        });

    if let Some(pid) = clicked {
        app.select(pid);
    }
}

/// One selectable Minecraft process, with the pid shown as a badge.
fn process_card(
    ui: &mut Ui,
    proc: &crate::discovery::ProcessInfo,
    selected: bool,
) -> egui::Response {
    let fill = if selected {
        Palette::ACCENT_DIM
    } else {
        Palette::CARD
    };

    let response = theme::card(fill, selected.then_some(Palette::ACCENT))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new(&proc.info).strong().color(Palette::TEXT));
                    ui.label(
                        RichText::new(if selected {
                            "selected"
                        } else {
                            "click to select"
                        })
                        .color(Palette::TEXT_DIM)
                        .small(),
                    );
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(theme::badge(&proc.pid.to_string(), Palette::ACCENT));
                });
            });
        })
        .response;

    response.interact(egui::Sense::click())
}

/// One dll from the home folder, with its size as a badge. Double-click to
/// inject immediately.
fn dll_row(ui: &mut Ui, entry: &DllEntry, selected: bool) -> egui::Response {
    let fill = if selected {
        Palette::ACCENT_DIM
    } else {
        Palette::CARD
    };

    let response = theme::card(fill, selected.then_some(Palette::ACCENT))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new("🧩").color(Palette::TEXT_DIM));
                ui.vertical(|ui| {
                    ui.label(RichText::new(&entry.name).strong().color(Palette::TEXT));
                    ui.label(
                        RichText::new("double-click to inject")
                            .color(Palette::TEXT_DIM)
                            .small(),
                    );
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(theme::badge(&entry.size, Palette::TEXT_DIM));
                });
            });
        })
        .response;

    response.interact(egui::Sense::click())
}

/// The status line at the bottom of the window, as a colored badge so the
/// outcome is readable at a glance.
fn status_banner(ui: &mut Ui, status: &InjectionStatus) {
    let (color, message) = match status {
        InjectionStatus::Failed(_) | InjectionStatus::Unverified(_) => {
            (Palette::ERROR, status.message())
        }
        InjectionStatus::Done(_) => (Palette::SUCCESS, status.message()),
        InjectionStatus::Injecting(_) => (Palette::WARNING, status.message()),
        _ => (Palette::TEXT_DIM, status.message()),
    };

    ui.label(RichText::new(message).color(color).strong());
}
