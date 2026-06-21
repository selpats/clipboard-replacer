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
    try {
        Remove-Item -Path $InstallDir -Recurse -Force -ErrorAction Stop
        Write-Host "Installation directory deleted: $InstallDir" -ForegroundColor Green
    } catch {
        # If deletion of the directory itself failed (e.g. locked because terminal is in it),
        # try to delete all files inside the directory, leaving it empty.
        $files = Get-ChildItem -Path $InstallDir -File -Recurse -ErrorAction SilentlyContinue
        foreach ($file in $files) {
            # Don't delete uninstall.ps1 if it's currently running (to avoid file locking errors)
            if ($file.Name -ne "uninstall.ps1") {
                Remove-Item -Path $file.FullName -Force -ErrorAction SilentlyContinue
            }
        }
        Write-Host "Could not delete the installation directory itself because it is locked (e.g., your terminal is open inside it)." -ForegroundColor Yellow
        Write-Host "All application files have been cleaned up. You can manually delete the empty directory '$InstallDir' after changing your directory ('cd ..')." -ForegroundColor Yellow
    }
}

Write-Host "`nUninstallation successfully completed." -ForegroundColor Cyan
