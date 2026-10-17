#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod devices;
mod panel;
mod settings;
mod streaming;
mod tray;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use egui_glow::egui_winit::winit;
use streamer_capture::Display;
use streamer_stream::Request;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::WindowId;

use panel::{Action, DeviceItem, DisplayItem, Panel, View};
use settings::{DeviceSettings, Settings, Source};
use streaming::{Phase, Status, Streaming, saved_display};
use tray::{Icon, Tray};

/// A click on the icon this soon after the panel has lost the focus to it
/// only closes the panel.
const REOPEN_GUARD: Duration = Duration::from_millis(300);

enum UserEvent {
    TrayClicked,
    StatusChanged,
    DevicesChanged(Vec<String>),
    Repaint(Duration),
}

/// A Tab5 that is plugged in.
struct Entry {
    serial: String,
    streaming: Option<Streaming>,
    /// What the last stream ended with.
    ended: Status,
}

impl Entry {
    fn status(&self) -> Status {
        match &self.streaming {
            Some(streaming) => streaming.status(),
            None => self.ended.clone(),
        }
    }
}

struct App {
    proxy: EventLoopProxy<UserEvent>,
    settings: Settings,
    devices: Vec<Entry>,
    /// None for all of them.
    selected: Option<String>,
    /// With their names, as of when the panel or the Display choices opened.
    displays: Vec<(Display, Option<String>)>,
    main_display: Option<u64>,
    tray: Option<Tray>,
    icon: Icon,
    panel: Option<Panel>,
    closed_at: Option<Instant>,
}

fn combined(phases: impl Iterator<Item = Phase>) -> Phase {
    phases.fold(Phase::Off, |all, phase| match (all, phase) {
        (Phase::Streaming, _) | (_, Phase::Streaming) => Phase::Streaming,
        (Phase::Waiting, _) | (_, Phase::Waiting) => Phase::Waiting,
        _ => Phase::Off,
    })
}

/// The value they share, or None when they differ.
fn shared<T: PartialEq>(mut values: impl Iterator<Item = T>) -> Option<T> {
    let first = values.next()?;
    values.all(|value| value == first).then_some(first)
}

impl App {
    fn save(&self) {
        if let Err(e) = self.settings.save() {
            eprintln!("settings: {e}");
        }
    }

    /// Indexes into `devices` the panel's settings go to.
    fn targets(&self) -> Vec<usize> {
        match (&self.selected, self.devices.len()) {
            (_, 1) | (None, _) => (0..self.devices.len()).collect(),
            (Some(serial), _) => self
                .devices
                .iter()
                .position(|entry| &entry.serial == serial)
                .into_iter()
                .collect(),
        }
    }

    fn start(&mut self, index: usize) {
        let proxy = Mutex::new(self.proxy.clone());
        let changed = Arc::new(move || {
            let _ = proxy.lock().unwrap().send_event(UserEvent::StatusChanged);
        });
        let entry = &mut self.devices[index];
        let settings = self.settings.device(&entry.serial);
        entry.streaming = Some(Streaming::start(&entry.serial, &settings, changed));
        entry.ended = Status::OFF;
    }

    fn devices_changed(&mut self, serials: Vec<String>) {
        self.devices.retain(|entry| serials.contains(&entry.serial));
        for serial in serials {
            if self.devices.iter().any(|entry| entry.serial == serial) {
                continue;
            }
            self.devices.push(Entry {
                serial: serial.clone(),
                streaming: None,
                ended: Status::OFF,
            });
            if self.settings.device(&serial).streaming {
                self.start(self.devices.len() - 1);
            }
        }
        if let Some(serial) = &self.selected
            && !self.devices.iter().any(|entry| &entry.serial == serial)
        {
            self.selected = self.devices.first().map(|entry| entry.serial.clone());
        }
        self.status_changed();
    }

    /// Follows the settings of the Tab5 at `index` changed from `before`.
    fn apply(&mut self, index: usize, before: DeviceSettings) {
        let entry = &self.devices[index];
        let settings = self.settings.device(&entry.serial);
        if settings == before {
            return;
        }
        let restart = settings.source != before.source || settings.rotation != before.rotation;
        match &entry.streaming {
            _ if !settings.streaming => self.devices[index].streaming = None,
            Some(_) if restart => {
                self.devices[index].streaming = None;
                self.start(index);
            }
            Some(streaming) => {
                if settings.touch != before.touch {
                    streaming.request(Request::Touch(settings.touch));
                }
            }
            None => self.start(index),
        }
    }

    fn change(&mut self, change: impl Fn(&mut DeviceSettings, &Status)) {
        for index in self.targets() {
            let entry = &self.devices[index];
            let before = self.settings.device(&entry.serial);
            let mut settings = before;
            change(&mut settings, &entry.status());
            self.settings.set_device(&entry.serial.clone(), settings);
            self.apply(index, before);
        }
        self.save();
        self.status_changed();
    }

