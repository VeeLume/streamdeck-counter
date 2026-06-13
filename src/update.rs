//! Self-update for plugins distributed outside the Elgato Store.
//!
//! On startup we ask GitHub for the latest release and compare it to the
//! running version. If newer, we register a custom URL protocol and show a
//! Windows toast with an "Update now" button. Clicking it launches a detached
//! PowerShell helper that performs the proven in-place swap:
//!
//!   capture StreamDeck.exe path -> kill the app + plugin -> download + extract
//!   the new package -> replace the plugin folder -> relaunch `--runinbk`.
//!
//! The swap lives in a PowerShell helper rather than in this process because it
//! kills the Stream Deck app (which terminates this plugin) and replaces this
//! very `.exe` — it must outlive us. The button uses *protocol* activation
//! (not an in-process callback), which is the only model that works for an
//! unpackaged exe with no registered AppUserModelID.
//!
//! Prototyped in this plugin; destined for `streamdeck-lib`.

use serde_json::Value;
use tracing::{info, warn};

const REPO: &str = "VeeLume/streamdeck-counter";
const CURRENT: &str = env!("CARGO_PKG_VERSION");
const USER_AGENT: &str = concat!("streamdeck-counter/", env!("CARGO_PKG_VERSION"));
const UUID: &str = "icu.veelume.counter";
/// Custom URL scheme the "Update now" button activates.
const SCHEME: &str = "cveelumeupdate";

/// Spawn the update check on a detached background thread so startup never
/// blocks on the network.
pub fn spawn_update_check() {
    std::thread::spawn(|| match check_latest() {
        Ok(Some(version)) => {
            info!("update available: {CURRENT} -> {version}");
            if let Err(e) = offer_update(&version) {
                warn!("failed to offer update: {e}");
            }
        }
        Ok(None) => info!("up to date (current {CURRENT})"),
        Err(e) => warn!("update check failed: {e}"),
    });
}

/// GET the latest release tag; return `Some(version)` only if it is newer.
fn check_latest() -> anyhow::Result<Option<String>> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let resp = ureq::get(&url)
        .set("User-Agent", USER_AGENT)
        .set("Accept", "application/vnd.github+json")
        .call()?;
    let release: Value = serde_json::from_reader(resp.into_reader())?;
    let tag = release
        .get("tag_name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let latest = tag.trim_start_matches('v').to_string();
    Ok(is_newer(&latest, CURRENT).then_some(latest))
}

/// Set up the protocol handler and surface the update toast (Windows only).
#[cfg(windows)]
fn offer_update(version: &str) -> anyhow::Result<()> {
    register_protocol()?;
    show_toast(version)?;
    Ok(())
}

#[cfg(not(windows))]
fn offer_update(_version: &str) -> anyhow::Result<()> {
    warn!("self-update is only implemented on Windows");
    Ok(())
}

/// Run the in-place update: write the helper and launch it with no window.
/// Invoked when this exe is started as `--apply-update` by the toast button.
#[cfg(windows)]
pub fn apply_now() -> anyhow::Result<()> {
    let helper = write_helper()?;
    spawn_helper_hidden(&helper)?;
    Ok(())
}

#[cfg(not(windows))]
pub fn apply_now() -> anyhow::Result<()> {
    Ok(())
}

/// Spawn the PowerShell helper with no console window. CREATE_NO_WINDOW alone
/// gives a windowless process that already survives this parent exiting and is
/// not part of Stream Deck's process tree. (DETACHED_PROCESS must NOT be ORed
/// in — the combination is contradictory and makes CreateProcess fail.)
#[cfg(windows)]
fn spawn_helper_hidden(helper: &std::path::Path) -> anyhow::Result<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(helper)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()?;
    Ok(())
}

// ── Windows implementation ───────────────────────────────────────────────────

/// PowerShell that downloads + installs the latest release in place. Mirrors
/// the validated `scripts/test-update-replace.ps1 -Apply` flow. `@@REPO@@` /
/// `@@UUID@@` are substituted before writing.
#[cfg(windows)]
const HELPER_TEMPLATE: &str = r##"
$ErrorActionPreference = 'Stop'
$repo = '@@REPO@@'
$uuid = '@@UUID@@'
$ua = 'streamdeck-counter-updater'
$pluginsRoot = Join-Path $env:APPDATA 'Elgato\StreamDeck\Plugins'
$pluginDir = Join-Path $pluginsRoot "$uuid.sdPlugin"

# download the latest packaged plugin
$rel = Invoke-RestMethod "https://api.github.com/repos/$repo/releases/latest" -Headers @{ 'User-Agent' = $ua }
$asset = $rel.assets | Where-Object { $_.name -like '*.streamDeckPlugin' } | Select-Object -First 1
if (-not $asset) { exit 1 }
$dl = Join-Path $env:TEMP $asset.name
Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $dl -Headers @{ 'User-Agent' = $ua }

