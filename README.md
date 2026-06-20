# Clipboard Replacer

`clipboard-replacer` is a lightweight, high-performance Windows background utility written in Rust. It monitors the system clipboard and automatically applies replacement rules to copied text in real-time.

## Features

- **Background Daemon**: Runs silently in the background without terminal windows.
- **Zero Idle CPU Overhead**: Uses OS-level sleep and string comparisons to maintain virtually 0% CPU usage.
- **Dynamic Configuration**: Loads rules from a `config.toml` file.
- **Hot-Reloading**: Automatically detects changes to `config.toml` every 2 seconds and reloads rules on the fly without restarting.
- **Dual Replacement Engines**:
  - **Simple Substring Replacements**: Fast, literal match and replace (no regex overhead or escape issues).
  - **Regular Expression Rules**: Powerful pattern matching and replacement.
- **Log Management**: Writes to a self-cleaning log file (`replacer.log`) capped at 5 MB to prevent disk bloat.
  - *Normal Mode*: Only logs critical errors.
  - *Debug Mode* (`--debug` or `-d`): Logs every replacement action and rule loading status.

---

## Installation & Removal

### Install & Add to Startup
Run the installation script in PowerShell:
```powershell
.\install.ps1
```
This script:
1. Copies `clipboard-replacer.exe` and `config.toml` to `%APPDATA%\clipboard-replacer\`.
2. Copies `uninstall.ps1` to the same folder for convenience.
3. Creates a startup shortcut (`.lnk`) in the Windows Startup directory so it loads automatically at logon.
4. Starts the program immediately in background mode.

### Uninstall
To stop the utility and completely remove it from Startup and AppData, run:
```powershell
.\uninstall.ps1
```

---

## Configuration (`config.toml`)

The configuration file is written in TOML format. It is located at `%APPDATA%\clipboard-replacer\config.toml` once installed.

> [!IMPORTANT]
> The simple replacements block (`[[replacement]]`) must always be defined **above** the regular expression rules block (`[[rule]]`).

### Configuration Example

```toml
# 1. Simple replacements (exact substring match, commented out by default)
# [[replacement]]
# pattern = 'http://x.com'
# to = 'https://fixupx.com'

# 2. Regular expression rules (regex pattern match)
[[rule]]
pattern = '(?:\bhttps?://)?(?:\bwww\.)?\bx\.com\b'
to = 'https://fixupx.com'

[[rule]]
pattern = '(?:\bhttps?://)?(?:\bwww\.)?\btwitter\.com\b'
to = 'https://fxtwitter.com'

[[rule]]
pattern = '(?:\bhttps?://)?(?:\bwww\.)?\bpixiv\.net/([^/]+/)?artworks/(\d+)'
to = 'https://phixiv.net/${1}artworks/$2'
```

### Explanation of Fields:
- **`[[replacement]]`**:
  - `pattern`: The exact literal string to search for.
  - `to`: The replacement string.
- **`[[rule]]`**:
  - `pattern`: A regular expression pattern. Single quotes (`'...'`) are recommended to avoid double-escaping backslashes.
  - `to`: The string or pattern capture group to replace the match with.

---

## Debugging and Logs

By default, the program is silent and only logs errors. If you wish to troubleshoot or monitor replacements:

### 1. View logs
Open the log file at `%APPDATA%\clipboard-replacer\replacer.log` or read it via PowerShell:
```powershell
Get-Content "$env:APPDATA\clipboard-replacer\replacer.log" -Tail 20
```

### 2. Run in Debug Mode
To record all replacements and config updates, run the executable with the `--debug` or `-d` argument:
```powershell
& "$env:APPDATA\clipboard-replacer\clipboard-replacer.exe" --debug
```
Alternatively, if you want the startup shortcut to always use debug mode, edit the shortcut properties in your Windows Startup directory and add `--debug` to the Target arguments.
