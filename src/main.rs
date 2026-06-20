#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

static DEBUG_MODE: AtomicBool = AtomicBool::new(false);
use std::time::{Duration, SystemTime};
use arboard::Clipboard;
use chrono::Local;
use regex::Regex;
use serde::Deserialize;

// Get the path for the log file
fn get_log_path() -> PathBuf {
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(parent) = exe_path.parent() {
            let alt_path = parent.join("config.toml");
            // If the program is installed in AppData or config.toml is nearby, write log to the same directory
            if alt_path.exists() || exe_path.to_string_lossy().contains("AppData") {
                return parent.join("replacer.log");
            }
        }
    }
    std::env::current_dir()
        .map(|p| p.join("replacer.log"))
        .unwrap_or_else(|_| PathBuf::from("replacer.log"))
}

// Write messages to the log file (and console, if available)
fn log_message(msg: &str, _is_error: bool) {
    let is_debug = DEBUG_MODE.load(Ordering::Relaxed);
    if !_is_error && !is_debug {
        return;
    }

    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
    let formatted = format!("[{}] {}\n", timestamp, msg);

    // Output to console in debug builds
    #[cfg(debug_assertions)]
    {
        if _is_error {
            eprint!("{}", formatted);
        } else {
            print!("{}", formatted);
        }
    }

    let log_path = get_log_path();

    // Check log file size to prevent it from growing indefinitely
    if let Ok(metadata) = std::fs::metadata(&log_path) {
        if metadata.len() > 5 * 1024 * 1024 { // 5 MB limit
            let _ = std::fs::write(&log_path, "[Info] Log file cleared due to size limit exceeded (5 MB).\n");
        }
    }

    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        use std::io::Write;
        let _ = file.write_all(formatted.as_bytes());
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

#[derive(Deserialize, Debug, Clone)]
struct SimpleReplacement {
    pattern: String,
    to: String,
}

#[derive(Deserialize, Debug, Clone)]
struct Rule {
    pattern: String,
    to: String,
}

#[derive(Deserialize, Debug, Clone)]
struct Config {
    #[serde(default)]
    replacement: Vec<SimpleReplacement>,
    #[serde(default)]
    rule: Vec<Rule>,
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
        if !path.exists() {
            if let Ok(exe_path) = std::env::current_exe() {
                if let Some(parent) = exe_path.parent() {
                    let alt_path = parent.join("config.toml");
                    if alt_path.exists() {
                        path = alt_path;
                    }
                }
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
            let default_config = r#"# [[replacement]]
# pattern = 'http://x.com'
# to = 'https://fixupx.com'

[[rule]]
pattern = '(?:\bhttps?://)?(?:\bwww\.)?\bx\.com\b'
to = 'https://fixupx.com'

[[rule]]
pattern = '(?:\bhttps?://)?(?:\bwww\.)?\btwitter\.com\b'
to = 'https://fxtwitter.com'

[[rule]]
pattern = '(?:\bhttps?://)?(?:\bwww\.)?\bpixiv\.net/([^/]+/)?artworks/(\d+)'
to = 'https://phixiv.net/${1}artworks/$2'
"#;
            if let Err(e) = std::fs::write(&self.path, default_config) {
                log_error!("Failed to create default configuration file: {}", e);
                return;
            }
            log_info!("Created default configuration file: {:?}", self.path);
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

    let mut last_clipboard_content: Option<String> = None;
    if let Ok(text) = clipboard.get_text() {
        last_clipboard_content = Some(text);
    }

    let mut loop_count: u32 = 0;

    loop {
        // Check for config modifications every 2 seconds (4 iterations)
        if loop_count % 4 == 0 {
            config_manager.check_and_reload();
        }
        loop_count = loop_count.wrapping_add(1);

        match clipboard.get_text() {
            Ok(current_text) => {
                let is_new = match &last_clipboard_content {
                    Some(last_text) => last_text != &current_text,
                    None => true,
                };

                if is_new {
                    last_clipboard_content = Some(current_text.clone());

                    let mut replaced_text = current_text.clone();
                    let mut changed = false;

                    // Apply simple replacements first
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

                    // Apply query parameter filtering for Twitter/X and YouTube
                    let filtered_text = filter_query_parameters(&replaced_text);
                    if filtered_text != replaced_text {
                        replaced_text = filtered_text;
                        changed = true;
                    }

                    if changed {
                        log_info!("Match detected! Performing replacement...");
                        match clipboard.set_text(replaced_text.clone()) {
                            Ok(_) => {
                                log_info!("  Before: {}", truncate_str(&current_text, 60));
                                log_info!("  After:  {}", truncate_str(&replaced_text, 60));
                                last_clipboard_content = Some(replaced_text);
                            }
                            Err(e) => {
                                log_error!("Failed to write to clipboard: {}", e);
                            }
                        }
                    }
                }
            }
            Err(arboard::Error::ContentNotAvailable) => {
                if last_clipboard_content.is_some() {
                    last_clipboard_content = None;
                }
            }
            Err(e) => {
                log_error!("Clipboard read error: {}", e);
            }
        }

        thread::sleep(Duration::from_millis(500));
    }
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

fn process_query_string(domain: &str, query_str: &str) -> String {
    let domain_lower = domain.to_lowercase();
    let mut kept_params = Vec::new();
    for pair in query_str.split('&') {
        if pair.is_empty() {
            continue;
        }
        let mut parts = pair.splitn(2, '=');
        if let Some(key) = parts.next() {
            let value = parts.next().unwrap_or("");
            
            let is_allowed = if domain_lower.contains("youtube.com") || domain_lower == "youtu.be" {
                key == "t" || key == "list" || key == "v"
            } else {
                // Twitter / X / fixupx / fxtwitter
                false
            };
            
            if is_allowed {
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

fn filter_query_parameters(text: &str) -> String {
    // Regex matching the targeted domains
    // Matches: protocol, www, domain, path, query, fragment
    let url_re = Regex::new(
        r"(?i)(?:\b(https?://))?(?:\b(www\.))?\b(youtube\.com|music\.youtube\.com|youtu\.be|twitter\.com|x\.com|fixupx\.com|fxtwitter\.com)\b(/[^?\s#]*)?(\?[^\s#]*)?(#[^\s]*)?"
    ).unwrap();

    url_re.replace_all(text, |caps: &regex::Captures| {
        let protocol = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        let www = caps.get(2).map(|m| m.as_str()).unwrap_or("");
        let domain = caps.get(3).map(|m| m.as_str()).unwrap_or("");
        let path = caps.get(4).map(|m| m.as_str()).unwrap_or("");
        let query_with_question = caps.get(5).map(|m| m.as_str()).unwrap_or("");
        let fragment = caps.get(6).map(|m| m.as_str()).unwrap_or("");

        // Check if this is part of a longer domain (like youtube.com.my or x.com.ua)
        let domain_match = caps.get(3).unwrap();
        if let Some(c) = text[domain_match.end()..].chars().next() {
            if c == '.' || c.is_alphanumeric() || c == '-' {
                // Return the whole match unchanged
                return caps.get(0).unwrap().as_str().to_string();
            }
        }

        let mut clean_query = String::new();
        let mut trailing_punct = "";

        // Determine which part has trailing punctuation
        let mut clean_fragment = String::new();
        if !fragment.is_empty() {
            let (f_base, f_punct) = strip_trailing_punctuation(fragment);
            clean_fragment = f_base.to_string();
            trailing_punct = f_punct;
            
            if !query_with_question.is_empty() {
                // If we have a fragment, the query string doesn't have trailing punctuation (the fragment does)
                let query_str = &query_with_question[1..];
                clean_query = process_query_string(domain, query_str);
            }
        } else if !query_with_question.is_empty() {
            let (q_base, q_punct) = strip_trailing_punctuation(query_with_question);
            trailing_punct = q_punct;
            if q_base.len() > 1 {
                let query_str = &q_base[1..];
                clean_query = process_query_string(domain, query_str);
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
        let re_x = Regex::new(r"(?:\bhttps?://)?(?:\bwww\.)?\bx\.com\b").unwrap();
        let re_tw = Regex::new(r"(?:\bhttps?://)?(?:\bwww\.)?\btwitter\.com\b").unwrap();

        // Check x.com with and without protocol
        assert_eq!(re_x.replace_all("x.com/mrsnanapple/status/123", "https://fixupx.com"), "https://fixupx.com/mrsnanapple/status/123");
        assert_eq!(re_x.replace_all("https://x.com/mrsnanapple/status/123", "https://fixupx.com"), "https://fixupx.com/mrsnanapple/status/123");
        assert_eq!(re_x.replace_all("http://www.x.com/mrsnanapple/status/123", "https://fixupx.com"), "https://fixupx.com/mrsnanapple/status/123");

        // Check twitter.com with and without protocol
        assert_eq!(re_tw.replace_all("twitter.com/user/status/123", "https://fxtwitter.com"), "https://fxtwitter.com/user/status/123");
        assert_eq!(re_tw.replace_all("https://twitter.com/user/status/123", "https://fxtwitter.com"), "https://fxtwitter.com/user/status/123");

        // Check that other domains ending in x.com are NOT replaced
        assert_eq!(re_x.replace_all("index.com", "https://fixupx.com"), "index.com");
        assert_eq!(re_x.replace_all("alex.com", "https://fixupx.com"), "alex.com");
    }

    #[test]
    fn test_filter_query_parameters() {
        // Twitter/X: strip all queries
        assert_eq!(
            filter_query_parameters("https://x.com/user/status/123?s=20&t=456"),
            "https://x.com/user/status/123"
        );
        assert_eq!(
            filter_query_parameters("http://www.twitter.com/user/status/123?s=20#ref"),
            "http://www.twitter.com/user/status/123#ref"
        );
        assert_eq!(
            filter_query_parameters("x.com/status/123?s=20&t=abc."),
            "x.com/status/123."
        );

        // YouTube: keep only t, list, v
        assert_eq!(
            filter_query_parameters("https://youtube.com/watch?v=abc&si=def&t=10"),
            "https://youtube.com/watch?v=abc&t=10"
        );
        assert_eq!(
            filter_query_parameters("https://music.youtube.com/watch?v=abc&si=def&list=xyz&extra=123"),
            "https://music.youtube.com/watch?v=abc&list=xyz"
        );
        assert_eq!(
            filter_query_parameters("youtu.be/abc?si=def&t=5"),
            "youtu.be/abc?t=5"
        );
        assert_eq!(
            filter_query_parameters("youtu.be/abc?si=def"),
            "youtu.be/abc"
        );
        
        // Complex text containing links
        let input_text = "Check this: https://youtube.com/watch?v=abc&si=def and also x.com/user/status/123?s=20.";
        let expected_text = "Check this: https://youtube.com/watch?v=abc and also x.com/user/status/123.";
        assert_eq!(filter_query_parameters(input_text), expected_text);
    }
}


