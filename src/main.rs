#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use cadviewer::converter;
use cadviewer::doc::Document;
use cadviewer::plot::build::{BuildReport, PlotRequest, build, model_extents};
use cadviewer::plot::style::ColorMode;
use cadviewer::plot::{PaperSize, PlotScene};
use cadviewer::render::skia;
use cadviewer::sheets::Sheet;
use eframe::egui;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

fn main() -> eframe::Result {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "--convert") {
        if args.len() != 3 {
            show_error("用法：Cadviewer.exe --convert <输入.dwg> <输出.pdf>");
            std::process::exit(2);
        }
        if let Err(error) = convert_file(Path::new(&args[1]), Path::new(&args[2])) {
            show_error(&error);
            std::process::exit(1);
        }
        return Ok(());
    }

    let initial_file = args
        .first()
        .map(PathBuf::from)
        .filter(|path| path.is_file());
    let native_options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        viewport: egui::ViewportBuilder::default()
            .with_title("Cadviewer")
            .with_inner_size([1180.0, 760.0])
            .with_min_inner_size([720.0, 480.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };

    eframe::run_native(
        "Cadviewer",
        native_options,
        Box::new(move |creation_context| {
            Ok(Box::new(CadviewerApp::new(creation_context, initial_file)))
        }),
    )
}

fn convert_file(input: &Path, output: &Path) -> Result<(), String> {
    let options = converter::ConvertOptions::default();
    converter::convert_to_pdf(input, output, &options)
        .map(|_pages| ())
        .map_err(|error| error.to_string())
}

fn show_error(message: &str) {
    let _ = rfd::MessageDialog::new()
        .set_title("Cadviewer")
        .set_description(message)
        .set_level(rfd::MessageLevel::Error)
        .show();
}

/// The plot request for one detected sheet, or for the whole model when the
/// drawing has no title-block frames (R-SHEET-6: that is never an error).
///
/// The paper choice deliberately mirrors `converter::scenes_for`: for a sheet
/// it follows the frame's *aspect ratio*, not its size in drawing units, so a
/// 1:100 frame measuring 42000 x 29700 units lands on A3 rather than A0.
/// Picking the paper any other way here would give the preview a different
/// sheet size from the exported PDF, and since lineweights are absolute
/// millimetres the lines would visibly differ in thickness between the two.
fn sheet_request(
    doc: &Document,
    sheets: &[Sheet],
    active: usize,
    mode: ColorMode,
) -> Option<PlotRequest> {
    let frame = sheets.get(active).map(|sheet| sheet.bounds);
    let window = frame.unwrap_or_else(|| model_extents(doc));
    if !window.valid() {
        return None;
    }
    let paper = match frame {
        Some(_) => {
            let ratio = window.width() / window.height().max(f64::EPSILON);
            if ratio >= 1.0 {
                PaperSize::fit(297.0 * ratio, 297.0)
            } else {
                PaperSize::fit(297.0, 297.0 / ratio)
            }
        }
        None => PaperSize::fit(window.width(), window.height()),
    };
    Some(PlotRequest {
        window,
        paper,
        margin_mm: converter::DEFAULT_MARGIN_MM,
        mode,
    })
}

/// Entity kinds the builder could not draw, with counts, for the warnings
/// area. TEXT and MTEXT are expected here until the text phase lands.
fn skipped_summary(report: &BuildReport) -> String {
    if report.skipped.is_empty() {
        return String::new();
    }
    let mut kinds: Vec<_> = report.skipped.iter().collect();
    kinds.sort();
    let list = kinds
        .iter()
        .map(|(kind, count)| format!("{kind} x{count}"))
        .collect::<Vec<_>>()
        .join("、");
    format!("未绘制实体：{list}")
}

struct LoadedDocument {
    source: PathBuf,
    /// Kept in memory so switching sheets re-plots the parsed document
    /// instead of re-running dwg2dxf.
    doc: Arc<Document>,
    sheets: Vec<Sheet>,
    scene: Arc<PlotScene>,
    item_count: usize,
    /// Loader warnings (LibreDWG stderr), held apart from the per-build
    /// skipped summary so switching sheets cannot accumulate copies of it.
    load_warnings: String,
    skipped: String,
}

