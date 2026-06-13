<#
.SYNOPSIS
  Manual test of the out-of-store update flow for the V's Counter plugin.

.DESCRIPTION
  Mirrors the mechanism we want to port to Rust:
    1. Check the latest GitHub release and download its .streamDeckPlugin
    2. Kill the Stream Deck app (and the running plugin process)
    3. Delete the installed plugin folder (= clean uninstall)
    4. Open the downloaded .streamDeckPlugin (launches the app + installs)

  Without -Apply it only does steps 1 (check + download) so you can verify the
  safe part first. Pass -Apply to run the destructive steps 2-4.

.EXAMPLE
  pwsh ./scripts/test-update.ps1            # check + download only
  pwsh ./scripts/test-update.ps1 -Apply     # full update flow
#>
[CmdletBinding()]
param(
    [string]$Repo = 'VeeLume/streamdeck-counter',
    [string]$Uuid = 'icu.veelume.counter',
    [switch]$Apply
)

$ErrorActionPreference = 'Stop'
$ua = 'streamdeck-counter-updater'
$pluginDir = Join-Path $env:APPDATA "Elgato\StreamDeck\Plugins\$Uuid.sdPlugin"

# ── 1. Check latest release ────────────────────────────────────────────────
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

# ── download ────────────────────────────────────────────────────────────────
$dl = Join-Path $env:TEMP $asset.name
Write-Host "Downloading $($asset.name) ..." -ForegroundColor Cyan
Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $dl -Headers @{ 'User-Agent' = $ua }
$sizeKb = [math]::Round((Get-Item $dl).Length / 1KB, 1)
Write-Host "  saved $sizeKb KB to $dl" -ForegroundColor Green

if (-not $Apply) {
    Write-Host ""
    Write-Host "Check + download OK. Re-run with -Apply to kill the app, delete the" -ForegroundColor Yellow
    Write-Host "plugin folder, and install $tag." -ForegroundColor Yellow
    return
}

# ── 2. Kill the Stream Deck app + plugin process ──────────────────────────────
Write-Host "Stopping Stream Deck app and plugin process ..." -ForegroundColor Cyan
Get-Process -Name 'StreamDeck' -ErrorAction SilentlyContinue | Stop-Process -Force
Get-Process -Name $Uuid       -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 2

# ── 3. Delete the installed plugin folder (retry while handles release) ───────
if (Test-Path $pluginDir) {
    Write-Host "Deleting $pluginDir ..." -ForegroundColor Cyan
    for ($i = 1; $i -le 5; $i++) {
        try {
            Remove-Item -Recurse -Force $pluginDir -ErrorAction Stop
            break
        } catch {
            if ($i -eq 5) { throw }
            Write-Host "  locked, retrying ($i/5) ..." -ForegroundColor DarkYellow
            Start-Sleep -Seconds 1
        }
    }
    Write-Host "  removed" -ForegroundColor Green
} else {
    Write-Host "  (nothing installed to remove)"
}

# ── 4. Open the .streamDeckPlugin → launches the app and installs ─────────────
Write-Host "Opening $dl to install $tag ..." -ForegroundColor Cyan
Start-Process $dl
Write-Host "Done. The Stream Deck app should start and install $tag." -ForegroundColor Green
