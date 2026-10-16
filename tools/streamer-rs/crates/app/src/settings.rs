//! What the user chose, kept across launches as `key=value` lines.

use std::fs;
use std::path::PathBuf;

const APP_DIR: &str = "Tab5 Streamer";
const FILE: &str = "settings.txt";

#[derive(Clone, Copy, PartialEq)]
pub enum Source {
    Main,
    /// A display made for the device, on macOS.
    Virtual,
}

#[derive(Clone, Copy, PartialEq)]
pub struct Settings {
    pub streaming: bool,
    pub source: Source,
    /// Counter-clockwise quarter turns; the device's orientation when None.
    pub rotation: Option<u32>,
    pub touch: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
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

impl Settings {
    /// Unknown keys and bad values keep their defaults.
    pub fn load() -> Self {
        let mut settings = Settings::default();
        let Some(text) = path().and_then(|path| fs::read_to_string(path).ok()) else {
            return settings;
        };
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim();
            match key.trim() {
                "streaming" => settings.streaming = flag(value).unwrap_or(settings.streaming),
                "source" => match value {
                    "main" => settings.source = Source::Main,
                    "virtual" if cfg!(target_os = "macos") => settings.source = Source::Virtual,
                    _ => {}
                },
                "rotation" => match value {
                    "auto" => settings.rotation = None,
                    value => {
                        if let Some(rotation) = value.parse().ok().filter(|r| *r < 4) {
                            settings.rotation = Some(rotation);
                        }
                    }
                },
                "touch" => settings.touch = flag(value).unwrap_or(settings.touch),
                _ => {}
            }
        }
        settings
    }

    pub fn save(&self) -> Result<(), String> {
        let path = path().ok_or("no settings directory")?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        let rotation = match self.rotation {
            Some(rotation) => rotation.to_string(),
            None => "auto".into(),
        };
        let source = match self.source {
            Source::Main => "main",
            Source::Virtual => "virtual",
        };
        let text = format!(
            "streaming={}\nsource={source}\nrotation={rotation}\ntouch={}\n",
            on_off(self.streaming),
            on_off(self.touch)
        );
        fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
    }
}
