//! Auto-update. The launcher asks GitHub for the latest release on start and
//! again every half hour. When it is newer than this build:
//!
//! * on Windows the package is downloaded in the background right away; the
//!   menu then offers to restart into the new version, and if the player just
//!   closes the game, the update is installed on the way out;
//! * elsewhere a banner links to the release page.
//!
//! Every build of the main branch is published by CI as the latest release,
//! tagged `v<major>.<minor>.<build number>` (see `.github/workflows/ci.yml`),
//! and the same version is baked into the exe through `GAMEBOX_VERSION`.
//! Local builds without it never check for updates.
//!
//! Set `GAMEBOX_NO_UPDATE=1` to turn the check off.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use bevy::prelude::*;
use bevy_egui::egui::{self, Align2, Color32, RichText, Vec2};
use bevy_egui::{EguiContexts, EguiPrimaryContextPass};
use serde::Deserialize;

use crate::theme::{self, regular, semibold};
use crate::AppState;

const REPO: &str = "frd21313123123/game-box";
/// Name of the package attached to every release by `.github/workflows/ci.yml`.
const PACKAGE_NAME: &str = "game-box-windows-x64.zip";
/// How often a running launcher asks GitHub again.
const RECHECK: Duration = Duration::from_secs(30 * 60);

/// Version of this build: `X.Y.<build number>` from CI, or `version` from
/// Cargo.toml for a local build.
pub const VERSION: &str = match option_env!("GAMEBOX_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};
/// Built by CI, so its version is comparable with the published releases.
const RELEASE_BUILD: bool = option_env!("GAMEBOX_VERSION").is_some();

#[cfg(windows)]
const UPDATE_SCRIPT: &str = r#"param([int]$ProcessId, [string]$Zip, [string]$Target, [switch]$Restart)
# Runs after the launcher has exited: unpack the package over the install
# folder and, if asked, start the new version. Saves live in the user
# profile, not here.
$ErrorActionPreference = 'Stop'
$exe = Join-Path $Target 'game-box.exe'
$log = Join-Path $env:TEMP 'game-box-update.log'
try {
    Wait-Process -Id $ProcessId -ErrorAction SilentlyContinue
    $stage = Join-Path $env:TEMP ('game-box-stage-' + [guid]::NewGuid().ToString('N'))
    Expand-Archive -LiteralPath $Zip -DestinationPath $stage -Force
    Copy-Item -Path (Join-Path $stage 'game-box\*') -Destination $Target -Recurse -Force
    Remove-Item -LiteralPath $stage -Recurse -Force
    Remove-Item -LiteralPath $Zip -Force
} catch {
    "$(Get-Date -Format s) update failed: $($_ | Out-String)" | Add-Content -Path $log
}
if ($Restart) { Start-Process -FilePath $exe -WorkingDirectory $Target }
"#;

pub struct UpdatePlugin;

impl Plugin for UpdatePlugin {
    fn build(&self, app: &mut App) {
        let (tx, rx) = mpsc::channel();
        let enabled = RELEASE_BUILD && std::env::var_os("GAMEBOX_NO_UPDATE").is_none();
        app.insert_resource(Updater {
            tx,
            rx: Mutex::new(rx),
            status: Status::Idle,
            enabled,
            checking: false,
            restart_requested: false,
            recheck: Timer::new(RECHECK, TimerMode::Repeating),
        })
        .add_systems(Startup, start_check)
        .add_systems(Update, (recheck, poll_updater))
        // Runs after the frame's `AppExit` (window closed, menu «Выход») is written.
        .add_systems(Last, install_on_exit)
        .add_systems(
            EguiPrimaryContextPass,
            update_banner
                .run_if(in_state(AppState::Menu))
                .run_if(resource_exists::<crate::ThemeReady>),
        );
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    /// Version without the leading `v`, e.g. `0.2.31`.
    pub version: String,
    /// Address of the release page, for platforms without the in-app installer.
    pub page_url: String,
    /// Direct download link of the Windows package.
    pub package_url: String,
    /// Size of the package in bytes, as GitHub reports it.
    pub size: u64,
}

enum Status {
    Idle,
    /// Newer release found, nothing downloaded (platforms without the installer).
    Available(Release),
    Downloading(Release, Arc<AtomicU64>),
    /// The package is on disk and will be installed on restart or on exit.
    Ready(Release, PathBuf),
    Installing,
    /// The message, and the release to retry or fall back to if there is one.
    Failed(String, Option<Release>),
}

#[derive(Resource)]
pub struct Updater {
    tx: mpsc::Sender<Msg>,
    rx: Mutex<mpsc::Receiver<Msg>>,
    status: Status,
    enabled: bool,
    checking: bool,
    restart_requested: bool,
    recheck: Timer,
}

enum Msg {
    Checked(Result<Option<Release>, String>),
    Downloaded(Result<PathBuf, String>),
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    html_url: String,
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize)]
struct ApiAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

/// `v0.2.1` / `0.2` → `(0, 2, 1)`. `None` for anything that is not plain numbers.
pub fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let mut parts = s.trim().trim_start_matches('v').split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = match parts.next() {
        Some(p) => p.parse().ok()?,
        None => 0,
    };
    let patch = match parts.next() {
        Some(p) => p.parse().ok()?,
        None => 0,
    };
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

fn start_check(mut updater: ResMut<Updater>) {
    if !updater.enabled {
        if !RELEASE_BUILD {
            info!("локальная сборка {VERSION}: обновления не проверяются");
        }
        return;
    }
    check(&mut updater);
}

fn check(updater: &mut Updater) {
    if updater.checking {
        return;
    }
    updater.checking = true;
    let tx = updater.tx.clone();
    std::thread::spawn(move || {
        let _ = tx.send(Msg::Checked(fetch_latest()));
    });
}

/// A launcher left open in the menu still learns about new builds.
fn recheck(time: Res<Time>, mut updater: ResMut<Updater>) {
    if !updater.enabled || !updater.recheck.tick(time.delta()).just_finished() {
        return;
    }
    if matches!(updater.status, Status::Idle | Status::Available(_) | Status::Failed(..)) {
        check(&mut updater);
    }
}

fn fetch_latest() -> Result<Option<Release>, String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    // `releases/latest` already skips drafts and pre-releases.
    let api: ApiRelease = ureq::get(&url)
        .set("User-Agent", "game-box-updater")
        .set("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(10))
        .call()
        .map_err(|e| e.to_string())?
        .into_json()
        .map_err(|e| e.to_string())?;

    let latest = parse_version(&api.tag_name)
        .ok_or_else(|| format!("непонятная версия релиза: {}", api.tag_name))?;
    let current = parse_version(VERSION).ok_or("непонятная версия программы")?;
    if latest <= current {
        return Ok(None);
    }

    let asset = api
        .assets
        .into_iter()
        .find(|a| a.name == PACKAGE_NAME)
        .ok_or_else(|| format!("в релизе {} нет файла {PACKAGE_NAME}", api.tag_name))?;
    // Only take packages from this repository's own releases.
    let prefix = format!("https://github.com/{REPO}/releases/download/");
    if !asset.browser_download_url.starts_with(&prefix) {
        return Err("ссылка на обновление ведёт не в репозиторий игры".into());
    }
    Ok(Some(Release {
        version: api.tag_name.trim_start_matches('v').to_string(),
        page_url: api.html_url,
        package_url: asset.browser_download_url,
        size: asset.size,
    }))
}

/// Downloads into `<dest>.part` and renames it once complete, so a half-done
/// download is never taken for a package.
fn download(url: &str, dest: &Path, expected: u64, progress: &AtomicU64) -> Result<(), String> {
    use std::io::{Read, Write};

    let response = ureq::get(url)
        .set("User-Agent", "game-box-updater")
        .timeout(Duration::from_secs(600))
        .call()
        .map_err(|e| e.to_string())?;
    let part = dest.with_extension("part");
    let mut file = std::fs::File::create(&part).map_err(|e| e.to_string())?;
    let mut reader = response.into_reader();
    let mut buf = vec![0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        total += n as u64;
        progress.store(total, Ordering::Relaxed);
    }
    file.flush().map_err(|e| e.to_string())?;
    drop(file);
    if expected > 0 && total != expected {
        let _ = std::fs::remove_file(&part);
        return Err(format!("скачано {total} байт из {expected}"));
    }
    std::fs::rename(&part, dest).map_err(|e| e.to_string())
}

fn start_download(updater: &mut Updater, release: Release) {
    let tx = updater.tx.clone();
    let url = release.package_url.clone();
    let expected = release.size;
    let progress = Arc::new(AtomicU64::new(0));
    let counter = progress.clone();
    updater.status = Status::Downloading(release, progress);
    std::thread::spawn(move || {
        let dir = std::env::temp_dir().join("game-box-update");
        let result = std::fs::create_dir_all(&dir)
            .map_err(|e| e.to_string())
            .and_then(|_| {
                let zip = dir.join(PACKAGE_NAME);
                download(&url, &zip, expected, &counter).map(|_| zip)
            });
        let _ = tx.send(Msg::Downloaded(result));
    });
}

/// Replaces the installed files once this process has exited, then starts the
/// new version if `restart` is set.
#[cfg(windows)]
fn spawn_installer(zip: &Path, restart: bool) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let target = exe.parent().ok_or("не найдена папка программы")?;
    let script = std::env::temp_dir().join("game-box-update.ps1");
    std::fs::write(&script, UPDATE_SCRIPT).map_err(|e| e.to_string())?;

    let mut cmd = std::process::Command::new("powershell.exe");
    cmd.args([
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-WindowStyle",
        "Hidden",
        "-File",
    ])
    .arg(&script)
    .arg("-ProcessId")
    .arg(std::process::id().to_string())
    .arg("-Zip")
    .arg(zip)
    .arg("-Target")
    .arg(target);
    if restart {
        cmd.arg("-Restart");
    }
    cmd.creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(not(windows))]
