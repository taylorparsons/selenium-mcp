use rmcp::handler::server::wrapper::Parameters;
use selenium_mcp::project::ensure_layout;
use selenium_mcp::server::{FlowNameArgs, SeleniumMcpServer};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

fn scratch(label: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "selenium-security-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn server(root: &std::path::Path) -> SeleniumMcpServer {
    let paths = ensure_layout(root).unwrap();
    SeleniumMcpServer::new(
        root.join("cache/drivers"),
        paths.artifacts_dir,
        paths.flows_dir,
        "security-test".into(),
    )
}

#[test]
fn runtime_layout_ignores_all_personal_data_and_restricts_access() {
    let root = scratch("private-runtime");
    let paths = ensure_layout(&root).unwrap();
    assert_eq!(
        std::fs::read_to_string(paths.selenium_dir.join(".gitignore")).unwrap(),
        "*\n"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&paths.selenium_dir)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(&paths.profiles_dir)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
}

#[tokio::test]
async fn flow_delete_rejects_traversal_and_preserves_outside_file() {
    let root = scratch("delete-traversal");
    let sentinel = root.join("outside.yaml");
    std::fs::write(&sentinel, "must survive").unwrap();
    let result = server(&root)
        .flow_delete(Parameters(FlowNameArgs {
            name: "../../outside".into(),
        }))
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(true));
    assert_eq!(std::fs::read_to_string(sentinel).unwrap(), "must survive");
}

#[tokio::test]
async fn flow_delete_still_removes_a_valid_named_flow() {
    let root = scratch("delete-valid");
    let app = server(&root);
    let flow = root.join(".selenium-mcp/flows/safe-name_1.yaml");
    std::fs::write(&flow, "synthetic fixture").unwrap();
    let result = app
        .flow_delete(Parameters(FlowNameArgs {
            name: "safe-name_1".into(),
        }))
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(false));
    assert!(!flow.exists());
}

#[cfg(unix)]
#[test]
fn layout_rejects_a_symlinked_runtime_parent() {
    let root = scratch("layout-link");
    let outside = scratch("layout-outside");
    std::os::unix::fs::symlink(&outside, root.join(".selenium-mcp")).unwrap();
    assert!(ensure_layout(&root).is_err());
    assert!(!outside.join("profiles").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn flow_delete_rejects_a_symlinked_storage_directory() {
    let root = scratch("delete-dir-link");
    let app = server(&root);
    let outside = scratch("delete-dir-outside");
    let sentinel = outside.join("sentinel.yaml");
    std::fs::write(&sentinel, "must survive").unwrap();
    let flows = root.join(".selenium-mcp/flows");
    std::fs::remove_dir(&flows).unwrap();
    std::os::unix::fs::symlink(&outside, &flows).unwrap();
    let result = app
        .flow_delete(Parameters(FlowNameArgs {
            name: "sentinel".into(),
        }))
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(true));
    assert_eq!(std::fs::read_to_string(sentinel).unwrap(), "must survive");
}

#[cfg(unix)]
#[tokio::test]
async fn flow_delete_rejects_a_symlinked_flow_file() {
    let root = scratch("delete-file-link");
    let app = server(&root);
    let sentinel = root.join("outside.yaml");
    std::fs::write(&sentinel, "must survive").unwrap();
    let link = root.join(".selenium-mcp/flows/sentinel.yaml");
    std::os::unix::fs::symlink(&sentinel, &link).unwrap();
    let result = app
        .flow_delete(Parameters(FlowNameArgs {
            name: "sentinel".into(),
        }))
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(true));
    assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
    assert_eq!(std::fs::read_to_string(sentinel).unwrap(), "must survive");
}
