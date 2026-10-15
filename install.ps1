# Installs Zquig for the current user: copies zquig.exe to %LOCALAPPDATA%\Programs\Zquig,
# adds a Start menu shortcut and starts it in the tray. No admin rights needed.
#   .\install.ps1              install or update
#   .\install.ps1 -Uninstall   remove Zquig, its shortcut and its registry entries
param([switch]$Uninstall)
$ErrorActionPreference = "Stop"

$dir = Join-Path $env:LOCALAPPDATA "Programs\Zquig"
$exe = Join-Path $dir "zquig.exe"
$shortcut = Join-Path ([Environment]::GetFolderPath("Programs")) "Zquig.lnk"
$run = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"

# Stop a running copy and wait until it has exited, so the exe can be replaced.
$running = Get-Process zquig -ErrorAction SilentlyContinue
if ($running) {
    $running | Stop-Process -Force
    $running | Wait-Process -Timeout 10 -ErrorAction SilentlyContinue
}

if ($Uninstall) {
    Remove-Item $shortcut -ErrorAction SilentlyContinue
    Remove-Item $dir -Recurse -Force -ErrorAction SilentlyContinue
    Remove-ItemProperty $run -Name Zquig -ErrorAction SilentlyContinue
    Remove-Item "HKCU:\Software\Classes\Zquig.Image" -Recurse -ErrorAction SilentlyContinue
    Remove-Item "HKCU:\Software\Classes\Applications\zquig.exe" -Recurse -ErrorAction SilentlyContinue
    foreach ($ext in "png","jpg","jpeg","bmp","gif","tif","tiff","webp","ico","pdf") {
        Remove-ItemProperty "HKCU:\Software\Classes\.$ext\OpenWithProgids" -Name Zquig.Image -ErrorAction SilentlyContinue
    }
    Write-Host "Zquig has been removed. Your settings are in $env:APPDATA\Zquig if you want to delete them too."
    return
}

# Use the exe next to this script (the release zip), or the build output when run from the source tree.
$source = @((Join-Path $PSScriptRoot "zquig.exe"), (Join-Path $PSScriptRoot "target\release\zquig.exe")) |
    Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $source) { throw "zquig.exe not found. Build it first with: cargo build --release" }

New-Item -ItemType Directory -Force $dir | Out-Null
# Windows can hold the old exe open for a moment after the process exits.
for ($try = 1; ; $try++) {
    try { Copy-Item $source $exe -Force; break }
    catch {
        if ($try -ge 10) { throw "Couldn't replace $exe. Quit Zquig from the tray and run the installer again." }
        Start-Sleep -Milliseconds 500
    }
}

$shell = New-Object -ComObject WScript.Shell
$link = $shell.CreateShortcut($shortcut)
$link.TargetPath = $exe
$link.WorkingDirectory = $dir
$link.IconLocation = "$exe,0"
$link.Description = "Screenshot annotation"
$link.Save()

# Keep "Start with Windows" pointing at the installed copy if it was turned on.
if (Get-ItemProperty $run -Name Zquig -ErrorAction SilentlyContinue) {
    Set-ItemProperty $run -Name Zquig -Value "`"$exe`" --background"
}

# Starting it once also points Explorer's "Open with" entry at the installed copy.
Start-Process $exe -ArgumentList "--background"
Write-Host "Zquig is installed in $dir and running in the tray. Find it in the Start menu as Zquig."
