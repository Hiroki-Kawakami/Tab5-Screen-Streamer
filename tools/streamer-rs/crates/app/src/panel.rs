//! The window the tray icon opens: created on opening, destroyed on closing.

use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui_glow::EguiGlow;
use egui_glow::egui_winit::winit;
use egui_glow::glow;
use glutin::config::ConfigTemplateBuilder;
use glutin::context::{ContextAttributesBuilder, NotCurrentGlContext, PossiblyCurrentContext};
use glutin::display::{GetGlDisplay, GlDisplay};
use glutin::surface::{GlSurface, Surface, SurfaceAttributesBuilder, WindowSurface};
use winit::dpi::{LogicalSize, Position};
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, NamedKey};
use winit::raw_window_handle::HasWindowHandle;
use winit::window::{Window, WindowAttributes, WindowId, WindowLevel};

use crate::settings::Source;
use crate::streaming::Phase;

const WIDTH: f64 = 300.0;
/// Until the first layout gives the height.
const INITIAL_HEIGHT: f64 = 240.0;
const MARGIN: f32 = 12.0;
/// The menu bar's own panels; Windows rounds and outlines the window itself.
#[cfg(target_os = "macos")]
const CORNER_RADIUS: f32 = 12.0;
const DOT_RADIUS: f32 = 4.0;
const ALL: &str = "All Devices";
const MIXED: &str = "Mixed";

pub struct DeviceItem {
    pub serial: String,
    pub phase: Phase,
}

pub struct DisplayItem {
    /// What `Source::Display` keeps for it.
    pub saved: u64,
    pub label: String,
}

/// What the panel shows of the selected Tab5s.
pub struct View {
    pub devices: Vec<DeviceItem>,
    /// None for all of them.
    pub selected: Option<String>,
    /// The phase of all of them together.
    pub all: Phase,
    /// On only when on for every selected Tab5; the same for `touch`.
    pub streaming: bool,
    /// None when the selected Tab5s differ; the same for `rotation`. The main
    /// display is given as `Source::Display` when it is among `displays`.
    pub source: Option<Source>,
    pub rotation: Option<Option<u32>>,
    pub touch: bool,
    pub touch_enabled: bool,
    /// None while no selected Tab5 is connected.
    pub brightness: Option<u8>,
    pub brightness_mixed: bool,
    pub message: Option<String>,
    /// Without the selected Tab5s' own virtual displays.
    pub displays: Vec<DisplayItem>,
}

/// None in `Select` is all the Tab5s.
pub enum Action {
    Select(Option<String>),
    Streaming(bool),
    Source(Source),
    Rotation(Option<u32>),
    Touch(bool),
    Brightness(u8),
    /// The Display choices are being opened.
    Displays,
    Quit,
}

pub struct Panel {
    window: Window,
    context: PossiblyCurrentContext,
    surface: Surface<WindowSurface>,
    egui: EguiGlow,
    height: f64,
    /// The slider's value while it is dragged, ahead of the device.
    brightness: Option<u8>,
    repaint_at: Option<Instant>,
    placed: bool,
    shown: bool,
    /// macOS sends Focused(false) for every new window, before it is shown.
    focused: bool,
}

