use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};

pub use dbx_core::update::UpdateInfo;

const UPDATE_FEED_URL: &str = "http://111.230.247.111/dbx/latest.json";
const MAX_INSTALLER_BYTES: usize = 512 * 1024 * 1024;
const CANCELED: &str = "Download canceled by user.";

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateDownloadSource {
    Official,
    Cnb,
}

#[derive(Debug, Deserialize)]
struct InstallerManifest {
    version: String,
    #[serde(default)]
    notes: String,
    url: String,
    #[serde(default)]
    sha256: String,
    #[serde(default, rename = "silentArgs")]
    silent_args: Option<String>,
}

#[derive(Clone, Serialize)]
struct DownloadProgress {
    downloaded: u64,
    total: Option<u64>,
}

struct ReadyInstaller {
    path: PathBuf,
    version: Version,
    silent_args: Option<String>,
}

enum PendingUpdate {
    Downloading(Arc<tokio::sync::watch::Sender<bool>>),
    Ready(ReadyInstaller),
    Installing,
}

#[derive(Default)]
pub struct PendingUpdateState {
    pending: Mutex<Option<PendingUpdate>>,
}

impl PendingUpdateState {
    fn begin(&self) -> Result<Arc<tokio::sync::watch::Sender<bool>>, String> {
        let mut pending = self.pending.lock().map_err(|_| "Update state unavailable".to_string())?;
        if pending.is_some() {
            return Err("An update is already in progress.".into());
        }
        let (sender, _) = tokio::sync::watch::channel(false);
        let sender = Arc::new(sender);
        *pending = Some(PendingUpdate::Downloading(sender.clone()));
        Ok(sender)
    }

    fn finish(
        &self,
        sender: &Arc<tokio::sync::watch::Sender<bool>>,
        ready: Option<ReadyInstaller>,
    ) -> Result<(), String> {
        let mut pending = self.pending.lock().map_err(|_| "Update state unavailable".to_string())?;
        if !matches!(pending.as_ref(), Some(PendingUpdate::Downloading(current)) if Arc::ptr_eq(current, sender)) {
            return Err(CANCELED.into());
        }
        if *sender.borrow() {
            *pending = None;
            return Err(CANCELED.into());
        }
        *pending = ready.map(PendingUpdate::Ready);
        Ok(())
    }

    fn cancel(&self) {
        if let Ok(mut pending) = self.pending.lock() {
            if let Some(PendingUpdate::Downloading(sender)) = pending.as_ref() {
                sender.send_replace(true);
                *pending = None;
            }
        }
    }
}

fn current_version() -> Result<Version, String> {
    Version::parse(env!("CARGO_PKG_VERSION")).map_err(|error| error.to_string())
}

fn manifest_version(manifest: &InstallerManifest) -> Result<Version, String> {
    Version::parse(manifest.version.trim_start_matches(['v', 'V']))
        .map_err(|error| format!("Invalid update version: {error}"))
}

fn http_client() -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder().timeout(Duration::from_secs(180));
    if let Some(proxy) = dbx_core::update::system_proxy_url() {
        builder = builder.proxy(reqwest::Proxy::all(proxy).map_err(|error| error.to_string())?);
    }
    builder.build().map_err(|error| error.to_string())
}

async fn fetch_manifest(_source: UpdateDownloadSource) -> Result<InstallerManifest, String> {
    let client = http_client()?;
    let response = client
        .get(UPDATE_FEED_URL)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|error| format!("Failed to check updates: {error}"))?;
    let manifest = response.json::<InstallerManifest>().await.map_err(|error| format!("Invalid update manifest: {error}"))?;
    manifest_version(&manifest)?;
    validate_manifest(&manifest)?;
    Ok(manifest)
}

fn validate_manifest(manifest: &InstallerManifest) -> Result<(), String> {
    let url = reqwest::Url::parse(&manifest.url).map_err(|error| format!("Invalid installer URL: {error}"))?;
    if url.scheme() != "http"
        || url.host_str() != Some("111.230.247.111")
        || !url.path().starts_with("/dbx/")
        || !url.path().to_ascii_lowercase().ends_with(".exe")
    {
        return Err("Update installer must be an .exe URL on the DBX update server.".into());
    }
    if !manifest.sha256.is_empty()
        && (manifest.sha256.len() != 64 || !manifest.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()))
    {
        return Err("Update manifest must include a valid SHA-256 hash.".into());
    }
    Ok(())
}

fn is_manual_update_only() -> bool {
    !cfg!(target_os = "windows")
        || cfg!(target_vendor = "win7")
        || cfg!(target_arch = "aarch64")
        || crate::data_dir::is_portable_mode()
}

