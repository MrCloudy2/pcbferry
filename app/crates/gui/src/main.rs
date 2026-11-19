//! pcbferry as a desktop app: pick (or drop) an Altium or KiCad design, pick where the result
//! goes, convert. The work runs on a thread; its log streams into the window.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use eframe::egui;
use pcbferry_core::{a2k, altium, engine, k2a};
use std::path::{Path, PathBuf};
use std::sync::mpsc;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("pcbferry")
            .with_inner_size([760.0, 640.0])
            .with_min_inner_size([520.0, 420.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    // `pcbferry-app [--convert] DESIGN`: start with that design chosen (Open With, file
    // associations), and with --convert, converting it
    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let convert = args.iter().any(|a| a.as_os_str() == "--convert");
    let input = args.into_iter().find(|p| Direction::of(p).is_some());
    eframe::run_native(
        "pcbferry",
        options,
        Box::new(move |cc| {
            let mut app = App::new();
            if let Some(p) = input {
                app.set_input(std::path::absolute(&p).unwrap_or(p));
                if convert {
                    app.start(&cc.egui_ctx);
                }
            }
            Ok(Box::new(app))
        }),
    )
}

#[derive(Clone, Copy, PartialEq)]
enum Direction {
    ToKicad,
    ToAltium,
}

impl Direction {
    fn of(path: &Path) -> Option<Direction> {
        let ext = path.extension()?.to_string_lossy().to_lowercase();
        match ext.as_str() {
            "prjpcb" | "schdoc" | "pcbdoc" => Some(Direction::ToKicad),
            "kicad_pro" | "kicad_pcb" | "kicad_sch" => Some(Direction::ToAltium),
            _ => None,
        }
    }
}

/// What the worker thread tells the window
enum Msg {
    Log(String),
    Progress(u64, u64),
    EngineReady(engine::Engine),
    Done(Result<Finished, String>),
}

struct Finished {
    ok: bool,
    out: PathBuf,
    /// The KiCad project to open, after Altium -> KiCad
    kicad_project: Option<PathBuf>,
    notes: Vec<String>,
}

enum State {
    Idle,
    Running {
        rx: mpsc::Receiver<Msg>,
        progress: Option<(u64, u64)>,
    },
}

struct App {
    direction: Direction,
    input: Option<PathBuf>,
    /// The boards a chosen .PrjPcb lists, and which one to convert
    boards: Vec<PathBuf>,
    board: usize,
    out: Option<PathBuf>,
    /// The output folder was picked by hand (rather than derived from the input)
    out_chosen: bool,
    opts: a2k::Options,
    k2a_check: bool,
    engine: Option<engine::Engine>,
    state: State,
    log: Vec<String>,
    result: Option<Result<Finished, String>>,
}

impl App {
    fn new() -> App {
        App {
            direction: Direction::ToKicad,
            input: None,
            boards: Vec::new(),
            board: 0,
            out: None,
            out_chosen: false,
            opts: a2k::Options {
                check: true,
                fill_zones: true,
                ..Default::default()
            },
            k2a_check: true,
            engine: engine::find(),
            state: State::Idle,
            log: Vec::new(),
            result: None,
        }
    }

    fn running(&self) -> bool {
        matches!(self.state, State::Running { .. })
    }

    fn set_input(&mut self, path: PathBuf) {
        if let Some(d) = Direction::of(&path) {
            self.direction = d;
        }
        self.boards = if altium::is(&path, "prjpcb") {
            altium::project_boards(&path).unwrap_or_default()
        } else {
            Vec::new()
        };
        self.board = 0;
        if !self.out_chosen {
            self.out = Some(default_out(&path, self.direction));
        }
        self.input = Some(path);
        self.result = None;
    }

    fn pick_input(&mut self) {
        let mut d = rfd::FileDialog::new();
        d = match self.direction {
            Direction::ToKicad => d.add_filter(
                "Altium project, schematic or board",
                &[
                    "PrjPcb", "prjpcb", "PRJPCB", "SchDoc", "schdoc", "SCHDOC", "PcbDoc", "pcbdoc",
                    "PCBDOC",
                ],
            ),
            Direction::ToAltium => d.add_filter(
                "KiCad project, schematic or board",
                &["kicad_pro", "kicad_sch", "kicad_pcb"],
            ),
        };
        if let Some(dir) = self.input.as_deref().and_then(Path::parent) {
            d = d.set_directory(dir);
        }
        if let Some(p) = d.pick_file() {
            self.set_input(p);
        }
    }

    fn pick_out(&mut self) {
        let mut d = rfd::FileDialog::new().set_title("Where should the converted design go?");
        let near = self
            .out
            .as_deref()
            .and_then(Path::parent)
            .or(self.input.as_deref().and_then(Path::parent));
        if let Some(dir) = near {
            d = d.set_directory(dir);
        }
        if let Some(folder) = d.pick_folder() {
            // A folder with things in it gets a new folder for the design inside it
            let empty = folder.read_dir().is_ok_and(|mut r| r.next().is_none());
            self.out = Some(match (&self.input, empty) {
                (Some(i), false) => unique(&folder.join(out_name(i, self.direction))),
                _ => folder,
            });
            self.out_chosen = true;
        }
    }

    fn start(&mut self, ctx: &egui::Context) {
        let (Some(input), Some(out)) = (self.input.clone(), self.out.clone()) else {
            return;
        };
        let (tx, rx) = mpsc::channel();
        self.state = State::Running { rx, progress: None };
        self.log.clear();
        self.result = None;
        let direction = self.direction;
        let mut opts = self.opts.clone();
        opts.board = self
            .boards
            .get(self.board)
            .cloned()
            .filter(|_| self.boards.len() > 1);
        let need_engine = direction == Direction::ToKicad || self.k2a_check;
        let engine = self.engine.clone();
        let k2a_check = self.k2a_check;
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let send = |m: Msg| {
                let _ = tx.send(m);
                ctx.request_repaint();
            };
            let log = |line: String| send(Msg::Log(line));
            let engine = match (need_engine, engine) {
                (true, None) => match download(&send) {
                    Ok(e) => {
                        send(Msg::EngineReady(e.clone()));
                        Some(e)
                    }
                    Err(e) => return send(Msg::Done(Err(e))),
                },
                (_, e) => e,
            };
            let done = match (direction, &engine) {
                (Direction::ToKicad, Some(engine)) => {
                    a2k::convert(engine, &input, &out, &opts, &log)
                        .map(|o| Finished {
                            ok: o.ok,
                            notes: a2k::notes(&o.summary),
                            kicad_project: o.project,
                            out: o.out,
                        })
                        .map_err(|e| e.to_string())
                }
                (Direction::ToKicad, None) => Err("no converter engine".into()),
                (Direction::ToAltium, _) => {
                    let check = engine.as_ref().filter(|_| k2a_check);
                    k2a::convert(&input, &out, check, &log).map(|o| Finished {
                        ok: o.ok,
                        notes: o.notes,
                        kicad_project: None,
                        out: o.out,
                    })
                }
            };
            send(Msg::Done(done));
        });
    }

    fn poll(&mut self) {
        let State::Running { rx, progress } = &mut self.state else {
            return;
        };
        let mut finished = None;
        while let Ok(m) = rx.try_recv() {
            match m {
                Msg::Log(l) => self.log.push(l),
                Msg::Progress(d, t) => *progress = Some((d, t)),
                Msg::EngineReady(e) => {
                    *progress = None;
                    self.engine = Some(e);
                }
                Msg::Done(r) => finished = Some(r),
            }
        }
        if let Some(r) = finished {
            self.state = State::Idle;
            self.result = Some(r);
            if !self.out_chosen {
                // Converting the same design again goes to a new folder
                self.out = self
                    .input
                    .as_deref()
                    .map(|p| default_out(p, self.direction));
            }
        }
    }
}