# extract (a .streamDeckPlugin is a zip)
$staging = Join-Path $env:TEMP "$uuid-update-staging"
if (Test-Path $staging) { Remove-Item -Recurse -Force $staging }
New-Item -ItemType Directory -Force -Path $staging | Out-Null
$zip = Join-Path $env:TEMP "$uuid-update.zip"
Copy-Item $dl $zip -Force
Expand-Archive -Path $zip -DestinationPath $staging -Force
$mf = Get-ChildItem -Path $staging -Recurse -Filter manifest.json | Select-Object -First 1
if (-not $mf) { exit 1 }
$src = $mf.Directory.FullName

# capture the Stream Deck exe path, then kill the app + plugin
$sdPath = Get-Process -Name 'StreamDeck' -ErrorAction SilentlyContinue | Select-Object -First 1 -ExpandProperty Path
if (-not $sdPath) { $sdPath = Join-Path ${env:ProgramFiles} 'Elgato\StreamDeck\StreamDeck.exe' }
Get-Process -Name 'StreamDeck' -ErrorAction SilentlyContinue | Stop-Process -Force
Get-Process -Name $uuid -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 2

# replace the installed folder with the new files
if (Test-Path $pluginDir) {
  for ($i = 0; $i -lt 5; $i++) {
    try { Remove-Item -Recurse -Force $pluginDir -ErrorAction Stop; break } catch { Start-Sleep -Seconds 1 }
  }
}
New-Item -ItemType Directory -Force -Path $pluginsRoot | Out-Null
Copy-Item -Recurse $src $pluginDir

# relaunch in the background (tray), like Windows auto-start
if (Test-Path $sdPath) { Start-Process $sdPath -ArgumentList '--runinbk' }
"##;

/// Write the apply helper to a stable temp path and return it.
#[cfg(windows)]
fn write_helper() -> anyhow::Result<std::path::PathBuf> {
    let script = HELPER_TEMPLATE
        .replace("@@REPO@@", REPO)
        .replace("@@UUID@@", UUID);
    let path = std::env::temp_dir().join(format!("{UUID}-apply-update.ps1"));
    std::fs::write(&path, script)?;
    Ok(path)
}

/// Register `cveelumeupdate://` under HKCU (no admin) so the toast button can
/// relaunch this exe as `--apply-update` via the shell. Routing through our own
/// (windows-subsystem) exe means no console window flashes — it then spawns the
/// PowerShell helper with CREATE_NO_WINDOW.
#[cfg(windows)]
fn register_protocol() -> anyhow::Result<()> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    let exe = std::env::current_exe()?;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (proto, _) = hkcu.create_subkey(format!("Software\\Classes\\{SCHEME}"))?;
    proto.set_value("", &format!("URL:{SCHEME} protocol"))?;
    proto.set_value("URL Protocol", &"")?;

    let (cmd, _) = hkcu.create_subkey(format!("Software\\Classes\\{SCHEME}\\shell\\open\\command"))?;
    let command = format!("\"{}\" --apply-update \"%1\"", exe.display());
    cmd.set_value("", &command)?;
    Ok(())
}

/// Show the update toast under Stream Deck's AppUserModelID (so it carries
/// Stream Deck's name + icon) with an "Update now" protocol button.
#[cfg(windows)]
fn show_toast(version: &str) -> anyhow::Result<()> {
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
    use windows::core::HSTRING;

    // {6D80...} is the well-known Program Files folder id, so this resolves the
    // same on any default Elgato install.
    const SD_AUMID: &str =
        r"{6D809377-6AF0-444B-8957-A3773F02200E}\Elgato\StreamDeck\StreamDeck.exe";

    let xml = format!(
        r#"<toast scenario="reminder">
  <visual>
    <binding template="ToastGeneric">
      <text>V's Counter update available</text>
      <text>{version} is ready to install.</text>
    </binding>
  </visual>
  <actions>
    <action content="Update now" activationType="protocol" arguments="{SCHEME}://apply"/>
    <action content="Later" activationType="system" arguments="dismiss"/>
  </actions>
</toast>"#
    );

    let doc = XmlDocument::new()?;
    doc.LoadXml(&HSTRING::from(xml))?;
    let toast = ToastNotification::CreateToastNotification(&doc)?;
    let notifier = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(SD_AUMID))?;
    notifier.Show(&toast)?;
    Ok(())
}

// ── Version compare ──────────────────────────────────────────────────────────

/// Compare dotted versions numerically (e.g. `0.1.10` > `0.1.9`), ignoring any
/// fourth component and non-numeric noise.
fn is_newer(latest: &str, current: &str) -> bool {
    parse(latest) > parse(current)
}

fn parse(v: &str) -> (u64, u64, u64) {
    let mut it = v.split('.').map(|s| s.trim().parse().unwrap_or(0));
    (
        it.next().unwrap_or(0),
        it.next().unwrap_or(0),
        it.next().unwrap_or(0),
    )
}

#[cfg(test)]
mod tests {
    use super::is_newer;

    #[test]
    fn detects_newer_versions() {
        assert!(is_newer("0.1.9", "0.1.8"));
        assert!(is_newer("0.2.0", "0.1.9"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(is_newer("0.1.10", "0.1.9")); // numeric, not lexical
    }

    #[test]
    fn ignores_same_or_older() {
        assert!(!is_newer("0.1.8", "0.1.8"));
        assert!(!is_newer("0.1.7", "0.1.8"));
    }
}
