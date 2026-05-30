// RasFocus - Tauri Rust Backend
// Cross-platform: Windows + macOS + Linux
// v3.0 — C++-level blocking with WH_KEYBOARD_LL hook (Windows)

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::State;
use serde::{Deserialize, Serialize};

// ==========================================
// --- KEYWORD DATABASES ---
// ==========================================
const HARDCORE_KEYWORDS: &[&str] = &[
    "porn", "xxx", "sex", "nude", "nsfw", "hentai", "milf", "blowjob",
    "xvideos", "pornhub", "xnxx", "xhamster", "brazzers", "onlyfans",
    "chaturbate", "spankbang", "redtube", "youporn",
    "চটি", "পর্ন", "সেক্স", "নগ্ন",
    "bhabi", "chudai", "bangla choti", "panu", "magi", "choda", "randi",
];

const ROMANTIC_KEYWORDS: &[&str] = &[
    "hot dance", "seductive", "item song", "belly dance",
    "kissing scene", "bikini", "sexy dance", "cleavage",
    "semi nude", "lingerie", "erotic", "navel show",
];

const DEFAULT_ADULT_SITES: &[&str] = &[
    "pornhub.com", "xvideos.com", "xnxx.com", "xhamster.com", "redtube.com",
    "youporn.com", "brazzers.com", "onlyfans.com", "chaturbate.com", "spankbang.com",
];

const REELS_PATTERNS: &[&str] = &[
    "facebook.com/reel",
    "instagram.com/reels",
    "youtube.com/shorts",
];

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
    // v3: keyboard hook stats
    pub keyboard_hook_active: bool,
    pub keyboard_blocked_count: i32,
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
            cb_adult_web: true, cb_fb_reels: true, cb_hardcore: true, cb_romantic: true,
            control_mode: 0, adult_religion: 0, adult_language: 0, total_blocked_count: 0,
            cb_periodic_popups: false, cb_24h_lock: false, lock_24h_end_ms: 0,
            is_adult_focus_active: false, focus_end_ms: 0, custom_keywords: vec![],
            keyboard_hook_active: false, keyboard_blocked_count: 0,
        }
    }
}

impl Default for StrictSettings {
    fn default() -> Self {
        StrictSettings {
            cb_silent_url: true, cb_dns_filter: false, cb_safe_search: true,
            cb_incognito: true, cb_strict_mode: false,
            is_strict_focus_active: false, strict_focus_end_ms: 0,
        }
    }
}

pub struct SharedState(Arc<Mutex<AppState>>);

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

// ==========================================
// --- FILE PATHS (cross-platform) ---
// ==========================================
fn get_data_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    let dir = PathBuf::from("C:\\ProgramData\\RasFocus");

    #[cfg(target_os = "macos")]
    let dir = {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        PathBuf::from(home).join("Library").join("Application Support").join("RasFocus")
    };

    #[cfg(target_os = "linux")]
    let dir = {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        PathBuf::from(home).join(".config").join("RasFocus")
    };

    let _ = fs::create_dir_all(&dir);
    dir
}

fn adult_settings_path() -> PathBuf { get_data_dir().join("rf_sys_data.json") }
fn strict_settings_path() -> PathBuf { get_data_dir().join("rf_strict_data.json") }
fn silent_log_path() -> PathBuf { get_data_dir().join("rf_silent_log.txt") }
fn keyboard_log_path() -> PathBuf { get_data_dir().join("rf_keyboard_blocked.txt") }

// ==========================================
// --- SAVE / LOAD ---
// ==========================================
fn save_adult(settings: &AdultSettings) {
    if let Ok(json) = serde_json::to_string_pretty(settings) {
        let _ = fs::write(adult_settings_path(), json);
    }
}

