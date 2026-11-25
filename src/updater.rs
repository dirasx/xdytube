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

pub fn check_release() -> Result<String> {
    let release = crate::api::http::get_json("https://api.github.com/repos/dirasx/xdytube/releases/latest")?;
    let tag = release.get("tag_name").and_then(|v| v.as_str()).ok_or_else(|| anyhow::anyhow!("Missing release version"))?;
    match release_is_newer(tag, env!("CARGO_PKG_VERSION")) {
        Some(true) => Ok(format!("Update {tag} available on GitHub")),
        Some(false) => Ok(format!("Up to date ({tag})")),
        None => Err(anyhow::anyhow!("Unsupported release version")),
    }
}

#[cfg(test)]
mod tests {
    use super::release_is_newer;

    #[test]
    fn compares_release_tags_with_package_versions() {
        assert_eq!(release_is_newer("R0.1", "0.1.0"), Some(false));
        assert_eq!(release_is_newer("R0.2", "0.1.0"), Some(true));
        assert_eq!(release_is_newer("R0.1.1", "0.1.0"), Some(true));
        assert_eq!(release_is_newer("v0.0.1-alpha", "0.1.0"), None);
        assert_eq!(release_is_newer("not-a-release", "0.1.0"), None);
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
