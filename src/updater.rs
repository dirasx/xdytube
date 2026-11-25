use anyhow::Result;

pub const RELEASES_PAGE: &str = "https://github.com/dirasx/xdytube/releases";

fn numeric_version(value: &str) -> Option<[u64; 3]> {
    let mut result = [0; 3];
    let mut count = 0;
    for (i, part) in value.split('.').enumerate() {
        if i >= 3 || part.is_empty() { return None; }
        result[i] = part.parse().ok()?;
        count += 1;
    }
    (count >= 2).then_some(result)
}

fn release_is_newer(tag: &str, current: &str) -> Option<bool> {
    let tag = tag.strip_prefix('R')?;
    Some(numeric_version(tag)? > numeric_version(current)?)
}

#[derive(Clone, Debug)]
pub struct UpdateAsset {
    pub version: String,
    url: String,
    digest: String,
}

fn select_update_asset(release: &serde_json::Value, current: &str) -> Result<Option<UpdateAsset>> {
    use anyhow::{Context, ensure};
    ensure!(release["draft"] == false && release["prerelease"] == false, "Release is not stable");
    let tag = release["tag_name"].as_str().context("Missing release tag")?;
    if !release_is_newer(tag, current).context("Invalid release version")? { return Ok(None); }
    let parts = numeric_version(tag.strip_prefix('R').context("Invalid release tag")?).context("Invalid release version")?;
    let version = format!("{}.{}.{}", parts[0], parts[1], parts[2]);
    let name = format!("xdytube-{version}-macos-arm64.zip");
    let expected_url = format!("{RELEASES_PAGE}/download/{tag}/{name}");
    let assets = release["assets"].as_array().context("Missing release assets")?;
    let mut found = assets.iter().filter(|a| a["name"] == name && a["state"] == "uploaded");
    let asset = found.next().context("No compatible release archive")?;
    ensure!(found.next().is_none(), "Duplicate release archives");
    let url = asset["browser_download_url"].as_str().context("Missing download URL")?;
    ensure!(url == expected_url, "Unexpected download URL");
    let digest = asset["digest"].as_str().and_then(|d| d.strip_prefix("sha256:")).context("Missing SHA-256")?;
    ensure!(digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit()), "Invalid SHA-256");
    Ok(Some(UpdateAsset { version, url: url.into(), digest: digest.to_ascii_lowercase() }))
}

pub fn check_release() -> Result<Option<UpdateAsset>> {
    let release = crate::api::http::get_json("https://api.github.com/repos/dirasx/xdytube/releases/latest")?;
    select_update_asset(&release, env!("CARGO_PKG_VERSION"))
}

fn swap_packages(old: &std::path::Path, new: &std::path::Path, backup: &std::path::Path) -> Result<()> {
    swap_packages_with(old, new, backup, |from, to| std::fs::rename(from, to))
}

fn swap_packages_with(old: &std::path::Path, new: &std::path::Path, backup: &std::path::Path, rename: impl Fn(&std::path::Path, &std::path::Path) -> std::io::Result<()>) -> Result<()> {
    use anyhow::{Context, ensure};
    ensure!(old.is_dir() && new.is_dir() && !backup.exists(), "Invalid update paths");
    rename(old, backup).context("Could not back up installed app")?;
    if let Err(error) = rename(new, old) {
        rename(backup, old).context("Update failed and backup could not be restored")?;
        return Err(error).context("Could not install update; previous version restored");
    }
    Ok(())
}

fn command_ok(command: &mut std::process::Command, error: &str) -> Result<()> {
    anyhow::ensure!(command.status()?.success(), "{error}");
    Ok(())
}

pub fn prepare_install(asset: UpdateAsset) -> Result<()> {
    use anyhow::{Context, ensure};
    use std::{path::Path, process::Command};
    let exe = std::env::current_exe()?;
    let app = exe.parent().and_then(Path::parent).and_then(Path::parent).context("Not running inside an app bundle")?;
    ensure!(app.file_name().is_some_and(|x| x == "xdytube.app"), "Update requires an installed xdytube.app");
    let parent = app.parent().context("Invalid application path")?;
    let temp = tempfile::Builder::new().prefix("xdytube-download-").tempdir()?;
    let archive = temp.path().join("release.zip");
    // GitHub's HTTPS release metadata supplies the expected SHA-256. The app
    // uses ad-hoc signing, so codesign below checks bundle integrity only; it
    // does not establish an independent developer identity.
    let bytes = crate::api::http::get_bytes(&asset.url, &[])?;
    std::fs::write(&archive, bytes)?;
    let candidate = validate_archive(&archive, &asset.digest, &asset.version, temp.path())?;
    let staging = tempfile::Builder::new().prefix(".xdytube-update-").tempdir_in(parent)?;
    let staged_app = staging.path().join("new.app");
    command_ok(Command::new("/usr/bin/ditto").arg(&candidate).arg(&staged_app), "Could not stage update")?;
    let stage = staging.keep();
    if let Err(error) = Command::new(&exe).arg("--apply-update").arg(std::process::id().to_string()).arg(app).arg(&stage).spawn() {
        let _ = std::fs::remove_dir_all(&stage);
        return Err(error).context("Could not start update helper");
    }
    Ok(())
}