impl Panel {
    /// Hidden until the first redraw, which the caller makes directly: Windows
    /// sends no redraw to a hidden window.
    pub fn open(event_loop: &ActiveEventLoop) -> Result<Self, String> {
        let attributes = WindowAttributes::default()
            .with_title("Tab5 Streamer")
            .with_inner_size(LogicalSize::new(WIDTH, INITIAL_HEIGHT))
            .with_decorations(false)
            .with_resizable(false)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_visible(false);
        #[cfg(windows)]
        let attributes = {
            use winit::platform::windows::{CornerPreference, WindowAttributesExtWindows};
            attributes
                .with_skip_taskbar(true)
                .with_undecorated_shadow(true)
                .with_corner_preference(CornerPreference::Round)
        };
        let transparent = cfg!(target_os = "macos");
        let attributes = attributes.with_transparent(transparent);
        let template = ConfigTemplateBuilder::new()
            .with_depth_size(0)
            .with_stencil_size(0)
            .with_transparency(transparent);
        let (window, config) = glutin_winit::DisplayBuilder::new()
            .with_window_attributes(Some(attributes.clone()))
            .build(event_loop, template, |mut configs| configs.next().unwrap())
            .map_err(|e| format!("GL config: {e}"))?;
        let display = config.display();
        let window = match window {
            Some(window) => window,
            None => glutin_winit::finalize_window(event_loop, attributes, &config)
                .map_err(|e| format!("window: {e}"))?,
        };
        let handle = window
            .window_handle()
            .map_err(|e| format!("window handle: {e}"))?
            .as_raw();
        let context = unsafe {
            display.create_context(&config, &ContextAttributesBuilder::new().build(Some(handle)))
        }
        .map_err(|e| format!("GL context: {e}"))?;
        let (width, height): (u32, u32) = window.inner_size().into();
        let surface_attributes = SurfaceAttributesBuilder::<WindowSurface>::new().build(
            handle,
            NonZeroU32::new(width).unwrap_or(NonZeroU32::MIN),
            NonZeroU32::new(height).unwrap_or(NonZeroU32::MIN),
        );
        let surface = unsafe { display.create_window_surface(&config, &surface_attributes) }
            .map_err(|e| format!("GL surface: {e}"))?;
        let context = context
            .make_current(&surface)
            .map_err(|e| format!("GL context: {e}"))?;
        let gl = unsafe {
            glow::Context::from_loader_function_cstr(|name| display.get_proc_address(name))
        };
        let egui = EguiGlow::new(
            event_loop,
            Arc::new(gl),
            None,
            Some(window.scale_factor() as f32),
            true,
        );
        Ok(Panel {
            window,
            context,
            surface,
            egui,
            height: INITIAL_HEIGHT,
            brightness: None,
            repaint_at: None,
            placed: false,
            shown: false,
            focused: false,
        })
    }

    pub fn id(&self) -> WindowId {
        self.window.id()
    }

    pub fn egui(&self) -> &egui::Context {
        &self.egui.egui_ctx
    }

    pub fn request_redraw(&self) {
        self.window.request_redraw();
    }

    pub fn repaint_after(&mut self, delay: Duration) {
        match delay.is_zero() {
            true => self.window.request_redraw(),
            false => self.repaint_at = Instant::now().checked_add(delay),
        }
    }

    /// When the next redraw is due, after it has been requested if it is due now.
    pub fn repaint_at(&mut self) -> Option<Instant> {
        if self.repaint_at.is_some_and(|at| at <= Instant::now()) {
            self.repaint_at = None;
            self.window.request_redraw();
        }
        self.repaint_at
    }

    /// True when the panel is to close.
    pub fn window_event(&mut self, event: &WindowEvent) -> bool {
        match event {
            WindowEvent::CloseRequested => return true,
            WindowEvent::Focused(true) => self.focused = true,
            WindowEvent::Focused(false) if self.focused => return true,
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed
                    && event.logical_key == Key::Named(NamedKey::Escape) =>
            {
                return true;
            }
            WindowEvent::Resized(size) => {
                if let (Some(width), Some(height)) =
                    (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
                {
                    self.surface.resize(&self.context, width, height);
                }
            }
            _ => {}
        }
        if self.egui.on_window_event(&self.window, event).repaint {
            self.window.request_redraw();
        }
        false
    }

    /// Lays the panel out and paints it. It is sized to the contents and
    /// placed with `place` before it is first shown, and again whenever the
    /// contents change height.
    pub fn redraw(
        &mut self,
        view: &View,
        place: impl Fn(LogicalSize<f64>) -> Option<Position>,
    ) -> Vec<Action> {
        let mut actions = Vec::new();
        let mut height = 0.0;
        let brightness = &mut self.brightness;
        self.egui.run(&self.window, |ui| {
            let frame = egui::Frame::central_panel(ui.style()).inner_margin(MARGIN);
            #[cfg(target_os = "macos")]
            let frame = frame
                .corner_radius(CORNER_RADIUS)
                .stroke(ui.visuals().window_stroke);
            let response = frame.show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                contents(ui, view, brightness, &mut actions);
            });
            height = response.response.rect.height() as f64;
        });
        let size = LogicalSize::new(WIDTH, height.ceil());
        if (size.height - self.height).abs() >= 1.0 || !self.placed {
            self.height = size.height;
            self.placed = true;
            let _ = self.window.request_inner_size(size);
            if let Some(position) = place(size) {
                self.window.set_outer_position(position);
            }
        }
        let clear = match cfg!(target_os = "macos") {
            true => egui::Color32::TRANSPARENT,
            false => self.egui.egui_ctx.global_style().visuals.panel_fill,
        };
        let size = self.window.inner_size().into();
        self.egui.painter.clear(size, clear.to_normalized_gamma_f32());
        self.egui.paint(&self.window);
        if let Err(e) = self.surface.swap_buffers(&self.context) {
            eprintln!("panel: {e}");
        }
        #[cfg(target_os = "macos")]
        invalidate_shadow(&self.window);
        if !self.shown {
            self.shown = true;
            self.window.set_visible(true);
            self.window.focus_window();
        }
        actions
    }
}