fn download(send: &dyn Fn(Msg)) -> Result<engine::Engine, String> {
    let info = engine::download_info()?;
    send(Msg::Log(format!(
        "pcbferry: downloading the converter engine ({:.0} MB), once",
        info.size as f64 / 1e6
    )));
    let progress = |d, t| send(Msg::Progress(d, t));
    let e = engine::install(&info, &progress)?;
    send(Msg::Log(format!(
        "pcbferry: engine ready: {}",
        e.cli.display()
    )));
    Ok(e)
}

fn out_name(input: &Path, d: Direction) -> String {
    let stem = input
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    match d {
        Direction::ToKicad => format!("{stem}-kicad"),
        Direction::ToAltium => format!("{stem}-altium"),
    }
}

/// NAME-kicad (or NAME-altium) next to the input, numbered if it is taken
fn default_out(input: &Path, d: Direction) -> PathBuf {
    unique(&input.with_file_name(out_name(input, d)))
}

fn unique(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    (2..)
        .map(|n| path.with_file_name(format!("{name}-{n}")))
        .find(|p| !p.exists())
        .unwrap()
}

/// Open a file or folder with what the system uses for it
fn open(path: &Path) {
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(program).arg(path).spawn();
}

const LABEL_WIDTH: f32 = 96.0;

