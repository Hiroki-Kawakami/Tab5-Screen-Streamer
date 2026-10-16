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

use crate::settings::{Settings, Source};
use crate::streaming::{Phase, Status};

const WIDTH: f64 = 300.0;
/// Until the first layout gives the height.
const INITIAL_HEIGHT: f64 = 240.0;
const MARGIN: f32 = 12.0;
/// The menu bar's own panels; Windows rounds and outlines the window itself.
#[cfg(target_os = "macos")]
const CORNER_RADIUS: f32 = 12.0;

pub enum Action {
    Brightness(u8),
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
        settings: &mut Settings,
        status: &Status,
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
                contents(ui, settings, status, brightness, &mut actions);
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

fn phase_text(phase: Phase) -> (&'static str, egui::Color32) {
    match phase {
        Phase::Off => ("Off", egui::Color32::GRAY),
        Phase::Waiting => ("Waiting for the Tab5", egui::Color32::from_rgb(0xe0, 0xa0, 0x30)),
        Phase::Streaming => ("Streaming", egui::Color32::from_rgb(0x40, 0xc0, 0x60)),
        Phase::Asleep => ("Display asleep", egui::Color32::from_rgb(0x60, 0x90, 0xe0)),
    }
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

fn source_text(source: Source) -> &'static str {
    match source {
        Source::Main => "Main display",
        Source::Virtual => "Virtual display",
    }
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

fn contents(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    status: &Status,
    brightness: &mut Option<u8>,
    actions: &mut Vec<Action>,
) {
    let phase = match settings.streaming {
        true => status.phase,
        false => Phase::Off,
    };
    ui.horizontal(|ui| {
        ui.strong("Tab5 Streamer");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (text, color) = phase_text(phase);
            ui.colored_label(color, text);
        });
    });
    ui.separator();

    let connected = matches!(status.phase, Phase::Streaming | Phase::Asleep);
    egui::Grid::new("settings")
        .num_columns(2)
        .spacing([16.0, 10.0])
        .min_col_width(110.0)
        .show(ui, |ui| {
            ui.label("Streaming");
            toggle(ui, &mut settings.streaming);
            ui.end_row();

            if cfg!(target_os = "macos") {
                ui.label("Display");
                egui::ComboBox::from_id_salt("source")
                    .selected_text(source_text(settings.source))
                    .show_ui(ui, |ui| {
                        for source in [Source::Main, Source::Virtual] {
                            ui.selectable_value(&mut settings.source, source, source_text(source));
                        }
                    });
                ui.end_row();
            }

            ui.label("Rotation");
            egui::ComboBox::from_id_salt("rotation")
                .selected_text(rotation_text(settings.rotation))
                .show_ui(ui, |ui| {
                    for rotation in [None, Some(0), Some(1), Some(2), Some(3)] {
                        let text = rotation_text(rotation);
                        ui.selectable_value(&mut settings.rotation, rotation, text);
                    }
                });
            ui.end_row();

            ui.label("Touch input");
            ui.add_enabled_ui(connected && status.touch, |ui| {
                toggle(ui, &mut settings.touch)
                    .on_disabled_hover_text("Turn Touch Input on in the Tab5's options");
            });
            ui.end_row();

            ui.label("Brightness");
            ui.add_enabled_ui(status.brightness.is_some(), |ui| {
                let widest = egui::WidgetText::from("100%").into_galley(
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
                let mut value = brightness.or(status.brightness).unwrap_or(0);
                let slider = egui::Slider::new(&mut value, 0..=100).suffix("%");
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

    if let Some(message) = &status.message {
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
