<#
.SYNOPSIS
  Show an "update available" Windows toast with an "Update now" button.

.DESCRIPTION
  Validates the notification mechanism we want to port to Rust:
    1. Register a custom URL protocol (HKCU, no admin) that runs apply-update.ps1
    2. Show a toast under the borrowed PowerShell AUMID with an "Update now"
       button that uses protocol activation to launch that scheme

  Clicking "Update now" triggers the in-place silent update. The toast itself
  is harmless; nothing updates until the button is pressed.

  NOTE: the WinRT toast API requires Windows PowerShell 5.1, so this script
  relaunches itself under powershell.exe if started from pwsh 7.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File .\scripts\test-update-notify.ps1
#>
[CmdletBinding()]
param(
    [string]$Scheme  = 'cveelumeupdate',
    [string]$Repo    = 'VeeLume/streamdeck-counter',
    [string]$Version = '',
    # Identity the toast appears under. Borrowing Stream Deck's AUMID makes the
    # toast show with Stream Deck's name + icon. {6D80...} is the well-known
    # Program Files folder id, so this is stable across machines. Pass the
    # PowerShell AUMID instead to fall back to a generic identity.
    [string]$Aumid = '{6D809377-6AF0-444B-8957-A3773F02200E}\Elgato\StreamDeck\StreamDeck.exe'
)

# WinRT toast projection is only available in Windows PowerShell 5.1.
if ($PSVersionTable.PSEdition -eq 'Core') {
    Write-Host "Relaunching under Windows PowerShell 5.1 for the WinRT toast API ..." -ForegroundColor DarkGray
    & "$env:WINDIR\System32\WindowsPowerShell\v1.0\powershell.exe" `
        -NoProfile -ExecutionPolicy Bypass -File $PSCommandPath -Scheme $Scheme -Repo $Repo -Version $Version -Aumid $Aumid
    return
}

$ErrorActionPreference = 'Stop'
$applyScript = Join-Path $PSScriptRoot 'apply-update.ps1'

# ── 1. Register the custom protocol (HKCU, no admin) ─────────────────────────
$root = "HKCU:\Software\Classes\$Scheme"
New-Item -Path $root -Force | Out-Null
Set-ItemProperty -Path $root -Name '(Default)'    -Value "URL:$Scheme protocol"
Set-ItemProperty -Path $root -Name 'URL Protocol' -Value ''
$cmdKey = "$root\shell\open\command"
New-Item -Path $cmdKey -Force | Out-Null
$command = 'powershell.exe -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File "{0}" "%1"' -f $applyScript
Set-ItemProperty -Path $cmdKey -Name '(Default)' -Value $command
Write-Host "Registered ${Scheme}:// -> $applyScript" -ForegroundColor Green

# ── version text for the toast ────────────────────────────────────────────────
if (-not $Version) {
    try {
        $rel = Invoke-RestMethod "https://api.github.com/repos/$Repo/releases/latest" `
            -Headers @{ 'User-Agent' = 'sd-updater' }
        $Version = $rel.tag_name
    } catch { $Version = 'a new version' }
}

# ── 2. Build + show the toast with an "Update now" protocol button ───────────
$null = [Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime]
$null = [Windows.UI.Notifications.ToastNotification,        Windows.UI.Notifications, ContentType = WindowsRuntime]
$null = [Windows.Data.Xml.Dom.XmlDocument,                 Windows.Data.Xml.Dom,     ContentType = WindowsRuntime]

$xml = @"
<toast scenario="reminder">
  <visual>
    <binding template="ToastGeneric">
      <text>V's Counter update available</text>
      <text>$Version is ready to install.</text>
    </binding>
  </visual>
  <actions>
    <action content="Update now" activationType="protocol" arguments="${Scheme}://apply"/>
    <action content="Later" activationType="system" arguments="dismiss"/>
  </actions>
</toast>
"@

$doc = New-Object Windows.Data.Xml.Dom.XmlDocument
$doc.LoadXml($xml)
$toast = New-Object Windows.UI.Notifications.ToastNotification $doc
[Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier($Aumid).Show($toast)

Write-Host "Toast shown under '$Aumid'." -ForegroundColor Cyan
Write-Host "Click 'Update now' to run the in-place update." -ForegroundColor Cyan
Write-Host "(To remove the protocol later: Remove-Item -Recurse 'HKCU:\Software\Classes\$Scheme')" -ForegroundColor DarkGray
