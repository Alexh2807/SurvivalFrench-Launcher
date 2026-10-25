# Compile le launcher et le publie en release GitHub : tous les launchers installés se mettent à jour au prochain démarrage.
# Avant : augmenter VERSION dans launcher.py (ex : 1.2.0 -> 1.2.1).
# Usage : powershell -ExecutionPolicy Bypass -File launcher\publish.ps1 "Ce qui change"
param([string]$Notes = "")
$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
$repo = (Get-Content (Join-Path $PSScriptRoot "launcher.json") -Raw | ConvertFrom-Json).update_repo
$version = [regex]::Match((Get-Content (Join-Path $PSScriptRoot "launcher.py") -Raw), 'VERSION = "([^"]+)"').Groups[1].Value
if (-not $repo -or -not $version) { throw "update_repo (launcher.json) ou VERSION (launcher.py) introuvable." }
cmd /c "gh release view v$version --repo $repo >nul 2>&1"
if ($LASTEXITCODE -eq 0) { throw "La version v$version est déjà publiée : augmente VERSION dans launcher.py." }

& (Join-Path $PSScriptRoot "build_exe.ps1")
$exe = Join-Path $root "SurvivalFrench-Launcher.exe"

Push-Location $PSScriptRoot
try {
    git add -A
    git diff --cached --quiet
    if ($LASTEXITCODE -ne 0) { git commit -q -m "Launcher v$version" }
    git push -q origin HEAD
} finally { Pop-Location }

if (-not $Notes) { $Notes = "Launcher SurvivalFrench v$version" }
gh release create "v$version" $exe --repo $repo --title "v$version" --notes $Notes
Write-Host "Publié : https://github.com/$repo/releases/tag/v$version"
