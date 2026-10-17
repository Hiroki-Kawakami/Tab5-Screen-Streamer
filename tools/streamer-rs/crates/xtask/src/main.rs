//! `cargo xtask dist`: builds the app and the CLI in release and packs them
//! into one archive in target/dist for the OS it runs on.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const APP_NAME: &str = "Tab5 Streamer";
const ARCHIVE_NAME: &str = "Tab5-Streamer";
const PACKAGE: &str = "streamer-app";
const CLI_PACKAGE: &str = "streamer";
const CLI_NAME: &str = "tab5-streamer";
#[cfg(target_os = "macos")]
const BUNDLE_ID: &str = "local.tab5-streamer";
#[cfg(target_os = "macos")]
const VERSION: &str = env!("CARGO_PKG_VERSION");

type Result<T> = std::result::Result<T, String>;

fn run(command: &mut Command) -> Result<()> {
    let status = command.status().map_err(|e| format!("{command:?}: {e}"))?;
    match status.success() {
        true => Ok(()),
        false => Err(format!("{command:?}: {status}")),
    }
}

fn workspace() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest.ancestors().nth(2).unwrap_or(manifest).to_path_buf()
}

fn target_dir(workspace: &Path) -> PathBuf {
    env::var_os("CARGO_TARGET_DIR").map_or_else(|| workspace.join("target"), PathBuf::from)
}

struct Binaries {
    app: PathBuf,
    cli: PathBuf,
}

fn build(workspace: &Path) -> Result<Binaries> {
    let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    run(Command::new(cargo)
        .current_dir(workspace)
        .args(["build", "--release", "-p", PACKAGE, "-p", CLI_PACKAGE]))?;
    let release = target_dir(workspace).join("release");
    let exe = |name: &str| release.join(format!("{name}{}", env::consts::EXE_SUFFIX));
    Ok(Binaries {
        app: exe(PACKAGE),
        cli: exe(CLI_NAME),
    })
}

fn remove(path: &Path) -> Result<()> {
    let removed = match path.is_dir() {
        true => fs::remove_dir_all(path),
        false => fs::remove_file(path),
    };
    match removed {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            Err(format!("{}: {e}", path.display()))
        }
        _ => Ok(()),
    }
}

fn copy(from: &Path, to: &Path) -> Result<()> {
    fs::copy(from, to)
        .map(|_| ())
        .map_err(|e| format!("{} -> {}: {e}", from.display(), to.display()))
}

#[cfg(target_os = "macos")]
fn info_plist() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>{PACKAGE}</string>
    <key>CFBundleIconFile</key>
    <string>app</string>
    <key>CFBundleIdentifier</key>
    <string>{BUNDLE_ID}</string>
    <key>CFBundleName</key>
    <string>{APP_NAME}</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>{VERSION}</string>
    <key>LSUIElement</key>
    <true/>
</dict>
</plist>
"#
    )
}

/// The .app, signed with CODESIGN_IDENTITY or else ad hoc, and the CLI,
/// zipped.
#[cfg(target_os = "macos")]
fn package(workspace: &Path, binaries: &Binaries, dist: &Path) -> Result<PathBuf> {
    let root = dist.join(format!("{ARCHIVE_NAME}-macos-{}", env::consts::ARCH));
    remove(&root)?;
    let app = root.join(format!("{APP_NAME}.app"));
    let contents = app.join("Contents");
    for dir in ["MacOS", "Resources"] {
        let dir = contents.join(dir);
        fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    copy(&binaries.app, &contents.join("MacOS").join(PACKAGE))?;
    let icon = workspace.join("crates/app/icons/app.icns");
    copy(&icon, &contents.join("Resources/app.icns"))?;
    let plist = contents.join("Info.plist");
    fs::write(&plist, info_plist()).map_err(|e| format!("{}: {e}", plist.display()))?;
    let identity = env::var("CODESIGN_IDENTITY").unwrap_or_else(|_| "-".into());
    run(Command::new("codesign").args(["--force", "--sign", &identity]).arg(&app))?;
    copy(&binaries.cli, &root.join(CLI_NAME))?;
    let archive = root.with_extension("zip");
    remove(&archive)?;
    // ditto keeps the bundle's symlinks and extended attributes, which the
    // signature covers.
    run(Command::new("ditto").args(["-c", "-k"]).arg(&root).arg(&archive))?;
    Ok(archive)
}

/// The app's executable under the app's name and the CLI, zipped. The app's
/// icon is linked in by its build script.
#[cfg(windows)]
fn package(_workspace: &Path, binaries: &Binaries, dist: &Path) -> Result<PathBuf> {
    let app = format!("{APP_NAME}.exe");
    let cli = format!("{CLI_NAME}.exe");
    copy(&binaries.app, &dist.join(&app))?;
    copy(&binaries.cli, &dist.join(&cli))?;
    let archive = dist.join(format!("{ARCHIVE_NAME}-windows-{}.zip", env::consts::ARCH));
    remove(&archive)?;
    // Windows' own bsdtar, which writes zip; a GNU tar earlier in PATH does not.
    let system = env::var_os("SystemRoot").ok_or("SystemRoot is not set")?;
    let tar = Path::new(&system).join("System32").join("tar.exe");
    run(Command::new(tar)
        .args(["-a", "-c", "-f"])
        .arg(&archive)
        .arg("-C")
        .arg(dist)
        .arg(&app)
        .arg(&cli))?;
    Ok(archive)
}

#[cfg(not(any(target_os = "macos", windows)))]
fn package(_workspace: &Path, _binaries: &Binaries, _dist: &Path) -> Result<PathBuf> {
    Err("no package for this OS".into())
}

fn dist() -> Result<PathBuf> {
    let workspace = workspace();
    let binaries = build(&workspace)?;
    let dist = target_dir(&workspace).join("dist");
    fs::create_dir_all(&dist).map_err(|e| format!("{}: {e}", dist.display()))?;
    package(&workspace, &binaries, &dist)
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    if args != ["dist"] {
        eprintln!("usage: cargo xtask dist");
        return ExitCode::from(2);
    }
    match dist() {
        Ok(archive) => {
            println!("{}", archive.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