fn load_adult() -> AdultSettings {
    if let Ok(content) = fs::read_to_string(adult_settings_path()) {
        if let Ok(mut s) = serde_json::from_str::<AdultSettings>(&content) {
            let now = now_ms();
            if s.cb_24h_lock && now >= s.lock_24h_end_ms {
                s.cb_24h_lock = false; s.is_adult_focus_active = false;
            } else if s.cb_24h_lock {
                s.is_adult_focus_active = true;
            }
            if s.is_adult_focus_active && s.control_mode == 0
                && s.focus_end_ms > 0 && now >= s.focus_end_ms && !s.cb_24h_lock {
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
    if let Ok(content) = fs::read_to_string(strict_settings_path()) {
        if let Ok(mut s) = serde_json::from_str::<StrictSettings>(&content) {
            if s.is_strict_focus_active && now_ms() >= s.strict_focus_end_ms {
                s.is_strict_focus_active = false;
            }
            return s;
        }
    }
    StrictSettings::default()
}

// ==========================================
// --- SILENT URL LOGGER ---
// ==========================================
fn log_silent_url(window_title: &str, url: &str) {
    if url.is_empty() { return; }
    let now = chrono::Local::now();
    let time_str = now.format("%Y-%m-%d %I:%M:%S %p").to_string();
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(silent_log_path()) {
        let _ = writeln!(f, "[{}] TITLE: {} | URL: {}", time_str, window_title, url);
    }
}

fn log_keyboard_block(keyword: &str, context: &str) {
    let now = chrono::Local::now();
    let time_str = now.format("%Y-%m-%d %I:%M:%S %p").to_string();
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(keyboard_log_path()) {
        let _ = writeln!(f, "[{}] KEYBOARD BLOCKED — keyword: '{}' | context: '{}'", time_str, keyword, context);
    }
}

// ==========================================
// --- HOSTS FILE HELPERS ---
// ==========================================
fn get_hosts_path() -> &'static str {
    #[cfg(target_os = "windows")]
    return "C:\\Windows\\System32\\drivers\\etc\\hosts";
    #[cfg(not(target_os = "windows"))]
    return "/etc/hosts";
}

fn build_hosts_content(clean_lines: &[String], strict: &StrictSettings) -> String {
    let mut content = clean_lines.join("\n");
    content.push('\n');
    if strict.cb_dns_filter || strict.cb_safe_search {
        content.push_str("\n# RasFocus Strict Start\n");
        if strict.cb_safe_search {
            content.push_str("216.239.38.120 google.com\n");
            content.push_str("216.239.38.120 www.google.com\n");
            content.push_str("216.239.38.120 google.com.bd\n");
            content.push_str("216.239.38.120 www.google.com.bd\n");
            content.push_str("204.79.197.220 bing.com\n");
            content.push_str("204.79.197.220 www.bing.com\n");
            content.push_str("211.73.64.227 youtube.com\n");
            content.push_str("211.73.64.227 www.youtube.com\n");
            content.push_str("2001:4860:4802:32::78 google.com\n");
            content.push_str("2001:4860:4802:32::78 www.google.com\n");
        }
        if strict.cb_dns_filter {
            for site in DEFAULT_ADULT_SITES {
                content.push_str(&format!("127.0.0.1 {}\n", site));
                content.push_str(&format!("127.0.0.1 www.{}\n", site));
            }
        }
        content.push_str("# RasFocus Strict End\n");
    }
    content
}

fn read_clean_hosts() -> Vec<String> {
    let mut clean_lines: Vec<String> = Vec::new();
    if let Ok(file) = fs::File::open(get_hosts_path()) {
        let reader = BufReader::new(file);
        let mut skip = false;
        for line in reader.lines().flatten() {
            if line.contains("# RasFocus Strict Start") { skip = true; }
            if !skip { clean_lines.push(line.clone()); }
            if line.contains("# RasFocus Strict End") { skip = false; }
        }
    }
    clean_lines
}

// ==========================================
// --- ENFORCE STRICT PROTOCOLS ---
// ==========================================
fn enforce_strict_protocols(strict: &StrictSettings) {
    let clean_lines = read_clean_hosts();
    let new_content = build_hosts_content(&clean_lines, strict);

    #[cfg(target_os = "windows")]
    {
        let temp = "C:\\Windows\\System32\\drivers\\etc\\hosts.rasfocus.tmp";
        if fs::write(temp, &new_content).is_ok() {
            let _ = Command::new("cmd")
                .args(["/C", &format!("copy /Y \"{}\" \"{}\"", temp, get_hosts_path())])
                .output();
            let _ = fs::remove_file(temp);
        }
        flush_dns_cache();
        toggle_safe_search_registry(strict.cb_safe_search);
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let temp = "/tmp/rasfocus_hosts.tmp";
        if fs::write(temp, &new_content).is_ok() {
            let _ = Command::new("sudo").args(["cp", temp, get_hosts_path()]).output();
            let _ = fs::remove_file(temp);
        }
        flush_dns_cache();
    }
}

// ==========================================
// --- DNS FLUSH (cross-platform) ---
// ==========================================
fn flush_dns_cache() {
    #[cfg(target_os = "windows")]
    let _ = Command::new("ipconfig").arg("/flushdns").output();

    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("dscacheutil").arg("-flushcache").output();
        let _ = Command::new("killall").args(["-HUP", "mDNSResponder"]).output();
    }

    #[cfg(target_os = "linux")]
    {
        let _ = Command::new("systemd-resolve").arg("--flush-caches").output();
        let _ = Command::new("nscd").args(["-i", "hosts"]).output();
        let _ = Command::new("resolvectl").arg("flush-caches").output();
    }
}

// ==========================================
// --- DNS FILTER (cross-platform) ---
// ==========================================
fn set_family_dns(enable: bool) {
    #[cfg(target_os = "windows")]
    {
        let args = if enable {
            "/C wmic nicconfig where (IPEnabled=TRUE) call SetDNSServerSearchOrder (\"1.1.1.3\", \"1.0.0.3\")"
        } else {
            "/C wmic nicconfig where (IPEnabled=TRUE) call SetDNSServerSearchOrder ()"
        };
        let _ = Command::new("cmd").args(["/C", args]).output();
        flush_dns_cache();
    }

    #[cfg(target_os = "macos")]
    {
        if let Ok(out) = Command::new("networksetup").arg("-listallnetworkservices").output() {
            let services = String::from_utf8_lossy(&out.stdout);
            for service in services.lines().skip(1) {
                let service = service.trim().trim_start_matches('*');
                if service.is_empty() { continue; }
                if enable {
                    let _ = Command::new("sudo")
                        .args(["networksetup", "-setdnsservers", service, "1.1.1.3", "1.0.0.3"])
                        .output();
                } else {
                    let _ = Command::new("sudo")
                        .args(["networksetup", "-setdnsservers", service, "Empty"])
                        .output();
                }
            }
        }
        flush_dns_cache();
    }

    #[cfg(target_os = "linux")]
    {
        if enable {
            let conf = "[Resolve]\nDNS=1.1.1.3 1.0.0.3\nFallbackDNS=\n";
            let conf_dir = "/etc/systemd/resolved.conf.d";
            let _ = Command::new("sudo").args(["mkdir", "-p", conf_dir]).output();
            let tmp = "/tmp/rasfocus_dns.conf";
            if fs::write(tmp, conf).is_ok() {
                let _ = Command::new("sudo")
                    .args(["cp", tmp, &format!("{}/rasfocus.conf", conf_dir)])
                    .output();
                let _ = fs::remove_file(tmp);
            }
        } else {
            let _ = Command::new("sudo")
                .args(["rm", "-f", "/etc/systemd/resolved.conf.d/rasfocus.conf"])
                .output();
        }
        let _ = Command::new("sudo").args(["systemctl", "restart", "systemd-resolved"]).output();
        flush_dns_cache();
    }
}

// ==========================================
// --- SAFE SEARCH REGISTRY (Windows only) ---
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
            let _ = Command::new("reg").args(["delete", key, "/v", name, "/f"]).output();
        }
    }
    flush_dns_cache();
}

