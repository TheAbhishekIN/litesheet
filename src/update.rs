//! Updates: ask GitHub for the latest release, download it, check that it is
//! signed by us, and swap it in for the running app once the app quits.
//!
//! The new version is unpacked next to nothing important (the user's caches
//! folder) and only replaces `LiteSheet.app` after this process has exited,
//! from a small shell script; if anything goes wrong the old app is put back.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

const LATEST_RELEASE: &str = "https://api.github.com/repos/TheAbhishekIN/litesheet/releases/latest";
const ASSET: &str = "LiteSheet-macos.zip";
/// The Apple Developer team that signs LiteSheet. Downloads signed by anyone
/// else are rejected.
const TEAM_ID: &str = "D585ND8TDZ";

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    /// e.g. "0.3.0"
    pub version: String,
    /// The release notes (Markdown).
    pub notes: String,
    /// The release's page on GitHub.
    pub page: String,
    /// The Mac download, if the release has one.
    pub download: Option<String>,
    pub size: u64,
}

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Whether `candidate` (e.g. "v0.3.0") is a newer version than `current`.
/// Pre-releases ("0.3.0-beta.1") never count as newer.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    fn parse(v: &str) -> Option<Vec<u64>> {
        let mut parts: Vec<u64> = v
            .trim()
            .trim_start_matches('v')
            .split('.')
            .map(|p| p.parse().ok())
            .collect::<Option<_>>()?;
        parts.resize(parts.len().max(3), 0);
        Some(parts)
    }
    match (parse(candidate), parse(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

/// The latest published release on GitHub.
pub async fn latest(client: &reqwest::Client) -> Result<Release, String> {
    let resp = client
        .get(LATEST_RELEASE)
        .header("User-Agent", format!("LiteSheet/{}", current_version()))
        .header("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| format!("Couldn't check for updates: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Couldn't check for updates ({})", resp.status()));
    }
    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Couldn't read the update information: {e}"))?;
    let tag = json["tag_name"]
        .as_str()
        .ok_or("The release has no version")?;
    let asset = json["assets"]
        .as_array()
        .and_then(|a| a.iter().find(|a| a["name"] == ASSET));
    Ok(Release {
        version: tag.trim_start_matches('v').to_string(),
        notes: json["body"].as_str().unwrap_or_default().to_string(),
        page: json["html_url"].as_str().unwrap_or_default().to_string(),
        download: asset
            .and_then(|a| a["browser_download_url"].as_str())
            .map(str::to_string),
        size: asset.and_then(|a| a["size"].as_u64()).unwrap_or(0),
    })
}

/// Where downloaded updates are unpacked.
fn staging_dir(version: &str) -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME").ok_or("No home directory")?;
    Ok(PathBuf::from(home)
        .join("Library/Caches/LiteSheet/updates")
        .join(version))
}

/// Download a release, unpack it and verify it. Returns the new app bundle.
/// `progress` counts the bytes received so far.
pub async fn download(
    client: &reqwest::Client,
    release: &Release,
    progress: Arc<AtomicU64>,
) -> Result<PathBuf, String> {
    let url = release
        .download
        .as_deref()
        .ok_or("This release has no Mac download")?;
    let dir = staging_dir(&release.version)?;
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't prepare the update: {e}"))?;

    let mut resp = client
        .get(url)
        .header("User-Agent", format!("LiteSheet/{}", current_version()))
        .timeout(std::time::Duration::from_secs(600))
        .send()
        .await
        .map_err(|e| format!("Couldn't download the update: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Couldn't download the update ({})", resp.status()));
    }
    let mut bytes = Vec::with_capacity(release.size as usize);
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| format!("The download was interrupted: {e}"))?
    {
        bytes.extend_from_slice(&chunk);
        progress.store(bytes.len() as u64, Ordering::Relaxed);
    }
    let zip = dir.join(ASSET);
    std::fs::write(&zip, &bytes).map_err(|e| format!("Couldn't save the update: {e}"))?;
    let release = release.clone();
    tokio::task::spawn_blocking(move || unpack_and_verify(&zip, &release.version))
        .await
        .map_err(|e| e.to_string())?
}

fn run(cmd: &str, args: &[&std::ffi::OsStr]) -> Result<std::process::Output, String> {
    Command::new(cmd)
        .args(args)
        .output()
        .map_err(|e| format!("Couldn't run {cmd}: {e}"))
}

