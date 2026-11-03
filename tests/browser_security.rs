use rmcp::handler::server::wrapper::Parameters;
use selenium_mcp::flow::Target;
use selenium_mcp::server::{
    FlowRunArgs, FlowSaveArgs, NavigateArgs, SeleniumMcpServer, SessionStartArgs, TypeTextArgs,
};
use std::path::{Path, PathBuf};
use thirtyfour::prelude::*;

fn root(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "selenium-browser-security-{label}-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn fixture() -> String {
    format!(
        "file://{}",
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/security.html")
            .canonicalize()
            .unwrap()
            .display()
    )
}

fn driver_cache() -> PathBuf {
    std::env::temp_dir().join("selenium-security-driver-cache")
}

async fn app(label: &str) -> (SeleniumMcpServer, PathBuf) {
    std::env::set_var("SELENIUM_MCP_DISABLE_MEMORY", "1");
    let root = root(label);
    let paths = selenium_mcp::project::ensure_layout(&root).unwrap();
    let app = SeleniumMcpServer::new(
        driver_cache(),
        paths.artifacts_dir,
        paths.flows_dir,
        "security-browser".into(),
    );
    let start = app
        .session_start(Parameters(SessionStartArgs {
            browser: "chrome".into(),
            headless: true,
            viewport_width: Some(1280),
            viewport_height: Some(800),
            profile: None,
        }))
        .await
        .unwrap();
    assert_eq!(start.is_error, Some(false), "{start:?}");
    assert_eq!(
        app.navigate(Parameters(NavigateArgs { url: fixture() }))
            .await
            .unwrap()
            .is_error,
        Some(false)
    );
    (app, root)
}

fn typing(text: &str) -> TypeTextArgs {
    TypeTextArgs {
        target: Target {
            css: Some("#password".into()),
            xpath: None,
            semantic: None,
        },
        text: text.into(),
        clear_first: true,
        secret: false,
        env_var: None,
    }
}

fn save() -> FlowSaveArgs {
    FlowSaveArgs {
        name: "safe-login".into(),
        description: String::new(),
        tags: vec![],
        base_url: None,
        env: vec![],
    }
}

#[tokio::test]
async fn real_browser_index_omits_form_values_and_hidden_content() {
    let mut caps = DesiredCapabilities::chrome();
    caps.set_headless().unwrap();
    let driver = WebDriver::managed(caps)
        .cache_dir(driver_cache())
        .await
        .unwrap();
    driver.goto(fixture()).await.unwrap();
    let ret = driver
        .execute(selenium_mcp::index::INDEX_SCRIPT, vec![])
        .await
        .unwrap();
    let rows: serde_json::Value = ret.convert().unwrap();
    driver.quit().await.unwrap();
    let serialized = serde_json::to_string(&rows).unwrap();
    assert!(
        !serialized.contains("synthetic-"),
        "values or hidden content escaped: {serialized}"
    );
    let rows = rows.as_array().unwrap();
    assert!(rows
        .iter()
        .any(|r| r["css"] == "#password" && r["text"] == ""));
    assert!(rows
        .iter()
        .any(|r| r["css"] == "#submit" && r["text"] == "Sign in"));
    assert!(!rows
        .iter()
        .any(|r| r["css"] == "#csrf" || r["css"] == "#hidden-child"));
}

#[tokio::test]
async fn detected_password_without_reference_blocks_recording() {
    let (app, root) = app("unsaveable").await;
    assert_eq!(
        app.type_text(Parameters(typing("synthetic-sensitive-input")))
            .await
            .unwrap()
            .is_error,
        Some(false)
    );
    let saved = app.flow_save(Parameters(save())).await.unwrap();
    app.session_end().await.unwrap();
    assert_eq!(saved.is_error, Some(true));
    assert!(!format!("{saved:?}").contains("synthetic-sensitive-input"));
    assert!(!root.join(".selenium-mcp/flows/safe-login.yaml").exists());
}

#[tokio::test]
async fn revealed_password_and_case_insensitive_autocomplete_are_sensitive() {
    for id in ["revealed-password", "verification"] {
        let (app, root) = app(id).await;
        let mut args = typing("synthetic-sensitive-input");
        args.target.css = Some(format!("#{id}"));
        assert_eq!(
            app.type_text(Parameters(args)).await.unwrap().is_error,
            Some(false)
        );
        let saved = app.flow_save(Parameters(save())).await.unwrap();
        app.session_end().await.unwrap();
        assert_eq!(
            saved.is_error,
            Some(true),
            "{id} must be treated as sensitive"
        );
        assert!(!root.join(".selenium-mcp/flows/safe-login.yaml").exists());
    }
}

#[tokio::test]
async fn environment_secret_saves_only_a_reference_and_replays_redacted() {
    std::env::set_var(
        "SELENIUM_SECURITY_TEST_PASSWORD",
        "synthetic-sensitive-input",
    );
    let (app, root) = app("env-reference").await;
    let mut args = typing("");
    args.env_var = Some("SELENIUM_SECURITY_TEST_PASSWORD".into());
    args.secret = true;
    assert_eq!(
        app.type_text(Parameters(args)).await.unwrap().is_error,
        Some(false)
    );
    let saved = app.flow_save(Parameters(save())).await.unwrap();
    app.session_end().await.unwrap();
    assert_eq!(saved.is_error, Some(false), "{saved:?}");
    let yaml = std::fs::read_to_string(root.join(".selenium-mcp/flows/safe-login.yaml")).unwrap();
    assert!(!yaml.contains("synthetic-sensitive-input"));
    assert!(yaml.contains("{env.SELENIUM_SECURITY_TEST_PASSWORD}"));
    let report = app
        .flow_run(Parameters(FlowRunArgs {
            name: "safe-login".into(),
            headless: true,
            heal_write_back: false,
            update_baselines: false,
        }))
        .await
        .unwrap();
    assert_eq!(report.is_error, Some(false), "{report:?}");
    assert!(!format!("{report:?}").contains("synthetic-sensitive-input"));
}