#[cfg(not(target_os = "windows"))]
fn toggle_safe_search_registry(_enable: bool) {}

// ==========================================
// --- GET FOREGROUND WINDOW TITLE ---
// ==========================================
fn get_foreground_window_title() -> String {
    #[cfg(target_os = "windows")]
    unsafe {
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;
        let hwnd = winapi::um::winuser::GetForegroundWindow();
        if hwnd.is_null() { return String::new(); }
        let mut buf = [0u16; 512];
        let len = winapi::um::winuser::GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        if len <= 0 { return String::new(); }
        OsString::from_wide(&buf[..len as usize]).to_string_lossy().to_lowercase()
    }

    #[cfg(target_os = "macos")]
    {
        if let Ok(o) = Command::new("osascript")
            .args(["-e",
                "tell application \"System Events\" to get title of front window of (first process whose frontmost is true)"
            ])
            .output()
        {
            let t = String::from_utf8_lossy(&o.stdout).trim().to_lowercase();
            if !t.is_empty() { return t; }
        }
        if let Ok(o) = Command::new("osascript")
            .args(["-e",
                "tell application \"System Events\" to get name of first process whose frontmost is true"
            ])
            .output()
        {
            return String::from_utf8_lossy(&o.stdout).trim().to_lowercase();
        }
        String::new()
    }

    #[cfg(target_os = "linux")]
    {
        if let Ok(o) = Command::new("xdotool").args(["getactivewindow", "getwindowname"]).output() {
            let t = String::from_utf8_lossy(&o.stdout).trim().to_lowercase();
            if !t.is_empty() { return t; }
        }
        if let Ok(o) = Command::new("gdbus")
            .args(["call", "--session",
                "--dest", "org.gnome.Shell",
                "--object-path", "/org/gnome/Shell",
                "--method", "org.gnome.Shell.Eval",
                "global.display.focus_window ? global.display.focus_window.title : ''"])
            .output()
        {
            return String::from_utf8_lossy(&o.stdout).trim().to_lowercase();
        }
        String::new()
    }
}

// ==========================================
// --- GET BROWSER URL ---
// ==========================================
fn get_browser_url() -> String {
    #[cfg(target_os = "windows")]
    {
        let script = r#"
try {
    Add-Type -AssemblyName UIAutomationClient
    Add-Type -AssemblyName UIAutomationTypes
    $src = '[DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();'
    $t = Add-Type -MemberDefinition $src -Name 'WU' -Namespace 'RF' -PassThru
    $hwnd = $t::GetForegroundWindow()
    $el = [System.Windows.Automation.AutomationElement]::FromHandle($hwnd)
    $cond = New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
        [System.Windows.Automation.ControlType]::Edit)
    $edit = $el.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $cond)
    if ($edit) {
        ($edit.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)).Current.Value
    }
} catch {}
"#;
        if let Ok(out) = Command::new("powershell")
            .args(["-WindowStyle", "Hidden", "-NonInteractive", "-Command", script])
            .output()
        {
            return String::from_utf8_lossy(&out.stdout).trim().to_lowercase();
        }
        String::new()
    }

    #[cfg(target_os = "macos")]
    {
        let browsers = [
            ("Google Chrome", "URL of active tab of front window"),
            ("Brave Browser",  "URL of active tab of front window"),
            ("Firefox",        "URL of active tab of front window"),
            ("Safari",         "URL of front document"),
        ];
        for (app, prop) in &browsers {
            let script = format!("tell application \"{}\" to get {}", app, prop);
            if let Ok(out) = Command::new("osascript").args(["-e", &script]).output() {
                let url = String::from_utf8_lossy(&out.stdout).trim().to_lowercase();
                if !url.is_empty() && !url.starts_with("error") && !url.contains("execution error") {
                    return url;
                }
            }
        }
        String::new()
    }

    #[cfg(target_os = "linux")]
    {
        let title = get_foreground_window_title();
        if let Some(url) = extract_url_from_title(&title) {
            return url;
        }
        title
    }
}