struct BuiltScene {
    /// Which loaded document this scene belongs to. A rebuild in flight when
    /// a new file finishes loading must not overwrite the new document.
    generation: u64,
    scene: Arc<PlotScene>,
    item_count: usize,
    skipped: String,
}

enum AppMessage {
    Loaded(Result<LoadedDocument, String>),
    Rebuilt(BuiltScene),
    Exported {
        path: PathBuf,
        result: Result<usize, String>,
    },
}

struct CadviewerApp {
    sender: Sender<AppMessage>,
    receiver: Receiver<AppMessage>,
    document: Option<LoadedDocument>,
    generation: u64,
    texture: Option<egui::TextureHandle>,
    /// Sheet point under the middle of the viewport, in millimetres measured
    /// from the top-left corner of the paper (y grows downwards, matching
    /// screen space so the pan maths below is a plain subtraction).
    center: egui::Pos2,
    /// Logical screen pixels per sheet millimetre.
    zoom: f32,
    fit_zoom: f32,
    view_size: egui::Vec2,
    render_dirty: bool,
    last_render: Instant,
    loading: bool,
    exporting: bool,
    fit_requested: bool,
    active_sheet: usize,
    color_mode: ColorMode,
    /// Set only when the active sheet or the colour mode actually changes.
    /// A rebuild walks every entity and expands every block, which takes
    /// seconds on a real drawing, so this must never be set from a hover, a
    /// zoom, a pan or an unconditional per-frame path.
    needs_rebuild: bool,
    rebuilding: bool,
    status: String,
    warnings: String,
}

impl CadviewerApp {
    fn new(context: &eframe::CreationContext<'_>, initial_file: Option<PathBuf>) -> Self {
        install_windows_font(&context.egui_ctx);
        context.egui_ctx.set_visuals(egui::Visuals::dark());
        let (sender, receiver) = mpsc::channel();
        let mut app = Self {
            sender,
            receiver,
            document: None,
            generation: 0,
            texture: None,
            center: egui::Pos2::ZERO,
            zoom: 1.0,
            fit_zoom: 1.0,
            view_size: egui::Vec2::ZERO,
            render_dirty: false,
            last_render: Instant::now() - Duration::from_secs(1),
            loading: false,
            exporting: false,
            fit_requested: false,
            active_sheet: 0,
            color_mode: ColorMode::Color,
            needs_rebuild: false,
            rebuilding: false,
            status: "拖入 DWG 文件，或点击“打开”".to_owned(),
            warnings: String::new(),
        };
        if let Some(path) = initial_file {
            app.begin_load(path, &context.egui_ctx);
        }
        app
    }

    fn begin_load(&mut self, path: PathBuf, context: &egui::Context) {
        if self.loading {
            return;
        }
        self.loading = true;
        self.status = format!("正在打开 {}…", file_name(&path));
        let mode = self.color_mode;
        let sender = self.sender.clone();
        let context = context.clone();
        std::thread::spawn(move || {
            let result = load_document(&path, mode);
            let _ = sender.send(AppMessage::Loaded(result));
            context.request_repaint();
        });
    }

