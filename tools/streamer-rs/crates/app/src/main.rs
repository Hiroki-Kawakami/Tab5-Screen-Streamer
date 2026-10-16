#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod panel;
mod settings;
mod streaming;
mod tray;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use egui_glow::egui_winit::winit;
use streamer_stream::Request;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::WindowId;

use panel::{Action, Panel};
use settings::Settings;
use streaming::{Phase, Status, Streaming};
use tray::{Icon, Tray};

/// A click on the icon this soon after the panel has lost the focus to it
/// only closes the panel.
const REOPEN_GUARD: Duration = Duration::from_millis(300);

enum UserEvent {
    TrayClicked,
    StatusChanged,
    Repaint(Duration),
}

struct App {
    proxy: EventLoopProxy<UserEvent>,
    settings: Settings,
    streaming: Option<Streaming>,
    /// What the last stream ended with.
    ended: Status,
    tray: Option<Tray>,
    icon: Icon,
    panel: Option<Panel>,
    closed_at: Option<Instant>,
}

impl App {
    fn status(&self) -> Status {
        match &self.streaming {
            Some(streaming) => streaming.status(),
            None => self.ended.clone(),
        }
    }

    fn save(&self) {
        if let Err(e) = self.settings.save() {
            eprintln!("settings: {e}");
        }
    }

    fn start(&mut self) {
        let proxy = Mutex::new(self.proxy.clone());
        let changed = Arc::new(move || {
            let _ = proxy.lock().unwrap().send_event(UserEvent::StatusChanged);
        });
        self.streaming = Some(Streaming::start(&self.settings, changed));
        self.ended = Status::OFF;
    }

    /// Follows the settings the panel has changed from `before`.
    fn apply(&mut self, before: Settings) {
        let settings = self.settings;
        if settings == before {
            return;
        }
        let restart = settings.source != before.source || settings.rotation != before.rotation;
        match &self.streaming {
            _ if !settings.streaming => self.streaming = None,
            Some(_) if restart => {
                self.streaming = None;
                self.start();
            }
            Some(streaming) => {
                if settings.touch != before.touch {
                    streaming.request(Request::Touch(settings.touch));
                }
            }
            None => self.start(),
        }
        self.save();
        self.status_changed();
    }

    fn status_changed(&mut self) {
        if let Some(streaming) = &self.streaming {
            let status = streaming.status();
            if status.phase == Phase::Off {
                self.ended = status;
                self.streaming = None;
                self.settings.streaming = false;
                self.save();
            }
        }
        let icon = match self.status().phase {
            Phase::Off => Icon::Disabled,
            Phase::Waiting => Icon::Disconnected,
            Phase::Asleep => Icon::Idle,
            Phase::Streaming => Icon::Streaming,
        };
        if icon != self.icon {
            self.icon = icon;
            if let Some(tray) = &mut self.tray {
                tray.set_icon(icon);
            }
        }
        if let Some(panel) = &self.panel {
            panel.request_redraw();
        }
    }

    fn toggle_panel(&mut self, event_loop: &ActiveEventLoop) {
        if self.panel.take().is_some()
            || self.closed_at.is_some_and(|at| at.elapsed() < REOPEN_GUARD)
        {
            return;
        }
        match Panel::open(event_loop) {
            Ok(panel) => {
                let proxy = Mutex::new(self.proxy.clone());
                panel.egui().set_request_repaint_callback(move |info| {
                    let _ = proxy.lock().unwrap().send_event(UserEvent::Repaint(info.delay));
                });
                self.panel = Some(panel);
                self.redraw(event_loop);
            }
            Err(e) => eprintln!("panel: {e}"),
        }
    }

    fn close_panel(&mut self) {
        self.panel = None;
        self.closed_at = Some(Instant::now());
    }

    fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        let status = self.status();
        let before = self.settings;
        let Some(panel) = &mut self.panel else {
            return;
        };
        let tray = &self.tray;
        let actions = panel.redraw(&mut self.settings, &status, |size| {
            tray.as_ref()?.place(size)
        });
        self.apply(before);
        for action in actions {
            match action {
                Action::Brightness(brightness) => {
                    if let Some(streaming) = &self.streaming {
                        streaming.request(Request::Brightness(brightness));
                    }
                }
                Action::Quit => {
                    self.panel = None;
                    if let Some(streaming) = self.streaming.take() {
                        streaming.join();
                    }
                    event_loop.exit();
                    return;
                }
            }
        }
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.tray.is_some() {
            return;
        }
        let proxy = self.proxy.clone();
        let clicked = Box::new(move || {
            let _ = proxy.send_event(UserEvent::TrayClicked);
        });
        match Tray::new(self.icon, clicked) {
            Ok(tray) => self.tray = Some(tray),
            Err(e) => {
                eprintln!("tray: {e}");
                event_loop.exit();
                return;
            }
        }
        if self.settings.streaming {
            self.start();
            self.status_changed();
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::TrayClicked => self.toggle_panel(event_loop),
            UserEvent::StatusChanged => self.status_changed(),
            UserEvent::Repaint(delay) => {
                if let Some(panel) = &mut self.panel {
                    panel.repaint_after(delay);
                }
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(panel) = &mut self.panel else {
            return;
        };
        if panel.id() != id {
            return;
        }
        if matches!(event, WindowEvent::RedrawRequested) {
            self.redraw(event_loop);
        } else if panel.window_event(&event) {
            self.close_panel();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(match self.panel.as_mut().and_then(Panel::repaint_at) {
            Some(at) => ControlFlow::WaitUntil(at),
            None => ControlFlow::Wait,
        });
    }
}

fn main() {
    if tray::hand_over() {
        return;
    }
    let mut builder = EventLoop::<UserEvent>::with_user_event();
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
        builder
            .with_activation_policy(ActivationPolicy::Accessory)
            .with_default_menu(false);
    }
    let event_loop = match builder.build() {
        Ok(event_loop) => event_loop,
        Err(e) => {
            eprintln!("event loop: {e}");
            std::process::exit(1);
        }
    };
    let mut app = App {
        proxy: event_loop.create_proxy(),
        settings: Settings::load(),
        streaming: None,
        ended: Status::OFF,
        tray: None,
        icon: Icon::Disabled,
        panel: None,
        closed_at: None,
    };
    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("event loop: {e}");
    }
}