fn extract_url_from_title(title: &str) -> Option<String> {
    for pattern in &["https://", "http://", "www."] {
        if let Some(pos) = title.find(pattern) {
            let rest = &title[pos..];
            let end = rest.find(|c: char| c == ' ' || c == '"' || c == '\'' || c == '\t')
                .unwrap_or(rest.len());
            if end > 0 {
                return Some(rest[..end].to_lowercase());
            }
        }
    }
    None
}

// ==========================================
// --- CLOSE ACTIVE TAB ---
// ==========================================
fn close_active_tab() {
    #[cfg(target_os = "windows")]
    let _ = Command::new("powershell")
        .args(["-WindowStyle", "Hidden", "-NonInteractive", "-Command",
            "[void][System.Reflection.Assembly]::LoadWithPartialName('System.Windows.Forms'); \
             [System.Windows.Forms.SendKeys]::SendWait('^w')"
        ])
        .output();

    #[cfg(target_os = "macos")]
    let _ = Command::new("osascript")
        .args(["-e", "tell application \"System Events\" to keystroke \"w\" using command down"])
        .output();

    #[cfg(target_os = "linux")]
    let _ = Command::new("xdotool").args(["key", "ctrl+w"]).output();
}

// ==========================================
// --- KILL PROCESS ---
// ==========================================
fn kill_process(name: &str) {
    #[cfg(target_os = "windows")]
    let _ = Command::new("taskkill").args(["/F", "/IM", name, "/T"]).output();

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    let _ = Command::new("pkill").args(["-f", name]).output();
}

// ==========================================
// --- KILL ALL BROWSERS ---
// ==========================================
fn kill_all_browsers() {
    #[cfg(target_os = "windows")]
    for name in &["chrome.exe", "msedge.exe", "brave.exe", "firefox.exe"] {
        kill_process(name);
    }

    #[cfg(target_os = "macos")]
    for name in &["Google Chrome", "Brave Browser", "Safari", "Firefox"] {
        let _ = Command::new("osascript")
            .args(["-e", &format!("tell application \"{}\" to quit", name)])
            .output();
    }

    #[cfg(target_os = "linux")]
    for name in &["chrome", "chromium", "brave", "firefox"] {
        kill_process(name);
    }
}

// ==========================================
// --- BLOCK SYSTEM TOOLS ---
// ==========================================
fn block_system_tools_if_open() {
    let title = get_foreground_window_title();
    let blocked_titles = [
        "task manager", "regedit", "uninstall", "control panel",
        "activity monitor", "system preferences", "system settings",
        "gnome-system-monitor", "ksysguard", "system monitor",
    ];
    if blocked_titles.iter().any(|b| title.contains(b)) {
        #[cfg(target_os = "windows")]
        { kill_process("Taskmgr.exe"); kill_process("regedit.exe"); }

        #[cfg(target_os = "macos")]
        {
            let _ = Command::new("osascript")
                .args(["-e", "tell application \"Activity Monitor\" to quit"])
                .output();
        }

        #[cfg(target_os = "linux")]
        {
            kill_process("gnome-system-monitor");
            kill_process("ksysguard");
            kill_process("xfce4-taskmanager");
        }
    }
}

// ==========================================
// --- KEYWORD HELPERS ---
// ==========================================
fn contains_keyword(haystack: &str, keywords: &[&str]) -> bool {
    let lower = haystack.to_lowercase();
    keywords.iter().any(|k| lower.contains(&k.to_lowercase()))
}

fn contains_custom_keyword(haystack: &str, keywords: &[String]) -> bool {
    let lower = haystack.to_lowercase();
    keywords.iter().any(|k| !k.is_empty() && lower.contains(&k.to_lowercase()))
}

fn contains_adult_site(haystack: &str) -> bool {
    let lower = haystack.to_lowercase();
    DEFAULT_ADULT_SITES.iter().any(|s| lower.contains(s))
}

fn contains_reels(url: &str) -> bool {
    let lower = url.to_lowercase();
    REELS_PATTERNS.iter().any(|p| lower.contains(p))
}

fn is_browser_title(title: &str) -> bool {
    ["chrome", "edge", "brave", "firefox", "safari", "chromium", "opera", "vivaldi"]
        .iter().any(|b| title.contains(b))
}

// ==========================================
// --- KEYBOARD HOOK (Windows — C++ style) ---
// WH_KEYBOARD_LL: global low-level keyboard hook
// Type করার সময় real-time buffer scan করে keyword detect করে
// ==========================================
#[cfg(target_os = "windows")]
mod keyboard_hook {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::OnceLock;
    use winapi::shared::minwindef::{LPARAM, LRESULT, WPARAM};
    use winapi::shared::windef::HHOOK;
    use winapi::um::winuser::{
        CallNextHookEx, GetMessageW, SetWindowsHookExW, UnhookWindowsHookEx,
        KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_SYSKEYDOWN,
        VK_BACK, VK_RETURN, VK_ESCAPE, VK_SPACE,
    };

    // Global hook handle — thread-local রাখি যাতে message loop একই thread এ থাকে
    static HOOK_HANDLE: OnceLock<Mutex<Option<HHOOK>>> = OnceLock::new();
    static HOOK_RUNNING: AtomicBool = AtomicBool::new(false);

