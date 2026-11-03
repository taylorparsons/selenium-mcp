use sha2::{Digest, Sha256};
use std::io;
use std::path::{Path, PathBuf};

pub struct ProjectPaths {
    pub root: PathBuf,
    pub selenium_dir: PathBuf,
    pub flows_dir: PathBuf,
    pub artifacts_dir: PathBuf,
    pub baselines_dir: PathBuf,
    pub profiles_dir: PathBuf,
}

pub fn resolve_project_root(start: &Path) -> io::Result<PathBuf> {
    let start = start.canonicalize()?;
    let mut current = start.as_path();
    loop {
        if current.join(".git").exists() {
            return Ok(current.to_path_buf());
        }
        match current.parent() {
            Some(parent) => current = parent,
            None => return Ok(start),
        }
    }
}

pub fn project_id(root: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(root.to_string_lossy().as_bytes());
    let digest = hasher.finalize();
    hex_prefix(&digest, 12)
}

pub(crate) fn hex_prefix(bytes: &[u8], hex_chars: usize) -> String {
    let byte_count = hex_chars.div_ceil(2);
    let mut out = String::with_capacity(hex_chars);
    for byte in &bytes[..byte_count] {
        out.push_str(&format!("{byte:02x}"));
    }
    out.truncate(hex_chars);
    out
}

pub fn ensure_layout(root: &Path) -> io::Result<ProjectPaths> {
    let selenium_dir = root.join(".selenium-mcp");
    let flows_dir = selenium_dir.join("flows");
    let artifacts_dir = selenium_dir.join("artifacts");
    // Baselines and flows can contain personal information. All runtime data
    // is private by default; sanitized public examples belong in fixtures/.
    let baselines_dir = selenium_dir.join("baselines");
    // Profiles hold persisted Chrome user-data-dirs, which can contain live
    // session cookies -- unconditionally gitignored, stricter than
    // artifacts/ (test evidence, not credentials).
    let profiles_dir = selenium_dir.join("profiles");
    reject_managed_symlinks(&selenium_dir)?;
    for dir in [&flows_dir, &artifacts_dir, &baselines_dir, &profiles_dir] {
        reject_managed_symlinks(dir)?;
    }
    std::fs::create_dir_all(&flows_dir)?;
    std::fs::create_dir_all(&artifacts_dir)?;
    std::fs::create_dir_all(&baselines_dir)?;
    std::fs::create_dir_all(&profiles_dir)?;
    let runtime_ignore = selenium_dir.join(".gitignore");
    reject_symlink(&runtime_ignore)?;
    std::fs::write(&runtime_ignore, "*\n")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for dir in [
            &selenium_dir,
            &flows_dir,
            &artifacts_dir,
            &baselines_dir,
            &profiles_dir,
        ] {
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
        }
    }
    let gitignore_path = artifacts_dir.join(".gitignore");
    reject_symlink(&gitignore_path)?;
    if !gitignore_path.exists() {
        std::fs::write(&gitignore_path, "*\n")?;
    }
    let profiles_gitignore_path = profiles_dir.join(".gitignore");
    reject_symlink(&profiles_gitignore_path)?;
    if !profiles_gitignore_path.exists() {
        std::fs::write(&profiles_gitignore_path, "*\n")?;
    }
    Ok(ProjectPaths {
        root: root.to_path_buf(),
        selenium_dir,
        flows_dir,
        artifacts_dir,
        baselines_dir,
        profiles_dir,
    })
}

/// All tool-managed names are single portable components, never paths.
pub fn validate_managed_name(name: &str) -> Result<(), crate::error::AppError> {
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(crate::error::AppError::InvalidTarget(
            "invalid managed name — use only letters, digits, '_' or '-'".into(),
        ));
    }
    Ok(())
}

fn reject_symlink(path: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "symlinks are not allowed in managed storage",
        )),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Check each component inside the runtime directory, including that directory
/// itself. System aliases above it (for example macOS /var) are allowed.
pub fn reject_managed_symlinks(directory: &Path) -> io::Result<()> {
    let ancestors: Vec<_> = directory.ancestors().collect();
    let count = ancestors
        .iter()
        .position(|p| p.file_name() == Some(std::ffi::OsStr::new(".selenium-mcp")))
        .map(|i| i + 1)
        .unwrap_or(1);
    for path in ancestors[..count].iter().rev() {
        reject_symlink(path)?;
    }
    Ok(())
}