fn spawn_installer(_zip: &Path, _restart: bool) -> Result<(), String> {
    Err("автоустановка доступна только в Windows".into())
}

fn poll_updater(mut updater: ResMut<Updater>, mut exit: EventWriter<AppExit>) {
    let messages: Vec<Msg> = {
        let rx = updater.rx.lock().unwrap_or_else(|e| e.into_inner());
        rx.try_iter().collect()
    };
    for msg in messages {
        match msg {
            Msg::Checked(result) => {
                updater.checking = false;
                match result {
                    Ok(Some(release)) => on_release_found(&mut updater, release),
                    Ok(None) => {}
                    // Network trouble is not worth a banner: the next check retries.
                    Err(e) => warn!("проверка обновлений не удалась: {e}"),
                }
            }
            Msg::Downloaded(Ok(zip)) => {
                if let Status::Downloading(r, _) = &updater.status {
                    info!("обновление {} скачано", r.version);
                    updater.status = Status::Ready(r.clone(), zip);
                }
            }
            Msg::Downloaded(Err(e)) => {
                let release = match &updater.status {
                    Status::Downloading(r, _) => Some(r.clone()),
                    _ => None,
                };
                warn!("не удалось скачать обновление: {e}");
                updater.status = Status::Failed(e, release);
            }
        }
    }

    if std::mem::take(&mut updater.restart_requested) {
        if let Status::Ready(_, zip) = &updater.status {
            match spawn_installer(zip, true) {
                Ok(()) => {
                    updater.status = Status::Installing;
                    exit.write(AppExit::Success);
                }
                Err(e) => updater.status = Status::Failed(e, None),
            }
        }
    }
}