    // Shared state pointer — hook callback এ access করতে হবে
    // SAFETY: hook callback শুধু read করে, write করে না সরাসরি
    static STATE_PTR: OnceLock<Arc<Mutex<AppState>>> = OnceLock::new();

    // Rolling keyboard buffer — শেষ 128 char রাখে
    // type করতে করতে keyword match হলে সাথে সাথে block করে
    static KEY_BUFFER: OnceLock<Mutex<String>> = OnceLock::new();

    fn get_buffer() -> &'static Mutex<String> {
        KEY_BUFFER.get_or_init(|| Mutex::new(String::with_capacity(128)))
    }

    pub fn init_state_ptr(state: Arc<Mutex<AppState>>) {
        let _ = STATE_PTR.set(state);
    }

    // Low-level keyboard hook callback — Windows kernel থেকে directly call হয়
    // C++ এর WH_KEYBOARD_LL callback এর Rust equivalent
    unsafe extern "system" fn keyboard_proc(
        n_code: i32,
        w_param: WPARAM,
        l_param: LPARAM,
    ) -> LRESULT {
        use winapi::um::winuser::HC_ACTION;

        if n_code == HC_ACTION as i32
            && (w_param as u32 == WM_KEYDOWN || w_param as u32 == WM_SYSKEYDOWN)
        {
            let kb = &*(l_param as *const KBDLLHOOKSTRUCT);
            let vk = kb.vkCode;

            // Backspace — buffer থেকে শেষ char মুছো
            if vk == VK_BACK as u32 {
                if let Ok(mut buf) = get_buffer().lock() {
                    buf.pop();
                }
            }
            // Enter/Escape/Space — buffer reset করো (নতুন context শুরু)
            else if vk == VK_RETURN as u32 || vk == VK_ESCAPE as u32 {
                if let Ok(mut buf) = get_buffer().lock() {
                    buf.clear();
                }
            }
            else {
                // Virtual key → char convert
                // ToUnicode দিয়ে actual character বের করো (Bangla সহ)
                let ch = vk_to_char(vk, kb.scanCode, kb.flags);
                if let Some(c) = ch {
                    if let Ok(mut buf) = get_buffer().lock() {
                        buf.push(c);
                        // Buffer ছোট রাখো — শেষ 200 char যথেষ্ট
                        if buf.len() > 200 {
                            let drain_to = buf.len() - 150;
                            buf.drain(..drain_to);
                        }
                    }
                }

                // Buffer check করো keyword এর জন্য
                check_buffer_and_block();
            }
        }

        // সবসময় next hook এ pass করো — block করলে শুধু tab close করব
        CallNextHookEx(std::ptr::null_mut(), n_code, w_param, l_param)
    }

    // Virtual key code → Unicode char (Windows API দিয়ে)
    unsafe fn vk_to_char(vk: u32, scan_code: u32, _flags: u32) -> Option<char> {
        use winapi::um::winuser::{GetKeyboardState, ToUnicode};
        let mut keyboard_state = [0u8; 256];
        if GetKeyboardState(keyboard_state.as_mut_ptr()) == 0 {
            return None;
        }
        let mut buf = [0u16; 4];
        let result = ToUnicode(
            vk,
            scan_code,
            keyboard_state.as_ptr(),
            buf.as_mut_ptr(),
            buf.len() as i32,
            0,
        );
        if result > 0 {
            let s = String::from_utf16_lossy(&buf[..result as usize]);
            s.chars().next()
        } else {
            None
        }
    }

    // Buffer এ keyword আছে কিনা চেক করো — থাকলে tab close + counter increment
    fn check_buffer_and_block() {
        let state_arc = match STATE_PTR.get() {
            Some(s) => s,
            None => return,
        };

        let snapshot = {
            match state_arc.try_lock() {
                Ok(lock) => lock.clone(),
                Err(_) => return, // lock busy — skip, পরের keystroke এ try করবে
            }
        };

        // Focus active না হলে keyboard block করব না
        if !snapshot.adult.is_adult_focus_active { return; }

        let buffer_lower = match get_buffer().lock() {
            Ok(buf) => buf.to_lowercase(),
            Err(_) => return,
        };

        let mut matched_keyword: Option<String> = None;

        if snapshot.adult.cb_hardcore {
            for kw in HARDCORE_KEYWORDS {
                if buffer_lower.contains(&kw.to_lowercase()) {
                    matched_keyword = Some(kw.to_string());
                    break;
                }
            }
        }

        if matched_keyword.is_none() && snapshot.adult.cb_romantic {
            for kw in ROMANTIC_KEYWORDS {
                if buffer_lower.contains(&kw.to_lowercase()) {
                    matched_keyword = Some(kw.to_string());
                    break;
                }
            }
        }

        if matched_keyword.is_none() {
            for kw in &snapshot.adult.custom_keywords {
                if !kw.is_empty() && buffer_lower.contains(&kw.to_lowercase()) {
                    matched_keyword = Some(kw.clone());
                    break;
                }
            }
        }

        if let Some(kw) = matched_keyword {
            // Buffer clear করো যাতে বারবার trigger না হয়
            if let Ok(mut buf) = get_buffer().lock() {
                buf.clear();
            }

            // Log করো
            log_keyboard_block(&kw, &buffer_lower);

            // Tab close করো + counter বাড়াও
            close_active_tab();

            // State update — blocking thread এর মতো
            if let Ok(mut lock) = state_arc.try_lock() {
                lock.adult.keyboard_blocked_count += 1;
                lock.adult.total_blocked_count += 1;
                let s = lock.adult.clone(); drop(lock);
                // save_adult async ভাবে — hook callback ব্লক করব না
                thread::spawn(move || save_adult(&s));
            }
        }
    }

    // Hook install করো — Windows message loop সহ
    // এটা নিজের dedicated thread এ চলে (C++ এর মতো)
    pub fn install_hook(state: Arc<Mutex<AppState>>) {
        if HOOK_RUNNING.load(Ordering::SeqCst) { return; }

        init_state_ptr(state);
        HOOK_RUNNING.store(true, Ordering::SeqCst);

        thread::Builder::new()
            .name("rf_keyboard_hook".into())
            .spawn(move || {
                unsafe {
                    // SetWindowsHookExW দিয়ে global keyboard hook install
                    // WH_KEYBOARD_LL = 13 — low-level hook, সব app এ কাজ করে
                    let hook = SetWindowsHookExW(
                        WH_KEYBOARD_LL,
                        Some(keyboard_proc),
                        std::ptr::null_mut(), // হ্যান্ডেল — null মানে current module
                        0, // thread ID 0 = সব thread এ hook
                    );

                    if hook.is_null() {
                        HOOK_RUNNING.store(false, Ordering::SeqCst);
                        return;
                    }

                    // Hook handle store করো (uninstall এর জন্য)
                    let handle_store = HOOK_HANDLE.get_or_init(|| Mutex::new(None));
                    if let Ok(mut h) = handle_store.lock() {
                        *h = Some(hook);
                    }

                    // Message loop — hook callback invoke করার জন্য দরকার
                    // C++ এ এটা WinMain এর message loop, এখানে dedicated thread
                    let mut msg: MSG = std::mem::zeroed();
                    loop {
                        if !HOOK_RUNNING.load(Ordering::SeqCst) { break; }

                        // GetMessageW block করে — CPU burn করে না
                        let ret = GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0);
                        if ret == 0 || ret == -1 { break; }
                        // TranslateMessage/DispatchMessage দরকার নেই —
                        // keyboard hook callback সরাসরি invoke হয়
                    }

                    // Cleanup
                    UnhookWindowsHookEx(hook);
                    HOOK_RUNNING.store(false, Ordering::SeqCst);
                }
            })
            .expect("keyboard hook thread spawn failed");
    }

    pub fn uninstall_hook() {
        HOOK_RUNNING.store(false, Ordering::SeqCst);
        // Message loop এ WM_QUIT পাঠাও যাতে thread বের হয়
        unsafe {
            if let Some(store) = HOOK_HANDLE.get() {
                if let Ok(h) = store.lock() {
                    if let Some(hook) = *h {
                        UnhookWindowsHookEx(hook);
                    }
                }
            }
        }
        // Buffer clear
        if let Some(buf) = KEY_BUFFER.get() {
            if let Ok(mut b) = buf.lock() {
                b.clear();
            }
        }
    }

    pub fn is_hook_running() -> bool {
        HOOK_RUNNING.load(Ordering::SeqCst)
    }
}

