// RasFocus - Tauri Rust Backend
// index.html is embedded directly into this binary via include_str!

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, CustomMenuItem, Manager, State, SystemTray, SystemTrayMenu};
use serde::{Deserialize, Serialize};

// ==========================================
// --- EMBEDDED FRONTEND ---
// ==========================================
const INDEX_HTML: &str = include_str!("index.html");

// ==========================================
// --- SHARED STATE ---
// ==========================================
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdultSettings {
    pub cb_adult_web: bool,
    pub cb_fb_reels: bool,
    pub cb_hardcore: bool,
    pub cb_romantic: bool,
    pub control_mode: i32,
    pub adult_religion: i32,
    pub adult_language: i32,
    pub total_blocked_count: i32,
    pub cb_periodic_popups: bool,
    pub cb_24h_lock: bool,
    pub lock_24h_end_ms: u64,
    pub is_adult_focus_active: bool,
    pub focus_end_ms: u64,
    pub custom_keywords: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrictSettings {
    pub cb_silent_url: bool,
    pub cb_dns_filter: bool,
    pub cb_safe_search: bool,
    pub cb_incognito: bool,
    pub cb_strict_mode: bool,
    pub is_strict_focus_active: bool,
    pub strict_focus_end_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppState {
    pub adult: AdultSettings,
    pub strict: StrictSettings,
}

impl Default for AdultSettings {
    fn default() -> Self {
        AdultSettings {
            cb_adult_web: true,
            cb_fb_reels: true,
            cb_hardcore: true,
            cb_romantic: true,
            control_mode: 0,
            adult_religion: 0,
            adult_language: 0,
            total_blocked_count: 0,
            cb_periodic_popups: false,
            cb_24h_lock: false,
            lock_24h_end_ms: 0,
            is_adult_focus_active: false,
            focus_end_ms: 0,
            custom_keywords: vec![],
        }
    }
}

impl Default for StrictSettings {
    fn default() -> Self {
        StrictSettings {
            cb_silent_url: true,
            cb_dns_filter: false,
            cb_safe_search: true,
            cb_incognito: true,
            cb_strict_mode: false,
            is_strict_focus_active: false,
            strict_focus_end_ms: 0,
        }
    }
}

pub struct SharedState(Arc<Mutex<AppState>>);

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// ==========================================
// --- FILE PATHS ---
// ==========================================
fn get_data_dir() -> PathBuf {
    let dir = PathBuf::from("C:\\ProgramData\\RasFocus");
    let _ = fs::create_dir_all(&dir);
    dir
}

fn adult_settings_path() -> PathBuf { get_data_dir().join("rf_sys_data.json") }
fn strict_settings_path() -> PathBuf { get_data_dir().join("rf_strict_data.json") }
fn silent_log_path() -> PathBuf { get_data_dir().join("rf_silent_log.txt") }

// ==========================================
// --- SAVE / LOAD ---
// ==========================================
fn save_adult(settings: &AdultSettings) {
    if let Ok(json) = serde_json::to_string_pretty(settings) {
        let _ = fs::write(adult_settings_path(), json);
    }
}

fn load_adult() -> AdultSettings {
    let path = adult_settings_path();
    if let Ok(content) = fs::read_to_string(&path) {
        if let Ok(mut s) = serde_json::from_str::<AdultSettings>(&content) {
            let now = now_ms();
            if s.cb_24h_lock && now >= s.lock_24h_end_ms {
                s.cb_24h_lock = false;
                s.is_adult_focus_active = false;
            } else if s.cb_24h_lock {
                s.is_adult_focus_active = true;
            }
            if s.is_adult_focus_active && s.control_mode == 0 && now >= s.focus_end_ms && !s.cb_24h_lock {
                s.is_adult_focus_active = false;
            }
            return s;
        }
    }
    AdultSettings::default()
}

fn save_strict(settings: &StrictSettings) {
    if let Ok(json) = serde_json::to_string_pretty(settings) {
        let _ = fs::write(strict_settings_path(), json);
    }
}

fn load_strict() -> StrictSettings {
    let path = strict_settings_path();
    if let Ok(content) = fs::read_to_string(&path) {
        if let Ok(mut s) = serde_json::from_str::<StrictSettings>(&content) {
            let now = now_ms();
            if s.is_strict_focus_active && now >= s.strict_focus_end_ms {
                s.is_strict_focus_active = false;
            }
            return s;
        }
    }
    StrictSettings::default()
}

// ==========================================
// --- STRICT PROTOCOLS: HOSTS FILE ---
// ==========================================
#[cfg(target_os = "windows")]
fn enforce_strict_protocols(strict: &StrictSettings) {
    let hosts_path = "C:\\Windows\\System32\\drivers\\etc\\hosts";
    let temp_path = "C:\\Windows\\System32\\drivers\\etc\\hosts.rasfocus.tmp";

    let mut clean_lines: Vec<String> = Vec::new();
    if let Ok(file) = fs::File::open(hosts_path) {
        let reader = BufReader::new(file);
        let mut skip = false;
        for line in reader.lines().flatten() {
            if line.contains("# RasFocus Strict Start") { skip = true; }
            if !skip { clean_lines.push(line); }
            if line.contains("# RasFocus Strict End") { skip = false; }
        }
    }

    if let Ok(mut f) = fs::File::create(temp_path) {
        for line in &clean_lines { let _ = writeln!(f, "{}", line); }

        if strict.cb_dns_filter || strict.cb_safe_search {
            let _ = writeln!(f, "\n# RasFocus Strict Start");
            if strict.cb_safe_search {
                let _ = writeln!(f, "216.239.38.120 google.com");
                let _ = writeln!(f, "216.239.38.120 www.google.com");
                let _ = writeln!(f, "204.79.197.220 bing.com");
                let _ = writeln!(f, "204.79.197.220 www.bing.com");
                let _ = writeln!(f, "211.73.64.227 youtube.com");
                let _ = writeln!(f, "211.73.64.227 www.youtube.com");
            }
            if strict.cb_dns_filter {
                for site in &["pornhub.com","xvideos.com","xnxx.com","xhamster.com","redtube.com"] {
                    let _ = writeln!(f, "127.0.0.1 {}", site);
                    let _ = writeln!(f, "127.0.0.1 www.{}", site);
                }
            }
            let _ = writeln!(f, "# RasFocus Strict End");
        }
    }

    let _ = Command::new("cmd")
        .args(["/C", &format!("copy /Y \"{}\" \"{}\"", temp_path, hosts_path)])
        .output();
    let _ = fs::remove_file(temp_path);
    let _ = Command::new("ipconfig").arg("/flushdns").output();
}

#[cfg(not(target_os = "windows"))]
fn enforce_strict_protocols(_strict: &StrictSettings) {}

// ==========================================
// --- DNS FILTER ---
// ==========================================
#[cfg(target_os = "windows")]
fn set_family_dns(enable: bool) {
    let args = if enable {
        "/C wmic nicconfig where (IPEnabled=TRUE) call SetDNSServerSearchOrder (\"1.1.1.3\", \"1.0.0.3\")"
    } else {
        "/C wmic nicconfig where (IPEnabled=TRUE) call SetDNSServerSearchOrder ()"
    };
    let _ = Command::new("cmd").args(["/C", args]).output();
    let _ = Command::new("ipconfig").arg("/flushdns").output();
}

#[cfg(not(target_os = "windows"))]
fn set_family_dns(_enable: bool) {}

// ==========================================
// --- SAFE SEARCH REGISTRY ---
// ==========================================
#[cfg(target_os = "windows")]
fn toggle_safe_search_registry(enable: bool) {
    let policies = [
        (r"HKLM\SOFTWARE\Policies\Google\Chrome", "ForceGoogleSafeSearch", "1"),
        (r"HKLM\SOFTWARE\Policies\Google\Chrome", "ForceYouTubeRestrict", "2"),
        (r"HKLM\SOFTWARE\Policies\Microsoft\Edge", "ForceGoogleSafeSearch", "1"),
        (r"HKLM\SOFTWARE\Policies\Microsoft\Edge", "ForceBingSafeSearch", "1"),
        (r"HKLM\SOFTWARE\Policies\Microsoft\Edge", "ForceYouTubeRestrict", "2"),
        (r"HKLM\SOFTWARE\Policies\BraveSoftware\Brave", "ForceGoogleSafeSearch", "1"),
    ];

    for (key, name, val) in &policies {
        if enable {
            let _ = Command::new("reg")
                .args(["add", key, "/v", name, "/t", "REG_DWORD", "/d", val, "/f"])
                .output();
        } else {
            let _ = Command::new("reg")
                .args(["delete", key, "/v", name, "/f"])
                .output();
        }
    }
    let _ = Command::new("ipconfig").arg("/flushdns").output();
}

#[cfg(not(target_os = "windows"))]
fn toggle_safe_search_registry(_enable: bool) {}

// ==========================================
// --- TAURI COMMANDS ---
// ==========================================

#[tauri::command]
fn get_state(state: State<SharedState>) -> AppState {
    state.0.lock().unwrap().clone()
}

#[tauri::command]
fn save_adult_settings(settings: AdultSettings, state: State<SharedState>) -> Result<(), String> {
    let mut lock = state.0.lock().unwrap();
    lock.adult = settings.clone();
    save_adult(&settings);
    Ok(())
}

#[tauri::command]
fn save_strict_settings(settings: StrictSettings, state: State<SharedState>) -> Result<(), String> {
    enforce_strict_protocols(&settings);
    let mut lock = state.0.lock().unwrap();
    lock.strict = settings.clone();
    save_strict(&settings);
    Ok(())
}

#[tauri::command]
fn toggle_dns_filter(enable: bool, state: State<SharedState>) -> Result<(), String> {
    set_family_dns(enable);
    let mut lock = state.0.lock().unwrap();
    lock.strict.cb_dns_filter = enable;
    let strict_clone = lock.strict.clone();
    enforce_strict_protocols(&strict_clone);
    save_strict(&strict_clone);
    Ok(())
}

#[tauri::command]
fn toggle_safe_search(enable: bool, state: State<SharedState>) -> Result<(), String> {
    toggle_safe_search_registry(enable);
    let mut lock = state.0.lock().unwrap();
    lock.strict.cb_safe_search = enable;
    let strict_clone = lock.strict.clone();
    enforce_strict_protocols(&strict_clone);
    save_strict(&strict_clone);
    Ok(())
}

#[tauri::command]
fn start_focus(hours: u64, mins: u64, is_strict: bool, state: State<SharedState>) -> Result<(), String> {
    let duration_ms = (hours * 3600 + mins * 60) * 1000;
    let end_ms = now_ms() + duration_ms;
    let mut lock = state.0.lock().unwrap();
    if is_strict {
        lock.strict.is_strict_focus_active = true;
        lock.strict.strict_focus_end_ms = end_ms;
        let s = lock.strict.clone();
        save_strict(&s);
    } else {
        lock.adult.is_adult_focus_active = true;
        lock.adult.focus_end_ms = end_ms;
        let s = lock.adult.clone();
        save_adult(&s);
    }
    Ok(())
}

#[tauri::command]
fn stop_focus(is_strict: bool, state: State<SharedState>) -> Result<(), String> {
    let mut lock = state.0.lock().unwrap();
    if is_strict {
        lock.strict.is_strict_focus_active = false;
        let s = lock.strict.clone();
        save_strict(&s);
    } else {
        if !lock.adult.cb_24h_lock {
            lock.adult.is_adult_focus_active = false;
            let s = lock.adult.clone();
            save_adult(&s);
        }
    }
    Ok(())
}

#[tauri::command]
fn start_24h_lock(state: State<SharedState>) -> Result<(), String> {
    let mut lock = state.0.lock().unwrap();
    lock.adult.cb_24h_lock = true;
    lock.adult.is_adult_focus_active = true;
    lock.adult.lock_24h_end_ms = now_ms() + 86_400_000;
    let s = lock.adult.clone();
    save_adult(&s);
    Ok(())
}

#[tauri::command]
fn add_custom_keyword(keyword: String, state: State<SharedState>) -> Result<(), String> {
    let mut lock = state.0.lock().unwrap();
    if !keyword.is_empty() && !lock.adult.custom_keywords.contains(&keyword) {
        lock.adult.custom_keywords.push(keyword);
        let s = lock.adult.clone();
        save_adult(&s);
    }
    Ok(())
}

#[tauri::command]
fn remove_custom_keyword(keyword: String, state: State<SharedState>) -> Result<(), String> {
    let mut lock = state.0.lock().unwrap();
    if !lock.adult.is_adult_focus_active {
        lock.adult.custom_keywords.retain(|k| k != &keyword);
        let s = lock.adult.clone();
        save_adult(&s);
    }
    Ok(())
}

#[tauri::command]
fn get_blocked_count(state: State<SharedState>) -> i32 {
    state.0.lock().unwrap().adult.total_blocked_count
}

#[tauri::command]
fn get_silent_log() -> Vec<String> {
    let path = silent_log_path();
    if let Ok(content) = fs::read_to_string(path) {
        content.lines().rev().take(100).map(String::from).collect()
    } else {
        vec![]
    }
}

#[tauri::command]
fn kill_browsers() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let _ = Command::new("taskkill").args(["/F", "/IM", "chrome.exe", "/T"]).output();
        let _ = Command::new("taskkill").args(["/F", "/IM", "msedge.exe", "/T"]).output();
        let _ = Command::new("taskkill").args(["/F", "/IM", "brave.exe", "/T"]).output();
    }
    Ok(())
}

