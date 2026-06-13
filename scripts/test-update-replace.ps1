<#
.SYNOPSIS
  Prompt-less update test: replace the installed plugin files in place.

.DESCRIPTION
  Alternative to the open-the-file install (which shows a confirmation dialog).
  Steps:
    1. Check the latest GitHub release and download its .streamDeckPlugin
    2. Extract it (a .streamDeckPlugin is a zip) to a staging folder
    3. Kill the Stream Deck app (and the running plugin process)
    4. Replace the installed plugin folder with the extracted files
    5. Relaunch the Stream Deck app

  Without -Apply it only does steps 1-2 (download + extract, both safe). Pass
  -Apply to run the destructive steps 3-5.

.EXAMPLE
  pwsh ./scripts/test-update-replace.ps1          # download + extract only
  pwsh ./scripts/test-update-replace.ps1 -Apply   # full in-place replace
#>
[CmdletBinding()]
param(
    [string]$Repo = 'VeeLume/streamdeck-counter',
    [string]$Uuid = 'icu.veelume.counter',
    [switch]$Apply
)

$ErrorActionPreference = 'Stop'
$ua = 'streamdeck-counter-updater'
$pluginsRoot = Join-Path $env:APPDATA 'Elgato\StreamDeck\Plugins'
$pluginDir = Join-Path $pluginsRoot "$Uuid.sdPlugin"

# ── 1. Check latest release + download ───────────────────────────────────────
Write-Host "Checking latest release of $Repo ..." -ForegroundColor Cyan
$rel = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" `
    -Headers @{ 'User-Agent' = $ua }
$tag = $rel.tag_name
Write-Host "  latest release : $tag"

$manifestPath = Join-Path $pluginDir 'manifest.json'
if (Test-Path $manifestPath) {
    $installed = (Get-Content $manifestPath -Raw | ConvertFrom-Json).Version
    Write-Host "  installed      : $installed"
} else {
    Write-Host "  installed      : (not installed)"
}

$asset = $rel.assets | Where-Object { $_.name -like '*.streamDeckPlugin' } | Select-Object -First 1
if (-not $asset) { throw "No .streamDeckPlugin asset on the latest release" }

$dl = Join-Path $env:TEMP $asset.name
Write-Host "Downloading $($asset.name) ..." -ForegroundColor Cyan
Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $dl -Headers @{ 'User-Agent' = $ua }
Write-Host "  saved to $dl" -ForegroundColor Green

# ── 2. Extract (a .streamDeckPlugin is a zip) ────────────────────────────────
$staging = Join-Path $env:TEMP "$Uuid-update-staging"
if (Test-Path $staging) { Remove-Item -Recurse -Force $staging }
New-Item -ItemType Directory -Force -Path $staging | Out-Null

# Expand-Archive wants a .zip extension, so copy first.
$zip = Join-Path $env:TEMP "$Uuid-update.zip"
Copy-Item $dl $zip -Force
Expand-Archive -Path $zip -DestinationPath $staging -Force
Write-Host "Extracted to $staging" -ForegroundColor Green

# Find the folder that holds manifest.json (robust to any nesting).
$mf = Get-ChildItem -Path $staging -Recurse -Filter manifest.json | Select-Object -First 1
if (-not $mf) { throw "No manifest.json found in the extracted package" }
$srcDir = $mf.Directory.FullName
$newVer = (Get-Content $mf.FullName -Raw | ConvertFrom-Json).Version
Write-Host "  package version: $newVer   ($srcDir)"

if (-not $Apply) {
    Write-Host ""
    Write-Host "Download + extract OK. Re-run with -Apply to replace the installed" -ForegroundColor Yellow
    Write-Host "files in place and relaunch Stream Deck." -ForegroundColor Yellow
    return
}

# ── 3. Capture the Stream Deck exe path, then kill app + plugin ───────────────
$sdPath = Get-Process -Name 'StreamDeck' -ErrorAction SilentlyContinue |
    Select-Object -First 1 -ExpandProperty Path
if (-not $sdPath) {
    # Fall back to the default install location if the app isn't running.
    $sdPath = Join-Path ${env:ProgramFiles} 'Elgato\StreamDeck\StreamDeck.exe'
}
Write-Host "Stopping Stream Deck app and plugin process ..." -ForegroundColor Cyan
Get-Process -Name 'StreamDeck' -ErrorAction SilentlyContinue | Stop-Process -Force
Get-Process -Name $Uuid       -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 2

# ── 4. Replace the installed folder with the extracted files ─────────────────
if (Test-Path $pluginDir) {
    Write-Host "Removing old $pluginDir ..." -ForegroundColor Cyan
    for ($i = 1; $i -le 5; $i++) {
        try { Remove-Item -Recurse -Force $pluginDir -ErrorAction Stop; break }
        catch {
            if ($i -eq 5) { throw }
            Write-Host "  locked, retrying ($i/5) ..." -ForegroundColor DarkYellow
            Start-Sleep -Seconds 1
        }
    }
}
New-Item -ItemType Directory -Force -Path $pluginsRoot | Out-Null
Copy-Item -Recurse $srcDir $pluginDir
Write-Host "Installed $newVer into $pluginDir" -ForegroundColor Green

# ── 5. Relaunch the Stream Deck app (background, like Windows auto-start) ─────
# Elgato's own Run-key launch uses --runinbk to start straight to the tray
# without showing the main window.
if (Test-Path $sdPath) {
    Write-Host "Relaunching Stream Deck in background ($sdPath --runinbk) ..." -ForegroundColor Cyan
    Start-Process $sdPath -ArgumentList '--runinbk'
    Write-Host "Done. No install dialog and no foreground window should appear." -ForegroundColor Green
} else {
    Write-Host "Could not find StreamDeck.exe to relaunch; start it manually." -ForegroundColor Yellow
}
