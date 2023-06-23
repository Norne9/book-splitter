use crate::split::StatusReport;
use crossbeam_channel::{unbounded, Receiver};
use native_dialog::FileDialog;
use std::path::PathBuf;
use tokio::runtime;

enum ParsingStatus {
    NotStarted,
    Working,
    Done,
    Error(anyhow::Error),
}

/// We derive Deserialize/Serialize so we can persist app state on shutdown.
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(default)] // if we add new fields, give them default values when deserializing old state
pub struct TemplateApp {
    // Example stuff:
    book_path: PathBuf,
    result_folder: PathBuf,
    header_req: String,
    start_chapter: usize,
    #[serde(skip)]
    lines_processed: usize,
    #[serde(skip)]
    chapters_saved: usize,
    #[serde(skip)]
    status: ParsingStatus,
    #[serde(skip)]
    last_hit: Option<String>,
    #[serde(skip)]
    runtime: runtime::Runtime,
    #[serde(skip)]
    channel: Option<Receiver<StatusReport>>,
}

impl Default for TemplateApp {
    fn default() -> Self {
        Self {
            book_path: Default::default(),
            result_folder: Default::default(),
            header_req: Default::default(),
            start_chapter: 1,
            lines_processed: 0,
            chapters_saved: 0,
            status: ParsingStatus::NotStarted,
            last_hit: None,
            runtime: runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .unwrap(),
            channel: None,
        }
    }
}

fn load_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "noto".to_string(),
        egui::FontData::from_static(include_bytes!("../fonts/NotoSans-Regular.ttf")),
    );
    fonts.font_data.insert(
        "jet".to_string(),
        egui::FontData::from_static(include_bytes!("../fonts/JetBrainsMono-Regular.ttf")),
    );
    fonts
        .families
        .get_mut(&egui::FontFamily::Proportional)
        .unwrap()
        .insert(0, "noto".to_owned());
    fonts
        .families
        .get_mut(&egui::FontFamily::Monospace)
        .unwrap()
        .insert(0, "jet".to_owned());
    ctx.set_fonts(fonts);
}

impl TemplateApp {
    /// Called once before the first frame.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // This is also where you can customize the look and feel of egui using
        // `cc.egui_ctx.set_visuals` and `cc.egui_ctx.set_fonts`.
        load_fonts(&cc.egui_ctx);
        cc.egui_ctx.set_pixels_per_point(2.0);

        // Load previous app state (if any).
        // Note that you must enable the `persistence` feature for this to work.
        if let Some(storage) = cc.storage {
            return eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default();
        }

        Default::default()
    }

    fn parse_channel(&mut self) {
        let mut drop_channel = false;
        if let Some(rx) = &self.channel {
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    StatusReport::Started => {
                        self.status = ParsingStatus::Working;
                        self.lines_processed = 0;
                        self.chapters_saved = 0;
                        self.last_hit = None;
                    }
                    StatusReport::LinesParsed(lines) => self.lines_processed = lines,
                    StatusReport::ChaptersSplit(chaps) => self.chapters_saved = chaps,
                    StatusReport::NewTitle(title) => self.last_hit = Some(title),
                    StatusReport::Error(e) => {
                        self.status = ParsingStatus::Error(e);
                        drop_channel = true;
                    }
                    StatusReport::Done => {
                        self.status = ParsingStatus::Done;
                        drop_channel = true;
                    }
                }
            }
        }

        if drop_channel {
            self.channel = None;
        }
    }
}

impl eframe::App for TemplateApp {
    /// Called each time the UI needs repainting, which may be many times per second.
    /// Put your widgets into a `SidePanel`, `TopPanel`, `CentralPanel`, `Window` or `Area`.
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.parse_channel();

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.with_layout(
                egui::Layout::top_down_justified(egui::Align::Center),
                |ui| {
                    // The central panel the region left after adding TopPanel's and SidePanel's
                    ui.heading("Book Splitter");

                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.label("Book path: ");
                            let mut path = self.book_path.display().to_string();
                            ui.text_edit_singleline(&mut path);
                            if ui.button("Browse").clicked() {
                                let path = FileDialog::new()
                                    .set_location("~/")
                                    .add_filter("Text", &["txt"])
                                    .show_open_single_file()
                                    .unwrap();

                                self.book_path = match path {
                                    Some(path) => path,
                                    None => PathBuf::default(),
                                };
                            }
                        });

                        ui.horizontal(|ui| {
                            ui.label("Result folder: ");
                            let mut path = self.result_folder.display().to_string();
                            ui.text_edit_singleline(&mut path);
                            if ui.button("Browse").clicked() {
                                let path = FileDialog::new().show_open_single_dir().unwrap();

                                self.result_folder = match path {
                                    Some(path) => path,
                                    None => PathBuf::default(),
                                };
                            }
                        });
                    });

                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.label("Header regex: ");
                            ui.text_edit_singleline(&mut self.header_req);
                        });
                        ui.horizontal(|ui| {
                            ui.label("Start chapter: ");
                            ui.add(egui::DragValue::new(&mut self.start_chapter).speed(0.1));
                        });
                    });

                    match self.status {
                        ParsingStatus::Working => {
                            ui.spinner();
                        }
                        _ => {
                            if ui.button("Start").clicked() {
                                let (tx, rx) = unbounded();
                                let pattern = self.header_req.clone();
                                let file = self.book_path.clone();
                                let folder = self.result_folder.clone();
                                let start_chapter = self.start_chapter;
                                self.channel = Some(rx);
                                self.runtime.spawn(async move {
                                    crate::split::split_chapters(
                                        pattern,
                                        file,
                                        folder,
                                        start_chapter,
                                        tx,
                                    )
                                        .await
                                });
                            }
                        }
                    }

                    match self.status {
                        ParsingStatus::NotStarted => {}
                        _ => {
                            ui.separator();

                            ui.horizontal(|ui| {
                                ui.label("Line: ");
                                ui.label(self.lines_processed.to_string());
                            });

                            ui.horizontal(|ui| {
                                ui.label("Chapters: ");
                                ui.label(self.chapters_saved.to_string());
                            });

                            if let Some(title) = &self.last_hit {
                                ui.horizontal(|ui| {
                                    ui.label("Last hit: ");
                                    ui.label(title);
                                });
                            }
                        }
                    }

                    if let ParsingStatus::Error(e) = &self.status {
                        ui.separator();
                        ui.label("Error: ");
                        ui.label(e.to_string());
                    }
                },
            );
        });
    }

    /// Called by the frame work to save state before shutdown.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }
}
