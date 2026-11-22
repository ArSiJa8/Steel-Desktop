mod config_editor;

use config_editor::{ConfigSettingsResponse, UpdateSettingPayload};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use tauri::Emitter;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

const OS_OPTIONS: [&str; 4] = ["Linux", "Linux - Musl", "MacOS", "Windows"];
const VERSION_FILE_NAME: &str = ".steel-version";
const RELEASES_API_URL: &str =
    "https://api.github.com/repos/Steel-Foundation/SteelMC/releases?per_page=1";
const LATEST_DOWNLOAD_BASE: &str =
    "https://github.com/Steel-Foundation/SteelMC/releases/latest/download";

#[derive(Default)]
struct AppState {
    folder_path: String,
    os: String,
    correct_binary_installed: bool,
    server_process: Option<Child>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OsConfig {
    options: Vec<&'static str>,
    default_os: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SelectResult {
    correct_binary_installed: bool,
    show_install_prompt: bool,
    status_message: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DownloadProgress {
    downloaded: u64,
    total: Option<u64>,
    percentage: Option<u8>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VersionInfo {
    installed_version: String,
    latest_version: Option<String>,
    is_newest: bool,
    show_update: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ServerStatus {
    is_running: bool,
    all_files_generated: bool,
}

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

fn binary_for_os(os: &str) -> Option<&'static str> {
    match os {
        "Linux" => Some("steel-linux"),
        "Linux - Musl" => Some("steel-linux-musl"),
        "MacOS" => Some("steel-macos"),
        "Windows" => Some("steel-windows.exe"),
        _ => None,
    }
}

fn detect_default_os() -> &'static str {
    match std::env::consts::OS {
        "linux" => "Linux",
        "macos" => "MacOS",
        "windows" => "Windows",
        _ => "Windows",
    }
}

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
fn get_os_config() -> OsConfig {
    OsConfig {
        options: OS_OPTIONS.to_vec(),
        default_os: detect_default_os(),
    }
}

#[tauri::command]
async fn pick_folder(app: tauri::AppHandle) -> Option<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    app.dialog()
        .file()
        .set_title("Select Folder")
        .pick_folder(move |folder| {
            let _ = tx.send(folder.map(|f| f.to_string()));
        });

    tauri::async_runtime::spawn_blocking(move || rx.recv().ok().flatten())
        .await
        .ok()
        .flatten()
}

#[tauri::command]
fn select_folder(
    state: tauri::State<'_, Mutex<AppState>>,
    folder_path: String,
    os: String,
) -> SelectResult {
    let trimmed = folder_path.trim();
    if trimmed.is_empty() {
        if let Ok(mut app_state) = state.lock() {
            app_state.folder_path.clear();
            app_state.os = os;
            app_state.correct_binary_installed = false;
        }
        return SelectResult {
            correct_binary_installed: false,
            show_install_prompt: false,
            status_message: "Please select a folder first.".to_string(),
        };
    }

    let installed = binary_for_os(&os)
        .map(|bin| Path::new(trimmed).join(bin).is_file())
        .unwrap_or(false);

    if let Ok(mut app_state) = state.lock() {
        app_state.folder_path = trimmed.to_string();
        app_state.os = os;
        app_state.correct_binary_installed = installed;
    }

    let status_message = if installed {
        "Correct binary installed.".to_string()
    } else {
        "Correct binary not found in selected folder.".to_string()
    };

    SelectResult {
        correct_binary_installed: installed,
        show_install_prompt: !installed,
        status_message,
    }
}

async fn fetch_latest_release(client: &reqwest::Client) -> Option<GitHubRelease> {
    let resp = client
        .get(RELEASES_API_URL)
        .header(reqwest::header::USER_AGENT, "steel-desktop")
        .send()
        .await
        .ok()?;

    if !resp.status().is_success() {
        return None;
    }

    resp.json::<Vec<GitHubRelease>>()
        .await
        .ok()?
        .into_iter()
        .next()
}

async fn resolve_download_info(
    client: &reqwest::Client,
    binary_name: &str,
) -> (String, Option<String>) {
    if let Some(release) = fetch_latest_release(client).await {
        let version = Some(release.tag_name);
        if let Some(asset) = release
            .assets
            .into_iter()
            .find(|a| a.name == binary_name)
        {
            return (asset.browser_download_url, version);
        }
        return (format!("{LATEST_DOWNLOAD_BASE}/{binary_name}"), version);
    }

    (format!("{LATEST_DOWNLOAD_BASE}/{binary_name}"), None)
}

#[tauri::command]
async fn get_version_info(state: tauri::State<'_, Mutex<AppState>>) -> Result<VersionInfo, String> {
    let folder_path = {
        let app_state = state
            .lock()
            .map_err(|_| "Failed to access application state.".to_string())?;
        app_state.folder_path.clone()
    };

    let version_path = Path::new(&folder_path).join(VERSION_FILE_NAME);
    let installed_opt = std::fs::read_to_string(&version_path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let client = reqwest::Client::new();
    let latest_version = fetch_latest_release(&client)
        .await
        .map(|r| r.tag_name.trim().to_string())
        .filter(|s| !s.is_empty());

    let (installed_version, is_newest, show_update) = match (&installed_opt, &latest_version) {
        (Some(inst), Some(lat)) => {
            let newest = inst == lat;
            (inst.clone(), newest, !newest)
        }
        (Some(inst), None) => (inst.clone(), false, false),
        (None, _) => ("unknown".to_string(), false, true),
    };

    Ok(VersionInfo {
        installed_version,
        latest_version,
        is_newest,
        show_update,
    })
}

#[tauri::command]
async fn install_binary(
    app: tauri::AppHandle,
    state: tauri::State<'_, Mutex<AppState>>,
) -> Result<SelectResult, String> {
    let (folder_path, os) = {
        let app_state = state
            .lock()
            .map_err(|_| "Failed to access application state.".to_string())?;
        (app_state.folder_path.clone(), app_state.os.clone())
    };

    if folder_path.trim().is_empty() {
        return Err("No folder selected.".to_string());
    }

    let binary_name =
        binary_for_os(&os).ok_or_else(|| format!("Unsupported operating system: {os}"))?;

    let client = reqwest::Client::new();
    let (download_url, release_version) = resolve_download_info(&client, binary_name).await;

    let mut response = client
        .get(&download_url)
        .header(reqwest::header::USER_AGENT, "steel-desktop")
        .send()
        .await
        .map_err(|e| format!("Failed to download binary: {e}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "Download failed with HTTP status {}.",
            response.status()
        ));
    }

    let total = response.content_length();
    let target_path = Path::new(&folder_path).join(binary_name);
    let mut file = std::fs::File::create(&target_path)
        .map_err(|e| format!("Failed to create file {}: {e}", target_path.display()))?;

    let mut downloaded: u64 = 0;
    let mut last_pct: Option<u8> = None;

    let _ = app.emit(
        "download-progress",
        DownloadProgress {
            downloaded: 0,
            total,
            percentage: total.map(|_| 0),
        },
    );

    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("Failed while downloading binary: {e}"))?
    {
        file.write_all(&chunk)
            .map_err(|e| format!("Failed to write binary to {}: {e}", target_path.display()))?;
        downloaded += chunk.len() as u64;

        let percentage = total.and_then(|t| {
            if t > 0 {
                Some(((downloaded as f64 / t as f64) * 100.0).min(100.0) as u8)
            } else {
                None
            }
        });

        if percentage != last_pct || percentage.is_none() {
            last_pct = percentage;
            let _ = app.emit(
                "download-progress",
                DownloadProgress {
                    downloaded,
                    total,
                    percentage,
                },
            );
        }
    }

    let _ = app.emit(
        "download-progress",
        DownloadProgress {
            downloaded,
            total: total.or(Some(downloaded)),
            percentage: Some(100),
        },
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&target_path)
            .map_err(|e| format!("Failed to read permissions: {e}"))?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&target_path, perms)
            .map_err(|e| format!("Failed to set executable permissions: {e}"))?;
    }

    let installed = target_path.is_file();
    if installed {
        if let Some(version) = release_version {
            let version_path = Path::new(&folder_path).join(VERSION_FILE_NAME);
            let _ = std::fs::write(version_path, version.trim());
        }
    }

    if let Ok(mut app_state) = state.lock() {
        app_state.correct_binary_installed = installed;
    }

    Ok(SelectResult {
        correct_binary_installed: installed,
        show_install_prompt: !installed,
        status_message: if installed {
            "Binary installed successfully.".to_string()
        } else {
            "Binary installation could not be verified.".to_string()
        },
    })
}

