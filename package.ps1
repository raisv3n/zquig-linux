# Builds the release exe and wraps it with the README and license in dist\Zquig-windows.zip.
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

cargo build --release
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

$stage = "dist\Zquig"
Remove-Item dist -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $stage | Out-Null
Copy-Item target\release\zquig.exe, install.ps1, README.md, LICENSE $stage
Compress-Archive -Path "$stage\*" -DestinationPath dist\Zquig-windows.zip
Write-Host "Created dist\Zquig-windows.zip"