/// AppKit shapes the shadow of a transparent window from what it showed last,
/// and does not follow GL drawing.
#[cfg(target_os = "macos")]
fn invalidate_shadow(window: &Window) {
    use winit::raw_window_handle::RawWindowHandle;
    let Ok(handle) = window.window_handle() else {
        return;
    };
    if let RawWindowHandle::AppKit(handle) = handle.as_raw() {
        let view = unsafe { handle.ns_view.cast::<objc2_app_kit::NSView>().as_ref() };
        if let Some(window) = view.window() {
            window.invalidateShadow();
        }
    }
}

impl Drop for Panel {
    fn drop(&mut self) {
        self.egui.destroy();
    }
}

fn phase_color(phase: Phase) -> egui::Color32 {
    match phase {
        Phase::Off => egui::Color32::GRAY,
        Phase::Waiting => egui::Color32::from_rgb(0xe0, 0xa0, 0x30),
        Phase::Streaming => egui::Color32::from_rgb(0x40, 0xc0, 0x60),
    }
}

pub fn name(serial: &str) -> String {
    let skip = serial.chars().count().saturating_sub(4);
    let last: String = serial.chars().skip(skip).collect();
    format!("Tab5 #{}", last.to_uppercase())
}

fn rotation_text(rotation: Option<u32>) -> &'static str {
    match rotation {
        None => "Auto",
        Some(0) => "0°",
        Some(1) => "90°",
        Some(2) => "180°",
        Some(_) => "270°",
    }
}

fn source_text(source: Source, displays: &[DisplayItem]) -> String {
    match source {
        Source::Main => "Main display".into(),
        Source::Virtual => "Virtual display".into(),
        Source::Display(saved) => displays
            .iter()
            .find(|display| display.saved == saved)
            .map_or("Missing display".into(), |display| display.label.clone()),
    }
}

fn sources(displays: &[DisplayItem]) -> Vec<Source> {
    let mut sources: Vec<Source> = displays
        .iter()
        .map(|display| Source::Display(display.saved))
        .collect();
    if sources.is_empty() {
        sources.push(Source::Main);
    }
    if cfg!(target_os = "macos") {
        sources.push(Source::Virtual);
    }
    sources
}

fn dot(ui: &mut egui::Ui, phase: Phase) {
    let size = egui::vec2(DOT_RADIUS * 2.0, ui.spacing().interact_size.y);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter()
        .circle_filled(rect.center(), DOT_RADIUS, phase_color(phase));
}

fn toggle(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let size = ui.spacing().interact_size.y * egui::vec2(1.8, 1.0);
    let (rect, mut response) = ui.allocate_exact_size(size, egui::Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    if ui.is_rect_visible(rect) {
        let t = ui.ctx().animate_bool_responsive(response.id, *on);
        let visuals = ui.style().interact_selectable(&response, *on);
        let radius = rect.height() / 2.0;
        ui.painter().rect(
            rect,
            radius,
            visuals.bg_fill,
            visuals.bg_stroke,
            egui::StrokeKind::Inside,
        );
        let x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), t);
        let center = egui::pos2(x, rect.center().y);
        ui.painter().circle(center, 0.75 * radius, visuals.bg_fill, visuals.fg_stroke);
    }
    response
}

/// A ComboBox showing "Mixed" while `current` is None.
fn choice<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    id: &str,
    current: Option<T>,
    choices: &[T],
    text: impl Fn(T) -> String,
) -> (Option<T>, egui::Response) {
    let mut chosen = None;
    let max_width = ui.available_width();
    let response = egui::ComboBox::from_id_salt(id)
        .selected_text(current.map_or(MIXED.into(), &text))
        .truncate()
        .show_ui(ui, |ui| {
            ui.set_max_width(max_width);
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
            for &value in choices {
                if ui
                    .selectable_label(current == Some(value), text(value))
                    .clicked()
                {
                    chosen = Some(value);
                }
            }
        })
        .response;
    (chosen.filter(|value| current != Some(*value)), response)
}