pub fn managed_path(
    directory: &Path,
    name: &str,
    suffix: &str,
) -> Result<PathBuf, crate::error::AppError> {
    validate_managed_name(name)?;
    reject_managed_symlinks(directory)?;
    let path = directory.join(format!("{name}{suffix}"));
    reject_symlink(&path)?;
    Ok(path)
}

/// Validates a profile name before it is used to build a filesystem path
/// (and eventually a `--user-data-dir` Chrome argument). Only
/// `[a-zA-Z0-9_-]+` is allowed, which rules out path traversal (`../evil`)
/// and path separators (`a/b`) before any filesystem or Chrome operation
/// happens.
pub fn validate_profile_name(name: &str) -> Result<(), crate::error::AppError> {
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(crate::error::AppError::InvalidProfileName(name.to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("selenium-mcp-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn resolve_project_root_finds_git_toplevel() {
        let root = temp_dir("git-toplevel");
        fs::create_dir_all(root.join(".git")).unwrap();
        let nested = root.join("a/b/c");
        fs::create_dir_all(&nested).unwrap();

        let found = resolve_project_root(&nested).unwrap();

        assert_eq!(found, root.canonicalize().unwrap());
    }

    #[test]
    fn resolve_project_root_falls_back_to_start_dir_when_no_git() {
        let dir = temp_dir("no-git");

        let found = resolve_project_root(&dir).unwrap();

        assert_eq!(found, dir.canonicalize().unwrap());
    }

    #[test]
    fn project_id_is_deterministic_and_twelve_hex_chars() {
        let root = PathBuf::from("/tmp/example-project");

        let id_a = project_id(&root);
        let id_b = project_id(&root);

        assert_eq!(id_a, id_b);
        assert_eq!(id_a.len(), 12);
        assert!(id_a.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn project_id_differs_for_different_roots() {
        let a = project_id(&PathBuf::from("/tmp/project-a"));
        let b = project_id(&PathBuf::from("/tmp/project-b"));

        assert_ne!(a, b);
    }

    #[test]
    fn ensure_layout_creates_flows_and_artifacts_dirs_with_gitignore() {
        let root = temp_dir("layout");

        let paths = ensure_layout(&root).unwrap();

        assert!(paths.flows_dir.is_dir());
        assert!(paths.artifacts_dir.is_dir());
        assert_eq!(
            fs::read_to_string(paths.artifacts_dir.join(".gitignore")).unwrap(),
            "*\n"
        );
    }

    #[test]
    fn ensure_layout_creates_a_baselines_dir_with_no_gitignore() {
        let root = temp_dir("layout-baselines");

        let paths = ensure_layout(&root).unwrap();

        assert!(paths.baselines_dir.is_dir());
        assert!(
            !paths.baselines_dir.join(".gitignore").exists(),
            "baselines must be committed, not gitignored like artifacts"
        );
    }

    #[test]
    fn ensure_layout_is_idempotent() {
        let root = temp_dir("layout-idempotent");

        ensure_layout(&root).unwrap();
        let paths = ensure_layout(&root).unwrap();

        assert!(paths.flows_dir.is_dir());
    }

    #[test]
    fn ensure_layout_creates_a_profiles_dir_with_gitignore() {
        let root = temp_dir("layout-profiles");

        let paths = ensure_layout(&root).unwrap();

        assert!(paths.profiles_dir.is_dir());
        assert_eq!(
            fs::read_to_string(paths.profiles_dir.join(".gitignore")).unwrap(),
            "*\n"
        );
    }

    #[test]
    fn validate_profile_name_accepts_safe_names() {
        for name in ["indeed", "my_profile-1", "ABC123"] {
            assert!(
                validate_profile_name(name).is_ok(),
                "expected {name:?} to be accepted"
            );
        }
    }

    #[test]
    fn validate_profile_name_rejects_empty_string() {
        assert!(validate_profile_name("").is_err());
    }

    #[test]
    fn validate_profile_name_rejects_path_traversal() {
        assert!(validate_profile_name("../evil").is_err());
    }

    #[test]
    fn validate_profile_name_rejects_path_separator() {
        assert!(validate_profile_name("a/b").is_err());
    }

    #[test]
    fn validate_profile_name_rejects_spaces() {
        assert!(validate_profile_name("a b").is_err());
    }
}
