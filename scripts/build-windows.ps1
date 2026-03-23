# Build script for Windows — run this on Windows to produce Squirrel installer
# Usage: powershell -ExecutionPolicy Bypass -File scripts\build-windows.ps1

$ErrorActionPreference = "Stop"

Write-Host "==> Edge Camera Discovery - Windows build"

# --- Prerequisites ---
if (-not (Get-Command "node" -ErrorAction SilentlyContinue)) {
    Write-Host "ERROR: Node.js is not installed or not in PATH." -ForegroundColor Red
    Write-Host ""
    Write-Host "Install Node.js using one of these methods:"
    Write-Host "  winget install OpenJS.NodeJS.LTS"
    Write-Host "  choco install nodejs-lts"
    Write-Host "  Or download from https://nodejs.org/"
    Write-Host ""
    Write-Host "After installing, open a NEW terminal and re-run this script."
    exit 1
}

Write-Host "    Node $(node -v) | npm $(npm -v)"

$ProjectDir = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $ProjectDir

# --- Install dependencies ---
Write-Host "==> Installing dependencies..."
npm install
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

# --- Build ---
Write-Host "==> Running electron-forge make for Windows..."
npm run make:win
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

# --- Output ---
Write-Host ""
Write-Host "==> Build complete. Artifacts:"
Get-ChildItem -Path "out\make" -Recurse -Include "*.exe","*.zip" | ForEach-Object {
    Write-Host "    $($_.FullName)"
}