    /// Re-plot the loaded document for the active sheet and colour mode.
    ///
    /// Runs off the UI thread: on a real drawing one sheet takes seconds to
    /// build, and blocking the event loop for that long would freeze the
    /// window. Called only from the `needs_rebuild` edge in `ui`.
    fn begin_rebuild(&mut self, context: &egui::Context) {
        let Some(document) = &self.document else {
            return;
        };
        if self.rebuilding {
            return;
        }
        let Some(request) = sheet_request(
            &document.doc,
            &document.sheets,
            self.active_sheet,
            self.color_mode,
        ) else {
            self.status = "所选图纸中没有可打印的二维实体".to_owned();
            return;
        };
        self.rebuilding = true;
        self.status = if document.sheets.is_empty() {
            "正在绘制…".to_owned()
        } else {
            format!("正在绘制第 {} 页…", self.active_sheet + 1)
        };
        let doc = Arc::clone(&document.doc);
        let generation = self.generation;
        let sender = self.sender.clone();
        let context = context.clone();
        std::thread::spawn(move || {
            let (scene, report) = build(&doc, &request);
            let _ = sender.send(AppMessage::Rebuilt(BuiltScene {
                generation,
                item_count: report.items,
                skipped: skipped_summary(&report),
                scene: Arc::new(scene),
            }));
            context.request_repaint();
        });
    }

    fn begin_export(&mut self, path: PathBuf, sheet: Option<usize>, context: &egui::Context) {
        let Some(document) = &self.document else {
            return;
        };
        self.exporting = true;
        self.status = format!("正在生成 {}…", file_name(&path));
        let source = document.source.clone();
        // Same entry point the command-line converter uses, so the exported
        // PDF cannot differ from what `Cadconvert.exe` would produce; the
        // preview matches it because `sheet_request` mirrors its window and
        // paper choice.
        let options = converter::ConvertOptions { mode: self.color_mode, sheet };
        let sender = self.sender.clone();
        let context = context.clone();
        std::thread::spawn(move || {
            let result = converter::convert_to_pdf(&source, &path, &options)
                .map_err(|error| error.to_string());
            let _ = sender.send(AppMessage::Exported { path, result });
            context.request_repaint();
        });
    }

    fn drain_messages(&mut self, context: &egui::Context) {
        while let Ok(message) = self.receiver.try_recv() {
            match message {
                AppMessage::Loaded(result) => {
                    self.loading = false;
                    match result {
                        Ok(document) => {
                            let title = format!("{} — Cadviewer", file_name(&document.source));
                            context.send_viewport_cmd(egui::ViewportCommand::Title(title));
                            self.generation += 1;
                            self.active_sheet = 0;
                            self.document = Some(document);
                            self.texture = None;
                            self.fit_requested = true;
                            self.render_dirty = true;
                            self.refresh_status();
                        }
                        Err(error) => {
                            self.status = error.clone();
                            show_error(&error);
                        }
                    }
                }
                AppMessage::Rebuilt(built) => {
                    self.rebuilding = false;
                    if built.generation != self.generation {
                        // Belongs to a document that has since been replaced.
                        continue;
                    }
                    if let Some(document) = &mut self.document {
                        document.scene = built.scene;
                        document.item_count = built.item_count;
                        document.skipped = built.skipped;
                    }
                    self.fit_requested = true;
                    self.render_dirty = true;
                    self.refresh_status();
                }
                AppMessage::Exported { path, result } => {
                    self.exporting = false;
                    match result {
                        Ok(pages) => {
                            self.status = format!("已导出 {pages} 页：{}", path.display());
                        }
                        Err(error) => {
                            self.status = error.clone();
                            show_error(&error);
                        }
                    }
                }
            }
        }
    }

    fn refresh_status(&mut self) {
        let Some(document) = &self.document else {
            return;
        };
        let page = if document.sheets.is_empty() {
            "整幅模型".to_owned()
        } else {
            format!("第 {}/{} 页", self.active_sheet + 1, document.sheets.len())
        };
        self.status = format!(
            "{} · {} · {} 个实体 · {} 个图元",
            file_name(&document.source),
            page,
            document.doc.entities.len(),
            document.item_count
        );
        self.warnings = [document.load_warnings.as_str(), document.skipped.as_str()]
            .iter()
            .filter(|part| !part.is_empty())
            .copied()
            .collect::<Vec<_>>()
            .join("\n");
    }

    fn open_dialog(&mut self, context: &egui::Context) {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("打开 DWG")
            .add_filter("CAD 图纸", &["dwg", "dxf"])
            .pick_file()
        {
            self.begin_load(path, context);
        }
    }