#[tauri::command]
pub async fn check_for_updates(
    _locale: Option<String>,
    source: Option<dbx_core::DownloadSource>,
) -> Result<UpdateInfo, String> {
    let source = match source.unwrap_or_default() {
        dbx_core::DownloadSource::Official => UpdateDownloadSource::Official,
        dbx_core::DownloadSource::Cnb => UpdateDownloadSource::Cnb,
    };
    let manifest = fetch_manifest(source).await?;
    let latest = manifest_version(&manifest)?;
    let current = current_version()?;
    let release_url = if cfg!(target_os = "windows") {
        manifest.url.clone()
    } else {
        format!("https://github.com/t8y2/dbx/releases/tag/v{latest}")
    };
    Ok(UpdateInfo {
        current_version: current.to_string(),
        latest_version: latest.to_string(),
        update_available: latest > current,
        portable_mode: crate::data_dir::is_portable_mode(),
        manual_update_only: is_manual_update_only(),
        release_name: format!("DBX v{latest}"),
        release_url,
        release_notes: manifest.notes,
    })
}

#[tauri::command]
pub async fn fetch_changelog(lang: Option<String>) -> Result<dbx_core::changelog::ChangelogData, String> {
    dbx_core::changelog::fetch_changelog(lang.as_deref().unwrap_or("en")).await
}

#[tauri::command]
pub async fn get_system_proxy_url() -> Option<String> {
    tauri::async_runtime::spawn_blocking(dbx_core::update::system_proxy_url).await.ok().flatten()
}

#[tauri::command]
pub fn cancel_update_download(state: tauri::State<'_, PendingUpdateState>) {
    state.cancel();
}

#[tauri::command]
pub async fn download_update(
    app: AppHandle,
    state: tauri::State<'_, PendingUpdateState>,
    source: UpdateDownloadSource,
    latest_version: Option<String>,
) -> Result<(), String> {
    if is_manual_update_only() {
        return Err("This build must be updated with a manual installer.".into());
    }
    let sender = state.begin()?;
    let result = download_installer(&app, source, latest_version.as_deref(), &sender).await;
    match result {
        Ok(ready) => state.finish(&sender, Some(ready)),
        Err(error) => {
            let _ = state.finish(&sender, None);
            Err(error)
        }
    }
}

async fn download_installer(
    app: &AppHandle,
    source: UpdateDownloadSource,
    requested_version: Option<&str>,
    sender: &tokio::sync::watch::Sender<bool>,
) -> Result<ReadyInstaller, String> {
    let manifest = fetch_manifest(source).await?;
    let version = manifest_version(&manifest)?;
    if version <= current_version()? {
        return Err("No update available.".into());
    }
    if requested_version.is_some_and(|requested| requested.trim_start_matches(['v', 'V']) != version.to_string()) {
        return Err("The available update changed; check for updates again.".into());
    }
    if *sender.borrow() {
        return Err(CANCELED.into());
    }
    let client = http_client()?;
    let bytes = download_installer_bytes(app, &client, &manifest.url, &manifest.sha256, sender).await?;
    let cache = app.path().app_cache_dir().map_err(|error| error.to_string())?.join("updates");
    std::fs::create_dir_all(&cache).map_err(|error| error.to_string())?;
    let path = cache.join(format!("DBX_{version}_x64-setup.exe"));
    std::fs::write(&path, bytes).map_err(|error| format!("Failed to save installer: {error}"))?;
    Ok(ReadyInstaller { path, version, silent_args: manifest.silent_args })
}

