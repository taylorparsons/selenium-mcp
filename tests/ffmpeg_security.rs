#[cfg(unix)]
#[test]
fn a_symlinked_cached_ffmpeg_is_rejected_before_use() {
    let root =
        std::env::temp_dir().join(format!("selenium-ffmpeg-security-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let outside = root.join("outside");
    std::fs::write(&outside, "synthetic executable").unwrap();
    let cache = root.join("bin");
    std::fs::create_dir_all(&cache).unwrap();
    let link = cache.join("ffmpeg");
    if link.symlink_metadata().is_err() {
        std::os::unix::fs::symlink(&outside, &link).unwrap();
    }
    assert!(selenium_mcp::ffmpeg::ensure_ffmpeg(&cache).is_err());
    assert!(outside.exists());
}