fn selector(ui: &mut egui::Ui, view: &View, actions: &mut Vec<Action>) {
    let selected = match &view.selected {
        Some(serial) => name(serial),
        None => ALL.into(),
    };
    egui::ComboBox::from_id_salt("device")
        .selected_text(selected)
        .show_ui(ui, |ui| {
            let items = std::iter::once((None, view.all))
                .chain(view.devices.iter().map(|d| (Some(&d.serial), d.phase)));
            for (serial, phase) in items {
                ui.horizontal(|ui| {
                    dot(ui, phase);
                    let text = serial.map_or(ALL.into(), |serial| name(serial));
                    let current = view.selected.as_ref() == serial;
                    if ui.selectable_label(current, text).clicked() && !current {
                        actions.push(Action::Select(serial.cloned()));
                    }
                });
            }
        });
    let phase = match &view.selected {
        Some(serial) => view.devices.iter().find(|d| &d.serial == serial).map(|d| d.phase),
        None => Some(view.all),
    };
    dot(ui, phase.unwrap_or(Phase::Off));
}

fn header(ui: &mut egui::Ui, view: &View, actions: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        ui.strong("Tab5 Streamer");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            match view.devices.as_slice() {
                [] => {
                    ui.colored_label(phase_color(Phase::Waiting), "Waiting for the Tab5");
                }
                [device] => {
                    ui.label(name(&device.serial));
                    dot(ui, device.phase);
                }
                _ => selector(ui, view, actions),
            }
        });
    });
}

fn contents(
    ui: &mut egui::Ui,
    view: &View,
    brightness: &mut Option<u8>,
    actions: &mut Vec<Action>,
) {
    header(ui, view, actions);
    ui.separator();

    ui.add_enabled_ui(!view.devices.is_empty(), |ui| {
        settings(ui, view, brightness, actions);
    });

    if let Some(message) = &view.message {
        ui.add_space(4.0);
        ui.label(egui::RichText::new(message).small().weak());
    }
    ui.separator();
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Quit").clicked() {
                actions.push(Action::Quit);
            }
        });
    });
}

fn settings(
    ui: &mut egui::Ui,
    view: &View,
    brightness: &mut Option<u8>,
    actions: &mut Vec<Action>,
) {
    egui::Grid::new("settings")
        .num_columns(2)
        .spacing([16.0, 10.0])
        .show(ui, |ui| {
            ui.label("Streaming");
            let mut streaming = view.streaming;
            if toggle(ui, &mut streaming).changed() {
                actions.push(Action::Streaming(streaming));
            }
            ui.end_row();

            ui.label("Display");
            let sources = sources(&view.displays);
            let text = |source| source_text(source, &view.displays);
            let (source, response) = choice(ui, "source", view.source, &sources, text);
            if response.clicked() {
                actions.push(Action::Displays);
            }
            if let Some(source) = source {
                actions.push(Action::Source(source));
            }
            ui.end_row();

            ui.label("Rotation");
            let rotations = [None, Some(0), Some(1), Some(2), Some(3)];
            let text = |rotation| rotation_text(rotation).to_string();
            if let (Some(rotation), _) = choice(ui, "rotation", view.rotation, &rotations, text) {
                actions.push(Action::Rotation(rotation));
            }
            ui.end_row();

            ui.label("Touch input");
            ui.add_enabled_ui(view.touch_enabled, |ui| {
                let mut touch = view.touch;
                let response = toggle(ui, &mut touch)
                    .on_disabled_hover_text("Turn Touch Input on in the Tab5's options");
                if response.changed() {
                    actions.push(Action::Touch(touch));
                }
            });
            ui.end_row();

            ui.label("Brightness");
            ui.add_enabled_ui(view.brightness.is_some(), |ui| {
                let widest = egui::WidgetText::from(MIXED).into_galley(
                    ui,
                    None,
                    f32::INFINITY,
                    egui::TextStyle::Button,
                );
                let padding = ui.spacing().button_padding.x * 2.0;
                let available = ui.available_width();
                let spacing = ui.spacing_mut();
                spacing.interact_size.x = spacing.interact_size.x.max(widest.size().x + padding);
                spacing.slider_width = available - spacing.interact_size.x - spacing.item_spacing.x;
                let mut value = brightness.or(view.brightness).unwrap_or(0);
                let slider = egui::Slider::new(&mut value, 0..=100);
                let slider = match view.brightness_mixed && brightness.is_none() {
                    true => slider.custom_formatter(|_, _| MIXED.into()),
                    false => slider.suffix("%"),
                };
                let response = ui.add(slider);
                if response.changed() {
                    actions.push(Action::Brightness(value));
                }
                *brightness = match response.dragged() {
                    true => Some(value),
                    false => None,
                };
            });
            ui.end_row();
        });
}