fn spawn_external_terminal(binary_path: &Path, working_dir: &Path) -> Result<Child, String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_CONSOLE: u32 = 0x00000010;

        return Command::new(binary_path)
            .current_dir(working_dir)
            .creation_flags(CREATE_NEW_CONSOLE)
            .spawn()
            .map_err(|e| format!("Failed to start server in terminal: {e}"));
    }

    #[cfg(target_os = "macos")]
    {
        let escaped_dir = working_dir.display().to_string().replace('"', "\\\"");
        let escaped_bin = binary_path.display().to_string().replace('"', "\\\"");
        let script = format!(
            "tell application \"Terminal\" to do script \"cd \\\"{escaped_dir}\\\" && \\\"{escaped_bin}\\\"\""
        );
        return Command::new("osascript")
            .arg("-e")
            .arg(script)
            .spawn()
            .map_err(|e| format!("Failed to open Terminal: {e}"));
    }

    #[cfg(target_os = "linux")]
    {
        let bin_str = binary_path.to_string_lossy().to_string();
        let terminals = [
            ("x-terminal-emulator", "-e"),
            ("gnome-terminal", "--"),
            ("konsole", "-e"),
            ("xfce4-terminal", "-e"),
            ("alacritty", "-e"),
            ("kitty", "-e"),
            ("xterm", "-e"),
        ];

        for (term, flag) in terminals {
            if let Ok(child) = Command::new(term)
                .arg(flag)
                .arg(&bin_str)
                .current_dir(working_dir)
                .spawn()
            {
                return Ok(child);
            }
        }
        return Err("No supported external terminal emulator found.".to_string());
    }
}