async fn download_installer_bytes(
    app: &AppHandle,
    client: &reqwest::Client,
    url: &str,
    expected_hash: &str,
    sender: &tokio::sync::watch::Sender<bool>,
) -> Result<Vec<u8>, String> {
    let mut response = client
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|error| format!("Failed to download installer: {error}"))?;
    let total = response.content_length();
    if total.is_some_and(|size| size > MAX_INSTALLER_BYTES as u64) {
        return Err("Installer is too large.".into());
    }
    let mut bytes = Vec::with_capacity(total.unwrap_or(0).min(MAX_INSTALLER_BYTES as u64) as usize);
    let _ = app.emit("update-download-progress", DownloadProgress { downloaded: 0, total });
    let mut last_percent = None;
    loop {
        if *sender.borrow() {
            return Err(CANCELED.into());
        }
        let chunk = tokio::select! {
            _ = wait_for_cancel(sender.subscribe()) => return Err(CANCELED.into()),
            result = response.chunk() => result.map_err(|error| format!("Failed to read installer: {error}"))?,
        };
        let Some(chunk) = chunk else { break };
        if bytes.len().saturating_add(chunk.len()) > MAX_INSTALLER_BYTES {
            return Err("Installer is too large.".into());
        }
        bytes.extend_from_slice(&chunk);
        let percent = total.filter(|total| *total > 0).map(|total| bytes.len() as u64 * 100 / total);
        if percent != last_percent {
            let _ = app.emit("update-download-progress", DownloadProgress { downloaded: bytes.len() as u64, total });
            last_percent = percent;
        }
    }
    if bytes.len() < 64 || !bytes.starts_with(b"MZ") {
        return Err("Downloaded file is not a Windows installer.".into());
    }
    let actual_hash = format!("{:x}", Sha256::digest(&bytes));
    if !expected_hash.is_empty() && actual_hash != expected_hash.to_ascii_lowercase() {
        return Err("Installer SHA-256 verification failed.".into());
    }
    if *sender.borrow() {
        return Err(CANCELED.into());
    }
    let _ = app.emit(
        "update-download-progress",
        DownloadProgress { downloaded: bytes.len() as u64, total: Some(bytes.len() as u64) },
    );
    Ok(bytes)
}

async fn wait_for_cancel(mut receiver: tokio::sync::watch::Receiver<bool>) {
    if *receiver.borrow() {
        return;
    }
    while receiver.changed().await.is_ok() {
        if *receiver.borrow_and_update() {
            return;
        }
    }
}

#[tauri::command]
pub fn install_downloaded_update(app: AppHandle, state: tauri::State<'_, PendingUpdateState>) -> Result<(), String> {
    let mut pending = state.pending.lock().map_err(|_| "Update state unavailable".to_string())?;
    let Some(PendingUpdate::Ready(ready)) = pending.as_ref() else {
        return Err("No downloaded update is ready.".into());
    };
    if ready.version <= current_version()? {
        return Err("Downloaded installer is not newer than this app.".into());
    }
    launch_installer_after_exit(&app, &ready.path, ready.silent_args.as_deref())?;
    *pending = Some(PendingUpdate::Installing);
    drop(pending);
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        if let Some(state) = app.try_state::<crate::CloseBehaviorState>() {
            state.allow_next_exit();
        }
        app.exit(0);
    });
    Ok(())
}

#[cfg(windows)]
fn launch_installer_after_exit(app: &AppHandle, installer: &std::path::Path, silent_args: Option<&str>) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let script = installer.with_extension("ps1");
    std::fs::write(&script, include_str!("update_installer.ps1"))
        .map_err(|error| format!("Failed to write update helper: {error}"))?;
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let mut command = std::process::Command::new("powershell.exe");
    command
        .args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&script)
        .arg("-OldPid")
        .arg(std::process::id().to_string())
        .arg("-Installer")
        .arg(installer)
        .arg("-AppExe")
        .arg(exe)
        .arg("-SilentArgs")
        .arg(silent_args.unwrap_or("/S /UPDATE /R"))
        .creation_flags(0x0800_0000 | 0x0000_0008)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let _ = app;
    command.spawn().map_err(|error| format!("Failed to start update helper: {error}"))?;
    Ok(())
}

#[cfg(not(windows))]
fn launch_installer_after_exit(_app: &AppHandle, _installer: &std::path::Path, _silent_args: Option<&str>) -> Result<(), String> {
    Err("Automatic installer updates are only available on Windows.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> InstallerManifest {
        InstallerManifest {
            version: "0.6.1".into(),
            notes: "Update".into(),
            url: "http://111.230.247.111/dbx/DBX_0.6.1_x64-setup.exe".into(),
            sha256: "a".repeat(64),
            silent_args: Some("/S /UPDATE /R".into()),
        }
    }

    #[test]
    fn requires_installer_from_dbx_server_and_valid_optional_sha256() {
        assert!(validate_manifest(&manifest()).is_ok());
        let mut invalid = manifest();
        invalid.url = invalid.url.replace("/dbx/", "/rebased/");
        assert!(validate_manifest(&invalid).is_err());
        invalid = manifest();
        invalid.sha256 = "invalid".into();
        assert!(validate_manifest(&invalid).is_err());
        invalid.sha256.clear();
        assert!(validate_manifest(&invalid).is_ok());
    }

    #[test]
    fn uses_rebased_style_single_update_feed() {
        assert_eq!(UPDATE_FEED_URL, "http://111.230.247.111/dbx/latest.json");
        assert!(validate_manifest(&manifest()).is_ok());
    }
}
