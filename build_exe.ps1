# Construit SurvivalFrench-Launcher.exe à la racine du projet.
# Usage : powershell -ExecutionPolicy Bypass -File launcher\build_exe.ps1
$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
$python = Join-Path $root ".tools\pyenv\Scripts\python.exe"
if (-not (Test-Path $python)) {
    python -m venv (Join-Path $root ".tools\pyenv")
    & $python -m pip install -q pyinstaller==6.16.0
}
$work = Join-Path $root "build\launcher"
& $python -m PyInstaller --noconfirm --clean --onefile --noconsole `
    --name "SurvivalFrench-Launcher" `
    --icon (Join-Path $PSScriptRoot "icon.ico") `
    --add-data "$(Join-Path $PSScriptRoot 'index.html');." `
    --add-data "$(Join-Path $PSScriptRoot 'launcher.json');." `
    --add-data "$(Join-Path $PSScriptRoot 'logo.png');." `
    --distpath $root --workpath $work --specpath $work `
    (Join-Path $PSScriptRoot "launcher.py")
Write-Host "OK : $(Join-Path $root 'SurvivalFrench-Launcher.exe')"