    fn status_changed(&mut self) {
        let mut ended = false;
        for entry in &mut self.devices {
            if let Some(streaming) = &entry.streaming {
                let status = streaming.status();
                if status.phase == Phase::Off {
                    entry.ended = status;
                    entry.streaming = None;
                    let mut settings = self.settings.device(&entry.serial);
                    settings.streaming = false;
                    self.settings.set_device(&entry.serial, settings);
                    ended = true;
                }
            }
        }
        if ended {
            self.save();
        }
        let icon = match combined(self.devices.iter().map(|entry| entry.status().phase)) {
            _ if self.devices.is_empty() => Icon::Disconnected,
            Phase::Off => Icon::Disabled,
            Phase::Waiting => Icon::Disconnected,
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

    fn view(&self) -> View {
        let statuses: Vec<Status> = self.devices.iter().map(Entry::status).collect();
        let targets = self.targets();
        let mut settings: Vec<DeviceSettings> = targets
            .iter()
            .map(|&index| self.settings.device(&self.devices[index].serial))
            .collect();
        if settings.is_empty() {
            settings.push(DeviceSettings::default());
        }
        let touchable: Vec<usize> = targets
            .iter()
            .copied()
            .filter(|&index| statuses[index].phase == Phase::Streaming && statuses[index].touch)
            .collect();
        let brightness: Vec<u8> = targets
            .iter()
            .filter_map(|&index| statuses[index].brightness)
            .collect();
        let own: Vec<u64> = targets
            .iter()
            .filter_map(|&index| statuses[index].virtual_display)
            .collect();
        let messages: Vec<String> = targets
            .iter()
            .filter_map(|&index| {
                let message = statuses[index].message.as_ref()?;
                Some(match targets.len() {
                    1 => message.clone(),
                    _ => format!("{}: {message}", panel::name(&self.devices[index].serial)),
                })
            })
            .collect();
        View {
            devices: self
                .devices
                .iter()
                .zip(&statuses)
                .map(|(entry, status)| DeviceItem {
                    serial: entry.serial.clone(),
                    phase: status.phase,
                })
                .collect(),
            selected: match self.devices.len() {
                1 => self.devices.first().map(|entry| entry.serial.clone()),
                _ => self.selected.clone(),
            },
            all: combined(statuses.iter().map(|status| status.phase)),
            streaming: settings.iter().all(|s| s.streaming),
            source: shared(settings.iter().map(|s| self.effective(s.source))),
            rotation: shared(settings.iter().map(|s| s.rotation)),
            touch: !touchable.is_empty()
                && touchable
                    .iter()
                    .all(|&index| self.settings.device(&self.devices[index].serial).touch),
            touch_enabled: !touchable.is_empty(),
            brightness: brightness.first().copied(),
            brightness_mixed: shared(brightness.iter()).is_none() && !brightness.is_empty(),
            message: (!messages.is_empty()).then(|| messages.join("\n")),
            displays: self.display_items(&own),
        }
    }

    fn refresh_displays(&mut self) {
        let displays = streamer_capture::displays().unwrap_or_default();
        self.displays = displays
            .into_iter()
            .map(|display| (display, streamer_capture::name(display.id)))
            .collect();
        self.main_display = streamer_capture::main_display().ok().map(|display| display.id);
    }

    /// Numbered by the whole list, so a display keeps its number in every
    /// Tab5's choices.
    fn display_items(&self, own: &[u64]) -> Vec<DisplayItem> {
        self.displays
            .iter()
            .enumerate()
            .filter(|(_, (display, _))| !own.contains(&display.id))
            .map(|(index, (display, name))| DisplayItem {
                saved: saved_display(index, display),
                label: format!("{}. {}", index + 1, name.as_deref().unwrap_or("Display")),
            })
            .collect()
    }

    /// `Source::Main` as the display it is now, which the choices list.
    fn effective(&self, source: Source) -> Source {
        let main = self.displays.iter().enumerate().find_map(|(index, (display, _))| {
            (Some(display.id) == self.main_display).then(|| saved_display(index, display))
        });
        match (source, main) {
            (Source::Main, Some(main)) => Source::Display(main),
            (source, _) => source,
        }
    }

    fn toggle_panel(&mut self, event_loop: &ActiveEventLoop) {
        if self.panel.take().is_some()
            || self.closed_at.is_some_and(|at| at.elapsed() < REOPEN_GUARD)
        {
            return;
        }
        self.refresh_displays();
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
        let view = self.view();
        let Some(panel) = &mut self.panel else {
            return;
        };
        let tray = &self.tray;
        let actions = panel.redraw(&view, |size| tray.as_ref()?.place(size));
        for action in actions {
            match action {
                Action::Select(serial) => {
                    self.selected = serial;
                    self.status_changed();
                }
                Action::Streaming(on) => self.change(|settings, _| settings.streaming = on),
                Action::Source(source) => self.change(|settings, _| settings.source = source),
                Action::Rotation(rotation) => {
                    self.change(|settings, _| settings.rotation = rotation)
                }
                Action::Touch(on) => self.change(|settings, status| {
                    if status.phase == Phase::Streaming && status.touch {
                        settings.touch = on;
                    }
                }),
                Action::Brightness(brightness) => {
                    for index in self.targets() {
                        if let Some(streaming) = &self.devices[index].streaming {
                            streaming.request(Request::Brightness(brightness));
                        }
                    }
                }
                Action::Displays => {
                    self.refresh_displays();
                    if let Some(panel) = &self.panel {
                        panel.request_redraw();
                    }
                }
                Action::Quit => {
                    self.panel = None;
                    for entry in &mut self.devices {
                        if let Some(streaming) = entry.streaming.take() {
                            streaming.join();
                        }
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
        let proxy = Mutex::new(self.proxy.clone());
        devices::watch(move |serials| {
            let _ = proxy
                .lock()
                .unwrap()
                .send_event(UserEvent::DevicesChanged(serials));
        });
        self.status_changed();
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::TrayClicked => self.toggle_panel(event_loop),
            UserEvent::StatusChanged => self.status_changed(),
            UserEvent::DevicesChanged(serials) => self.devices_changed(serials),
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
        devices: Vec::new(),
        selected: None,
        displays: Vec::new(),
        main_display: None,
        tray: None,
        icon: Icon::Disabled,
        panel: None,
        closed_at: None,
    };
    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("event loop: {e}");
    }
}
