# Construit le launcher (Rust + WebView2) et le dépose à la racine du projet : SurvivalFrench-Launcher.exe.
# Usage : powershell -ExecutionPolicy Bypass -File launcher\build_exe.ps1
# Prérequis : Rust (rustup, cible x86_64-pc-windows-msvc) et les outils de compilation C++ de Visual Studio.
$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
Push-Location $PSScriptRoot
try {
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw "La compilation a échoué." }
} finally { Pop-Location }
$exe = Join-Path $root "SurvivalFrench-Launcher.exe"
# Un launcher ouvert verrouille son exe : Windows accepte de le renommer, pas de l'écraser.
if (Test-Path $exe) {
    try { Remove-Item $exe -ErrorAction Stop } catch { Rename-Item $exe ("SurvivalFrench-Launcher.old-{0}.exe" -f [DateTimeOffset]::Now.ToUnixTimeSeconds()) }
}
Copy-Item (Join-Path $PSScriptRoot "target\release\SurvivalFrench-Launcher.exe") $exe
Write-Host ("OK : {0} ({1:N1} Mo)" -f $exe, ((Get-Item $exe).Length / 1MB))
