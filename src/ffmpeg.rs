use crate::error::AppError;
use std::path::{Path, PathBuf};

/// Discovery order: cached binary at `bin_dir` -> PATH -> download once into
/// `bin_dir`. Mirrors the driver discovery `thirtyfour::WebDriver::managed()`
/// does for chromedriver (D-20260707-1557) — same shape, different binary.
pub fn ensure_ffmpeg(bin_dir: &Path) -> Result<PathBuf, AppError> {
    let cached =
        crate::project::managed_path(bin_dir, "ffmpeg", if cfg!(windows) { ".exe" } else { "" })?;
    if cached.exists() {
        return Ok(cached);
    }

    if ffmpeg_sidecar::command::ffmpeg_is_installed() {
        return Ok(PathBuf::from("ffmpeg"));
    }

    let url = std::env::var("SELENIUM_MCP_FFMPEG_URL").map_err(|_| AppError::Ffmpeg(
        "install a trusted ffmpeg on PATH, or configure SELENIUM_MCP_FFMPEG_URL and SELENIUM_MCP_FFMPEG_SHA256 for a verified download".into()
    ))?;
    let expected = std::env::var("SELENIUM_MCP_FFMPEG_SHA256").map_err(|_| {
        AppError::Ffmpeg(
            "SELENIUM_MCP_FFMPEG_SHA256 is required before downloading an archive".into(),
        )
    })?;
    validate_download(&url, &expected)?;
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let staging = crate::project::managed_path(bin_dir, &format!("download-{unique}"), "")?;
    std::fs::create_dir_all(&staging)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(bin_dir, std::fs::Permissions::from_mode(0o700))?;
        std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o700))?;
    }
    let archive = ffmpeg_sidecar::download::download_ffmpeg_package(&url, &staging)
        .map_err(|e| AppError::Ffmpeg(e.to_string()))?;
    verify_archive(&archive, &expected)?;
    ffmpeg_sidecar::download::unpack_ffmpeg(&archive, bin_dir)
        .map_err(|e| AppError::Ffmpeg(e.to_string()))?;

    if !cached.exists() {
        return Err(AppError::Ffmpeg(
            "ffmpeg download completed but the binary is missing from the cache dir".to_string(),
        ));
    }
    Ok(cached)
}

fn validate_download(url: &str, expected: &str) -> Result<(), AppError> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|_| AppError::Ffmpeg("invalid FFmpeg download URL".into()))?;
    if parsed.scheme() != "https"
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || expected.len() != 64
        || !expected.bytes().all(|c| c.is_ascii_hexdigit())
    {
        return Err(AppError::Ffmpeg("FFmpeg downloads require HTTPS without embedded credentials/query/fragment and a 64-character SHA-256".into()));
    }
    Ok(())
}

fn verify_archive(path: &Path, expected: &str) -> Result<(), AppError> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    let digest = hash.finalize();
    let actual = crate::project::hex_prefix(&digest, 64);
    if actual != expected.to_ascii_lowercase() {
        return Err(AppError::Ffmpeg(
            "FFmpeg archive SHA-256 mismatch; archive was not unpacked".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn download_policy_rejects_unverified_or_credential_bearing_urls() {
        let hash = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";
        assert!(validate_download("https://example.com/ffmpeg.zip", hash).is_ok());
        for url in [
            "http://example.com/ffmpeg.zip",
            "https://user:password@example.com/ffmpeg.zip",
            "https://example.com/ffmpeg.zip?token=synthetic",
            "file:///tmp/ffmpeg.zip",
        ] {
            assert!(validate_download(url, hash).is_err());
        }
        assert!(validate_download("https://example.com/ffmpeg.zip", "invalid").is_err());
    }

    #[test]
    fn mismatched_ffmpeg_archives_are_rejected_before_extraction() {
        let root = temp_dir("sha256");
        let path = root.join("archive.zip");
        fs::write(&path, b"test").unwrap();
        assert!(verify_archive(
            &path,
            "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"
        )
        .is_ok());
        assert!(verify_archive(
            &path,
            "9F86D081884C7D659A2FEAA0C55AD015A3BF4F1B2B0B822CD15D6C15B0F00A08"
        )
        .is_ok());
        assert!(verify_archive(&path, &"0".repeat(64)).is_err());
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "selenium-mcp-ffmpeg-test-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn ensure_ffmpeg_returns_the_cached_binary_when_it_already_exists() {
        let dir = temp_dir("cached");
        let binary_name = if cfg!(windows) {
            "ffmpeg.exe"
        } else {
            "ffmpeg"
        };
        let cached = dir.join(binary_name);
        fs::write(&cached, b"#!/bin/sh\necho fake ffmpeg\n").unwrap();

        let found = ensure_ffmpeg(&dir).unwrap();

        assert_eq!(found, cached);
    }
}