fn on_release_found(updater: &mut Updater, release: Release) {
    // Already fetching or holding this version (or a newer one)?
    let have = match &updater.status {
        Status::Downloading(r, _) | Status::Ready(r, _) => Some(r.version.clone()),
        Status::Installing => return,
        _ => None,
    };
    if have.is_some_and(|v| parse_version(&v) >= parse_version(&release.version)) {
        return;
    }
    info!("доступна новая версия {}", release.version);
    if cfg!(windows) {
        start_download(updater, release);
    } else {
        updater.status = Status::Available(release);
    }
}

/// Closing the game with a downloaded update installs it without restarting.
fn install_on_exit(mut exits: EventReader<AppExit>, mut updater: ResMut<Updater>) {
    if exits.is_empty() {
        return;
    }
    exits.clear();
    if let Status::Ready(r, zip) = &updater.status {
        match spawn_installer(zip, false) {
            Ok(()) => info!("обновление {} установится после выхода", r.version),
            Err(e) => warn!("не удалось запустить установку обновления: {e}"),
        }
        updater.status = Status::Installing;
    }
}

fn update_banner(mut contexts: EguiContexts, mut updater: ResMut<Updater>) -> Result {
    let ctx = contexts.ctx_mut()?.clone();

    // Version of this build, so it is easy to tell whether an update arrived.
    let version = if RELEASE_BUILD {
        format!("версия {VERSION}")
    } else {
        format!("версия {VERSION} (локальная сборка)")
    };
    crate::theme::show_area(&ctx, egui::Area::new(egui::Id::new("app_version"))
        .anchor(Align2::RIGHT_BOTTOM, Vec2::new(-20.0, -14.0))
        .interactable(false)
        .order(egui::Order::Foreground), |ui| {
            ui.label(RichText::new(version).font(regular(12.0)).color(theme::TEXT_DIM));
        });

    if matches!(updater.status, Status::Idle | Status::Installing) {
        return Ok(());
    }

    let mut restart = false;
    let mut retry: Option<Release> = None;
    let mut dismiss = false;
    crate::theme::show_area(&ctx, egui::Area::new(egui::Id::new("update_banner"))
        .anchor(Align2::LEFT_BOTTOM, Vec2::new(28.0, -20.0))
        .order(egui::Order::Foreground), |ui| {
            egui::Frame::new()
                .fill(theme::PANEL)
                .stroke(theme::stroke(1.0, theme::with_alpha(theme::ACCENT, 160)))
                .corner_radius(10)
                .inner_margin(egui::Margin::symmetric(16, 10))
                .show(ui, |ui| {
                    let title = |text: String| RichText::new(text).font(semibold(15.0)).color(theme::TEXT);
                    let hint = |text: &str| RichText::new(text).font(regular(12.5)).color(theme::TEXT_DIM);
                    match &updater.status {
                        Status::Available(r) => {
                            ui.horizontal(|ui| {
                                ui.label(title(format!("Доступна новая версия {}", r.version)));
                                if ui.button("Открыть страницу релиза").clicked() {
                                    ctx.open_url(egui::OpenUrl::new_tab(&r.page_url));
                                }
                            });
                        }
                        Status::Downloading(r, progress) => {
                            let done = progress.load(Ordering::Relaxed);
                            let text = if r.size > 0 {
                                let pct = (done as f64 / r.size as f64 * 100.0).min(100.0);
                                format!("Скачиваю версию {}… {pct:.0}%", r.version)
                            } else {
                                format!("Скачиваю версию {}… {:.1} МБ", r.version, done as f64 / 1_048_576.0)
                            };
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(title(text));
                            });
                            // Keep the percentage moving without other input.
                            ctx.request_repaint_after(Duration::from_millis(200));
                        }
                        Status::Ready(r, _) => {
                            ui.horizontal(|ui| {
                                ui.label(title(format!("Версия {} скачана", r.version)));
                                if ui.button("Перезапустить и обновить").clicked() {
                                    restart = true;
                                }
                            });
                            ui.label(hint("Или просто закройте игру: обновление установится при выходе."));
                        }
                        Status::Failed(e, release) => {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(format!("Не удалось обновиться: {e}"))
                                        .font(semibold(15.0))
                                        .color(Color32::from_rgb(240, 110, 110)),
                                );
                                if let Some(r) = release {
                                    if cfg!(windows) && ui.button("Повторить").clicked() {
                                        retry = Some(r.clone());
                                    }
                                    if ui.button("Открыть страницу релиза").clicked() {
                                        ctx.open_url(egui::OpenUrl::new_tab(&r.page_url));
                                    }
                                }
                                if ui.button("✕").clicked() {
                                    dismiss = true;
                                }
                            });
                        }
                        Status::Idle | Status::Installing => {}
                    }
                });
        });

    if restart {
        updater.restart_requested = true;
    }
    if let Some(r) = retry {
        start_download(&mut updater, r);
    }
    if dismiss {
        updater.status = Status::Idle;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically() {
        assert_eq!(parse_version("v0.2.0"), Some((0, 2, 0)));
        assert_eq!(parse_version("1.10"), Some((1, 10, 0)));
        assert!(parse_version("v0.10.0") > parse_version("0.9.9"));
        assert_eq!(parse_version("v0.1.0"), parse_version("0.1.0"));
        assert_eq!(parse_version("nightly"), None);
        assert_eq!(parse_version("1.2.3.4"), None);
        assert_eq!(parse_version("1.2-beta"), None);
    }

    #[test]
    fn build_numbers_order_releases() {
        // CI tags builds v<major>.<minor>.<build number>.
        assert!(parse_version("v0.2.25") > parse_version("0.2.0"));
        assert!(parse_version("v0.2.25") > parse_version("v0.1.0"));
        assert!(parse_version("v0.2.100") > parse_version("v0.2.99"));
        assert!(parse_version("v0.3.101") > parse_version("v0.2.100"));
    }

    #[test]
    fn own_version_parses() {
        assert!(parse_version(VERSION).is_some(), "VERSION = {VERSION}");
    }

    #[test]
    fn download_writes_whole_file_only() {
        let dir = std::env::temp_dir().join(format!("gb-update-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("pkg.zip");
        // A failed request leaves neither the package nor a partial file behind.
        let progress = AtomicU64::new(0);
        assert!(download("http://127.0.0.1:9/nothing", &dest, 10, &progress).is_err());
        assert!(!dest.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
