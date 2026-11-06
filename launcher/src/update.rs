//! Auto-update: on start the launcher asks GitHub for the latest release. If it
//! is newer than this build, a banner appears in the menu. On Windows the
//! banner's button downloads the release package, closes the launcher and lets
//! a small PowerShell script replace the files and start the new version.
//!
//! Set `GAMEBOX_NO_UPDATE=1` to turn the check off (for example on a dev machine).

use std::path::{Path, PathBuf};
use std::sync::{mpsc, Mutex};
use std::time::Duration;

use bevy::prelude::*;
use bevy_egui::egui::{self, Align2, Color32, RichText, Vec2};
use bevy_egui::{EguiContexts, EguiPrimaryContextPass};
use serde::Deserialize;

use crate::theme::{self, semibold};
use crate::AppState;

const REPO: &str = "frd21313123123/game-box";
/// Name of the package attached to every release by `.github/workflows/release.yml`.
const PACKAGE_NAME: &str = "game-box-windows-x64.zip";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(windows)]
const UPDATE_SCRIPT: &str = r#"param([int]$ProcessId, [string]$Zip, [string]$Target)
# Runs after the launcher has exited: unpack the package over the install
# folder and start the new version. Saves live in the user profile, not here.
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
Start-Process -FilePath $exe -WorkingDirectory $Target
"#;

pub struct UpdatePlugin;

impl Plugin for UpdatePlugin {
    fn build(&self, app: &mut App) {
        let (tx, rx) = mpsc::channel();
        app.insert_resource(Updater {
            tx,
            rx: Mutex::new(rx),
            status: Status::Idle,
        })
        .add_systems(Startup, start_check)
        .add_systems(Update, poll_updater)
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
    /// Version without the leading `v`, e.g. `0.2.0`.
    pub version: String,
    /// Address of the release page, for platforms without the in-app installer.
    pub page_url: String,
    /// Direct download link of the Windows package.
    pub package_url: String,
}

enum Status {
    Idle,
    Available(Release),
    Downloading(Release),
    Installing,
    /// The message, and the release page to fall back to if there is one.
    Failed(String, Option<String>),
}

#[derive(Resource)]
pub struct Updater {
    tx: mpsc::Sender<Msg>,
    rx: Mutex<mpsc::Receiver<Msg>>,
    status: Status,
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

fn start_check(updater: Res<Updater>) {
    if std::env::var_os("GAMEBOX_NO_UPDATE").is_some() {
        return;
    }
    let tx = updater.tx.clone();
    std::thread::spawn(move || {
        let _ = tx.send(Msg::Checked(fetch_latest()));
    });
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
    let current = parse_version(CURRENT_VERSION).ok_or("непонятная версия программы")?;
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
    }))
}

fn download(url: &str, dest: &Path) -> Result<(), String> {
    let response = ureq::get(url)
        .set("User-Agent", "game-box-updater")
        .timeout(Duration::from_secs(600))
        .call()
        .map_err(|e| e.to_string())?;
    let mut file = std::fs::File::create(dest).map_err(|e| e.to_string())?;
    std::io::copy(&mut response.into_reader(), &mut file).map_err(|e| e.to_string())?;
    Ok(())
}

fn start_download(updater: &mut Updater, release: Release) {
    let tx = updater.tx.clone();
    let url = release.package_url.clone();
    updater.status = Status::Downloading(release);
    std::thread::spawn(move || {
        let dir = std::env::temp_dir().join("game-box-update");
        let result = std::fs::create_dir_all(&dir)
            .map_err(|e| e.to_string())
            .and_then(|_| {
                let zip = dir.join(PACKAGE_NAME);
                download(&url, &zip).map(|_| zip)
            });
        let _ = tx.send(Msg::Downloaded(result));
    });
}

