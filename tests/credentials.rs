use std::path::PathBuf;

fn scratch(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "selenium-credentials-{label}-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn loading_a_literal_secret_flow_fails_without_echoing_the_secret() {
    let root = scratch("literal-secret");
    let path = root.join("login.yaml");
    std::fs::write(&path, "name: login\nsteps:\n  - action: type_text\n    target: { css: '#password' }\n    text: audit-sensitive-value\n    secret: true\n").unwrap();
    let result = selenium_mcp::flow::load_flow(&path);
    assert!(
        result.is_err(),
        "literal secret flows must be rejected before browser launch"
    );
    assert!(!result
        .err()
        .unwrap()
        .to_string()
        .contains("audit-sensitive-value"));
}

#[test]
fn loading_a_secret_env_reference_still_succeeds() {
    let root = scratch("env-secret");
    let path = root.join("login.yaml");
    std::fs::write(&path, "name: login\nenv: [TEST_PASSWORD]\nsteps:\n  - action: type_text\n    target: { css: '#password' }\n    text: '{env.TEST_PASSWORD}'\n    secret: true\n").unwrap();
    assert!(selenium_mcp::flow::load_flow(&path).is_ok());
}

#[tokio::test]
async fn disabling_memory_overrides_even_an_external_endpoint() {
    std::env::set_var("SELENIUM_MCP_DISABLE_MEMORY", "1");
    std::env::set_var("CHROMA_URL", "http://127.0.0.1:9");
    let root = scratch("disabled-memory");
    let chroma =
        selenium_mcp::chroma::ChromaHandle::new(root.clone(), std::time::Duration::from_secs(10));
    assert!(chroma.ensure_client().await.is_err());
    assert!(!root.join("chroma").exists());
}
