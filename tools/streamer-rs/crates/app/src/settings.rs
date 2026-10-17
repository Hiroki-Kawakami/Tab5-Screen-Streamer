//! What the user chose for each Tab5, kept across launches as
//! `<serial>.key=value` lines.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

const APP_DIR: &str = "Tab5 Streamer";
const FILE: &str = "settings.txt";

#[derive(Clone, Copy, PartialEq)]
pub enum Source {
    Main,
    /// A display id on macOS, an index into the displays list elsewhere.
    Display(u64),
    /// A display made for the device, on macOS.
    Virtual,
}

#[derive(Clone, Copy, PartialEq)]
pub struct DeviceSettings {
    pub streaming: bool,
    pub source: Source,
    /// Counter-clockwise quarter turns; the device's orientation when None.
    pub rotation: Option<u32>,
    pub touch: bool,
}

impl Default for DeviceSettings {
    fn default() -> Self {
        DeviceSettings {
            streaming: true,
            source: Source::Main,
            rotation: None,
            touch: true,
        }
    }
}

fn path() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    let base = std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Application Support"));
    #[cfg(windows)]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(not(any(target_os = "macos", windows)))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")));
    Some(base?.join(APP_DIR).join(FILE))
}

fn flag(value: &str) -> Option<bool> {
    match value {
        "on" => Some(true),
        "off" => Some(false),
        _ => None,
    }
}

fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

#[derive(Default)]
pub struct Settings {
    devices: BTreeMap<String, DeviceSettings>,
}

impl Settings {
    pub fn device(&self, serial: &str) -> DeviceSettings {
        self.devices.get(serial).copied().unwrap_or_default()
    }

    pub fn set_device(&mut self, serial: &str, settings: DeviceSettings) {
        self.devices.insert(serial.into(), settings);
    }

    /// Unknown keys and bad values keep their defaults.
    pub fn load() -> Self {
        let mut devices = BTreeMap::<String, DeviceSettings>::new();
        if let Some(text) = path().and_then(|path| fs::read_to_string(path).ok()) {
            for line in text.lines() {
                let Some((key, value)) = line.split_once('=') else {
                    continue;
                };
                let Some((serial, key)) = key.trim().split_once('.') else {
                    continue;
                };
                let settings = devices.entry(serial.to_string()).or_default();
                settings.set(key, value.trim());
            }
        }
        Settings { devices }
    }

    pub fn save(&self) -> Result<(), String> {
        let path = path().ok_or("no settings directory")?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        let mut text = String::new();
        for (serial, settings) in &self.devices {
            settings.write(serial, &mut text);
        }
        fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
    }
}

impl DeviceSettings {
    fn set(&mut self, key: &str, value: &str) {
        match key {
            "streaming" => self.streaming = flag(value).unwrap_or(self.streaming),
            "source" => match value {
                "main" => self.source = Source::Main,
                "virtual" if cfg!(target_os = "macos") => self.source = Source::Virtual,
                value => {
                    if let Ok(display) = value.parse() {
                        self.source = Source::Display(display);
                    }
                }
            },
            "rotation" => match value {
                "auto" => self.rotation = None,
                value => {
                    if let Some(rotation) = value.parse().ok().filter(|r| *r < 4) {
                        self.rotation = Some(rotation);
                    }
                }
            },
            "touch" => self.touch = flag(value).unwrap_or(self.touch),
            _ => {}
        }
    }

    fn write(&self, serial: &str, text: &mut String) {
        let rotation = match self.rotation {
            Some(rotation) => rotation.to_string(),
            None => "auto".into(),
        };
        let source = match self.source {
            Source::Main => "main".to_string(),
            Source::Display(display) => display.to_string(),
            Source::Virtual => "virtual".to_string(),
        };
        let streaming = on_off(self.streaming);
        let touch = on_off(self.touch);
        for (key, value) in [
            ("streaming", streaming),
            ("source", &source),
            ("rotation", &rotation),
            ("touch", touch),
        ] {
            text.push_str(&format!("{serial}.{key}={value}\n"));
        }
    }
}