    fn export_dialog(&mut self, context: &egui::Context, current_only: bool) {
        let Some(document) = &self.document else {
            return;
        };
        let stem = document
            .source
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("drawing")
            .to_owned();
        // With no frames detected there is exactly one page either way.
        let sheet = (current_only && !document.sheets.is_empty()).then_some(self.active_sheet + 1);
        let default_name = match sheet {
            Some(index) => format!("{stem}-{index}.pdf"),
            None => format!("{stem}.pdf"),
        };
        if let Some(path) = rfd::FileDialog::new()
            .set_title("导出 PDF")
            .set_file_name(default_name)
            .add_filter("PDF 文档", &["pdf"])
            .save_file()
        {
            let path = if path.extension().is_none() {
                path.with_extension("pdf")
            } else {
                path
            };
            self.begin_export(path, sheet, context);
        }
    }

    fn fit_to_view(&mut self) {
        let Some(document) = &self.document else {
            return;
        };
        if self.view_size.x <= 1.0 || self.view_size.y <= 1.0 {
            return;
        }
        let paper = document.scene.paper;
        let width = (paper.width_mm as f32).max(1.0);
        let height = (paper.height_mm as f32).max(1.0);
        self.fit_zoom = (self.view_size.x / width).min(self.view_size.y / height) * 0.94;
        self.zoom = self.fit_zoom.max(0.000_001);
        self.center = egui::pos2(width * 0.5, height * 0.5);
        self.render_dirty = true;
        self.fit_requested = false;
    }

    fn zoom_at(&mut self, factor: f32, pointer: Option<egui::Pos2>, canvas: egui::Rect) {
        if self.document.is_none() {
            return;
        }
        let old_zoom = self.zoom;
        let min_zoom = (self.fit_zoom * 0.05).max(0.000_001);
        let max_zoom = (self.fit_zoom * 200.0).max(min_zoom * 2.0);
        self.zoom = (self.zoom * factor).clamp(min_zoom, max_zoom);
        if let Some(pointer) = pointer {
            let offset = pointer - canvas.center();
            let document_point = self.center + offset / old_zoom;
            self.center = document_point - offset / self.zoom;
        }
        self.render_dirty = true;
    }

    fn render_viewport(&mut self, context: &egui::Context, canvas: egui::Rect) {
        let Some(document) = &self.document else {
            return;
        };
        let ppp = context.pixels_per_point();
        let desired_width = (canvas.width() * ppp).max(1.0);
        let desired_height = (canvas.height() * ppp).max(1.0);
        let cap_factor = (4096.0 / desired_width.max(desired_height)).min(1.0);
        let render_scale = ppp * cap_factor;
        let width = (canvas.width() * render_scale).round().max(1.0) as u32;
        let height = (canvas.height() * render_scale).round().max(1.0) as u32;

        // Pixels per sheet millimetre for this frame, and the top-left of the
        // visible window in the full-sheet pixel grid at that resolution.
        let pixels_per_mm = self.zoom * render_scale;
        let origin_x = self.center.x * pixels_per_mm - width as f32 * 0.5;
        let origin_y = self.center.y * pixels_per_mm - height as f32 * 0.5;

        let Some(pixmap) = skia::render_window(
            &document.scene,
            pixels_per_mm,
            origin_x,
            origin_y,
            width,
            height,
        ) else {
            self.status = "无法分配渲染缓冲区".to_owned();
            return;
        };

        // tiny-skia pixmaps are premultiplied; every pixel here is opaque
        // because the window is filled before anything is drawn.
        let image = egui::ColorImage::from_rgba_premultiplied(
            [width as usize, height as usize],
            pixmap.data(),
        );
        self.texture =
            Some(context.load_texture("cad-viewport", image, egui::TextureOptions::LINEAR));
        self.render_dirty = false;
        self.last_render = Instant::now();
    }
}