/// Unzip the download and make sure it is a LiteSheet build signed by us,
/// of the version we expect.
fn unpack_and_verify(zip: &Path, version: &str) -> Result<PathBuf, String> {
    let dir = zip.parent().ok_or("Bad download path")?;
    let out = run(
        "/usr/bin/ditto",
        &[
            "-x".as_ref(),
            "-k".as_ref(),
            zip.as_os_str(),
            dir.as_os_str(),
        ],
    )?;
    if !out.status.success() {
        return Err("Couldn't unpack the update".into());
    }
    let _ = std::fs::remove_file(zip);
    let app = dir.join("LiteSheet.app");
    if !app.is_dir() {
        return Err("The update doesn't contain LiteSheet.app".into());
    }

    let verify = run(
        "/usr/bin/codesign",
        &[
            "--verify".as_ref(),
            "--deep".as_ref(),
            "--strict".as_ref(),
            app.as_os_str(),
        ],
    )?;
    if !verify.status.success() {
        return Err("The update's signature is not valid, so it wasn't installed".into());
    }
    let info = run(
        "/usr/bin/codesign",
        &["-dv".as_ref(), "--verbose=2".as_ref(), app.as_os_str()],
    )?;
    let details = String::from_utf8_lossy(&info.stderr);
    if !details
        .lines()
        .any(|l| l.trim() == format!("TeamIdentifier={TEAM_ID}"))
    {
        return Err(
            "The update isn't signed by the LiteSheet developer, so it wasn't installed".into(),
        );
    }

    let plist = app.join("Contents/Info.plist");
    let out = run(
        "/usr/libexec/PlistBuddy",
        &[
            "-c".as_ref(),
            "Print :CFBundleShortVersionString".as_ref(),
            plist.as_os_str(),
        ],
    )?;
    let found = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if found != version {
        return Err(format!(
            "The download is version {found}, not {version}, so it wasn't installed"
        ));
    }
    Ok(app)
}

/// The `LiteSheet.app` this process runs from, if an update can replace it.
pub fn installed_bundle() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let bundle = exe
        .ancestors()
        .nth(3)
        .filter(|p| p.extension().is_some_and(|e| e == "app"))
        .ok_or("LiteSheet isn't running from an app bundle")?
        .to_path_buf();
    if bundle.to_string_lossy().contains("/AppTranslocation/") {
        return Err(
            "Move LiteSheet to your Applications folder to turn on automatic updates".into(),
        );
    }
    let parent = bundle.parent().ok_or("Bad app location")?;
    let probe = parent.join(format!(".litesheet-update-{}", std::process::id()));
    std::fs::write(&probe, b"").map_err(|_| {
        format!(
            "LiteSheet can't update itself in {} (no permission)",
            parent.display()
        )
    })?;
    let _ = std::fs::remove_file(probe);
    Ok(bundle)
}

/// Replace `bundle` with `new_app` once this process exits, then optionally
/// open the new version. If the swap fails the old app is restored.
pub fn install_on_exit(new_app: &Path, bundle: &Path, relaunch: bool) -> Result<(), String> {
    use std::os::unix::process::CommandExt;
    const SCRIPT: &str = r#"
pid="$1"; new="$2"; dest="$3"; relaunch="$4"
while kill -0 "$pid" 2>/dev/null; do sleep 0.2; done
old="$dest.previous"
rm -rf "$old"
if mv "$dest" "$old"; then
  if mv "$new" "$dest"; then rm -rf "$old"; else mv "$old" "$dest"; fi
fi
if [ "$relaunch" = 1 ]; then open "$dest"; fi
"#;
    Command::new("/bin/sh")
        .arg("-c")
        .arg(SCRIPT)
        .arg("litesheet-update")
        .arg(std::process::id().to_string())
        .arg(new_app)
        .arg(bundle)
        .arg(if relaunch { "1" } else { "0" })
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        // Its own process group, so it outlives us.
        .process_group(0)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Couldn't start the updater: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(is_newer("v0.3.0", "0.2.1"));
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(is_newer("1.0", "0.9.9"));
        assert!(!is_newer("v0.2.1", "0.2.1"));
        assert!(!is_newer("0.2.0", "0.2.1"));
        assert!(!is_newer("0.3.0-beta.1", "0.2.1"));
        assert!(!is_newer("nonsense", "0.2.1"));
        assert!(!is_newer("0.3", "0.3.0"));
    }
}
