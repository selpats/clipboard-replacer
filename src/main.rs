#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread;

static DEBUG_MODE: AtomicBool = AtomicBool::new(false);
use std::time::{Duration, SystemTime};
use arboard::Clipboard;
use chrono::Local;
use regex::Regex;
use serde::{Deserialize, Serialize};



// Get the path for a log file
fn get_log_path(filename: &str) -> PathBuf {
    if let Ok(exe_path) = std::env::current_exe()
        && let Some(parent) = exe_path.parent() {
            let alt_path = parent.join("config.toml");
            // If the program is installed in AppData or config.toml is nearby, write log to the same directory
            if alt_path.exists() || exe_path.to_string_lossy().contains("AppData") {
                return parent.join(filename);
            }
        }
    std::env::current_dir()
        .map(|p| p.join(filename))
        .unwrap_or_else(|_| PathBuf::from(filename))
}

fn write_to_log_file(path: &std::path::Path, content: &str) {
    // Check log file size to prevent it from growing indefinitely (5 MB limit)
    if let Ok(metadata) = std::fs::metadata(path)
        && metadata.len() > 5 * 1024 * 1024 {
            let _ = std::fs::write(path, "[Info] Log file cleared due to size limit exceeded (5 MB).\n");
        }

    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        use std::io::Write;
        let _ = file.write_all(content.as_bytes());
    }
}

// Write messages to the log files (and console, if available)
fn log_message(msg: &str, _is_error: bool) {
    let is_debug = DEBUG_MODE.load(Ordering::Relaxed);
    if !_is_error && !is_debug {
        return;
    }

    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
    let formatted = format!("[{}] {}\n", timestamp, msg);

    if _is_error {
        let error_log_path = get_log_path("error.log");
        write_to_log_file(&error_log_path, &formatted);
    }

    if is_debug {
        let debug_log_path = get_log_path("debug.log");
        write_to_log_file(&debug_log_path, &formatted);
    }
}

macro_rules! log_info {
    ($($arg:tt)*) => {
        log_message(&format!($($arg)*), false);
    };
}

macro_rules! log_error {
    ($($arg:tt)*) => {
        log_message(&format!($($arg)*), true);
    };
}

#[derive(Deserialize, Serialize, Debug, Clone)]
struct SimpleReplacement {
    pattern: String,
    to: String,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
struct Rule {
    pattern: String,
    to: String,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
struct Config {
    #[serde(default)]
    replacement: Vec<SimpleReplacement>,
    #[serde(default)]
    rule: Vec<Rule>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            replacement: Vec::new(),
            rule: vec![
                Rule {
                    pattern: r"(?:\bhttps?://)?(?:\bwww\.)?\bx\.com/([a-zA-Z0-9_]{1,15}/status/\d+)".to_string(),
                    to: "https://fixupx.com/$1".to_string(),
                },
                Rule {
                    pattern: r"(?:\bhttps?://)?(?:\bwww\.)?\btwitter\.com/([a-zA-Z0-9_]{1,15}/status/\d+)".to_string(),
                    to: "https://fxtwitter.com/$1".to_string(),
                },
                Rule {
                    pattern: r"(?:\bhttps?://)?(?:\bwww\.)?\bpixiv\.net/([^/]+/)?artworks/(\d+)".to_string(),
                    to: "https://phixiv.net/${1}artworks/$2".to_string(),
                },
            ],
        }
    }
}

struct CompiledRule {
    pattern: String,
    regex: Regex,
    to: String,
}

struct ConfigManager {
    path: PathBuf,
    last_modified: Option<SystemTime>,
    replacements: Vec<SimpleReplacement>,
    rules: Vec<CompiledRule>,
}

impl ConfigManager {
    fn new() -> Self {
        let mut path = std::env::current_dir()
            .map(|p| p.join("config.toml"))
            .unwrap_or_else(|_| PathBuf::from("config.toml"));

        // If config doesn't exist in CWD, look next to the executable
        if !path.exists()
            && let Ok(exe_path) = std::env::current_exe()
                && let Some(parent) = exe_path.parent() {
                    let alt_path = parent.join("config.toml");
                    if alt_path.exists() {
                        path = alt_path;
                    }
                }

        Self {
            path,
            last_modified: None,
            replacements: Vec::new(),
            rules: Vec::new(),
        }
    }