// Non-Windows stub — compile error ছাড়াই cross-platform রাখতে
#[cfg(not(target_os = "windows"))]
mod keyboard_hook {
    use super::*;
    pub fn install_hook(_state: Arc<Mutex<AppState>>) {
        // macOS/Linux: keyboard hook আলাদাভাবে handle করা হয়
        // macOS: CGEventTap (accessibility permission দরকার)
        // Linux: /dev/input/event* বা evdev
        // এই version এ Windows-only
    }
    pub fn uninstall_hook() {}
    pub fn is_hook_running() -> bool { false }
}

// ==========================================
// --- BACKGROUND MONITORING THREAD ---
// ==========================================
fn start_background_thread(state_arc: Arc<Mutex<AppState>>) {
    thread::spawn(move || {
        let mut last_title = String::new();
        let mut last_logged_url = String::new();
        let mut last_popup_ms = now_ms();
        let mut last_save_ms = now_ms();
        let mut hook_was_active = false;

        loop {
            thread::sleep(Duration::from_millis(500));

            // State snapshot + timer expiry checks
            let snapshot = {
                let mut lock = state_arc.lock().unwrap();
                let now = now_ms();

                if lock.adult.cb_24h_lock && now >= lock.adult.lock_24h_end_ms {
                    lock.adult.cb_24h_lock = false;
                    lock.adult.is_adult_focus_active = false;
                    let s = lock.adult.clone(); drop(lock);
                    save_adult(&s); continue;
                }
                if lock.adult.cb_24h_lock { lock.adult.is_adult_focus_active = true; }

                if lock.adult.is_adult_focus_active && lock.adult.control_mode == 0
                    && lock.adult.focus_end_ms > 0 && now >= lock.adult.focus_end_ms
                    && !lock.adult.cb_24h_lock
                {
                    lock.adult.is_adult_focus_active = false;
                    lock.adult.keyboard_hook_active = false;
                    let s = lock.adult.clone(); drop(lock);
                    save_adult(&s); continue;
                }

                if lock.strict.is_strict_focus_active && lock.strict.strict_focus_end_ms > 0
                    && now >= lock.strict.strict_focus_end_ms
                {
                    lock.strict.is_strict_focus_active = false;
                    let s = lock.strict.clone(); drop(lock);
                    save_strict(&s); continue;
                }

                lock.clone()
            };

            let now = now_ms();

            // ── KEYBOARD HOOK LIFECYCLE MANAGEMENT ──────────────────
            // Focus active হলে hook চালু করো, বন্ধ হলে uninstall করো
            let should_hook = snapshot.adult.is_adult_focus_active
                && (snapshot.adult.cb_hardcore || snapshot.adult.cb_romantic
                    || !snapshot.adult.custom_keywords.is_empty());

            if should_hook && !keyboard_hook::is_hook_running() {
                keyboard_hook::install_hook(state_arc.clone());
                hook_was_active = true;
                // State এ hook status update করো
                if let Ok(mut lock) = state_arc.try_lock() {
                    lock.adult.keyboard_hook_active = true;
                }
            } else if !should_hook && hook_was_active && keyboard_hook::is_hook_running() {
                keyboard_hook::uninstall_hook();
                hook_was_active = false;
                if let Ok(mut lock) = state_arc.try_lock() {
                    lock.adult.keyboard_hook_active = false;
                }
            }

            // Periodic popup every 25 min
            if snapshot.adult.cb_periodic_popups && snapshot.adult.is_adult_focus_active {
                if now - last_popup_ms >= 25 * 60 * 1000 {
                    last_popup_ms = now;
                }
            }

            // Periodic save every 30s
            if now - last_save_ms >= 30_000 {
                last_save_ms = now;
                let lock = state_arc.lock().unwrap();
                let s = lock.adult.clone(); drop(lock);
                save_adult(&s);
            }

            // ── WINDOW MONITORING ──────────────────────────────────────
            let title = get_foreground_window_title();
            if title.is_empty() { continue; }

            // 1. INCOGNITO BLOCK
            if snapshot.strict.cb_incognito {
                if title.contains("incognito") || title.contains("inprivate")
                    || title.contains("private browsing")
                {
                    close_active_tab();
                    continue;
                }
            }

            // 2. STRICT MODE: Block system tools
            if snapshot.strict.cb_strict_mode
                || snapshot.strict.is_strict_focus_active
                || snapshot.adult.is_adult_focus_active
            {
                block_system_tools_if_open();
            }

            // 3. ADULT CONTENT DETECTION (title-based — existing logic)
            let any_check = snapshot.adult.cb_adult_web || snapshot.adult.cb_hardcore
                || snapshot.adult.cb_romantic || snapshot.adult.cb_fb_reels
                || snapshot.adult.is_adult_focus_active;

            if !any_check { continue; }

            // Title change detect হলে চেক করো
            if title != last_title {
                last_title = title.clone();
                let mut blocked = false;

                if snapshot.adult.cb_hardcore && !blocked {
                    blocked = contains_keyword(&title, HARDCORE_KEYWORDS);
                }
                if snapshot.adult.cb_romantic && !blocked {
                    blocked = contains_keyword(&title, ROMANTIC_KEYWORDS);
                }
                if !blocked {
                    blocked = contains_custom_keyword(&title, &snapshot.adult.custom_keywords);
                }

                if blocked {
                    close_active_tab();
                    let mut lock = state_arc.lock().unwrap();
                    lock.adult.total_blocked_count += 1;
                    let s = lock.adult.clone(); drop(lock);
                    save_adult(&s);
                    last_title = String::new();
                } else if is_browser_title(&title) {
                    // Browser detect হলে URL check
                    let url = get_browser_url();
                    let mut url_blocked = false;

                    if snapshot.adult.cb_adult_web && !url_blocked {
                        url_blocked = contains_adult_site(&url) || contains_adult_site(&title);
                    }
                    if !url_blocked && snapshot.adult.cb_fb_reels {
                        url_blocked = contains_reels(&url);
                    }

                    // Silent URL logger
                    if snapshot.strict.cb_silent_url && !url_blocked
                        && !url.is_empty() && url != last_logged_url
                    {
                        last_logged_url = url.clone();
                        log_silent_url(&title, &url);
                    }

                    if url_blocked {
                        close_active_tab();
                        thread::sleep(Duration::from_millis(280));
                        let mut lock = state_arc.lock().unwrap();
                        lock.adult.total_blocked_count += 1;
                        let s = lock.adult.clone(); drop(lock);
                        save_adult(&s);
                        last_title = String::new();
                    }
                }
            }
        }
    });
}

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
    lock.adult = settings.clone(); drop(lock);
    save_adult(&settings);
    Ok(())
}

