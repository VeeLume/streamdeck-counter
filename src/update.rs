//! Prototype self-update check for plugins distributed outside the Elgato
//! Store (which therefore get no automatic updates).
//!
//! On startup we ask the GitHub Releases API for the latest tag, compare it to
//! the running version, and — if newer — download the packed `.streamDeckPlugin`
//! and hand it to the OS. Opening that file makes the Stream Deck app install
//! and restart the plugin (the closest thing to a true auto-update; a running
//! plugin can't replace its own locked `.exe`).
//!
//! This is a counter-local prototype destined for `streamdeck-lib`, where it
//! becomes a reusable `UpdateChecker { repo, current_version, behavior }` built
//! on the lib's existing `reqwest` client.

use std::io::copy;
use std::path::{Path, PathBuf};

use serde_json::Value;
use tracing::{info, warn};

/// GitHub `owner/repo` to check for releases.
const REPO: &str = "VeeLume/streamdeck-counter";
/// The running version, baked in at compile time (matches the release tag).
const CURRENT: &str = env!("CARGO_PKG_VERSION");
/// GitHub requires a User-Agent on API requests.
const USER_AGENT: &str = concat!("streamdeck-counter/", env!("CARGO_PKG_VERSION"));

/// Spawn the update check on a detached background thread so startup never
/// blocks on the network.
pub fn spawn_update_check() {
    std::thread::spawn(|| {
        if let Err(e) = check_and_update() {
            warn!("update check failed: {e}");
        }
    });
}

fn check_and_update() -> anyhow::Result<()> {
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
    let latest = tag.trim_start_matches('v');

    if !is_newer(latest, CURRENT) {
        info!("up to date (current {CURRENT}, latest {latest})");
        return Ok(());
    }
    info!("update available: {CURRENT} -> {latest}");

    let Some(dl_url) = find_plugin_asset(&release) else {
        warn!("latest release {tag} has no .streamDeckPlugin asset; skipping");
        return Ok(());
    };

    let path = download(&dl_url)?;
    info!("downloaded update to {}", path.display());
    open_installer(&path)?;
    info!("handed update to the OS installer");
    Ok(())
}

/// Find the `.streamDeckPlugin` asset's download URL in a release JSON object.
fn find_plugin_asset(release: &Value) -> Option<String> {
    release
        .get("assets")?
        .as_array()?
        .iter()
        .find(|a| {
            a.get("name")
                .and_then(Value::as_str)
                .is_some_and(|n| n.ends_with(".streamDeckPlugin"))
        })
        .and_then(|a| a.get("browser_download_url"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn download(url: &str) -> anyhow::Result<PathBuf> {
    let resp = ureq::get(url).set("User-Agent", USER_AGENT).call()?;
    let path = std::env::temp_dir().join("icu.veelume.counter-update.streamDeckPlugin");
    let mut file = std::fs::File::create(&path)?;
    copy(&mut resp.into_reader(), &mut file)?;
    Ok(path)
}

/// Open the downloaded plugin with its default handler so the Stream Deck app
/// installs it.
#[cfg(target_os = "windows")]
fn open_installer(path: &Path) -> anyhow::Result<()> {
    // `start` needs an (empty) title arg first when a path is quoted.
    std::process::Command::new("cmd")
        .arg("/C")
        .arg("start")
        .arg("")
        .arg(path)
        .spawn()?;
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn open_installer(path: &Path) -> anyhow::Result<()> {
    std::process::Command::new("open").arg(path).spawn()?;
    Ok(())
}

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
        assert!(!is_newer("0.1.8", "0.1.8")); // 4th part / tag noise ignored
    }
}