    fn check_and_reload(&mut self) {
        // Create default config if it doesn't exist
        if !self.path.exists() {
            match toml::to_string_pretty(&Config::default()) {
                Ok(toml_string) => {
                    let commented_toml = format!(
                        "# [[replacement]]\n\
                         # pattern = 'http://x.com'\n\
                         # to = 'https://fixupx.com'\n\n\
                         {}",
                        toml_string
                    );
                    if let Err(e) = std::fs::write(&self.path, commented_toml) {
                        log_error!("Failed to create default configuration file: {}", e);
                        return;
                    }
                    log_info!("Created default configuration file: {:?}", self.path);
                }
                Err(e) => {
                    log_error!("Failed to serialize default config: {}", e);
                    return;
                }
            }
        }

        let current_modified = std::fs::metadata(&self.path)
            .and_then(|m| m.modified())
            .ok();

        let needs_reload = match (self.last_modified, current_modified) {
            (Some(last), Some(current)) => current > last,
            _ => true,
        };

        if needs_reload {
            log_info!("Loading/reloading configuration from {:?}", self.path);
            match std::fs::read_to_string(&self.path) {
                Ok(content) => {
                    match toml::from_str::<Config>(&content) {
                        Ok(raw_config) => {
                            let mut new_rules = Vec::new();
                            for (i, r) in raw_config.rule.iter().enumerate() {
                                match Regex::new(&r.pattern) {
                                    Ok(re) => {
                                        new_rules.push(CompiledRule {
                                            pattern: r.pattern.clone(),
                                            regex: re,
                                            to: r.to.clone(),
                                        });
                                    }
                                    Err(e) => {
                                        log_error!("Rule #{}: invalid regular expression '{}': {}", i + 1, r.pattern, e);
                                    }
                                }
                            }
                            log_info!("Successfully loaded simple replacements: {}", raw_config.replacement.len());
                            for rep in &raw_config.replacement {
                                log_info!("  - '{}' -> '{}'", rep.pattern, rep.to);
                            }
                            log_info!("Successfully loaded regex rules: {}", new_rules.len());
                            for r in &new_rules {
                                log_info!("  - '{}' -> '{}'", r.pattern, r.to);
                            }
                            self.replacements = raw_config.replacement;
                            self.rules = new_rules;
                            self.last_modified = current_modified;
                        }
                        Err(e) => {
                            log_error!("TOML parsing error: {}", e);
                        }
                    }
                }
                Err(e) => {
                    log_error!("Failed to read configuration file: {}", e);
                }
            }
        }
    }
}