#[tauri::command]
fn save_strict_settings(settings: StrictSettings, state: State<SharedState>) -> Result<(), String> {
    enforce_strict_protocols(&settings);
    let mut lock = state.0.lock().unwrap();
    lock.strict = settings.clone(); drop(lock);
    save_strict(&settings);
    Ok(())
}

#[tauri::command]
fn toggle_dns_filter(enable: bool, state: State<SharedState>) -> Result<(), String> {
    set_family_dns(enable);
    let mut lock = state.0.lock().unwrap();
    lock.strict.cb_dns_filter = enable;
    let s = lock.strict.clone(); drop(lock);
    enforce_strict_protocols(&s);
    save_strict(&s);
    Ok(())
}

#[tauri::command]
fn toggle_safe_search(enable: bool, state: State<SharedState>) -> Result<(), String> {
    toggle_safe_search_registry(enable);
    let mut lock = state.0.lock().unwrap();
    lock.strict.cb_safe_search = enable;
    let s = lock.strict.clone(); drop(lock);
    enforce_strict_protocols(&s);
    save_strict(&s);
    Ok(())
}

#[tauri::command]
fn start_focus(hours: u64, mins: u64, is_strict: bool, state: State<SharedState>) -> Result<(), String> {
    let mut lock = state.0.lock().unwrap();
    if is_strict {
        if hours == 0 && mins == 0 { return Err("At least 1 minute required".into()); }
        let end_ms = now_ms() + (hours * 3600 + mins * 60) * 1000;
        lock.strict.is_strict_focus_active = true;
        lock.strict.strict_focus_end_ms = end_ms;
        let s = lock.strict.clone(); drop(lock);
        save_strict(&s);
    } else {
        let end_ms = if lock.adult.control_mode == 0 {
            if hours == 0 && mins == 0 { return Err("At least 1 minute required".into()); }
            now_ms() + (hours * 3600 + mins * 60) * 1000
        } else {
            0
        };
        lock.adult.is_adult_focus_active = true;
        lock.adult.focus_end_ms = end_ms;
        let adult_s = lock.adult.clone();
        let strict_s = lock.strict.clone(); drop(lock);
        save_adult(&adult_s);
        enforce_strict_protocols(&strict_s);
    }
    Ok(())
}

