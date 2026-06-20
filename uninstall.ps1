# Uninstallation script for clipboard-replacer
$ErrorActionPreference = "Continue"

$AppName = "clipboard-replacer"
$InstallDir = "$env:APPDATA\$AppName"
$StartupFolder = "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\Startup"
$ShortcutPath = "$StartupFolder\$AppName.lnk"

Write-Host "=== Uninstalling $AppName ===" -ForegroundColor Cyan

# 1. Stop the program
Write-Host "Stopping process..."
Stop-Process -Name $AppName -ErrorAction SilentlyContinue
Start-Sleep -Seconds 1

# 2. Remove shortcut from startup
if (Test-Path $ShortcutPath) {
    Remove-Item -Path $ShortcutPath -Force
    Write-Host "Shortcut removed from Startup." -ForegroundColor Green
}

# 3. Remove installation directory
if (Test-Path $InstallDir) {
    # Move the location to Temp folder so we don't lock the directory we are about to delete
    Set-Location $env:TEMP
    Remove-Item -Path $InstallDir -Recurse -Force
    Write-Host "Installation directory deleted: $InstallDir" -ForegroundColor Green
}

Write-Host "`nUninstallation successfully completed." -ForegroundColor Cyan