fn are_required_files_present(folder: &Path) -> bool {
    let required_dirs = [".logs", "config", "saves"];
    if !required_dirs.iter().all(|d| folder.join(d).is_dir()) {
        return false;
    }

    let config_dir = folder.join("config");
    let required_config_files = ["config.toml", "favicon.png", "groups.toml", "worlds.toml"];
    required_config_files
        .iter()
        .all(|f| config_dir.join(f).is_file())
}

#[tauri::command]
fn get_server_status(state: tauri::State<'_, Mutex<AppState>>) -> ServerStatus {
    let Ok(mut app_state) = state.lock() else {
        return ServerStatus {
            is_running: false,
            all_files_generated: false,
        };
    };

    let is_running = if let Some(child) = app_state.server_process.as_mut() {
        match child.try_wait() {
            Ok(None) => true,
            _ => {
                app_state.server_process = None;
                false
            }
        }
    } else {
        false
    };

    let folder_path = app_state.folder_path.trim();
    let all_files_generated = if folder_path.is_empty() {
        false
    } else {
        are_required_files_present(Path::new(folder_path))
    };

    ServerStatus {
        is_running,
        all_files_generated,
    }
}

#[tauri::command]
async fn generate_files(state: tauri::State<'_, Mutex<AppState>>) -> Result<bool, String> {
    let (folder_path, os) = {
        let mut app_state = state
            .lock()
            .map_err(|_| "Failed to access application state.".to_string())?;

        if let Some(child) = app_state.server_process.as_mut() {
            if matches!(child.try_wait(), Ok(None)) {
                return Err("Stop the server before generating files.".to_string());
            }
            app_state.server_process = None;
        }

        (app_state.folder_path.trim().to_string(), app_state.os.clone())
    };

    if folder_path.is_empty() {
        return Err("No folder selected.".to_string());
    }

    let binary_name =
        binary_for_os(&os).ok_or_else(|| format!("Unsupported operating system: {os}"))?;

    tauri::async_runtime::spawn_blocking(move || {
        let working_dir = Path::new(&folder_path);
        let binary_path = working_dir.join(binary_name);
        if !binary_path.is_file() {
            return Err(format!("Binary not found: {}", binary_path.display()));
        }

        let mut cmd = Command::new(&binary_path);
        cmd.current_dir(working_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Failed to start binary in background: {e}"))?;

        let (tx, rx) = std::sync::mpsc::channel::<String>();

        if let Some(stdout) = child.stdout.take() {
            let tx_out = tx.clone();
            std::thread::spawn(move || {
                let reader = BufReader::new(stdout);
                for line in reader.lines().map_while(Result::ok) {
                    if tx_out.send(line).is_err() {
                        break;
                    }
                }
            });
        }

        if let Some(stderr) = child.stderr.take() {
            let tx_err = tx.clone();
            std::thread::spawn(move || {
                let reader = BufReader::new(stderr);
                for line in reader.lines().map_while(Result::ok) {
                    if tx_err.send(line).is_err() {
                        break;
                    }
                }
            });
        }

        drop(tx);

        let mut started = false;
        for line in rx {
            if line.contains("Started Steel Server") {
                started = true;
                break;
            }
        }

        let _ = child.kill();
        let _ = child.wait();

        if !started && !are_required_files_present(working_dir) {
            return Err("Server exited before 'Started Steel Server' was logged.".to_string());
        }

        Ok(are_required_files_present(working_dir))
    })
    .await
    .map_err(|e| format!("Background task failed: {e}"))?
}

#[tauri::command]
fn start_server(state: tauri::State<'_, Mutex<AppState>>) -> Result<bool, String> {
    let mut app_state = state
        .lock()
        .map_err(|_| "Failed to access application state.".to_string())?;

    if let Some(child) = app_state.server_process.as_mut() {
        if matches!(child.try_wait(), Ok(None)) {
            return Ok(true);
        }
        app_state.server_process = None;
    }

    let folder_path = app_state.folder_path.trim().to_string();
    if folder_path.is_empty() {
        return Err("No folder selected.".to_string());
    }

    let binary_name = binary_for_os(&app_state.os)
        .ok_or_else(|| format!("Unsupported operating system: {}", app_state.os))?;

    let working_dir = Path::new(&folder_path);
    let binary_path = working_dir.join(binary_name);
    if !binary_path.is_file() {
        return Err(format!("Binary not found: {}", binary_path.display()));
    }

    let child = spawn_external_terminal(&binary_path, working_dir)?;
    app_state.server_process = Some(child);
    Ok(true)
}

#[tauri::command]
fn stop_server(state: tauri::State<'_, Mutex<AppState>>) -> Result<bool, String> {
    let mut app_state = state
        .lock()
        .map_err(|_| "Failed to access application state.".to_string())?;

    if let Some(mut child) = app_state.server_process.take() {
        match child.try_wait() {
            Ok(Some(_)) => {}
            _ => {
                child
                    .kill()
                    .map_err(|e| format!("Failed to stop server: {e}"))?;
                let _ = child.wait();
            }
        }
    }

    Ok(false)
}

#[tauri::command]
fn open_groups_file(
    app: tauri::AppHandle,
    state: tauri::State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    let folder_path = {
        let app_state = state
            .lock()
            .map_err(|_| "Failed to access application state.".to_string())?;
        app_state.folder_path.trim().to_string()
    };

    if folder_path.is_empty() {
        return Err("No folder selected.".to_string());
    }

    let groups_path = Path::new(&folder_path).join("config").join("groups.toml");
    if !groups_path.is_file() {
        return Err("groups.toml not found. Please generate server files first.".to_string());
    }

    app.opener()
        .open_path(groups_path.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| format!("Failed to open groups.toml: {e}"))
}

#[tauri::command]
fn get_config_settings(
    state: tauri::State<'_, Mutex<AppState>>,
) -> Result<ConfigSettingsResponse, String> {
    let folder_path = {
        let app_state = state
            .lock()
            .map_err(|_| "Failed to access application state.".to_string())?;
        app_state.folder_path.trim().to_string()
    };

    if folder_path.is_empty() {
        return Err("No folder selected.".to_string());
    }

    config_editor::load_all_settings(Path::new(&folder_path))
}

#[tauri::command]
fn update_config_setting(
    state: tauri::State<'_, Mutex<AppState>>,
    payload: UpdateSettingPayload,
) -> Result<(), String> {
    let folder_path = {
        let app_state = state
            .lock()
            .map_err(|_| "Failed to access application state.".to_string())?;
        app_state.folder_path.trim().to_string()
    };

    if folder_path.is_empty() {
        return Err("No folder selected.".to_string());
    }

    config_editor::save_setting(Path::new(&folder_path), payload)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(Mutex::new(AppState::default()))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            greet,
            get_os_config,
            pick_folder,
            select_folder,
            get_version_info,
            install_binary,
            get_server_status,
            generate_files,
            start_server,
            stop_server,
            open_groups_file,
            get_config_settings,
            update_config_setting
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