fn validate_archive(archive: &std::path::Path, digest: &str, version: &str, destination: &std::path::Path) -> Result<std::path::PathBuf> {
    use anyhow::ensure;
    use std::{io::Read, path::{Component, Path}, process::Command};
    ensure!(file_sha256(archive)? == digest, "Update checksum mismatch");
    let listing = Command::new("/usr/bin/unzip").args(["-Z", "-1"]).arg(&archive).output()?;
    ensure!(listing.status.success(), "Invalid update archive");
    let listing = std::str::from_utf8(&listing.stdout)?;
    ensure!(!listing.is_empty(), "Empty update archive");
    for line in listing.lines() {
        ensure!(line.starts_with("xdytube.app/") && !line.contains('\\') && Path::new(line).components().all(|c| matches!(c, Component::Normal(_))), "Unsafe archive path");
    }
    let modes = Command::new("/usr/bin/unzip").args(["-Z", "-l"]).arg(&archive).output()?;
    ensure!(modes.status.success() && !String::from_utf8_lossy(&modes.stdout).lines().any(|l| l.starts_with('l')), "Unsafe symbolic link in archive");
    command_ok(Command::new("/usr/bin/ditto").args(["-x", "-k"]).arg(&archive).arg(destination), "Could not extract update")?;
    let candidate = destination.join("xdytube.app");
    command_ok(Command::new("/usr/bin/codesign").args(["--verify", "--deep", "--strict"]).arg(&candidate), "Invalid application signature")?;
    let mut header = [0; 8];
    std::fs::File::open(candidate.join("Contents/MacOS/xdytube"))?.read_exact(&mut header)?;
    ensure!(header == [0xcf, 0xfa, 0xed, 0xfe, 0x0c, 0x00, 0x00, 0x01], "Update is not for Apple Silicon");
    let plist = candidate.join("Contents/Info.plist");
    let read_plist = |key: &str| -> Result<String> {
        let out = Command::new("/usr/bin/plutil").args(["-extract", key, "raw", "-o", "-"]).arg(&plist).output()?;
        ensure!(out.status.success(), "Invalid application metadata");
        Ok(String::from_utf8(out.stdout)?.trim().to_owned())
    };
    ensure!(read_plist("CFBundleIdentifier")? == "io.github.dirasx.xdytube" && read_plist("CFBundleShortVersionString")? == version, "Update identity or version mismatch");
    Ok(candidate)
}

pub fn apply_update(pid: u32, app: &std::path::Path, stage: &std::path::Path) -> Result<()> {
    use anyhow::{Context, ensure};
    ensure!(pid > 0 && pid <= i32::MAX as u32, "Invalid application process");
    ensure!(app.file_name().is_some_and(|x| x == "xdytube.app") && stage.parent() == app.parent() && stage.file_name().is_some_and(|n| n.to_string_lossy().starts_with(".xdytube-update-")), "Invalid update location");
    for _ in 0..200 {
        // SAFETY: signal 0 only checks the presence of a positive process id.
        if unsafe { libc::kill(pid as i32, 0) } != 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) { break; }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    // SAFETY: signal 0 only checks the presence of a positive process id.
    ensure!(unsafe { libc::kill(pid as i32, 0) } != 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH), "Application did not close");
    let result = swap_packages(app, &stage.join("new.app"), &stage.join("backup.app"));
    let message = match &result { Ok(()) => "Update installed successfully".to_owned(), Err(e) => format!("Update failed: {e:#}") };
    let _ = std::fs::write(crate::store::data_dir().join("update-result.txt"), message);
    let backup = stage.join("backup.app");
    let reopen = if app.is_dir() { app } else { backup.as_path() };
    let _ = std::process::Command::new("/usr/bin/open").arg(reopen).spawn().context("Could not reopen app");
    result
}

#[cfg(test)]
mod tests {
    use super::{release_is_newer, select_update_asset, swap_packages, swap_packages_with, validate_archive};

