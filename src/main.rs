#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use cadviewer::{converter, fonts, pdf};
use eframe::egui;
use resvg::tiny_skia;
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
    let converted = converter::convert_to_svg(input)?;
    pdf::svg_to_pdf(&converted.svg, output)
}

fn show_error(message: &str) {
    let _ = rfd::MessageDialog::new()
        .set_title("Cadviewer")
        .set_description(message)
        .set_level(rfd::MessageLevel::Error)
        .show();
}

struct LoadedDocument {
    source: PathBuf,
    svg: Arc<String>,
    tree: Arc<resvg::usvg::Tree>,
    entity_count: usize,
    warnings: String,
}

enum AppMessage {
    Loaded(Result<LoadedDocument, String>),
    Exported {
        path: PathBuf,
        result: Result<(), String>,
    },
}

struct CadviewerApp {
    sender: Sender<AppMessage>,
    receiver: Receiver<AppMessage>,
    document: Option<LoadedDocument>,
    texture: Option<egui::TextureHandle>,
    center: egui::Pos2,
    zoom: f32,
    fit_zoom: f32,
    view_size: egui::Vec2,
    render_dirty: bool,
    last_render: Instant,
    loading: bool,
    exporting: bool,
    fit_requested: bool,
    status: String,
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
            status: "拖入 DWG 文件，或点击“打开”".to_owned(),
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
        let sender = self.sender.clone();
        let context = context.clone();
        std::thread::spawn(move || {
            let result = load_document(&path);
            let _ = sender.send(AppMessage::Loaded(result));
            context.request_repaint();
        });
    }

    fn begin_export(&mut self, path: PathBuf, context: &egui::Context) {
        let Some(document) = &self.document else {
            return;
        };
        self.exporting = true;
        self.status = format!("正在生成 {}…", file_name(&path));
        let svg = Arc::clone(&document.svg);
        let sender = self.sender.clone();
        let context = context.clone();
        std::thread::spawn(move || {
            let result = pdf::svg_to_pdf(&svg, &path);
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
                            self.status = format!(
                                "{} · {} 个二维图元{}",
                                file_name(&document.source),
                                document.entity_count,
                                if document.warnings.is_empty() {
                                    ""
                                } else {
                                    " · 部分高级对象已跳过"
                                }
                            );
                            self.document = Some(document);
                            self.texture = None;
                            self.fit_requested = true;
                            self.render_dirty = true;
                        }
                        Err(error) => {
                            self.status = error.clone();
                            show_error(&error);
                        }
                    }
                }
                AppMessage::Exported { path, result } => {
                    self.exporting = false;
                    match result {
                        Ok(()) => {
                            self.status = format!("已导出 {}", path.display());
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

    fn open_dialog(&mut self, context: &egui::Context) {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("打开 DWG")
            .add_filter("CAD 图纸", &["dwg", "dxf"])
            .pick_file()
        {
            self.begin_load(path, context);
        }
    }

    fn export_dialog(&mut self, context: &egui::Context) {
        let Some(document) = &self.document else {
            return;
        };
        let default_name = document
            .source
            .file_stem()
            .and_then(|name| name.to_str())
            .map(|name| format!("{name}.pdf"))
            .unwrap_or_else(|| "drawing.pdf".to_owned());
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
            self.begin_export(path, context);
        }
    }

    fn fit_to_view(&mut self) {
        let Some(document) = &self.document else {
            return;
        };
        if self.view_size.x <= 1.0 || self.view_size.y <= 1.0 {
            return;
        }
        let size = document.tree.size();
        let width = size.width().max(1.0);
        let height = size.height().max(1.0);
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
        let Some(mut pixmap) = tiny_skia::Pixmap::new(width, height) else {
            self.status = "无法分配渲染缓冲区".to_owned();
            return;
        };
        pixmap.fill(tiny_skia::Color::WHITE);

        let scale = self.zoom * render_scale;
        let tx = width as f32 * 0.5 - self.center.x * scale;
        let ty = height as f32 * 0.5 - self.center.y * scale;
        let transform = tiny_skia::Transform::from_row(scale, 0.0, 0.0, scale, tx, ty);
        resvg::render(&document.tree, transform, &mut pixmap.as_mut());

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
                    if ui
                        .add_enabled(
                            self.document.is_some() && !self.exporting,
                            egui::Button::new("导出 PDF"),
                        )
                        .clicked()
                    {
                        self.export_dialog(&context);
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
                    if self.loading || self.exporting {
                        ui.spinner();
                    }
                    ui.label(
                        egui::RichText::new(&self.status)
                            .small()
                            .color(egui::Color32::from_gray(180)),
                    );
                });
            });

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

fn load_document(path: &Path) -> Result<LoadedDocument, String> {
    let converted = converter::convert_to_svg(path)?;
    let mut options = resvg::usvg::Options::default();
    fonts::configure(&mut options, &converted.svg);
    let tree = resvg::usvg::Tree::from_str(&converted.svg, &options)
        .map_err(|error| format!("无法构建二维场景：{error}"))?;
    Ok(LoadedDocument {
        source: path.to_owned(),
        svg: Arc::new(converted.svg),
        tree: Arc::new(tree),
        entity_count: converted.entity_count,
        warnings: converted.warnings,
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
