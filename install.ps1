# Installation script for clipboard-replacer in Windows startup
$ErrorActionPreference = "Stop"

$AppName = "clipboard-replacer"
$InstallDir = "$env:APPDATA\$AppName"
$StartupFolder = "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\Startup"
$ShortcutPath = "$StartupFolder\$AppName.lnk"

Write-Host "=== Installing $AppName ===" -ForegroundColor Cyan

# 1. Create install directory if it doesn't exist
if (!(Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir | Out-Null
    Write-Host "Installation directory created: $InstallDir" -ForegroundColor Green
} else {
    Write-Host "Installation directory already exists: $InstallDir"
}

# 2. Check for the compiled binary
$ExeSource = Join-Path $PSScriptRoot "target\release\clipboard-replacer.exe"
if (!(Test-Path $ExeSource)) {
    # Try searching in current directory (if script is run from the root of built project)
    $ExeSource = Join-Path $PSScriptRoot "clipboard-replacer.exe"
}

if (!(Test-Path $ExeSource)) {
    Write-Error "Compiled file clipboard-replacer.exe not found. Please run 'cargo build --release' before installing."
    exit
}

# Stop any running instances of the program
Write-Host "Stopping running instances of the program..."
Stop-Process -Name $AppName -ErrorAction SilentlyContinue
Start-Sleep -Seconds 1

# 3. Copy the executable and uninstall script
Copy-Item -Path $ExeSource -Destination "$InstallDir\" -Force
Write-Host "Executable file copied." -ForegroundColor Green

$UninstallSource = Join-Path $PSScriptRoot "uninstall.ps1"
if (Test-Path $UninstallSource) {
    Copy-Item -Path $UninstallSource -Destination "$InstallDir\" -Force
    Write-Host "Uninstallation script copied." -ForegroundColor Green
}

# 4. Copy config.toml (only if it doesn't exist in installation directory, to save user settings)
$ConfigDest = Join-Path $InstallDir "config.toml"
if (!(Test-Path $ConfigDest)) {
    $ConfigSource = Join-Path $PSScriptRoot "config.toml"
    if (Test-Path $ConfigSource) {
        Copy-Item -Path $ConfigSource -Destination "$InstallDir\"
        Write-Host "Created default configuration file." -ForegroundColor Green
    }
} else {
    Write-Host "Configuration file already exists (skipped to keep your settings)." -ForegroundColor Yellow
}

# 5. Create shortcut in Windows Startup folder
Write-Host "Adding to Windows Startup..."
$WshShell = New-Object -ComObject WScript.Shell
$Shortcut = $WshShell.CreateShortcut($ShortcutPath)
$Shortcut.TargetPath = Join-Path $InstallDir "$AppName.exe"
$Shortcut.WorkingDirectory = $InstallDir
$Shortcut.Description = "Clipboard replacement and link fixer utility"
$Shortcut.Save()
Write-Host "Shortcut successfully added to Startup: $ShortcutPath" -ForegroundColor Green

# 6. Launch the program in background
Write-Host "Starting program..."
Start-Process -FilePath (Join-Path $InstallDir "$AppName.exe") -WorkingDirectory $InstallDir
Write-Host "Program launched in background!" -ForegroundColor Green
Write-Host "`nInstallation successfully completed! The program will now start automatically at Windows logon." -ForegroundColor Cyan
