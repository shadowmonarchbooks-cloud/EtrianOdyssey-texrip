#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod windows_app {
    use eframe::egui;
    use eo_extract::{default_output_path, extract_rom_to_directory, ExtractionReport};
    use std::{
        path::{Path, PathBuf},
        process::Command,
        sync::mpsc::{self, Receiver, TryRecvError},
        thread,
        time::Duration,
    };

    pub fn run() -> eframe::Result<()> {
        let options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([760.0, 560.0])
                .with_min_inner_size([640.0, 460.0]),
            ..Default::default()
        };
        eframe::run_native(
            "EO-TexRip",
            options,
            Box::new(|cc| Ok(Box::new(TexRipApp::new(cc)))),
        )
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum StatusKind {
        Idle,
        Running,
        Success,
        Error,
    }

    struct TexRipApp {
        rom_path: String,
        output_path: String,
        status: String,
        status_kind: StatusKind,
        running: bool,
        result_rx: Option<Receiver<Result<ExtractionReport, String>>>,
        last_report: Option<ExtractionReport>,
    }

    impl TexRipApp {
        fn new(cc: &eframe::CreationContext<'_>) -> Self {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Self {
                rom_path: String::new(),
                output_path: String::new(),
                status: "Choose a decrypted EOU1 or EO2U ROM to begin.".to_owned(),
                status_kind: StatusKind::Idle,
                running: false,
                result_rx: None,
                last_report: None,
            }
        }

        fn set_rom(&mut self, path: PathBuf) {
            self.rom_path = path.display().to_string();
            self.output_path = default_output_path(&path).display().to_string();
            self.status = "Ready to extract.".to_owned();
            self.status_kind = StatusKind::Idle;
            self.last_report = None;
        }

        fn choose_rom(&mut self) {
            if let Some(path) = rfd::FileDialog::new()
                .set_title("Choose decrypted Etrian Odyssey ROM")
                .add_filter(
                    "Nintendo 3DS ROM",
                    &["3ds", "cci", "cia", "cxi", "ncch", "romfs"],
                )
                .pick_file()
            {
                self.set_rom(path);
            }
        }

        fn choose_output(&mut self) {
            let mut dialog = rfd::FileDialog::new().set_title("Choose extraction folder");
            if !self.output_path.trim().is_empty() {
                dialog = dialog.set_directory(Path::new(&self.output_path));
            }
            if let Some(path) = dialog.pick_folder() {
                self.output_path = path.display().to_string();
            }
        }

        fn start_extraction(&mut self) {
            let rom = PathBuf::from(self.rom_path.trim());
            if self.rom_path.trim().is_empty() {
                self.status = "Choose a decrypted ROM first.".to_owned();
                self.status_kind = StatusKind::Error;
                return;
            }
            if !rom.is_file() {
                self.status = "The selected ROM file does not exist.".to_owned();
                self.status_kind = StatusKind::Error;
                return;
            }

            let output = if self.output_path.trim().is_empty() {
                default_output_path(&rom)
            } else {
                PathBuf::from(self.output_path.trim())
            };
            self.output_path = output.display().to_string();
            self.running = true;
            self.status_kind = StatusKind::Running;
            self.status = "Reading ROM, unpacking containers, and decoding textures...".to_owned();
            self.last_report = None;

            let (tx, rx) = mpsc::channel();
            self.result_rx = Some(rx);
            thread::spawn(move || {
                let result =
                    extract_rom_to_directory(&rom, &output).map_err(|error| error.to_string());
                let _ = tx.send(result);
            });
        }

        fn poll_result(&mut self, ctx: &egui::Context) {
            if !self.running {
                return;
            }
            ctx.request_repaint_after(Duration::from_millis(150));
            let poll = self.result_rx.as_ref().map(Receiver::try_recv);
            match poll {
                Some(Ok(Ok(report))) => {
                    self.running = false;
                    self.result_rx = None;
                    if report.textures_written == 0 {
                        self.status = format!(
                            "Finished, but no supported textures were decoded. {} warning(s) were written to the report.",
                            report.issues.len()
                        );
                        self.status_kind = StatusKind::Error;
                    } else {
                        self.status = format!(
                            "Extraction complete: {} texture(s), {} warning(s).",
                            report.textures_written,
                            report.issues.len()
                        );
                        self.status_kind = StatusKind::Success;
                    }
                    self.last_report = Some(report);
                }
                Some(Ok(Err(error))) => {
                    self.running = false;
                    self.result_rx = None;
                    self.status = format!("Extraction failed: {error}");
                    self.status_kind = StatusKind::Error;
                }
                Some(Err(TryRecvError::Disconnected)) => {
                    self.running = false;
                    self.result_rx = None;
                    self.status = "Extraction worker stopped unexpectedly.".to_owned();
                    self.status_kind = StatusKind::Error;
                }
                Some(Err(TryRecvError::Empty)) | None => {}
            }
        }

        fn open_output(&mut self) {
            let path = PathBuf::from(self.output_path.trim());
            if !path.is_dir() {
                self.status = "The output folder does not exist yet.".to_owned();
                self.status_kind = StatusKind::Error;
                return;
            }
            if let Err(error) = Command::new("explorer").arg(&path).spawn() {
                self.status = format!("Could not open output folder: {error}");
                self.status_kind = StatusKind::Error;
            }
        }

        fn status_color(&self, visuals: &egui::Visuals) -> egui::Color32 {
            match self.status_kind {
                StatusKind::Idle => visuals.text_color(),
                StatusKind::Running => egui::Color32::LIGHT_BLUE,
                StatusKind::Success => egui::Color32::LIGHT_GREEN,
                StatusKind::Error => egui::Color32::LIGHT_RED,
            }
        }
    }

    impl eframe::App for TexRipApp {
        fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
            self.poll_result(ctx);

            let dropped = ctx.input(|input| input.raw.dropped_files.clone());
            if let Some(path) = dropped.into_iter().find_map(|file| file.path) {
                if !self.running {
                    self.set_rom(path);
                }
            }

            egui::CentralPanel::default().show(ctx, |ui| {
                ui.heading("EO-TexRip");
                ui.label(format!(
                    "{} · Native EOU1 / EO2U texture extractor",
                    env!("CARGO_PKG_VERSION")
                ));
                ui.add_space(6.0);
                ui.label("Select a decrypted/cleartext Nintendo 3DS ROM. EO-TexRip reads the ROM locally and writes decoded PNG textures to the folder you choose.");
                ui.add_space(14.0);

                ui.group(|ui| {
                    ui.label(egui::RichText::new("Decrypted ROM").strong());
                    ui.horizontal(|ui| {
                        let width = (ui.available_width() - 92.0).max(120.0);
                        ui.add_enabled(
                            !self.running,
                            egui::TextEdit::singleline(&mut self.rom_path).desired_width(width),
                        );
                        if ui
                            .add_enabled(!self.running, egui::Button::new("Browse…"))
                            .clicked()
                        {
                            self.choose_rom();
                        }
                    });
                    ui.small("You can also drag a ROM file onto this window.");

                    ui.add_space(10.0);
                    ui.label(egui::RichText::new("Output folder").strong());
                    ui.horizontal(|ui| {
                        let width = (ui.available_width() - 92.0).max(120.0);
                        ui.add_enabled(
                            !self.running,
                            egui::TextEdit::singleline(&mut self.output_path).desired_width(width),
                        );
                        if ui
                            .add_enabled(!self.running, egui::Button::new("Browse…"))
                            .clicked()
                        {
                            self.choose_output();
                        }
                    });
                });

                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    if self.running {
                        ui.spinner();
                    }
                    if ui
                        .add_enabled(
                            !self.running && !self.rom_path.trim().is_empty(),
                            egui::Button::new("Extract Textures"),
                        )
                        .clicked()
                    {
                        self.start_extraction();
                    }
                    if ui
                        .add_enabled(
                            !self.running && !self.output_path.trim().is_empty(),
                            egui::Button::new("Open Output Folder"),
                        )
                        .clicked()
                    {
                        self.open_output();
                    }
                });

                ui.add_space(12.0);
                ui.colored_label(self.status_color(ui.visuals()), &self.status);

                if let Some(report) = &self.last_report {
                    ui.add_space(12.0);
                    ui.separator();
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Game profile:").strong());
                        ui.monospace(&report.profile_id);
                    });
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Textures written:").strong());
                        ui.label(report.textures_written.to_string());
                    });
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Warnings:").strong());
                        ui.label(report.issues.len().to_string());
                    });

                    if !report.issues.is_empty() {
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new("Recent warnings").strong());
                        egui::ScrollArea::vertical()
                            .max_height(150.0)
                            .show(ui, |ui| {
                                for issue in report.issues.iter().take(8) {
                                    ui.monospace(format!(
                                        "{} · {} · {}",
                                        issue.stage, issue.source, issue.message
                                    ));
                                }
                            });
                        ui.small("The full warning list is saved in extraction-report.json.");
                    }
                }

                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.small("EO-TexRip does not include or download Nintendo keys, ROMs, firmware, or game assets. Encrypted content is rejected rather than guessed.");
                });
            });
        }
    }
}

#[cfg(windows)]
fn main() -> eframe::Result<()> {
    windows_app::run()
}

#[cfg(not(windows))]
fn main() {
    eprintln!(
        "EO-TexRip {} desktop packaging is currently Windows-only.",
        env!("CARGO_PKG_VERSION")
    );
}