#[tauri::command]
fn stop_focus(is_strict: bool, state: State<SharedState>) -> Result<(), String> {
    let mut lock = state.0.lock().unwrap();
    if is_strict {
        lock.strict.is_strict_focus_active = false;
        let s = lock.strict.clone(); drop(lock);
        save_strict(&s);
    } else if !lock.adult.cb_24h_lock {
        lock.adult.is_adult_focus_active = false;
        lock.adult.keyboard_hook_active = false;
        keyboard_hook::uninstall_hook();
        let s = lock.adult.clone(); drop(lock);
        save_adult(&s);
    }
    Ok(())
}

#[tauri::command]
fn start_24h_lock(state: State<SharedState>) -> Result<(), String> {
    let mut lock = state.0.lock().unwrap();
    lock.adult.cb_24h_lock = true;
    lock.adult.is_adult_focus_active = true;
    lock.adult.lock_24h_end_ms = now_ms() + 86_400_000;
    let s = lock.adult.clone(); drop(lock);
    save_adult(&s);
    Ok(())
}

#[tauri::command]
fn add_custom_keyword(keyword: String, state: State<SharedState>) -> Result<(), String> {
    let mut lock = state.0.lock().unwrap();
    if !keyword.is_empty() && !lock.adult.custom_keywords.contains(&keyword) {
        lock.adult.custom_keywords.push(keyword);
        let s = lock.adult.clone(); drop(lock);
        save_adult(&s);
    }
    Ok(())
}

#[tauri::command]
fn remove_custom_keyword(keyword: String, state: State<SharedState>) -> Result<(), String> {
    let mut lock = state.0.lock().unwrap();
    if !lock.adult.is_adult_focus_active {
        lock.adult.custom_keywords.retain(|k| k != &keyword);
        let s = lock.adult.clone(); drop(lock);
        save_adult(&s);
    }
    Ok(())
}

#[tauri::command]
fn get_blocked_count(state: State<SharedState>) -> i32 {
    state.0.lock().unwrap().adult.total_blocked_count
}

#[tauri::command]
fn get_keyboard_blocked_count(state: State<SharedState>) -> i32 {
    state.0.lock().unwrap().adult.keyboard_blocked_count
}

#[tauri::command]
fn get_hook_status(state: State<SharedState>) -> bool {
    let _ = state; // state বাদ দিয়ে directly check করি
    keyboard_hook::is_hook_running()
}

#[tauri::command]
fn get_silent_log() -> Vec<String> {
    if let Ok(content) = fs::read_to_string(silent_log_path()) {
        content.lines().rev().take(100).map(String::from).collect()
    } else { vec![] }
}

#[tauri::command]
fn get_keyboard_log() -> Vec<String> {
    if let Ok(content) = fs::read_to_string(keyboard_log_path()) {
        content.lines().rev().take(50).map(String::from).collect()
    } else { vec![] }
}

#[tauri::command]
fn kill_browsers() -> Result<(), String> {
    kill_all_browsers();
    Ok(())
}

#[tauri::command]
fn get_now_ms() -> u64 { now_ms() }

// ==========================================
// --- MAIN ---
// ==========================================
fn main() {
    let adult = load_adult();
    let strict = load_strict();
    enforce_strict_protocols(&strict);

    let shared = SharedState(Arc::new(Mutex::new(AppState { adult, strict })));
    start_background_thread(shared.0.clone());

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
            get_keyboard_blocked_count,
            get_hook_status,
            get_silent_log,
            get_keyboard_log,
            kill_browsers,
            get_now_ms,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