impl eframe::App for CadviewerApp {
    fn ui(&mut self, root_ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let context = root_ui.ctx().clone();
        self.drain_messages(&context);

        // The only place a rebuild starts. `needs_rebuild` is an edge set by
        // a sheet click, the colour toggle or a fresh load -- never by the
        // frame loop -- so an idle or merely hovered window rebuilds nothing.
        if self.needs_rebuild && !self.rebuilding {
            self.needs_rebuild = false;
            self.begin_rebuild(&context);
        }

        let dropped_file = context.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .filter_map(|file| file.path.clone())
                .find(|path| {
                    path.extension()
                        .and_then(|value| value.to_str())
                        .is_some_and(|ext| {
                            ext.eq_ignore_ascii_case("dwg") || ext.eq_ignore_ascii_case("dxf")
                        })
                })
        });
        if let Some(path) = dropped_file {
            self.begin_load(path, &context);
        }

        let open_shortcut = context.input_mut(|input| {
            input.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::CTRL,
                egui::Key::O,
            ))
        });
        if open_shortcut {
            self.open_dialog(&context);
        }

        egui::Panel::top("toolbar")
            .exact_size(48.0)
            .show(root_ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.add_space(8.0);
                    if ui
                        .add_enabled(!self.loading, egui::Button::new("打开"))
                        .clicked()
                    {
                        self.open_dialog(&context);
                    }
                    let ready = self.document.is_some() && !self.exporting;
                    if ui
                        .add_enabled(ready, egui::Button::new("导出当前页"))
                        .clicked()
                    {
                        self.export_dialog(&context, true);
                    }
                    if ui
                        .add_enabled(ready, egui::Button::new("导出全部"))
                        .clicked()
                    {
                        self.export_dialog(&context, false);
                    }
                    let mut mono = self.color_mode == ColorMode::Monochrome;
                    if ui
                        .add_enabled(
                            self.document.is_some(),
                            egui::Checkbox::new(&mut mono, "单色打印"),
                        )
                        .changed()
                    {
                        self.color_mode = if mono {
                            ColorMode::Monochrome
                        } else {
                            ColorMode::Color
                        };
                        self.needs_rebuild = true;
                    }
                    ui.separator();
                    if ui
                        .add_enabled(self.document.is_some(), egui::Button::new("−"))
                        .clicked()
                    {
                        self.zoom_at(1.0 / 1.2, None, egui::Rect::NOTHING);
                    }
                    let zoom_percent = if self.fit_zoom > 0.0 {
                        self.zoom / self.fit_zoom * 100.0
                    } else {
                        100.0
                    };
                    ui.label(format!("{zoom_percent:.0}%"));
                    if ui
                        .add_enabled(self.document.is_some(), egui::Button::new("+"))
                        .clicked()
                    {
                        self.zoom_at(1.2, None, egui::Rect::NOTHING);
                    }
                    if ui
                        .add_enabled(self.document.is_some(), egui::Button::new("适合窗口"))
                        .clicked()
                    {
                        self.fit_requested = true;
                    }
                    ui.separator();
                    if self.loading || self.exporting || self.rebuilding {
                        ui.spinner();
                    }
                    ui.label(
                        egui::RichText::new(&self.status)
                            .small()
                            .color(egui::Color32::from_gray(180)),
                    );
                });
            });

        if !self.warnings.is_empty() {
            egui::Panel::bottom("warnings").show(root_ui, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(64.0)
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(&self.warnings)
                                .small()
                                .color(egui::Color32::from_rgb(206, 172, 108)),
                        );
                    });
            });
        }

        let mut clicked_sheet = None;
        if let Some(document) = &self.document
            && !document.sheets.is_empty()
        {
            let sheets = &document.sheets;
            let active = self.active_sheet;
            egui::Panel::left("sheets")
                .default_size(168.0)
                .show(root_ui, |ui| {
                    ui.add_space(6.0);
                    ui.heading(format!("图纸 ({})", sheets.len()));
                    ui.separator();
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for (index, sheet) in sheets.iter().enumerate() {
                            let label = format!(
                                "{:>2}. {:.0} x {:.0}",
                                sheet.index,
                                sheet.bounds.width(),
                                sheet.bounds.height()
                            );
                            if ui.selectable_label(index == active, label).clicked() {
                                clicked_sheet = Some(index);
                            }
                        }
                    });
                });
        }
        // Clicking the sheet already on screen must not start a rebuild.
        if let Some(index) = clicked_sheet
            && index != self.active_sheet
        {
            self.active_sheet = index;
            self.needs_rebuild = true;
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(egui::Color32::from_rgb(29, 33, 40)))
            .show(root_ui, |ui| {
                let canvas = ui.max_rect();
                let response = ui.allocate_rect(canvas, egui::Sense::click_and_drag());
                if self.view_size != canvas.size() {
                    self.view_size = canvas.size();
                    if self.document.is_some() {
                        self.fit_requested = true;
                    }
                }
                if self.fit_requested {
                    self.fit_to_view();
                }

                if response.dragged() && self.document.is_some() {
                    let delta = context.input(|input| input.pointer.delta());
                    self.center -= delta / self.zoom;
                    self.render_dirty = true;
                    context.set_cursor_icon(egui::CursorIcon::Grabbing);
                } else if response.hovered() && self.document.is_some() {
                    context.set_cursor_icon(egui::CursorIcon::Grab);
                }

                if response.hovered() {
                    let scroll = context.input(|input| input.smooth_scroll_delta.y);
                    if scroll.abs() > 0.01 {
                        let factor = (scroll * 0.0025).exp();
                        self.zoom_at(factor, response.hover_pos(), canvas);
                    }
                }
                if response.double_clicked() {
                    self.fit_requested = true;
                }

                if self.render_dirty {
                    let dragging = response.dragged();
                    if !dragging || self.last_render.elapsed() >= Duration::from_millis(28) {
                        self.render_viewport(&context, canvas);
                    } else {
                        context.request_repaint_after(Duration::from_millis(16));
                    }
                }

                if let Some(texture) = &self.texture {
                    ui.painter().image(
                        texture.id(),
                        canvas,
                        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                } else if !self.loading {
                    ui.painter().text(
                        canvas.center(),
                        egui::Align2::CENTER_CENTER,
                        "将 DWG 文件拖到这里",
                        egui::FontId::proportional(20.0),
                        egui::Color32::from_gray(145),
                    );
                    ui.painter().text(
                        canvas.center() + egui::vec2(0.0, 32.0),
                        egui::Align2::CENTER_CENTER,
                        "滚轮缩放 · 拖动平移 · 双击适合窗口",
                        egui::FontId::proportional(13.0),
                        egui::Color32::from_gray(100),
                    );
                }
            });
    }
}

fn load_document(path: &Path, mode: ColorMode) -> Result<LoadedDocument, String> {
    let loaded = converter::load(path).map_err(|error| error.to_string())?;
    let doc = loaded.doc;
    let sheets = cadviewer::sheets::detect(&doc);
    let request = sheet_request(&doc, &sheets, 0, mode)
        .ok_or_else(|| "图纸中没有可打印的二维实体".to_owned())?;
    let (scene, report) = build(&doc, &request);
    Ok(LoadedDocument {
        source: path.to_owned(),
        doc: Arc::new(doc),
        sheets,
        scene: Arc::new(scene),
        item_count: report.items,
        load_warnings: loaded.warnings,
        skipped: skipped_summary(&report),
    })
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("图纸")
        .to_owned()
}

fn install_windows_font(context: &egui::Context) {
    let candidates = [
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\msyh.ttf",
        r"C:\Windows\Fonts\simhei.ttf",
    ];
    let Some(bytes) = candidates.iter().find_map(|path| std::fs::read(path).ok()) else {
        return;
    };
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "windows-cjk".to_owned(),
        Arc::new(egui::FontData::from_owned(bytes)),
    );
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, "windows-cjk".to_owned());
    }
    context.set_fonts(fonts);
}