fn get_text_with_retry(clipboard: &mut Clipboard) -> Result<String, arboard::Error> {
    let mut attempts = 0;
    loop {
        match clipboard.get_text() {
            Ok(text) => return Ok(text),
            Err(arboard::Error::ClipboardOccupied) => {
                attempts += 1;
                if attempts >= 5 {
                    return Err(arboard::Error::ClipboardOccupied);
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return Err(e),
        }
    }
}

fn set_text_with_retry(clipboard: &mut Clipboard, text: String) -> Result<(), arboard::Error> {
    let mut attempts = 0;
    loop {
        match clipboard.set_text(text.clone()) {
            Ok(_) => return Ok(()),
            Err(arboard::Error::ClipboardOccupied) => {
                attempts += 1;
                if attempts >= 5 {
                    return Err(arboard::Error::ClipboardOccupied);
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return Err(e),
        }
    }
}

fn main() {
    if std::env::args().any(|arg| arg == "--debug" || arg == "-d") {
        DEBUG_MODE.store(true, Ordering::SeqCst);
    }

    log_info!("==================================================");
    log_info!("            Clipboard Replacer Started            ");
    log_info!("==================================================");

    let mut clipboard = match Clipboard::new() {
        Ok(cb) => cb,
        Err(e) => {
            log_error!("Failed to access clipboard: {}", e);
            std::process::exit(1);
        }
    };

    let mut config_manager = ConfigManager::new();
    config_manager.check_and_reload();

    // Set up and load privacy parameters rules from uBlock Origin list
    let cache_path = get_cache_path();
    let mut initial_rules = Vec::new();
    
    if cache_path.exists() {
        match std::fs::read_to_string(&cache_path) {
            Ok(content) => {
                initial_rules = parse_rules_from_str(&content);
                log_info!("Loaded {} rules from local cache: {:?}", initial_rules.len(), cache_path);
            }
            Err(e) => {
                log_error!("Failed to read cached rules: {}", e);
            }
        }
    }
    
    let ubo_rules = Arc::new(RwLock::new(initial_rules));
    
    // Background thread to download/update the rules file from GitHub every 12 hours
    let ubo_rules_clone = Arc::clone(&ubo_rules);
    let cache_path_clone = cache_path.clone();
    std::thread::spawn(move || {
        let mut retry_delay = Duration::from_secs(60); // Start retrying after 1 minute
        let max_retry_delay = Duration::from_secs(60 * 60); // Maximum retry interval is 1 hour
        let success_delay = Duration::from_secs(12 * 60 * 60); // 12 hours on success

        loop {
            log_info!("Fetching latest privacy-removeparam.txt from GitHub...");
            match fetch_and_save_rules(&cache_path_clone) {
                Ok(content) => {
                    log_info!("Successfully downloaded privacy-removeparam.txt.");
                    let new_rules = parse_rules_from_str(&content);
                    log_info!("Parsed {} rules from downloaded file.", new_rules.len());
                    if !new_rules.is_empty()
                        && let Ok(mut w) = ubo_rules_clone.write() {
                            *w = new_rules;
                        }
                    // Reset retry delay and sleep for 12 hours on success
                    retry_delay = Duration::from_secs(60);
                    std::thread::sleep(success_delay);
                }
                Err(e) => {
                    log_error!("Failed to fetch privacy-removeparam.txt: {}. Retrying in {} seconds...", e, retry_delay.as_secs());
                    std::thread::sleep(retry_delay);
                    // Double the delay for the next attempt, capped at 1 hour
                    retry_delay = std::cmp::min(retry_delay * 2, max_retry_delay);
                }
            }
        }
    });

    let mut last_clipboard_content: Option<String> = None;
    if let Ok(text) = get_text_with_retry(&mut clipboard) {
        last_clipboard_content = Some(text);
    }

    let mut last_error_msg: Option<String> = None;
    let mut loop_count: u32 = 0;

    loop {
        // Check for config modifications every 2 seconds (4 iterations)
        if loop_count.is_multiple_of(4) {
            config_manager.check_and_reload();
        }
        loop_count = loop_count.wrapping_add(1);

        match get_text_with_retry(&mut clipboard) {
            Ok(current_text) => {
                last_error_msg = None;
                let is_new = match &last_clipboard_content {
                    Some(last_text) => last_text != &current_text,
                    None => true,
                };

                if is_new {
                    last_clipboard_content = Some(current_text.clone());

                    let mut replaced_text = current_text.clone();
                    let mut changed = false;

                    // Apply query parameter filtering based on uBlock Origin privacy rules first
                    let rules_read_lock = ubo_rules.read().unwrap();
                    let filtered_text = filter_query_parameters(&replaced_text, &rules_read_lock);
                    if filtered_text != replaced_text {
                        replaced_text = filtered_text;
                        changed = true;
                    }

                    // Apply simple replacements next
                    for rep in &config_manager.replacements {
                        if replaced_text.contains(&rep.pattern) {
                            replaced_text = replaced_text.replace(&rep.pattern, &rep.to);
                            changed = true;
                        }
                    }

                    // Apply regular expression rules
                    for rule in &config_manager.rules {
                        if rule.regex.is_match(&replaced_text) {
                            let new_text = rule.regex.replace_all(&replaced_text, &rule.to).into_owned();
                            if new_text != replaced_text {
                                replaced_text = new_text;
                                changed = true;
                            }
                        }
                    }

                    if changed {
                        log_info!("Match detected! Performing replacement...");
                        match set_text_with_retry(&mut clipboard, replaced_text.clone()) {
                            Ok(_) => {
                                log_info!("  Before: {}", truncate_str(&current_text, 60));
                                log_info!("  After:  {}", truncate_str(&replaced_text, 60));
                                last_clipboard_content = Some(replaced_text);
                            }
                            Err(e) => {
                                let mut is_system_lock = false;
                                let err_msg = match e {
                                    arboard::Error::ClipboardOccupied => {
                                        if let Some(holder) = get_clipboard_holder_info() {
                                            if holder.contains("LockApp.exe") || holder.contains("LogonUI.exe") {
                                                is_system_lock = true;
                                            }
                                            format!("Clipboard occupied by {}", holder)
                                        } else {
                                            "Clipboard occupied by another party".to_string()
                                        }
                                    }
                                    other => other.to_string(),
                                };
                                if !is_system_lock {
                                    log_error!("Failed to write to clipboard: {}", err_msg);
                                }
                            }
                        }
                    }
                }
            }
            Err(arboard::Error::ContentNotAvailable) => {
                if last_clipboard_content.is_some() {
                    last_clipboard_content = None;
                }
                last_error_msg = None;
            }
            Err(e) => {
                let mut is_system_lock = false;
                let err_msg = match e {
                    arboard::Error::ClipboardOccupied => {
                        let holder_info = if let Some(holder) = get_clipboard_holder_info() {
                            if holder.contains("LockApp.exe") || holder.contains("LogonUI.exe") {
                                is_system_lock = true;
                            }
                            format!("by {}", holder)
                        } else {
                            "by another party".to_string()
                        };
                        if let Some(ref last_content) = last_clipboard_content {
                            format!("Clipboard occupied {} (Last known content: '{}')", holder_info, truncate_str(last_content, 60))
                        } else {
                            format!("Clipboard occupied {} (Last known content: None)", holder_info)
                        }
                    }
                    other => {
                        if let Some(ref last_content) = last_clipboard_content {
                            format!("{} (Last known content: '{}')", other, truncate_str(last_content, 60))
                        } else {
                            format!("{} (Last known content: None)", other)
                        }
                    }
                };

                if !is_system_lock {
                    let should_log = match &last_error_msg {
                        Some(last) => last != &err_msg,
                        None => true,
                    };

                    if should_log {
                        log_error!("Clipboard read error: {}", err_msg);
                        last_error_msg = Some(err_msg);
                    }
                } else {
                    last_error_msg = Some(err_msg);
                }
            }
        }

        thread::sleep(Duration::from_millis(500));
    }
}

#[cfg(windows)]
fn get_clipboard_holder_info() -> Option<String> {
    use std::ffi::c_void;
    use std::path::PathBuf;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetOpenClipboardWindow() -> *mut c_void;
        fn GetWindowThreadProcessId(hwnd: *mut c_void, lpdw_process_id: *mut u32) -> u32;
        fn GetForegroundWindow() -> *mut c_void;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(
            dw_desired_access: u32,
            b_inherit_handle: i32,
            dw_process_id: u32,
        ) -> *mut c_void;
        fn QueryFullProcessImageNameW(
            h_process: *mut c_void,
            dw_flags: u32,
            lp_exe_name: *mut u16,
            lpdw_size: *mut u32,
        ) -> i32;
        fn CloseHandle(h_object: *mut c_void) -> i32;
    }

    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;

    unsafe {
        let mut hwnd = GetOpenClipboardWindow();
        let mut is_direct_owner = true;
        if hwnd.is_null() {
            hwnd = GetForegroundWindow();
            is_direct_owner = false;
        }
        if hwnd.is_null() {
            return None;
        }

        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 {
            if is_direct_owner {
                return Some("Unknown Process (PID 0)".to_string());
            } else {
                return Some("Unknown Process (PID 0) (Foreground Window)".to_string());
            }
        }

        let process_handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process_handle.is_null() {
            if is_direct_owner {
                return Some(format!("Unknown Process (PID {})", pid));
            } else {
                return Some(format!("Unknown Process (PID {}) (Foreground Window)", pid));
            }
        }

        let mut buf = [0u16; 1024];
        let mut size = buf.len() as u32;
        let success = QueryFullProcessImageNameW(process_handle, 0, buf.as_mut_ptr(), &mut size);
        CloseHandle(process_handle);

        if success != 0 && size > 0 {
            let path_os = String::from_utf16_lossy(&buf[..size as usize]);
            let path = PathBuf::from(path_os);
            let name_str = if let Some(name) = path.file_name() {
                name.to_string_lossy().into_owned()
            } else {
                path.to_string_lossy().into_owned()
            };

            if is_direct_owner {
                return Some(format!("{} (PID {})", name_str, pid));
            } else {
                return Some(format!("{} (PID {}) (Foreground Window)", name_str, pid));
            }
        }

        if is_direct_owner {
            Some(format!("Unknown Process (PID {})", pid))
        } else {
            Some(format!("Unknown Process (PID {}) (Foreground Window)", pid))
        }
    }
}

#[cfg(not(windows))]
fn get_clipboard_holder_info() -> Option<String> {
    None
}

fn strip_trailing_punctuation(s: &str) -> (&str, &str) {
    let mut end_idx = s.len();
    while end_idx > 0 {
        let last_char = s[..end_idx].chars().next_back().unwrap();
        if last_char == '.' || last_char == ',' || last_char == ';' || last_char == ':' || last_char == '!' || last_char == '?' || last_char == ')' || last_char == ']' || last_char == '"' || last_char == '\'' {
            end_idx -= last_char.len_utf8();
        } else {
            break;
        }
    }
    (&s[..end_idx], &s[end_idx..])
}

fn domain_matches(url_domain: &str, config_domain: &str) -> bool {
    let url_domain_lower = url_domain.to_lowercase();
    let config_domain_lower = config_domain.to_lowercase();
    
    if url_domain_lower == config_domain_lower {
        return true;
    }
    
    if url_domain_lower.ends_with(&format!(".{}", config_domain_lower)) {
        return true;
    }
    
    false
}

#[derive(Debug, Clone)]
struct UboRule {
    parameter: Box<str>,
    is_regex: bool,
    regex: Option<Regex>,
    domains: Box<[Box<str>]>,
    excluded_domains: Box<[Box<str>]>,
    is_global: bool,
    url_pattern: Option<Regex>,
}

fn wildcard_to_regex(pattern: &str) -> Option<Regex> {
    if pattern.is_empty() || pattern == "*" {
        return None;
    }
    let mut regex_str = String::new();
    for c in pattern.chars() {
        match c {
            '*' => regex_str.push_str(".*"),
            '?' => regex_str.push_str("\\?"),
            '.' => regex_str.push_str("\\."),
            '+' => regex_str.push_str("\\+"),
            '^' => regex_str.push_str("[/?:&=#]"),
            '(' | ')' | '[' | ']' | '{' | '}' | '|' | '$' | '\\' => {
                regex_str.push('\\');
                regex_str.push(c);
            }
            _ => regex_str.push(c),
        }
    }
    Regex::new(&regex_str).ok()
}

fn parse_ubo_line(line: &str) -> Option<UboRule> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('!') || line.starts_with('#') || line.starts_with("@@") {
        return None;
    }

    let removeparam_idx = line.find("removeparam")?;
    let mut left = &line[..removeparam_idx];
    if left.ends_with('$') || left.ends_with(',') {
        left = &left[..left.len() - 1];
    }
    if left.ends_with('$') || left.ends_with(',') {
        left = &left[..left.len() - 1];
    }

    let left_clean = left.trim();
    let mut url_pattern = None;
    if !left_clean.is_empty()
        && !left_clean.starts_with("||")
        && !left_clean.starts_with('$')
        && left_clean != "*"
    {
        url_pattern = wildcard_to_regex(left_clean);
    }

    let right = &line[removeparam_idx..];
    let mut param = "";
    let mut modifiers = Vec::new();

    if let Some(rest) = right.strip_prefix("removeparam=") {
        let mut parts = rest.split(',');
        if let Some(p) = parts.next() {
            param = p.trim();
        }
        for modifier in parts {
            modifiers.push(modifier.trim());
        }
    } else {
        param = "*";
    }

    if param.is_empty() {
        return None;
    }

    let mut is_regex = false;
    let clean_param = Box::from(param);
    let mut regex = None;

    if param.starts_with('/') && param.ends_with('/') && param.len() > 2 {
        is_regex = true;
        let regex_pattern = &param[1..param.len() - 1];
        if let Ok(re) = Regex::new(regex_pattern) {
            regex = Some(re);
        } else {
            return None;
        }
    }

    let mut domains = Vec::new();
    let mut excluded_domains = Vec::new();
    let mut is_global = true;

    if let Some(stripped) = left.strip_prefix("||") {
        is_global = false;
        let mut domain_part = stripped;
        if domain_part.ends_with('^') {
            domain_part = &domain_part[..domain_part.len() - 1];
        }
        let clean_domain = domain_part.trim_end_matches('/').trim_end_matches('^');
        domains.push(clean_domain.to_lowercase().into_boxed_str());
    }

    if let Some(left_modifiers) = left.strip_prefix('$') {
        for mod_str in left_modifiers.split(',') {
            modifiers.push(mod_str.trim());
        }
    }

    for modifier in modifiers {
        if let Some(val) = modifier.strip_prefix("domain=") {
            is_global = false;
            for d in val.split('|') {
                let d = d.trim();
                if let Some(stripped) = d.strip_prefix('~') {
                    excluded_domains.push(stripped.to_lowercase().into_boxed_str());
                } else {
                    domains.push(d.to_lowercase().into_boxed_str());
                }
            }
        } else if let Some(val) = modifier.strip_prefix("to=") {
            for d in val.split('|') {
                let d = d.trim();
                if let Some(stripped) = d.strip_prefix('~') {
                    excluded_domains.push(stripped.to_lowercase().into_boxed_str());
                } else {
                    is_global = false;
                    domains.push(d.to_lowercase().into_boxed_str());
                }
            }
        }
    }

    Some(UboRule {
        parameter: clean_param,
        is_regex,
        regex,
        domains: domains.into_boxed_slice(),
        excluded_domains: excluded_domains.into_boxed_slice(),
        is_global,
        url_pattern,
    })
}

fn parse_rules_from_str(content: &str) -> Vec<UboRule> {
    let mut rules = Vec::new();
    for line in content.lines() {
        if let Some(rule) = parse_ubo_line(line) {
            rules.push(rule);
        }
    }
    rules
}

fn get_cache_path() -> PathBuf {
    if let Ok(exe_path) = std::env::current_exe()
        && let Some(parent) = exe_path.parent() {
            let alt_path = parent.join("config.toml");
            if alt_path.exists() || exe_path.to_string_lossy().contains("AppData") {
                return parent.join("privacy-removeparam.txt");
            }
        }
    std::env::current_dir()
        .map(|p| p.join("privacy-removeparam.txt"))
        .unwrap_or_else(|_| PathBuf::from("privacy-removeparam.txt"))
}

fn fetch_and_save_rules(cache_path: &std::path::Path) -> Result<String, String> {
    let url = "https://raw.githubusercontent.com/uBlockOrigin/uAssets/refs/heads/master/filters/privacy-removeparam.txt";
    
    let response = minreq::get(url)
        .with_timeout(10)
        .send()
        .map_err(|e| format!("HTTP request failed: {}", e))?;
        
    let body = response.as_str()
        .map_err(|e| format!("Failed to read response body: {}", e))?;
        
    if let Err(e) = std::fs::write(cache_path, body) {
        log_error!("Failed to write rules to cache file: {}", e);
    }
    
    Ok(body.to_string())
}

fn should_filter_param(url_domain: &str, full_url: &str, param_name: &str, rules: &[UboRule]) -> bool {
    for rule in rules {
        let param_matches = if rule.is_regex {
            if let Some(ref re) = rule.regex {
                re.is_match(param_name)
            } else {
                false
            }
        } else {
            rule.parameter.as_ref() == "*" || rule.parameter.as_ref() == param_name
        };

        if !param_matches {
            continue;
        }

        if let Some(ref url_re) = rule.url_pattern {
            if !url_re.is_match(full_url) {
                continue;
            }
        }

        let mut is_excluded = false;
        for excl in rule.excluded_domains.iter() {
            if domain_matches(url_domain, excl) {
                is_excluded = true;
                break;
            }
        }
        if is_excluded {
            continue;
        }

        if rule.is_global {
            return true;
        }

        for rule_dom in rule.domains.iter() {
            if domain_matches(url_domain, rule_dom) {
                return true;
            }
        }
    }

    false
}

fn process_query_string_ubo(domain: &str, full_url: &str, query_str: &str, rules: &[UboRule]) -> String {
    let mut kept_params = Vec::new();
    for pair in query_str.split('&') {
        if pair.is_empty() {
            continue;
        }
        let mut parts = pair.splitn(2, '=');
        if let Some(key) = parts.next() {
            let value = parts.next().unwrap_or("");
            
            let should_remove = should_filter_param(domain, full_url, key, rules);
            
            if !should_remove {
                if value.is_empty() {
                    kept_params.push(key.to_string());
                } else {
                    kept_params.push(format!("{}={}", key, value));
                }
            }
        }
    }
    
    if kept_params.is_empty() {
        String::new()
    } else {
        format!("?{}", kept_params.join("&"))
    }
}

fn filter_query_parameters(text: &str, ubo_rules: &[UboRule]) -> String {
    let url_re = Regex::new(
        r"(?i)(?:\b(https?://))?(?:\b(www\.))?\b([a-zA-Z0-9.-]+\.[a-zA-Z]{2,})\b(/[^?\s#]*)?(\?[^\s#]*)?(#[^\s]*)?"
    ).unwrap();

    url_re.replace_all(text, |caps: &regex::Captures| {
        let protocol = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        let www = caps.get(2).map(|m| m.as_str()).unwrap_or("");
        let domain = caps.get(3).map(|m| m.as_str()).unwrap_or("");
        let path = caps.get(4).map(|m| m.as_str()).unwrap_or("");
        let query_with_question = caps.get(5).map(|m| m.as_str()).unwrap_or("");
        let fragment = caps.get(6).map(|m| m.as_str()).unwrap_or("");

        if query_with_question.is_empty() {
            return caps.get(0).unwrap().as_str().to_string();
        }

        let domain_match = caps.get(3).unwrap();
        if let Some(c) = text[domain_match.end()..].chars().next()
            && (c == '.' || c.is_alphanumeric() || c == '-') {
                return caps.get(0).unwrap().as_str().to_string();
            }

        let full_url = caps.get(0).unwrap().as_str();
        let mut clean_query = String::new();
        let trailing_punct;

        let mut clean_fragment = String::new();
        if !fragment.is_empty() {
            let (f_base, f_punct) = strip_trailing_punctuation(fragment);
            clean_fragment = f_base.to_string();
            trailing_punct = f_punct;
            
            let query_str = &query_with_question[1..];
            clean_query = process_query_string_ubo(domain, full_url, query_str, ubo_rules);
        } else {
            let (q_base, q_punct) = strip_trailing_punctuation(query_with_question);
            trailing_punct = q_punct;
            if q_base.len() > 1 {
                let query_str = &q_base[1..];
                clean_query = process_query_string_ubo(domain, full_url, query_str, ubo_rules);
            }
        }

        format!("{}{}{}{}{}{}{}", protocol, www, domain, path, clean_query, clean_fragment, trailing_punct)
    }).into_owned()
}

fn truncate_str(s: &str, max_len: usize) -> String {
    let clean_s = s.replace("\n", " ").replace("\r", "");
    if clean_s.chars().count() > max_len {
        let truncated: String = clean_s.chars().take(max_len - 3).collect();
        format!("{}...", truncated)
    } else {
        clean_s
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pixiv_regex() {
        let re = Regex::new(r"(?:\bhttps?://)?(?:\bwww\.)?\bpixiv\.net/([^/]+/)?artworks/(\d+)").unwrap();
        let text_with_proto = "https://www.pixiv.net/en/artworks/139578917";
        let text_no_proto = "pixiv.net/en/artworks/139578917";
        let text_no_lang = "pixiv.net/artworks/139578917";

        assert!(re.is_match(text_with_proto));
        assert_eq!(re.replace_all(text_with_proto, "https://phixiv.net/${1}artworks/$2"), "https://phixiv.net/en/artworks/139578917");

        assert!(re.is_match(text_no_proto));
        assert_eq!(re.replace_all(text_no_proto, "https://phixiv.net/${1}artworks/$2"), "https://phixiv.net/en/artworks/139578917");

        assert!(re.is_match(text_no_lang));
        assert_eq!(re.replace_all(text_no_lang, "https://phixiv.net/${1}artworks/$2"), "https://phixiv.net/artworks/139578917");

        // Should not match other subdomains or texts ending in pixiv.net
        assert_eq!(re.replace_all("mypixiv.net/artworks/123", "https://phixiv.net/${1}artworks/$2"), "mypixiv.net/artworks/123");
    }

    #[test]
    fn test_x_and_twitter_regex() {
        let re_x = Regex::new(r"(?:\bhttps?://)?(?:\bwww\.)?\bx\.com/([a-zA-Z0-9_]{1,15}/status/\d+)").unwrap();
        let re_tw = Regex::new(r"(?:\bhttps?://)?(?:\bwww\.)?\btwitter\.com/([a-zA-Z0-9_]{1,15}/status/\d+)").unwrap();

        // Check x.com statuses with and without protocol
        assert_eq!(re_x.replace_all("x.com/mrsnanapple/status/123", "https://fixupx.com/$1"), "https://fixupx.com/mrsnanapple/status/123");
        assert_eq!(re_x.replace_all("https://x.com/mrsnanapple/status/123", "https://fixupx.com/$1"), "https://fixupx.com/mrsnanapple/status/123");
        assert_eq!(re_x.replace_all("http://www.x.com/mrsnanapple/status/123", "https://fixupx.com/$1"), "https://fixupx.com/mrsnanapple/status/123");

        // Check twitter.com statuses with and without protocol
        assert_eq!(re_tw.replace_all("twitter.com/user/status/123", "https://fxtwitter.com/$1"), "https://fxtwitter.com/user/status/123");
        assert_eq!(re_tw.replace_all("https://twitter.com/user/status/123", "https://fxtwitter.com/$1"), "https://fxtwitter.com/user/status/123");

        // Check that profiles are NOT replaced
        assert_eq!(re_x.replace_all("https://x.com/arocro07", "https://fixupx.com/$1"), "https://x.com/arocro07");
        assert_eq!(re_tw.replace_all("https://twitter.com/user", "https://fxtwitter.com/$1"), "https://twitter.com/user");

        // Check that other domains ending in x.com are NOT replaced
        assert_eq!(re_x.replace_all("index.com/user/status/123", "https://fixupx.com/$1"), "index.com/user/status/123");
    }

    #[test]
    fn test_filter_query_parameters() {
        let test_rules_str = r#"
            $removeparam=utm_source
            $removeparam=utm_medium
            $removeparam=utm_campaign
            $removeparam=utm_term
            $removeparam=utm_content
            $removeparam=fbclid
            $removeparam=gclid
            $removeparam=si
            $removeparam=s,domain=twitter.com|x.com
            $removeparam=cxt,domain=twitter.com|x.com
            .com/*/status/$removeparam=t,domain=twitter.com|x.com
        "#;
        let query_filters = parse_rules_from_str(test_rules_str);

        // Twitter/X: strip all queries
        assert_eq!(
            filter_query_parameters("https://x.com/user/status/123?s=20&t=456", &query_filters),
            "https://x.com/user/status/123"
        );
        assert_eq!(
            filter_query_parameters("http://www.twitter.com/user/status/123?s=20#ref", &query_filters),
            "http://www.twitter.com/user/status/123#ref"
        );
        assert_eq!(
            filter_query_parameters("x.com/user/status/123?s=20&t=abc.", &query_filters),
            "x.com/user/status/123."
        );

        // YouTube: remove si
        assert_eq!(
            filter_query_parameters("https://youtube.com/watch?v=abc&si=def&t=10", &query_filters),
            "https://youtube.com/watch?v=abc&t=10"
        );
        assert_eq!(
            filter_query_parameters("https://music.youtube.com/watch?v=abc&si=def&list=xyz&extra=123", &query_filters),
            "https://music.youtube.com/watch?v=abc&list=xyz&extra=123"
        );
        assert_eq!(
            filter_query_parameters("youtu.be/abc?si=def&t=5", &query_filters),
            "youtu.be/abc?t=5"
        );
        assert_eq!(
            filter_query_parameters("youtu.be/abc?si=def", &query_filters),
            "youtu.be/abc"
        );
        
        // Complex text containing links
        let input_text = "Check this: https://youtube.com/watch?v=abc&si=def and also x.com/user/status/123?s=20.";
        let expected_text = "Check this: https://youtube.com/watch?v=abc and also x.com/user/status/123.";
        assert_eq!(filter_query_parameters(input_text, &query_filters), expected_text);
    }
}