/// A label, a path (shortened to fit, in full on hover) and a button at the right; true when
/// the button is clicked
fn path_row(
    ui: &mut egui::Ui,
    label: &str,
    path: Option<&Path>,
    hint: &str,
    button: &str,
    enabled: bool,
) -> bool {
    ui.horizontal(|ui| {
        ui.add_sized([LABEL_WIDTH, 20.0], egui::Label::new(label));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let clicked = ui.add_enabled(enabled, egui::Button::new(button)).clicked();
            ui.with_layout(
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| match path {
                    Some(p) => {
                        let text = p.display().to_string();
                        ui.add(egui::Label::new(text.clone()).truncate())
                            .on_hover_text(text);
                    }
                    None => {
                        ui.add(egui::Label::new(egui::RichText::new(hint).weak()).truncate());
                    }
                },
            );
            clicked
        })
        .inner
    })
    .inner
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll();
        let ctx = ui.ctx().clone();
        // Files dropped on the window
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        if !self.running() {
            if let Some(p) = dropped.into_iter().find(|p| Direction::of(p).is_some()) {
                self.set_input(p);
            }
        }
        let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());
        let running = self.running();

        egui::Panel::top("direction").show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.heading("pcbferry");
                ui.add_space(12.0);
                ui.add_enabled_ui(!running, |ui| {
                    let before = self.direction;
                    ui.selectable_value(&mut self.direction, Direction::ToKicad, "Altium to KiCad");
                    ui.selectable_value(
                        &mut self.direction,
                        Direction::ToAltium,
                        "KiCad to Altium",
                    );
                    if self.direction != before {
                        // An input of the other kind no longer fits
                        if self.input.as_deref().and_then(Direction::of) != Some(self.direction) {
                            self.input = None;
                            self.boards.clear();
                            if !self.out_chosen {
                                self.out = None;
                            }
                        }
                        self.result = None;
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.weak(format!("v{}", engine::VERSION));
                });
            });
            ui.add_space(6.0);
        });

        egui::Panel::bottom("engine").show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| match &self.engine {
                Some(e) => {
                    ui.weak("Engine:");
                    ui.weak(e.cli.display().to_string())
                        .on_hover_text("pcbferry's patched kicad-cli, which reads Altium files");
                }
                None => {
                    ui.weak("The converter engine (about 150 MB) is downloaded on the first Altium to KiCad conversion.");
                }
            });
            ui.add_space(4.0);
        });

        egui::CentralPanel::default().show(ui, |ui| {
            if hovering {
                ui.painter().rect_stroke(
                    ui.max_rect().shrink(4.0),
                    6.0,
                    egui::Stroke::new(2.0, ui.visuals().selection.bg_fill),
                    egui::StrokeKind::Inside,
                );
            }
            let label = match self.direction {
                Direction::ToKicad => "Altium design",
                Direction::ToAltium => "KiCad design",
            };
            let hint = match self.direction {
                Direction::ToKicad => "a .PrjPcb, .SchDoc or .PcbDoc: choose one or drop it here",
                Direction::ToAltium => "a .kicad_pro, .kicad_sch or .kicad_pcb: choose one or drop it here",
            };
            if path_row(ui, label, self.input.as_deref(), hint, "Choose…", !running) {
                self.pick_input();
            }
            if self.boards.len() > 1 {
                ui.horizontal(|ui| {
                    ui.add_sized([LABEL_WIDTH, 20.0], egui::Label::new("Board"));
                    let names: Vec<String> = self
                        .boards
                        .iter()
                        .map(|b| b.file_name().unwrap_or_default().to_string_lossy().into_owned())
                        .collect();
                    ui.add_enabled_ui(!running, |ui| {
                        egui::ComboBox::from_id_salt("board")
                            .selected_text(names[self.board].clone())
                            .show_index(ui, &mut self.board, names.len(), |i| names[i].clone());
                    });
                    ui.weak("A KiCad project holds one board");
                });
            }
            if path_row(ui, "Save to", self.out.as_deref(), "a new folder next to the design", "Change…", !running) {
                self.pick_out();
            }

            ui.add_space(8.0);
            ui.add_enabled_ui(!running, |ui| {
                egui::CollapsingHeader::new("Options").show(ui, |ui| match self.direction {
                    Direction::ToKicad => {
                        ui.checkbox(&mut self.opts.check, "Check the result (netlist, ERC, DRC)");
                        ui.checkbox(&mut self.opts.fill_zones, "Fill zones (Altium computes planes only when it writes output)");
                        let mut rules = !self.opts.no_custom_rules;
                        ui.checkbox(&mut rules, "Translate scoped design rules into KiCad custom rules");
                        self.opts.no_custom_rules = !rules;
                        ui.checkbox(
                            &mut self.opts.remove_unused_lands,
                            "Remove unused inner-layer pad and via lands, as Altium planes have none",
                        );
                        ui.horizontal(|ui| {
                            let mut on = self.opts.footprint_slash.is_some();
                            ui.checkbox(&mut on, "Replace '/' in footprint names with");
                            let mut c = self.opts.footprint_slash.clone().unwrap_or_else(|| "_".into());
                            ui.add_enabled(on, egui::TextEdit::singleline(&mut c).desired_width(24.0));
                            let c: String = c.chars().take(1).collect();
                            self.opts.footprint_slash = (on && !c.is_empty()).then_some(c);
                        });
                    }
                    Direction::ToAltium => {
                        ui.checkbox(
                            &mut self.k2a_check,
                            "Check the result: read it back with KiCad's Altium importer and compare",
                        );
                    }
                });
            });

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let ready = self.input.is_some() && self.out.is_some() && !running;
                let label = match (self.direction, &self.engine) {
                    (Direction::ToKicad, None) => "Download the engine and convert",
                    (Direction::ToAltium, None) if self.k2a_check => "Convert (downloads the engine to check)",
                    (Direction::ToKicad, _) => "Convert to KiCad",
                    (Direction::ToAltium, _) => "Convert to Altium",
                };
                let button = egui::Button::new(egui::RichText::new(label).strong()).min_size(egui::vec2(180.0, 30.0));
                if ui.add_enabled(ready, button).clicked() {
                    self.start(&ctx);
                }
                if let State::Running { progress, .. } = &self.state {
                    match progress {
                        Some((d, t)) if *t > 0 => {
                            ui.add(egui::ProgressBar::new(*d as f32 / *t as f32).show_percentage().desired_width(220.0));
                            ui.label("downloading the engine");
                        }
                        _ => {
                            ui.spinner();
                            ui.label("converting…");
                        }
                    }
                }
            });

            match &self.result {
                None => {}
                Some(Err(e)) => {
                    ui.add_space(8.0);
                    ui.colored_label(ui.visuals().error_fg_color, format!("✘ {e}"));
                }
                Some(Ok(r)) => {
                    ui.add_space(8.0);
                    if r.ok {
                        ui.colored_label(egui::Color32::from_rgb(60, 170, 90), format!("✔ Converted: {}", r.out.display()));
                    } else {
                        ui.colored_label(
                            ui.visuals().error_fg_color,
                            format!("✘ A step failed; the log below says which. Output: {}", r.out.display()),
                        );
                    }
                    for n in r.notes.iter().take(12) {
                        ui.label(format!("• {n}"));
                    }
                    if r.notes.len() > 12 {
                        ui.weak(format!("… and {} more in the log", r.notes.len() - 12));
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Open the folder").clicked() {
                            open(&r.out);
                        }
                        if let Some(p) = &r.kicad_project {
                            if ui.button("Open in KiCad").clicked() {
                                open(p);
                            }
                        }
                    });
                }
            }

            ui.add_space(8.0);
            ui.separator();
            // Paths in the log relative to the output's folder, and the engine as kicad-cli
            let base = self.out.as_deref().and_then(Path::parent).map(|p| format!("{}{}", p.display(), std::path::MAIN_SEPARATOR));
            let cli = self.engine.as_ref().map(|e| e.cli.display().to_string());
            egui::ScrollArea::both().stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
                for line in &self.log {
                    let mut line = line.clone();
                    if let Some(c) = &cli {
                        line = line.replace(c.as_str(), "kicad-cli");
                    }
                    if let Some(b) = &base {
                        line = line.replace(b.as_str(), "");
                    }
                    ui.add(egui::Label::new(egui::RichText::new(line).monospace().size(11.5)).extend());
                }
            });
        });
    }
}