    #[test]
    fn compares_release_tags_with_package_versions() {
        assert_eq!(release_is_newer("R0.1", "0.1.0"), Some(false));
        assert_eq!(release_is_newer("R0.2", "0.1.0"), Some(true));
        assert_eq!(release_is_newer("R0.1.1", "0.1.0"), Some(true));
        assert_eq!(release_is_newer("v0.0.1-alpha", "0.1.0"), None);
        assert_eq!(release_is_newer("not-a-release", "0.1.0"), None);
    }

    #[test]
    fn selects_only_valid_release_assets() {
        let release = serde_json::json!({"tag_name":"R0.2", "draft":false, "prerelease":false,
            "assets":[{"name":"xdytube-0.2.0-macos-arm64.zip", "state":"uploaded",
                "digest":format!("sha256:{}", "a".repeat(64)),
                "browser_download_url":"https://github.com/dirasx/xdytube/releases/download/R0.2/xdytube-0.2.0-macos-arm64.zip"}]});
        let asset = select_update_asset(&release, "0.1.0").unwrap().unwrap();
        assert_eq!(asset.digest, "a".repeat(64));
        assert_eq!(asset.version, "0.2.0");
        assert!(select_update_asset(&release, "0.2.0").unwrap().is_none());
        let mut invalid = release.clone();
        invalid["assets"][0]["browser_download_url"] = "https://example.com/payload.zip".into();
        assert!(select_update_asset(&invalid, "0.1.0").is_err());
        invalid = release.clone();
        invalid["assets"][0]["digest"] = "sha256:xyz".into();
        assert!(select_update_asset(&invalid, "0.1.0").is_err());
        invalid = release.clone();
        invalid["prerelease"] = true.into();
        assert!(select_update_asset(&invalid, "0.1.0").is_err());
    }

    #[test]
    fn replacement_keeps_a_backup_and_preserves_old_app_on_failure() {
        let root = tempfile::tempdir().unwrap();
        let old = root.path().join("xdytube.app");
        let new = root.path().join("new.app");
        let backup = root.path().join("backup.app");
        std::fs::create_dir(&old).unwrap();
        std::fs::write(old.join("version"), "old").unwrap();
        assert!(swap_packages(&old, &new, &backup).is_err());
        assert_eq!(std::fs::read_to_string(old.join("version")).unwrap(), "old");
        std::fs::create_dir(&new).unwrap();
        std::fs::write(new.join("version"), "new").unwrap();
        let count = std::cell::Cell::new(0);
        let simulated_failure = |from: &std::path::Path, to: &std::path::Path| {
            count.set(count.get() + 1);
            if count.get() == 2 { return Err(std::io::Error::other("simulated installation error")); }
            std::fs::rename(from, to)
        };
        assert!(swap_packages_with(&old, &new, &backup, simulated_failure).is_err());
        assert_eq!(std::fs::read_to_string(old.join("version")).unwrap(), "old");
        swap_packages(&old, &new, &backup).unwrap();
        assert_eq!(std::fs::read_to_string(old.join("version")).unwrap(), "new");
        assert_eq!(std::fs::read_to_string(backup.join("version")).unwrap(), "old");
    }

    #[test]
    #[ignore = "requires a downloaded R0.1 release archive"]
    fn validates_published_archive_without_touching_installed_app() {
        let archive = std::path::PathBuf::from(std::env::var_os("XDYTUBE_RELEASE_FIXTURE").unwrap());
        let root = tempfile::tempdir().unwrap();
        let digest = "87fa6973e6bef77d5820e5c483d752a8248221067182efd57a1935b6f4af6139";
        assert!(validate_archive(&archive, &"0".repeat(64), "0.1.0", root.path()).is_err());
        assert!(!root.path().join("xdytube.app").exists());
        let app = validate_archive(&archive, digest, "0.1.0", root.path()).unwrap();
        assert!(app.join("Contents/MacOS/xdytube").is_file());
        let installed = root.path().join("installed.app");
        let backup = root.path().join("backup.app");
        std::fs::create_dir(&installed).unwrap();
        std::fs::write(installed.join("old-version"), "preserve").unwrap();
        swap_packages(&installed, &app, &backup).unwrap();
        assert!(installed.join("Contents/MacOS/xdytube").is_file());
        assert_eq!(std::fs::read_to_string(backup.join("old-version")).unwrap(), "preserve");
    }
}

/// SHA-256 of a file, as lowercase hex.
pub(crate) fn file_sha256(path: &std::path::Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}