#[tauri::command]
fn get_now_ms() -> u64 {
    now_ms()
}

// ==========================================
// --- MAIN ---
// ==========================================
fn main() {
    let adult = load_adult();
    let strict = load_strict();
    let shared = SharedState(Arc::new(Mutex::new(AppState { adult, strict })));

    // Background timer thread
    let state_arc = shared.0.clone();
    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_secs(5));
            let mut lock = state_arc.lock().unwrap();
            let now = now_ms();

            if lock.adult.cb_24h_lock && now >= lock.adult.lock_24h_end_ms {
                lock.adult.cb_24h_lock = false;
                lock.adult.is_adult_focus_active = false;
                let s = lock.adult.clone();
                drop(lock);
                save_adult(&s);
                continue;
            }

            if lock.adult.is_adult_focus_active && lock.adult.control_mode == 0
                && now >= lock.adult.focus_end_ms && !lock.adult.cb_24h_lock
            {
                lock.adult.is_adult_focus_active = false;
                let s = lock.adult.clone();
                drop(lock);
                save_adult(&s);
                continue;
            }

            if lock.strict.is_strict_focus_active && now >= lock.strict.strict_focus_end_ms {
                lock.strict.is_strict_focus_active = false;
                let s = lock.strict.clone();
                drop(lock);
                save_strict(&s);
                continue;
            }
        }
    });

    tauri::Builder::default()
        .manage(shared)
        .invoke_handler(tauri::generate_handler![
            get_state,
            save_adult_settings,
            save_strict_settings,
            toggle_dns_filter,
            toggle_safe_search,
            start_focus,
            stop_focus,
            start_24h_lock,
            add_custom_keyword,
            remove_custom_keyword,
            get_blocked_count,
            get_silent_log,
            kill_browsers,
            get_now_ms,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