/// Replaces the installed files once this process has exited.
#[cfg(windows)]
fn spawn_installer(zip: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let target = exe.parent().ok_or("не найдена папка программы")?;
    let script = std::env::temp_dir().join("game-box-update.ps1");
    std::fs::write(&script, UPDATE_SCRIPT).map_err(|e| e.to_string())?;

    std::process::Command::new("powershell.exe")
        .args([
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
        .arg(target)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(not(windows))]
fn spawn_installer(_zip: &Path) -> Result<(), String> {
    Err("автоустановка доступна только в Windows".into())
}

fn poll_updater(mut updater: ResMut<Updater>, mut exit: EventWriter<AppExit>) {
    let messages: Vec<Msg> = {
        let rx = updater.rx.lock().unwrap_or_else(|e| e.into_inner());
        rx.try_iter().collect()
    };
    for msg in messages {
        match msg {
            Msg::Checked(Ok(Some(release))) => {
                info!("доступна новая версия {}", release.version);
                updater.status = Status::Available(release);
            }
            Msg::Checked(Ok(None)) => {}
            Msg::Checked(Err(e)) => warn!("проверка обновлений не удалась: {e}"),
            Msg::Downloaded(Ok(zip)) => match spawn_installer(&zip) {
                Ok(()) => {
                    updater.status = Status::Installing;
                    exit.write(AppExit::Success);
                }
                Err(e) => updater.status = Status::Failed(e, None),
            },
            Msg::Downloaded(Err(e)) => {
                let page = match &updater.status {
                    Status::Downloading(r) => Some(r.page_url.clone()),
                    _ => None,
                };
                updater.status = Status::Failed(e, page);
            }
        }
    }
}

fn update_banner(mut contexts: EguiContexts, mut updater: ResMut<Updater>) -> Result {
    let ctx = contexts.ctx_mut()?.clone();
    if matches!(updater.status, Status::Idle) {
        return Ok(());
    }

    let mut start = false;
    crate::theme::show_area(&ctx, egui::Area::new(egui::Id::new("update_banner"))
        .anchor(Align2::LEFT_BOTTOM, Vec2::new(28.0, -20.0))
        .order(egui::Order::Foreground), |ui| {
            egui::Frame::new()
                .fill(theme::PANEL)
                .stroke(theme::stroke(1.0, theme::with_alpha(theme::ACCENT, 160)))
                .corner_radius(10)
                .inner_margin(egui::Margin::symmetric(16, 10))
                .show(ui, |ui| {
                    ui.horizontal(|ui| match &updater.status {
                        Status::Available(r) => {
                            ui.label(
                                RichText::new(format!("Доступна новая версия {}", r.version))
                                    .font(semibold(15.0))
                                    .color(theme::TEXT),
                            );
                            if cfg!(windows) {
                                if ui.button("Обновить и перезапустить").clicked()
                                {
                                    start = true;
                                }
                            } else if ui.button("Открыть страницу релиза").clicked()
                            {
                                ctx.open_url(egui::OpenUrl::new_tab(&r.page_url));
                            }
                        }
                        Status::Downloading(r) => {
                            ui.spinner();
                            ui.label(
                                RichText::new(format!("Скачиваю версию {}…", r.version))
                                    .font(semibold(15.0))
                                    .color(theme::TEXT),
                            );
                        }
                        Status::Installing => {
                            ui.spinner();
                            ui.label(
                                RichText::new("Перезапуск для обновления…")
                                    .font(semibold(15.0))
                                    .color(theme::TEXT),
                            );
                        }
                        Status::Failed(e, page) => {
                            ui.label(
                                RichText::new(format!("Не удалось обновиться: {e}"))
                                    .font(semibold(15.0))
                                    .color(Color32::from_rgb(240, 110, 110)),
                            );
                            if let Some(page) = page {
                                if ui.button("Открыть страницу релиза").clicked()
                                {
                                    ctx.open_url(egui::OpenUrl::new_tab(page));
                                }
                            }
                        }
                        Status::Idle => {}
                    });
                });
        });

    if start {
        if let Status::Available(r) = std::mem::replace(&mut updater.status, Status::Idle) {
            start_download(&mut updater, r);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_version;

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
}
