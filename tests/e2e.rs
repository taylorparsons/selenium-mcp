use rmcp::handler::server::wrapper::Parameters;
use selenium_mcp::flow::{AssertCondition, Target};
use selenium_mcp::server::{
    run_flow, AssertThatArgs, AssertVisualArgs, BaselineSaveArgs, ClickArgs, FindSemanticArgs,
    FlowNameArgs, FlowSaveArgs, KnowledgeSearchArgs, NavigateArgs, ProfileClearArgs,
    RecordStartArgs, RecordStopArgs, RunFlowOptions, RunHistoryArgs, ScreenshotArgs,
    SeleniumMcpServer, SessionStartArgs, TypeTextArgs,
};

fn driver_cache_dir() -> std::path::PathBuf {
    std::env::temp_dir().join("selenium-mcp-e2e-driver-cache")
}

fn scratch_artifacts_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "selenium-mcp-e2e-artifacts-{name}-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn scratch_flows_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "selenium-mcp-e2e-flows-{name}-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn scratch_baselines_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "selenium-mcp-e2e-baselines-{name}-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn fixture_url(name: &str) -> String {
    let path = std::env::current_dir().unwrap().join("fixtures").join(name);
    format!("file://{}", path.canonicalize().unwrap().display())
}

fn chrome_args(headless: bool) -> SessionStartArgs {
    SessionStartArgs {
        browser: "chrome".to_string(),
        headless,
        viewport_width: Some(1280),
        viewport_height: Some(800),
        profile: None,
    }
}

fn scratch_profiles_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "selenium-mcp-e2e-profiles-{name}-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// One Chroma process and one loaded embedding model shared across every
/// test in this binary — spawning Chroma or loading the ONNX model per test
/// would be both slow and resource-heavy under `cargo test`'s default
/// parallel execution. `ensure_client()` already serializes concurrent
/// spawns on the same handle via its internal mutex, so this is safe to
/// call from many tests at once.
fn shared_semantic_deps() -> (
    selenium_mcp::chroma::ChromaHandle,
    selenium_mcp::embedding::EmbeddingEngine,
) {
    static SHARED: std::sync::OnceLock<(
        selenium_mcp::chroma::ChromaHandle,
        selenium_mcp::embedding::EmbeddingEngine,
    )> = std::sync::OnceLock::new();
    SHARED
        .get_or_init(|| {
            let chroma = selenium_mcp::chroma::ChromaHandle::new(
                std::env::temp_dir().join(format!(
                    "selenium-mcp-e2e-shared-chroma-{}",
                    std::process::id()
                )),
                selenium_mcp::chroma::DEFAULT_IDLE_TIMEOUT,
            );
            let embedder =
                selenium_mcp::embedding::EmbeddingEngine::new(std::env::temp_dir().join(format!(
                    "selenium-mcp-e2e-shared-models-{}",
                    std::process::id()
                )));
            (chroma, embedder)
        })
        .clone()
}

/// Standard server construction for tests that don't need a custom
/// artifacts/flows dir: unique per-test project_id (so each test's indexed
/// elements land in their own Chroma collection) plus the shared semantic
/// deps above.
fn test_server(name: &str) -> SeleniumMcpServer {
    let (chroma, embedder) = shared_semantic_deps();
    SeleniumMcpServer::new(
        driver_cache_dir(),
        scratch_artifacts_dir(name),
        scratch_flows_dir(name),
        format!("e2e_{}", name.replace('-', "_")),
    )
    .with_semantic_deps(chroma, embedder)
}

#[tokio::test]
async fn session_start_then_session_end_succeeds() {
    let server = test_server("basic");

    let start = server
        .session_start(Parameters(chrome_args(true)))
        .await
        .unwrap();
    assert_eq!(start.is_error, Some(false));

    let end = server.session_end().await.unwrap();
    assert_eq!(end.is_error, Some(false));
}

#[tokio::test]
async fn session_end_without_a_session_reports_no_active_session() {
    let server = test_server("no-session");

    let end = server.session_end().await.unwrap();

    assert_eq!(end.is_error, Some(true));
    let text = &end.content[0].as_text().unwrap().text;
    assert!(text.contains("no active browser session"));
}

#[tokio::test]
async fn session_start_twice_reports_already_active() {
    let server = test_server("twice");
    server
        .session_start(Parameters(chrome_args(true)))
        .await
        .unwrap();

    let second = server
        .session_start(Parameters(chrome_args(true)))
        .await
        .unwrap();
    assert_eq!(second.is_error, Some(true));

    server.session_end().await.unwrap();
}

#[tokio::test]
async fn navigate_click_and_type_text_drive_the_fixture_form() {
    let server = test_server("form");
    server
        .session_start(Parameters(chrome_args(true)))
        .await
        .unwrap();

    let nav = server
        .navigate(Parameters(NavigateArgs {
            url: fixture_url("login.html"),
        }))
        .await
        .unwrap();
    assert_eq!(nav.is_error, Some(false));

    let typed = server
        .type_text(Parameters(TypeTextArgs {
            target: Target {
                css: Some("#username".to_string()),
                xpath: None,
                semantic: None,
            },
            text: "selenium".to_string(),
            clear_first: false,
            secret: false,
            env_var: None,
        }))
        .await
        .unwrap();
    assert_eq!(typed.is_error, Some(false));

    let clicked = server
        .click(Parameters(ClickArgs {
            target: Target {
                css: Some("#submit-button".to_string()),
                xpath: None,
                semantic: None,
            },
        }))
        .await
        .unwrap();
    assert_eq!(clicked.is_error, Some(false));

    server.session_end().await.unwrap();
}

#[tokio::test]
async fn screenshot_saves_a_real_png_into_the_artifacts_dir() {
    let artifacts_dir = scratch_artifacts_dir("screenshot");

    let (chroma, embedder) = shared_semantic_deps();
    let server = SeleniumMcpServer::new(
        driver_cache_dir(),
        artifacts_dir.clone(),
        scratch_flows_dir("screenshot"),
        "e2e_screenshot".to_string(),
    )
    .with_semantic_deps(chroma, embedder);
    server
        .session_start(Parameters(chrome_args(true)))
        .await
        .unwrap();
    server
        .navigate(Parameters(NavigateArgs {
            url: fixture_url("login.html"),
        }))
        .await
        .unwrap();

    let shot = server
        .screenshot(Parameters(ScreenshotArgs {
            name: Some("login-page".to_string()),
        }))
        .await
        .unwrap();
    assert_eq!(shot.is_error, Some(false));

    let png_path = artifacts_dir.join("login-page.png");
    let bytes = std::fs::read(&png_path).unwrap();
    assert_eq!(
        &bytes[0..8],
        b"\x89PNG\r\n\x1a\n",
        "file must start with the PNG magic bytes"
    );

    server.session_end().await.unwrap();
}

#[tokio::test]
async fn record_start_and_record_stop_produce_a_valid_mp4_and_gif() {
    let server = test_server("recording");
    server
        .session_start(Parameters(chrome_args(true)))
        .await
        .unwrap();
    server
        .navigate(Parameters(NavigateArgs {
            url: fixture_url("login.html"),
        }))
        .await
        .unwrap();

    let start = server
        .record_start(Parameters(RecordStartArgs { fps: Some(5) }))
        .await
        .unwrap();
    assert_eq!(start.is_error, Some(false));

    server
        .type_text(Parameters(TypeTextArgs {
            target: Target {
                css: Some("#username".to_string()),
                xpath: None,
                semantic: None,
            },
            text: "selenium".to_string(),
            clear_first: false,
            secret: false,
            env_var: None,
        }))
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(900)).await;

    let stop = server
        .record_stop(Parameters(RecordStopArgs {
            formats: vec!["mp4".to_string(), "gif".to_string()],
        }))
        .await
        .unwrap();
    assert_eq!(stop.is_error, Some(false));

    let paths: Vec<&str> = stop.content[0].as_text().unwrap().text.lines().collect();
    assert_eq!(paths.len(), 2);
    for path in paths {
        let metadata = std::fs::metadata(path).unwrap();
        assert!(metadata.len() > 0, "{path} must be non-empty");
    }

    server.session_end().await.unwrap();
}

#[tokio::test]
async fn record_stop_without_a_recording_reports_no_active_recording() {
    let server = test_server("no-recording");

    let stop = server
        .record_stop(Parameters(RecordStopArgs {
            formats: vec!["mp4".to_string()],
        }))
        .await
        .unwrap();

    assert_eq!(stop.is_error, Some(true));
    let text = &stop.content[0].as_text().unwrap().text;
    assert!(text.contains("no active recording"));
}

#[tokio::test]
async fn assert_that_reports_pass_and_fail_without_throwing() {
    let server = test_server("assert");
    server
        .session_start(Parameters(chrome_args(true)))
        .await
        .unwrap();
    server
        .navigate(Parameters(NavigateArgs {
            url: fixture_url("login.html"),
        }))
        .await
        .unwrap();

    let passing = server
        .assert_that(Parameters(AssertThatArgs {
            that: AssertCondition {
                element_visible: Some(Target {
                    css: Some("#login-form".to_string()),
                    xpath: None,
                    semantic: None,
                }),
                ..Default::default()
            },
        }))
        .await
        .unwrap();
    assert_eq!(passing.is_error, Some(false));

    let failing = server
        .assert_that(Parameters(AssertThatArgs {
            that: AssertCondition {
                text_visible: Some("this text is not on the page".to_string()),
                ..Default::default()
            },
        }))
        .await
        .unwrap();
    assert_eq!(failing.is_error, Some(true));

    server.session_end().await.unwrap();
}

// -- Persistent profiles (T-004) -------------------------------------------

#[tokio::test]
async fn session_start_rejects_a_path_traversal_profile_name() {
    let server = test_server("profile-traversal");

    let start = server
        .session_start(Parameters(SessionStartArgs {
            browser: "chrome".to_string(),
            headless: true,
            viewport_width: None,
            viewport_height: None,
            profile: Some("../evil".to_string()),
        }))
        .await
        .unwrap();

    assert_eq!(start.is_error, Some(true));
    let text = &start.content[0].as_text().unwrap().text;
    assert!(
        text.contains("invalid profile name"),
        "expected an invalid-profile-name error, got: {text}"
    );
    // No session was started (validation fails before any Chrome process
    // launches), so there is nothing to tear down with session_end.
}

#[tokio::test]
async fn session_start_with_same_profile_persists_login_across_sessions() {
    let (chroma, embedder) = shared_semantic_deps();
    let server = SeleniumMcpServer::new(
        driver_cache_dir(),
        scratch_artifacts_dir("profile-persist"),
        scratch_flows_dir("profile-persist"),
        "e2e_profile_persist".to_string(),
    )
    .with_semantic_deps(chroma, embedder)
    .with_profiles_dir(scratch_profiles_dir("profile-persist"));

    // First session: log in and confirm the logged-in state is reached.
    let start = server
        .session_start(Parameters(SessionStartArgs {
            browser: "chrome".to_string(),
            headless: true,
            viewport_width: Some(1280),
            viewport_height: Some(800),
            profile: Some("p1".to_string()),
        }))
        .await
        .unwrap();
    assert_eq!(start.is_error, Some(false));

    server
        .navigate(Parameters(NavigateArgs {
            url: fixture_url("login-persistent.html"),
        }))
        .await
        .unwrap();

    server
        .type_text(Parameters(TypeTextArgs {
            target: Target {
                css: Some("#username".to_string()),
                xpath: None,
                semantic: None,
            },
            text: "selenium".to_string(),
            clear_first: false,
            secret: false,
            env_var: None,
        }))
        .await
        .unwrap();
    server
        .type_text(Parameters(TypeTextArgs {
            target: Target {
                css: Some("#password".to_string()),
                xpath: None,
                semantic: None,
            },
            text: "hunter2".to_string(),
            clear_first: false,
            secret: false,
            env_var: None,
        }))
        .await
        .unwrap();
    server
        .click(Parameters(ClickArgs {
            target: Target {
                css: Some("#submit-button".to_string()),
                xpath: None,
                semantic: None,
            },
        }))
        .await
        .unwrap();

    let logged_in = server
        .assert_that(Parameters(AssertThatArgs {
            that: AssertCondition {
                element_visible: Some(Target {
                    css: Some("#result".to_string()),
                    xpath: None,
                    semantic: None,
                }),
                ..Default::default()
            },
        }))
        .await
        .unwrap();
    assert_eq!(logged_in.is_error, Some(false));

    server.session_end().await.unwrap();

    // Second session, same profile name, same server instance: no typing or
    // clicking at all -- the logged-in state must already be there.
    let start2 = server
        .session_start(Parameters(SessionStartArgs {
            browser: "chrome".to_string(),
            headless: true,
            viewport_width: Some(1280),
            viewport_height: Some(800),
            profile: Some("p1".to_string()),
        }))
        .await
        .unwrap();
    assert_eq!(start2.is_error, Some(false));

    server
        .navigate(Parameters(NavigateArgs {
            url: fixture_url("login-persistent.html"),
        }))
        .await
        .unwrap();

    let still_logged_in = server
        .assert_that(Parameters(AssertThatArgs {
            that: AssertCondition {
                element_visible: Some(Target {
                    css: Some("#result".to_string()),
                    xpath: None,
                    semantic: None,
                }),
                ..Default::default()
            },
        }))
        .await
        .unwrap();
    assert_eq!(
        still_logged_in.is_error,
        Some(false),
        "expected the persisted profile to carry the logged-in state across sessions"
    );

    server.session_end().await.unwrap();
}

#[tokio::test]
async fn session_start_without_profile_never_carries_state() {
    let server = test_server("profile-none");

    // First session: log in and confirm the logged-in state is reached.
    let start = server
        .session_start(Parameters(chrome_args(true)))
        .await
        .unwrap();
    assert_eq!(start.is_error, Some(false));

    server
        .navigate(Parameters(NavigateArgs {
            url: fixture_url("login-persistent.html"),
        }))
        .await
        .unwrap();

    server
        .type_text(Parameters(TypeTextArgs {
            target: Target {
                css: Some("#username".to_string()),
                xpath: None,
                semantic: None,
            },
            text: "selenium".to_string(),
            clear_first: false,
            secret: false,
            env_var: None,
        }))
        .await
        .unwrap();
    server
        .type_text(Parameters(TypeTextArgs {
            target: Target {
                css: Some("#password".to_string()),
                xpath: None,
                semantic: None,
            },
            text: "hunter2".to_string(),
            clear_first: false,
            secret: false,
            env_var: None,
        }))
        .await
        .unwrap();
    server
        .click(Parameters(ClickArgs {
            target: Target {
                css: Some("#submit-button".to_string()),
                xpath: None,
                semantic: None,
            },
        }))
        .await
        .unwrap();

    let logged_in = server
        .assert_that(Parameters(AssertThatArgs {
            that: AssertCondition {
                element_visible: Some(Target {
                    css: Some("#result".to_string()),
                    xpath: None,
                    semantic: None,
                }),
                ..Default::default()
            },
        }))
        .await
        .unwrap();
    assert_eq!(logged_in.is_error, Some(false));

    server.session_end().await.unwrap();

    // Second session, no profile: fresh ephemeral profile, no carried state.
    let start2 = server
        .session_start(Parameters(chrome_args(true)))
        .await
        .unwrap();
    assert_eq!(start2.is_error, Some(false));

    server
        .navigate(Parameters(NavigateArgs {
            url: fixture_url("login-persistent.html"),
        }))
        .await
        .unwrap();

    let not_logged_in = server
        .assert_that(Parameters(AssertThatArgs {
            that: AssertCondition {
                element_visible: Some(Target {
                    css: Some("#result".to_string()),
                    xpath: None,
                    semantic: None,
                }),
                ..Default::default()
            },
        }))
        .await
        .unwrap();
    assert_eq!(
        not_logged_in.is_error,
        Some(true),
        "expected no carried-over state without a profile"
    );

    server.session_end().await.unwrap();
}

#[tokio::test]
async fn flow_save_writes_the_recorded_actions_as_yaml() {
    let flows_dir = scratch_flows_dir("save");
    let (chroma, embedder) = shared_semantic_deps();
    let server = SeleniumMcpServer::new(
        driver_cache_dir(),
        scratch_artifacts_dir("save"),
        flows_dir.clone(),
        "e2e_save".to_string(),
    )
    .with_semantic_deps(chroma, embedder);
    server
        .session_start(Parameters(chrome_args(true)))
        .await
        .unwrap();
    server
        .navigate(Parameters(NavigateArgs {
            url: fixture_url("login.html"),
        }))
        .await
        .unwrap();
    server
        .click(Parameters(ClickArgs {
            target: Target {
                css: Some("#submit-button".to_string()),
                xpath: None,
                semantic: None,
            },
        }))
        .await
        .unwrap();

    let saved = server
        .flow_save(Parameters(FlowSaveArgs {
            name: "e2e-recorded-flow".to_string(),
            description: "recorded by the e2e test".to_string(),
            tags: vec!["e2e".to_string()],
            base_url: None,
            env: vec![],
        }))
        .await
        .unwrap();
    assert_eq!(saved.is_error, Some(false));

    let loaded = selenium_mcp::flow::load_flow(&flows_dir.join("e2e-recorded-flow.yaml")).unwrap();
    assert_eq!(loaded.steps.len(), 2);

    let listed = server.flow_list().await.unwrap();
    assert!(listed.content[0]
        .as_text()
        .unwrap()
        .text
        .contains("e2e-recorded-flow"));

    let deleted = server
        .flow_delete(Parameters(FlowNameArgs {
            name: "e2e-recorded-flow".to_string(),
        }))
        .await
        .unwrap();
    assert_eq!(deleted.is_error, Some(false));

    server.session_end().await.unwrap();
}

#[tokio::test]
async fn flow_run_replays_a_saved_flow_headlessly_and_reports_pass() {
    let flows_dir = scratch_flows_dir("run");
    let flow = selenium_mcp::flow::Flow {
        name: "run-target".to_string(),
        description: String::new(),
        tags: vec![],
        base_url: None,
        env: vec![],
        created_epoch: None,
        viewport_width: None,
        viewport_height: None,
        profile: None,
        steps: vec![
            selenium_mcp::flow::FlowStep::Navigate {
                url: fixture_url("login.html"),
            },
            selenium_mcp::flow::FlowStep::AssertThat {
                that: AssertCondition {
                    element_visible: Some(Target {
                        css: Some("#login-form".to_string()),
                        xpath: None,
                        semantic: None,
                    }),
                    ..Default::default()
                },
            },
        ],
    };
    selenium_mcp::flow::save_flow(&flows_dir.join("run-target.yaml"), &flow).unwrap();

    let server = SeleniumMcpServer::new(
        driver_cache_dir(),
        scratch_artifacts_dir("run"),
        flows_dir,
        "e2e_run".to_string(),
    );

    let result = server
        .flow_run(Parameters(selenium_mcp::server::FlowRunArgs {
            name: "run-target".to_string(),
            headless: true,
            heal_write_back: false,
            update_baselines: false,
        }))
        .await
        .unwrap();

    assert_eq!(result.is_error, Some(false));
    let report_json = &result.content[0].as_text().unwrap().text;
    assert!(report_json.contains("\"passed\": true"));
}

#[tokio::test]
async fn flow_run_reports_failure_when_an_assertion_fails() {
    let flows_dir = scratch_flows_dir("run-fail");
    let fixture_contents = std::fs::read_to_string("fixtures/flows/failing.yaml").unwrap();
    let resolved = fixture_contents.replace("FIXTURE_URL_PLACEHOLDER", &fixture_url("login.html"));
    std::fs::write(flows_dir.join("failing-fixture.yaml"), resolved).unwrap();

    let server = SeleniumMcpServer::new(
        driver_cache_dir(),
        scratch_artifacts_dir("run-fail"),
        flows_dir,
        "e2e_run_fail".to_string(),
    );

    let result = server
        .flow_run(Parameters(selenium_mcp::server::FlowRunArgs {
            name: "failing-fixture".to_string(),
            headless: true,
            heal_write_back: false,
            update_baselines: false,
        }))
        .await
        .unwrap();

    assert_eq!(result.is_error, Some(true));
}

// -- Flow-level persistent profile (T-005) ---------------------------------

#[tokio::test]
async fn flow_with_secret_type_text_and_profile_reaches_authenticated_state_and_persists() {
    std::env::set_var("SELENIUM_E2E_FIXTURE_PASSWORD", "hunter2");
    let (chroma, embedder) = shared_semantic_deps();
    let profiles_dir = scratch_profiles_dir("flow-profile");

    let login_flow = selenium_mcp::flow::Flow {
        name: "profile-login".to_string(),
        description: String::new(),
        tags: vec![],
        base_url: None,
        env: vec!["SELENIUM_E2E_FIXTURE_PASSWORD".into()],
        created_epoch: None,
        viewport_width: None,
        viewport_height: None,
        profile: Some("flow-p1".to_string()),
        steps: vec![
            selenium_mcp::flow::FlowStep::Navigate {
                url: fixture_url("login-persistent.html"),
            },
            selenium_mcp::flow::FlowStep::TypeText {
                target: Target {
                    css: Some("#username".to_string()),
                    xpath: None,
                    semantic: None,
                },
                text: "selenium".to_string(),
                clear_first: false,
                secret: false,
            },
            selenium_mcp::flow::FlowStep::TypeText {
                target: Target {
                    css: Some("#password".to_string()),
                    xpath: None,
                    semantic: None,
                },
                text: "{env.SELENIUM_E2E_FIXTURE_PASSWORD}".to_string(),
                clear_first: false,
                secret: true,
            },
            selenium_mcp::flow::FlowStep::Click {
                target: Target {
                    css: Some("#submit-button".to_string()),
                    xpath: None,
                    semantic: None,
                },
            },
            selenium_mcp::flow::FlowStep::AssertThat {
                that: AssertCondition {
                    element_visible: Some(Target {
                        css: Some("#result".to_string()),
                        xpath: None,
                        semantic: None,
                    }),
                    ..Default::default()
                },
            },
        ],
    };

    let report = run_flow(
        &login_flow,
        RunFlowOptions {
            driver_cache_dir: &driver_cache_dir(),
            headless: true,
            chroma: &chroma,
            embedder: &embedder,
            project_id: "e2e_flow_profile",
            heal_write_back: false,
            flow_path: None,
            baselines_dir: &scratch_baselines_dir("flow-profile"),
            artifacts_dir: &scratch_artifacts_dir("flow-profile"),
            update_baselines: false,
            profiles_dir: &profiles_dir,
        },
    )
    .await;
    assert!(report.passed, "login flow did not pass: {report:?}");

    // Second, separate run_flow invocation (a fresh Chrome launch) against
    // the same profile: no typing or clicking at all -- the logged-in state
    // must already be there if the flow-level profile persisted it.
    let revisit_flow = selenium_mcp::flow::Flow {
        name: "profile-login-revisit".to_string(),
        description: String::new(),
        tags: vec![],
        base_url: None,
        env: vec![],
        created_epoch: None,
        viewport_width: None,
        viewport_height: None,
        profile: Some("flow-p1".to_string()),
        steps: vec![
            selenium_mcp::flow::FlowStep::Navigate {
                url: fixture_url("login-persistent.html"),
            },
            selenium_mcp::flow::FlowStep::AssertThat {
                that: AssertCondition {
                    element_visible: Some(Target {
                        css: Some("#result".to_string()),
                        xpath: None,
                        semantic: None,
                    }),
                    ..Default::default()
                },
            },
        ],
    };

    let revisit_report = run_flow(
        &revisit_flow,
        RunFlowOptions {
            driver_cache_dir: &driver_cache_dir(),
            headless: true,
            chroma: &chroma,
            embedder: &embedder,
            project_id: "e2e_flow_profile",
            heal_write_back: false,
            flow_path: None,
            baselines_dir: &scratch_baselines_dir("flow-profile"),
            artifacts_dir: &scratch_artifacts_dir("flow-profile"),
            update_baselines: false,
            profiles_dir: &profiles_dir,
        },
    )
    .await;
    assert!(
        revisit_report.passed,
        "revisit flow did not pass (profile did not persist login): {revisit_report:?}"
    );
}

// -- profile_clear tool (T-006) ---------------------------------------------

#[tokio::test]
async fn profile_clear_removes_an_existing_profile_directory() {
    let (chroma, embedder) = shared_semantic_deps();
    let profiles_dir = scratch_profiles_dir("clear-existing");
    let server = SeleniumMcpServer::new(
        driver_cache_dir(),
        scratch_artifacts_dir("clear-existing"),
        scratch_flows_dir("clear-existing"),
        "e2e_clear_existing".to_string(),
    )
    .with_semantic_deps(chroma, embedder)
    .with_profiles_dir(profiles_dir.clone());

    let profile_path = profiles_dir.join("p1");
    std::fs::create_dir_all(&profile_path).unwrap();
    std::fs::write(profile_path.join("Default"), b"dummy chrome profile data").unwrap();

    let result = server
        .profile_clear(Parameters(ProfileClearArgs {
            name: "p1".to_string(),
        }))
        .await
        .unwrap();

    assert_eq!(result.is_error, Some(false));
    let text = &result.content[0].as_text().unwrap().text;
    assert!(
        text.contains("cleared"),
        "expected a 'cleared' message, got: {text}"
    );
    assert!(!profile_path.exists());
}

#[tokio::test]
async fn profile_clear_on_a_never_created_profile_succeeds_with_a_distinct_message() {
    let (chroma, embedder) = shared_semantic_deps();
    let profiles_dir = scratch_profiles_dir("clear-missing");
    let server = SeleniumMcpServer::new(
        driver_cache_dir(),
        scratch_artifacts_dir("clear-missing"),
        scratch_flows_dir("clear-missing"),
        "e2e_clear_missing".to_string(),
    )
    .with_semantic_deps(chroma, embedder)
    .with_profiles_dir(profiles_dir);

    let result = server
        .profile_clear(Parameters(ProfileClearArgs {
            name: "never-created".to_string(),
        }))
        .await
        .unwrap();

    assert_eq!(result.is_error, Some(false));
    let text = &result.content[0].as_text().unwrap().text;
    assert!(
        text.contains("nothing to clear"),
        "expected a 'nothing to clear' message, got: {text}"
    );
}

#[tokio::test]
async fn profile_clear_rejects_an_invalid_profile_name() {
    let (chroma, embedder) = shared_semantic_deps();
    let profiles_dir = scratch_profiles_dir("clear-invalid");
    let server = SeleniumMcpServer::new(
        driver_cache_dir(),
        scratch_artifacts_dir("clear-invalid"),
        scratch_flows_dir("clear-invalid"),
        "e2e_clear_invalid".to_string(),
    )
    .with_semantic_deps(chroma, embedder)
    .with_profiles_dir(profiles_dir);

    let result = server
        .profile_clear(Parameters(ProfileClearArgs {
            name: "../evil".to_string(),
        }))
        .await
        .unwrap();

    assert_eq!(result.is_error, Some(true));
    let text = &result.content[0].as_text().unwrap().text;
    assert!(
        text.contains("invalid profile name"),
        "expected an invalid-profile-name error, got: {text}"
    );
}

// -- Self-healing (T-008a) -------------------------------------------------

fn broken_heal_flow(name: &str) -> selenium_mcp::flow::Flow {
    selenium_mcp::flow::Flow {
        name: name.to_string(),
        description: String::new(),
        tags: vec![],
        base_url: None,
        env: vec![],
        created_epoch: None,
        viewport_width: None,
        viewport_height: None,
        profile: None,
        steps: vec![
            selenium_mcp::flow::FlowStep::Navigate {
                url: fixture_url("login.html"),
            },
            selenium_mcp::flow::FlowStep::Click {
                target: Target {
                    css: Some("#this-selector-does-not-exist".to_string()),
                    xpath: None,
                    semantic: Some("the sign in submit button".to_string()),
                },
            },
        ],
    }
}

#[tokio::test]
async fn flow_run_heals_a_broken_selector_and_writes_it_back_when_asked() {
    let flows_dir = scratch_flows_dir("heal-write-back");
    let flow_path = flows_dir.join("heal-target.yaml");
    selenium_mcp::flow::save_flow(&flow_path, &broken_heal_flow("heal-target")).unwrap();

    let (chroma, embedder) = shared_semantic_deps();
    let server = SeleniumMcpServer::new(
        driver_cache_dir(),
        scratch_artifacts_dir("heal-write-back"),
        flows_dir,
        "e2e_heal_write_back".to_string(),
    )
    .with_semantic_deps(chroma, embedder);

    let result = server
        .flow_run(Parameters(selenium_mcp::server::FlowRunArgs {
            name: "heal-target".to_string(),
            headless: true,
            heal_write_back: true,
            update_baselines: false,
        }))
        .await
        .unwrap();

    assert_eq!(result.is_error, Some(false));
    let report_json = &result.content[0].as_text().unwrap().text;
    assert!(
        report_json.contains("\"old_selector\": \"#this-selector-does-not-exist\""),
        "report was: {report_json}"
    );
    assert!(
        report_json.contains("\"new_selector\": \"#submit-button\""),
        "report was: {report_json}"
    );

    let reloaded = selenium_mcp::flow::load_flow(&flow_path).unwrap();
    match &reloaded.steps[1] {
        selenium_mcp::flow::FlowStep::Click { target } => {
            assert_eq!(target.css.as_deref(), Some("#submit-button"));
            assert_eq!(
                target.semantic.as_deref(),
                Some("the sign in submit button"),
                "healing must not clobber the semantic description that made healing possible"
            );
        }
        other => panic!("expected a Click step, got {other:?}"),
    }

    let raw_yaml = std::fs::read_to_string(&flow_path).unwrap();
    assert!(
        !raw_yaml.contains("null"),
        "healed flow YAML must stay clean, got:\n{raw_yaml}"
    );
}

#[tokio::test]
async fn flow_run_heals_but_does_not_write_back_by_default() {
    let flows_dir = scratch_flows_dir("heal-no-write-back");
    let flow_path = flows_dir.join("heal-target.yaml");
    selenium_mcp::flow::save_flow(&flow_path, &broken_heal_flow("heal-target")).unwrap();
    let original_yaml = std::fs::read_to_string(&flow_path).unwrap();

    let (chroma, embedder) = shared_semantic_deps();
    let server = SeleniumMcpServer::new(
        driver_cache_dir(),
        scratch_artifacts_dir("heal-no-write-back"),
        flows_dir,
        "e2e_heal_no_write_back".to_string(),
    )
    .with_semantic_deps(chroma, embedder);

    let result = server
        .flow_run(Parameters(selenium_mcp::server::FlowRunArgs {
            name: "heal-target".to_string(),
            headless: true,
            heal_write_back: false,
            update_baselines: false,
        }))
        .await
        .unwrap();

    assert_eq!(
        result.is_error,
        Some(false),
        "the step should still pass via healing even without write-back"
    );

    let unchanged_yaml = std::fs::read_to_string(&flow_path).unwrap();
    assert_eq!(
        original_yaml, unchanged_yaml,
        "flow file on disk must be untouched when heal_write_back is false"
    );
}

// -- Knowledge base (T-008b) -----------------------------------------------

#[tokio::test]
async fn run_history_and_knowledge_search_find_recorded_runs() {
    let flows_dir = scratch_flows_dir("knowledge");

    let passing_flow = selenium_mcp::flow::Flow {
        name: "kb-passing".to_string(),
        description: String::new(),
        tags: vec![],
        base_url: None,
        env: vec![],
        created_epoch: None,
        viewport_width: None,
        viewport_height: None,
        profile: None,
        steps: vec![
            selenium_mcp::flow::FlowStep::Navigate {
                url: fixture_url("login.html"),
            },
            selenium_mcp::flow::FlowStep::AssertThat {
                that: AssertCondition {
                    element_visible: Some(Target {
                        css: Some("#login-form".to_string()),
                        xpath: None,
                        semantic: None,
                    }),
                    ..Default::default()
                },
            },
        ],
    };
    selenium_mcp::flow::save_flow(&flows_dir.join("kb-passing.yaml"), &passing_flow).unwrap();

    let fixture_contents = std::fs::read_to_string("fixtures/flows/failing.yaml").unwrap();
    let resolved = fixture_contents.replace("FIXTURE_URL_PLACEHOLDER", &fixture_url("login.html"));
    std::fs::write(flows_dir.join("failing-fixture.yaml"), resolved).unwrap();

    let (chroma, embedder) = shared_semantic_deps();
    let server = SeleniumMcpServer::new(
        driver_cache_dir(),
        scratch_artifacts_dir("knowledge"),
        flows_dir,
        "e2e_knowledge".to_string(),
    )
    .with_semantic_deps(chroma, embedder);

    let passing_result = server
        .flow_run(Parameters(selenium_mcp::server::FlowRunArgs {
            name: "kb-passing".to_string(),
            headless: true,
            heal_write_back: false,
            update_baselines: false,
        }))
        .await
        .unwrap();
    assert_eq!(passing_result.is_error, Some(false));

    let failing_result = server
        .flow_run(Parameters(selenium_mcp::server::FlowRunArgs {
            name: "failing-fixture".to_string(),
            headless: true,
            heal_write_back: false,
            update_baselines: false,
        }))
        .await
        .unwrap();
    assert_eq!(failing_result.is_error, Some(true));

    // Chroma's `get()` occasionally lags a write by a beat under rapid
    // back-to-back upserts to the same collection (observed directly: the
    // upsert returns 200 and no error is logged, yet the very next get()
    // sometimes misses it) -- poll briefly rather than assume the first
    // read is authoritative. This is Chroma-side eventual consistency, not
    // an application bug; real callers don't hit it because they don't
    // call run_history microseconds after the write that populates it.
    let mut flow_names: Vec<String> = Vec::new();
    let mut history_text = String::new();
    for _ in 0..10 {
        let history = server
            .run_history(Parameters(RunHistoryArgs {
                flow: None,
                limit: Some(20),
            }))
            .await
            .unwrap();
        assert_eq!(history.is_error, Some(false));
        history_text = history.content[0].as_text().unwrap().text.clone();
        let history_json: serde_json::Value = serde_json::from_str(&history_text).unwrap();
        flow_names = history_json
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["flow_name"].as_str().unwrap().to_string())
            .collect();
        if flow_names.contains(&"kb-passing".to_string())
            && flow_names.contains(&"failing-fixture".to_string())
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }
    let flow_names: Vec<&str> = flow_names.iter().map(String::as_str).collect();
    assert!(
        flow_names.contains(&"kb-passing") && flow_names.contains(&"failing-fixture"),
        "expected both runs in history, got: {history_text}"
    );
    // Newest-first: the failing run was recorded after the passing run.
    let failing_pos = flow_names
        .iter()
        .position(|n| *n == "failing-fixture")
        .unwrap();
    let passing_pos = flow_names.iter().position(|n| *n == "kb-passing").unwrap();
    assert!(
        failing_pos < passing_pos,
        "expected the more recent failing run to sort before the passing run, got order: {flow_names:?}"
    );

    let search = server
        .knowledge_search(Parameters(KnowledgeSearchArgs {
            query: "what failed recently".to_string(),
            top_k: Some(5),
        }))
        .await
        .unwrap();
    assert_eq!(search.is_error, Some(false));
    let search_text = &search.content[0].as_text().unwrap().text;
    let search_json: serde_json::Value = serde_json::from_str(search_text).unwrap();
    let search_names: Vec<&str> = search_json
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["flow_name"].as_str().unwrap())
        .collect();
    assert!(
        search_names.contains(&"failing-fixture"),
        "expected the failing run to appear in knowledge_search results, got: {search_text}"
    );
}

fn run_cli_test(project_dir: &std::path::Path, extra_args: &[&str]) -> std::process::ExitStatus {
    run_cli_test_with_env(project_dir, extra_args, &[])
}

/// Same as `run_cli_test`, plus extra environment variables scoped to just
/// the spawned child process. Never mutates the parent test process's own
/// env (unsafe under `cargo test`'s default parallel execution -- see the
/// Chroma lifecycle tests below for the established discipline this
/// follows).
fn run_cli_test_with_env(
    project_dir: &std::path::Path,
    extra_args: &[&str],
    extra_env: &[(&str, &str)],
) -> std::process::ExitStatus {
    let binary = std::path::PathBuf::from(env!("CARGO_BIN_EXE_selenium-mcp"));
    let mut cmd = std::process::Command::new(binary);
    cmd.arg("test")
        .args(extra_args)
        .current_dir(project_dir)
        .env("HOME", std::env::var("HOME").unwrap_or_default());
    for (key, value) in extra_env {
        cmd.env(key, value);
    }
    cmd.status().unwrap()
}

#[tokio::test]
async fn cli_test_exits_zero_when_every_flow_passes() {
    let project_dir =
        std::env::temp_dir().join(format!("selenium-mcp-e2e-cli-pass-{}", std::process::id()));
    let flows_dir = project_dir.join(".selenium-mcp/flows");
    std::fs::create_dir_all(&flows_dir).unwrap();
    std::fs::create_dir_all(project_dir.join(".git")).unwrap();

    let flow = selenium_mcp::flow::Flow {
        name: "passing".to_string(),
        description: String::new(),
        tags: vec![],
        base_url: None,
        env: vec![],
        created_epoch: None,
        viewport_width: None,
        viewport_height: None,
        profile: None,
        steps: vec![
            selenium_mcp::flow::FlowStep::Navigate {
                url: fixture_url("login.html"),
            },
            selenium_mcp::flow::FlowStep::AssertThat {
                that: AssertCondition {
                    element_visible: Some(Target {
                        css: Some("#login-form".to_string()),
                        xpath: None,
                        semantic: None,
                    }),
                    ..Default::default()
                },
            },
        ],
    };
    selenium_mcp::flow::save_flow(&flows_dir.join("passing.yaml"), &flow).unwrap();

    let status = run_cli_test(&project_dir, &[]);
    assert_eq!(status.code(), Some(0));
}

#[tokio::test]
async fn cli_test_exits_one_when_a_flow_fails() {
    let project_dir =
        std::env::temp_dir().join(format!("selenium-mcp-e2e-cli-fail-{}", std::process::id()));
    let flows_dir = project_dir.join(".selenium-mcp/flows");
    std::fs::create_dir_all(&flows_dir).unwrap();
    std::fs::create_dir_all(project_dir.join(".git")).unwrap();

    let fixture_contents = std::fs::read_to_string("fixtures/flows/failing.yaml").unwrap();
    let resolved = fixture_contents.replace("FIXTURE_URL_PLACEHOLDER", &fixture_url("login.html"));
    std::fs::write(flows_dir.join("failing.yaml"), resolved).unwrap();

    let status = run_cli_test(&project_dir, &[]);
    assert_eq!(status.code(), Some(1));
}

#[tokio::test]
async fn cli_test_exits_two_on_a_malformed_flow_file() {
    let project_dir =
        std::env::temp_dir().join(format!("selenium-mcp-e2e-cli-infra-{}", std::process::id()));
    let flows_dir = project_dir.join(".selenium-mcp/flows");
    std::fs::create_dir_all(&flows_dir).unwrap();
    std::fs::create_dir_all(project_dir.join(".git")).unwrap();
    std::fs::copy("fixtures/flows/bad.yaml", flows_dir.join("bad.yaml")).unwrap();

    let status = run_cli_test(&project_dir, &[]);
    assert_eq!(status.code(), Some(2));
}

// -- Chroma lifecycle (T-007b) --------------------------------------------
//
// These spawn a real `chroma` CLI process on a free port (no mocking, same
// discipline as the thirtyfour/ffmpeg-sidecar e2e tests above). Each test
// uses its own cache dir and collection name so parallel `cargo test`
// execution doesn't race on the same Chroma data directory.

use selenium_mcp::chroma::{ChromaHandle, DEFAULT_IDLE_TIMEOUT};

fn chroma_cache_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "selenium-mcp-e2e-chroma-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[tokio::test]
async fn chroma_spawns_on_demand_and_answers_a_real_count_call() {
    assert!(
        std::env::var("CHROMA_URL").is_err(),
        "unset CHROMA_URL in the ambient environment to run this spawn-path test"
    );
    let handle = ChromaHandle::new(chroma_cache_dir("spawn-count"), DEFAULT_IDLE_TIMEOUT);

    let client = handle.ensure_client().await.unwrap();
    let collection_id = client
        .get_or_create_collection("selenium_mcp_test_spawn_count", "cosine")
        .await
        .unwrap();
    let count = client.count(&collection_id).await.unwrap();

    assert_eq!(count, 0);
}

#[tokio::test]
async fn chroma_idle_shutdown_kills_the_spawned_process() {
    assert!(
        std::env::var("CHROMA_URL").is_err(),
        "unset CHROMA_URL in the ambient environment to run this spawn-path test"
    );
    let handle = ChromaHandle::new(
        chroma_cache_dir("idle-shutdown"),
        std::time::Duration::from_secs(2),
    );

    let client = handle.ensure_client().await.unwrap();
    let url = client.base_url().to_string();
    client
        .get_or_create_collection("selenium_mcp_test_idle", "cosine")
        .await
        .unwrap();

    tokio::time::sleep(std::time::Duration::from_secs(4)).await;

    let dead = reqwest::Client::new()
        .get(format!("{url}/api/v2/heartbeat"))
        .timeout(std::time::Duration::from_secs(1))
        .send()
        .await
        .is_err();
    assert!(
        dead,
        "expected the idle-reaped Chroma process to stop answering heartbeats"
    );
}

#[tokio::test]
async fn chroma_upsert_then_query_round_trips_with_cosine_scores() {
    assert!(
        std::env::var("CHROMA_URL").is_err(),
        "unset CHROMA_URL in the ambient environment to run this spawn-path test"
    );
    let handle = ChromaHandle::new(chroma_cache_dir("upsert-query"), DEFAULT_IDLE_TIMEOUT);
    let client = handle.ensure_client().await.unwrap();
    let collection_id = client
        .get_or_create_collection("selenium_mcp_test_upsert_query", "cosine")
        .await
        .unwrap();

    client
        .upsert(
            &collection_id,
            &["near".to_string(), "far".to_string()],
            &[vec![0.1, 0.2, 0.3], vec![0.9, -0.8, 0.1]],
            &["close doc".to_string(), "far doc".to_string()],
            &[
                serde_json::json!({"kind": "near"}),
                serde_json::json!({"kind": "far"}),
            ],
        )
        .await
        .unwrap();

    let result = client
        .query(&collection_id, &[0.1, 0.2, 0.31], 2, None)
        .await
        .unwrap();

    assert_eq!(result.matches.len(), 2);
    assert_eq!(result.matches[0].id, "near");
    assert_eq!(
        result.matches[0]
            .metadata
            .as_ref()
            .unwrap()
            .get("kind")
            .unwrap(),
        "near"
    );
    assert!(result.matches[0].distance.unwrap() < result.matches[1].distance.unwrap());
}

// -- Element indexing (T-007c) --------------------------------------------

#[tokio::test]
async fn navigate_auto_indexes_the_fixture_page_into_chroma() {
    let (chroma, embedder) = shared_semantic_deps();
    let project_id = "e2e_indexing_test";
    let server = SeleniumMcpServer::new(
        driver_cache_dir(),
        scratch_artifacts_dir("indexing"),
        scratch_flows_dir("indexing"),
        project_id.to_string(),
    )
    .with_semantic_deps(chroma.clone(), embedder);

    server
        .session_start(Parameters(chrome_args(true)))
        .await
        .unwrap();
    let url = fixture_url("login.html");
    server
        .navigate(Parameters(NavigateArgs { url: url.clone() }))
        .await
        .unwrap();
    server.session_end().await.unwrap();

    let client = chroma.ensure_client().await.unwrap();
    let collection_id = client
        .get_or_create_collection(
            &selenium_mcp::index::elements_collection_name(project_id),
            "cosine",
        )
        .await
        .unwrap();

    let count = client.count(&collection_id).await.unwrap();
    assert!(
        count >= 2,
        "expected at least the username input and submit button to be indexed, got {count}"
    );

    let expected_id =
        selenium_mcp::index::element_id(&selenium_mcp::index::normalize_url(&url), "#username");
    let got = client.get(&collection_id, None).await.unwrap();
    assert!(
        got.matches.iter().any(|m| m.id == expected_id),
        "expected the #username element to be indexed under its deterministic sha256 id"
    );
    let username_doc = got
        .matches
        .iter()
        .find(|m| m.id == expected_id)
        .and_then(|m| m.document.clone())
        .unwrap();
    assert!(
        username_doc.contains("input"),
        "document was: {username_doc}"
    );
}

// -- find_semantic (T-007d) ------------------------------------------------

#[tokio::test]
async fn find_semantic_resolves_the_username_input_by_description() {
    let server = test_server("find-semantic");
    server
        .session_start(Parameters(chrome_args(true)))
        .await
        .unwrap();
    server
        .navigate(Parameters(NavigateArgs {
            url: fixture_url("login.html"),
        }))
        .await
        .unwrap();

    let found = server
        .find_semantic(Parameters(FindSemanticArgs {
            query: "the username text input field".to_string(),
            top_k: Some(5),
        }))
        .await
        .unwrap();

    assert_eq!(found.is_error, Some(false));
    let text = &found.content[0].as_text().unwrap().text;
    assert!(
        text.contains("\"css\": \"#username\""),
        "expected #username to resolve, got: {text}"
    );

    server.session_end().await.unwrap();
}

#[tokio::test]
async fn find_semantic_reports_no_match_without_panicking() {
    let server = test_server("find-semantic-nomatch");
    server
        .session_start(Parameters(chrome_args(true)))
        .await
        .unwrap();
    server
        .navigate(Parameters(NavigateArgs {
            url: fixture_url("login.html"),
        }))
        .await
        .unwrap();

    let found = server
        .find_semantic(Parameters(FindSemanticArgs {
            query: "a purple hyperspace teleportation console".to_string(),
            top_k: Some(5),
        }))
        .await
        .unwrap();

    // Every candidate in a 2-element fixture is a real, verifiable element,
    // so a nonsense query still "resolves" to whichever is least-bad --
    // this asserts the call completes and returns well-formed JSON rather
    // than panicking, not that it refuses to match.
    assert!(found.content[0].as_text().is_some());

    server.session_end().await.unwrap();
}

// -- memory_stats (T-008c) -------------------------------------------------

#[tokio::test]
async fn memory_stats_reports_counts_and_disk_usage_when_chroma_is_reachable() {
    let server = test_server("memory-stats");

    let stats = server.memory_stats().await.unwrap();
    assert_eq!(stats.is_error, Some(false));
    let text = &stats.content[0].as_text().unwrap().text;
    let json: serde_json::Value = serde_json::from_str(text).unwrap();

    assert!(
        json["chroma"]["status"] == "connected" || json["chroma"]["status"] == "spawned",
        "expected a reachable Chroma status, got: {text}"
    );
    assert!(json["elements_count"].is_number(), "got: {text}");
    assert!(json["runs_count"].is_number(), "got: {text}");
    assert!(
        json["cache_disk_usage_bytes"].as_u64().unwrap() > 0,
        "expected non-zero cache disk usage (the driver cache alone guarantees this), got: {text}"
    );
}

// The disabled/unreachable path is covered by a pure unit test
// (stats_from_client_reports_disabled_with_no_client_no_network_no_env in
// src/server.rs) instead of an e2e test here — an earlier version of this
// test set CHROMA_URL via std::env::set_var, which is process-global and
// leaked into the concurrently-running reachable-Chroma test above,
// corrupting its result (observed directly: that test saw
// status: "connected" against the unreachable URL this test injected).
// Testing the pure `client: None` branch directly avoids touching process
// env entirely.

// -- Visual regression (T-009c) --------------------------------------------

#[tokio::test]
async fn baseline_save_then_assert_visual_passes_against_an_identical_view() {
    let (chroma, embedder) = shared_semantic_deps();
    let server = SeleniumMcpServer::new(
        driver_cache_dir(),
        scratch_artifacts_dir("visual-pass"),
        scratch_flows_dir("visual-pass"),
        "e2e_visual_pass".to_string(),
    )
    .with_semantic_deps(chroma, embedder)
    .with_baselines_dir(scratch_baselines_dir("visual-pass"));

    server
        .session_start(Parameters(chrome_args(true)))
        .await
        .unwrap();
    server
        .navigate(Parameters(NavigateArgs {
            url: fixture_url("login.html"),
        }))
        .await
        .unwrap();

    let saved = server
        .baseline_save(Parameters(BaselineSaveArgs {
            name: "login-page".to_string(),
            target: None,
        }))
        .await
        .unwrap();
    assert_eq!(saved.is_error, Some(false));

    let result = server
        .assert_visual(Parameters(AssertVisualArgs {
            name: "login-page".to_string(),
            target: None,
            masks: vec![],
            threshold: None,
        }))
        .await
        .unwrap();

    assert_eq!(result.is_error, Some(false));
    let text = &result.content[0].as_text().unwrap().text;
    let json: serde_json::Value = serde_json::from_str(text).unwrap();
    assert_eq!(json["passed"], true);
    assert_eq!(
        json["ratio"], 0.0,
        "identical screenshots must diff to zero, got: {text}"
    );

    server.session_end().await.unwrap();
}

#[tokio::test]
async fn assert_visual_fails_and_writes_a_diff_artifact_when_the_page_changed() {
    let (chroma, embedder) = shared_semantic_deps();
    let artifacts_dir = scratch_artifacts_dir("visual-fail");
    let server = SeleniumMcpServer::new(
        driver_cache_dir(),
        artifacts_dir.clone(),
        scratch_flows_dir("visual-fail"),
        "e2e_visual_fail".to_string(),
    )
    .with_semantic_deps(chroma, embedder)
    .with_baselines_dir(scratch_baselines_dir("visual-fail"));

    server
        .session_start(Parameters(chrome_args(true)))
        .await
        .unwrap();
    server
        .navigate(Parameters(NavigateArgs {
            url: fixture_url("login.html"),
        }))
        .await
        .unwrap();

    server
        .baseline_save(Parameters(BaselineSaveArgs {
            name: "login-page".to_string(),
            target: None,
        }))
        .await
        .unwrap();

    // The fixture's own JS reveals a hidden <p id="result"> on submit --
    // a real, deterministic visual change tied to existing fixture
    // behavior, not a synthetic DOM injection.
    server
        .click(Parameters(ClickArgs {
            target: Target {
                css: Some("#submit-button".to_string()),
                xpath: None,
                semantic: None,
            },
        }))
        .await
        .unwrap();

    // Revealing the fixture's small hidden <p> doesn't move the default
    // 0.1% ratio threshold on a full 1280x800 viewport -- use an explicit
    // tiny threshold so this real, deterministic change (not a synthetic
    // one) reliably crosses it.
    let result = server
        .assert_visual(Parameters(AssertVisualArgs {
            name: "login-page".to_string(),
            target: None,
            masks: vec![],
            threshold: Some(0.00001),
        }))
        .await
        .unwrap();

    assert_eq!(result.is_error, Some(true));
    let text = &result.content[0].as_text().unwrap().text;
    let json: serde_json::Value = serde_json::from_str(text).unwrap();
    assert_eq!(json["passed"], false);
    assert!(json["ratio"].as_f64().unwrap() > 0.0, "got: {text}");

    let diff_image_path = json["diff_image"]
        .as_str()
        .expect("diff_image path expected");
    assert!(
        diff_image_path.starts_with(artifacts_dir.to_str().unwrap()),
        "diff image should live in the artifacts dir, got: {diff_image_path}"
    );
    let bytes = std::fs::read(diff_image_path).unwrap();
    assert_eq!(
        &bytes[0..8],
        b"\x89PNG\r\n\x1a\n",
        "diff artifact must be a real PNG"
    );

    server.session_end().await.unwrap();
}

#[tokio::test]
async fn assert_visual_reports_a_distinct_failure_for_mismatched_baseline_conditions() {
    let (chroma, embedder) = shared_semantic_deps();
    let baselines_dir = scratch_baselines_dir("visual-meta-mismatch");
    let server = SeleniumMcpServer::new(
        driver_cache_dir(),
        scratch_artifacts_dir("visual-meta-mismatch"),
        scratch_flows_dir("visual-meta-mismatch"),
        "e2e_visual_meta_mismatch".to_string(),
    )
    .with_semantic_deps(chroma, embedder)
    .with_baselines_dir(baselines_dir.clone());

    server
        .session_start(Parameters(chrome_args(true)))
        .await
        .unwrap();
    server
        .navigate(Parameters(NavigateArgs {
            url: fixture_url("login.html"),
        }))
        .await
        .unwrap();
    server
        .baseline_save(Parameters(BaselineSaveArgs {
            name: "login-page".to_string(),
            target: None,
        }))
        .await
        .unwrap();

    // Corrupt the recorded baseline metadata to simulate "captured under
    // different conditions" without needing a second browser session at a
    // different viewport size.
    let meta_path = baselines_dir.join("login-page.json");
    let mut meta: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&meta_path).unwrap()).unwrap();
    meta["width"] = serde_json::json!(99999);
    std::fs::write(&meta_path, serde_json::to_string_pretty(&meta).unwrap()).unwrap();

    let result = server
        .assert_visual(Parameters(AssertVisualArgs {
            name: "login-page".to_string(),
            target: None,
            masks: vec![],
            threshold: None,
        }))
        .await
        .unwrap();

    assert_eq!(result.is_error, Some(true));
    let text = &result.content[0].as_text().unwrap().text;
    let json: serde_json::Value = serde_json::from_str(text).unwrap();
    assert_eq!(
        json["reason"], "baseline captured under different conditions",
        "got: {text}"
    );
    assert!(
        json["ratio"].is_null(),
        "a metadata mismatch must not compute a pixel ratio, got: {text}"
    );

    server.session_end().await.unwrap();
}

// -- M6 Hardening (T-010) --------------------------------------------------
//
// FlowStep::AssertVisual wiring, --update-baselines, report.json/--json,
// degradation paths (no Chroma), self-healing's failure branch, and the
// idle memory budget.

#[tokio::test]
async fn flow_run_passes_an_assert_visual_step_against_an_unchanged_page() {
    let name = "visual-step-pass";
    let baselines_dir = scratch_baselines_dir(name);
    let flows_dir = scratch_flows_dir(name);
    let (chroma, embedder) = shared_semantic_deps();
    let server = SeleniumMcpServer::new(
        driver_cache_dir(),
        scratch_artifacts_dir(name),
        flows_dir.clone(),
        format!("e2e_{}", name.replace('-', "_")),
    )
    .with_semantic_deps(chroma, embedder)
    .with_baselines_dir(baselines_dir.clone());

    server
        .session_start(Parameters(chrome_args(true)))
        .await
        .unwrap();
    server
        .navigate(Parameters(NavigateArgs {
            url: fixture_url("login.html"),
        }))
        .await
        .unwrap();
    server
        .baseline_save(Parameters(BaselineSaveArgs {
            name: "login-page".to_string(),
            target: None,
        }))
        .await
        .unwrap();
    server.session_end().await.unwrap();

    let flow = selenium_mcp::flow::Flow {
        name: "visual-check".to_string(),
        description: String::new(),
        tags: vec![],
        base_url: None,
        env: vec![],
        created_epoch: None,
        viewport_width: None,
        viewport_height: None,
        profile: None,
        steps: vec![
            selenium_mcp::flow::FlowStep::Navigate {
                url: fixture_url("login.html"),
            },
            selenium_mcp::flow::FlowStep::AssertVisual {
                baseline: "login-page".to_string(),
                threshold: Some(0.00001),
                masks: vec![],
            },
        ],
    };
    selenium_mcp::flow::save_flow(&flows_dir.join("visual-check.yaml"), &flow).unwrap();

    let result = server
        .flow_run(Parameters(selenium_mcp::server::FlowRunArgs {
            name: "visual-check".to_string(),
            headless: true,
            heal_write_back: false,
            update_baselines: false,
        }))
        .await
        .unwrap();

    assert_eq!(result.is_error, Some(false));
    let text = &result.content[0].as_text().unwrap().text;
    let report: serde_json::Value = serde_json::from_str(text).unwrap();
    assert_eq!(report["passed"], true, "got: {text}");
}

#[tokio::test]
async fn flow_run_fails_an_assert_visual_step_when_the_page_changed_and_writes_a_diff_artifact() {
    let name = "visual-step-fail";
    let baselines_dir = scratch_baselines_dir(name);
    let artifacts_dir = scratch_artifacts_dir(name);
    let flows_dir = scratch_flows_dir(name);
    let (chroma, embedder) = shared_semantic_deps();
    let server = SeleniumMcpServer::new(
        driver_cache_dir(),
        artifacts_dir.clone(),
        flows_dir.clone(),
        format!("e2e_{}", name.replace('-', "_")),
    )
    .with_semantic_deps(chroma, embedder)
    .with_baselines_dir(baselines_dir.clone());

    server
        .session_start(Parameters(chrome_args(true)))
        .await
        .unwrap();
    server
        .navigate(Parameters(NavigateArgs {
            url: fixture_url("login.html"),
        }))
        .await
        .unwrap();
    server
        .baseline_save(Parameters(BaselineSaveArgs {
            name: "login-page".to_string(),
            target: None,
        }))
        .await
        .unwrap();
    server.session_end().await.unwrap();

    // Same real-fixture-change technique as the assert_visual tool test
    // (clicking #submit-button reveals the fixture's real hidden <p>) --
    // and the same tight threshold, since the default 0.1% ratio isn't
    // crossed by this small a change on a full 1280x800 viewport.
    let flow = selenium_mcp::flow::Flow {
        name: "visual-check-fail".to_string(),
        description: String::new(),
        tags: vec![],
        base_url: None,
        env: vec![],
        created_epoch: None,
        viewport_width: None,
        viewport_height: None,
        profile: None,
        steps: vec![
            selenium_mcp::flow::FlowStep::Navigate {
                url: fixture_url("login.html"),
            },
            selenium_mcp::flow::FlowStep::Click {
                target: Target {
                    css: Some("#submit-button".to_string()),
                    xpath: None,
                    semantic: None,
                },
            },
            selenium_mcp::flow::FlowStep::AssertVisual {
                baseline: "login-page".to_string(),
                threshold: Some(0.00001),
                masks: vec![],
            },
        ],
    };
    selenium_mcp::flow::save_flow(&flows_dir.join("visual-check-fail.yaml"), &flow).unwrap();

    let result = server
        .flow_run(Parameters(selenium_mcp::server::FlowRunArgs {
            name: "visual-check-fail".to_string(),
            headless: true,
            heal_write_back: false,
            update_baselines: false,
        }))
        .await
        .unwrap();

    assert_eq!(result.is_error, Some(true));
    let text = &result.content[0].as_text().unwrap().text;
    let report: serde_json::Value = serde_json::from_str(text).unwrap();
    assert_eq!(report["passed"], false, "got: {text}");
    let visual_step = &report["steps"][2];
    assert_eq!(visual_step["passed"], false);
    let error_msg = visual_step["error"]
        .as_str()
        .expect("expected an error message");
    assert!(
        error_msg.contains("diff image:"),
        "expected the diff image path inlined in the step error, got: {error_msg}"
    );

    let diff_files: Vec<_> = std::fs::read_dir(&artifacts_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("visual-diff-login-page-")
        })
        .collect();
    assert_eq!(diff_files.len(), 1, "expected exactly one diff artifact");
    let bytes = std::fs::read(diff_files[0].path()).unwrap();
    assert_eq!(
        &bytes[0..8],
        &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
        "diff artifact should be a real PNG"
    );
}

#[tokio::test]
async fn flow_run_creates_a_baseline_via_an_assert_visual_step_when_update_baselines_is_true() {
    let name = "visual-step-update";
    let baselines_dir = scratch_baselines_dir(name);
    let flows_dir = scratch_flows_dir(name);
    let (chroma, embedder) = shared_semantic_deps();
    let server = SeleniumMcpServer::new(
        driver_cache_dir(),
        scratch_artifacts_dir(name),
        flows_dir.clone(),
        format!("e2e_{}", name.replace('-', "_")),
    )
    .with_semantic_deps(chroma, embedder)
    .with_baselines_dir(baselines_dir.clone());

    let flow = selenium_mcp::flow::Flow {
        name: "visual-update".to_string(),
        description: String::new(),
        tags: vec![],
        base_url: None,
        env: vec![],
        created_epoch: None,
        viewport_width: None,
        viewport_height: None,
        profile: None,
        steps: vec![
            selenium_mcp::flow::FlowStep::Navigate {
                url: fixture_url("login.html"),
            },
            selenium_mcp::flow::FlowStep::AssertVisual {
                baseline: "fresh-baseline".to_string(),
                threshold: None,
                masks: vec![],
            },
        ],
    };
    selenium_mcp::flow::save_flow(&flows_dir.join("visual-update.yaml"), &flow).unwrap();
    assert!(!baselines_dir.join("fresh-baseline.png").exists());

    let result = server
        .flow_run(Parameters(selenium_mcp::server::FlowRunArgs {
            name: "visual-update".to_string(),
            headless: true,
            heal_write_back: false,
            update_baselines: true,
        }))
        .await
        .unwrap();

    assert_eq!(
        result.is_error,
        Some(false),
        "capturing a baseline should never itself fail the step"
    );
    assert!(baselines_dir.join("fresh-baseline.png").exists());
    assert!(baselines_dir.join("fresh-baseline.json").exists());
}

#[tokio::test]
async fn flow_run_reports_a_clean_failure_when_selector_is_broken_and_semantic_healing_also_fails()
{
    let name = "heal-attempt-fails";
    let flows_dir = scratch_flows_dir(name);
    let (chroma, embedder) = shared_semantic_deps();
    let server = SeleniumMcpServer::new(
        driver_cache_dir(),
        scratch_artifacts_dir(name),
        flows_dir.clone(),
        format!("e2e_{}", name.replace('-', "_")),
    )
    .with_semantic_deps(chroma, embedder);

    // resolve_semantic has no relevance threshold -- it live-verifies
    // whatever candidates come back and resolves to the first one that's
    // still present and visible. So a nonsense query on the *same* page
    // that's freshly indexed would still resolve to something. To force a
    // genuine "attempted, nothing verified" outcome: index login.html
    // (step 1), then move to a page with none of those elements (step 2) --
    // resolve_semantic's fallback (no URL filter) query then only has
    // login.html's stale candidates, none of which exist on this page, so
    // every one fails live-DOM verification.
    let flow = selenium_mcp::flow::Flow {
        name: "unhealable".to_string(),
        description: String::new(),
        tags: vec![],
        base_url: None,
        env: vec![],
        created_epoch: None,
        viewport_width: None,
        viewport_height: None,
        profile: None,
        steps: vec![
            selenium_mcp::flow::FlowStep::Navigate {
                url: fixture_url("login.html"),
            },
            selenium_mcp::flow::FlowStep::Navigate {
                url: "about:blank".to_string(),
            },
            selenium_mcp::flow::FlowStep::Click {
                target: Target {
                    css: Some("#this-selector-does-not-exist".to_string()),
                    xpath: None,
                    semantic: Some("the sign in submit button".to_string()),
                },
            },
        ],
    };
    selenium_mcp::flow::save_flow(&flows_dir.join("unhealable.yaml"), &flow).unwrap();

    let result = server
        .flow_run(Parameters(selenium_mcp::server::FlowRunArgs {
            name: "unhealable".to_string(),
            headless: true,
            heal_write_back: false,
            update_baselines: false,
        }))
        .await
        .unwrap();

    assert_eq!(result.is_error, Some(true));
    let text = &result.content[0].as_text().unwrap().text;
    let report: serde_json::Value = serde_json::from_str(text).unwrap();
    assert_eq!(report["passed"], false, "got: {text}");
    let click_step = &report["steps"][2];
    assert_eq!(click_step["passed"], false);
    assert!(
        click_step["error"].as_str().is_some(),
        "the failed click must carry an error, not panic, got: {text}"
    );
    assert!(
        click_step
            .get("healed")
            .map(|v| v.is_null())
            .unwrap_or(true),
        "healing should not have found a verifiable candidate on a page with none \
         of the indexed elements, got: {text}"
    );
}

#[tokio::test]
async fn cli_test_writes_report_json_into_the_artifacts_dir_by_default() {
    let project_dir = std::env::temp_dir().join(format!(
        "selenium-mcp-e2e-cli-report-json-{}",
        std::process::id()
    ));
    let flows_dir = project_dir.join(".selenium-mcp/flows");
    std::fs::create_dir_all(&flows_dir).unwrap();
    std::fs::create_dir_all(project_dir.join(".git")).unwrap();

    let flow = selenium_mcp::flow::Flow {
        name: "passing".to_string(),
        description: String::new(),
        tags: vec![],
        base_url: None,
        env: vec![],
        created_epoch: None,
        viewport_width: None,
        viewport_height: None,
        profile: None,
        steps: vec![
            selenium_mcp::flow::FlowStep::Navigate {
                url: fixture_url("login.html"),
            },
            selenium_mcp::flow::FlowStep::AssertThat {
                that: AssertCondition {
                    element_visible: Some(Target {
                        css: Some("#login-form".to_string()),
                        xpath: None,
                        semantic: None,
                    }),
                    ..Default::default()
                },
            },
        ],
    };
    selenium_mcp::flow::save_flow(&flows_dir.join("passing.yaml"), &flow).unwrap();

    let status = run_cli_test(&project_dir, &[]);
    assert_eq!(status.code(), Some(0));

    let report_path = project_dir.join(".selenium-mcp/artifacts/report.json");
    let contents = std::fs::read_to_string(&report_path).expect("report.json should exist");
    let report: serde_json::Value = serde_json::from_str(&contents).unwrap();
    assert_eq!(report["outcome"], "all_passed", "got: {contents}");
    assert_eq!(report["reports"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn cli_test_json_flag_overrides_the_report_path() {
    let project_dir = std::env::temp_dir().join(format!(
        "selenium-mcp-e2e-cli-json-flag-{}",
        std::process::id()
    ));
    let flows_dir = project_dir.join(".selenium-mcp/flows");
    std::fs::create_dir_all(&flows_dir).unwrap();
    std::fs::create_dir_all(project_dir.join(".git")).unwrap();

    let flow = selenium_mcp::flow::Flow {
        name: "passing".to_string(),
        description: String::new(),
        tags: vec![],
        base_url: None,
        env: vec![],
        created_epoch: None,
        viewport_width: None,
        viewport_height: None,
        profile: None,
        steps: vec![selenium_mcp::flow::FlowStep::Navigate {
            url: fixture_url("login.html"),
        }],
    };
    selenium_mcp::flow::save_flow(&flows_dir.join("passing.yaml"), &flow).unwrap();

    let custom_path = project_dir.join("custom-report.json");
    let status = run_cli_test(&project_dir, &["--json", custom_path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(0));
    assert!(
        custom_path.exists(),
        "--json should redirect the report there"
    );
    assert!(!project_dir
        .join(".selenium-mcp/artifacts/report.json")
        .exists());
}

#[tokio::test]
async fn cli_test_update_baselines_captures_a_baseline_via_an_assert_visual_step() {
    let project_dir = std::env::temp_dir().join(format!(
        "selenium-mcp-e2e-cli-update-baselines-{}",
        std::process::id()
    ));
    let flows_dir = project_dir.join(".selenium-mcp/flows");
    std::fs::create_dir_all(&flows_dir).unwrap();
    std::fs::create_dir_all(project_dir.join(".git")).unwrap();

    let flow = selenium_mcp::flow::Flow {
        name: "visual".to_string(),
        description: String::new(),
        tags: vec![],
        base_url: None,
        env: vec![],
        created_epoch: None,
        viewport_width: None,
        viewport_height: None,
        profile: None,
        steps: vec![
            selenium_mcp::flow::FlowStep::Navigate {
                url: fixture_url("login.html"),
            },
            selenium_mcp::flow::FlowStep::AssertVisual {
                baseline: "cli-baseline".to_string(),
                threshold: None,
                masks: vec![],
            },
        ],
    };
    selenium_mcp::flow::save_flow(&flows_dir.join("visual.yaml"), &flow).unwrap();

    let baseline_png = project_dir.join(".selenium-mcp/baselines/cli-baseline.png");
    assert!(!baseline_png.exists());

    let status = run_cli_test(&project_dir, &["--update-baselines"]);
    assert_eq!(
        status.code(),
        Some(0),
        "capturing a baseline should never fail the run"
    );
    assert!(baseline_png.exists());
    assert!(project_dir
        .join(".selenium-mcp/baselines/cli-baseline.json")
        .exists());
}

#[tokio::test]
async fn cli_test_passes_a_selector_only_flow_even_when_chroma_is_unreachable() {
    let project_dir = std::env::temp_dir().join(format!(
        "selenium-mcp-e2e-cli-chroma-down-{}",
        std::process::id()
    ));
    let flows_dir = project_dir.join(".selenium-mcp/flows");
    std::fs::create_dir_all(&flows_dir).unwrap();
    std::fs::create_dir_all(project_dir.join(".git")).unwrap();

    let flow = selenium_mcp::flow::Flow {
        name: "passing".to_string(),
        description: String::new(),
        tags: vec![],
        base_url: None,
        env: vec![],
        created_epoch: None,
        viewport_width: None,
        viewport_height: None,
        profile: None,
        steps: vec![
            selenium_mcp::flow::FlowStep::Navigate {
                url: fixture_url("login.html"),
            },
            selenium_mcp::flow::FlowStep::Click {
                target: Target {
                    css: Some("#username".to_string()),
                    xpath: None,
                    semantic: None,
                },
            },
            selenium_mcp::flow::FlowStep::TypeText {
                target: Target {
                    css: Some("#username".to_string()),
                    xpath: None,
                    semantic: None,
                },
                text: "someone".to_string(),
                clear_first: false,
                secret: false,
            },
            selenium_mcp::flow::FlowStep::AssertThat {
                that: AssertCondition {
                    element_visible: Some(Target {
                        css: Some("#login-form".to_string()),
                        xpath: None,
                        semantic: None,
                    }),
                    ..Default::default()
                },
            },
        ],
    };
    selenium_mcp::flow::save_flow(&flows_dir.join("passing.yaml"), &flow).unwrap();

    // A closed loopback port -- guaranteed connection-refused, no real
    // Chroma involved. Scoped to this child process's env only (never the
    // parent test process's), same isolation discipline as the Chroma
    // lifecycle tests above.
    let status = run_cli_test_with_env(&project_dir, &[], &[("CHROMA_URL", "http://127.0.0.1:1")]);

    assert_eq!(
        status.code(),
        Some(0),
        "selector-based flows must pass even when Chroma can't be reached (FR-011)"
    );
}

#[tokio::test]
async fn idle_server_stays_under_the_thirty_megabyte_rss_budget() {
    let project_dir =
        std::env::temp_dir().join(format!("selenium-mcp-e2e-rss-{}", std::process::id()));
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::create_dir_all(project_dir.join(".git")).unwrap();

    let binary = std::path::PathBuf::from(env!("CARGO_BIN_EXE_selenium-mcp"));
    let mut child = std::process::Command::new(&binary)
        .current_dir(&project_dir)
        .env("HOME", std::env::var("HOME").unwrap_or_default())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    // Hold the stdin handle open (no EOF) rather than closing it, so the
    // stdio MCP server sits idle exactly as a connected-but-quiet MCP
    // client would leave it, instead of seeing EOF and exiting.
    let _stdin = child.stdin.take();

    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

    let rss_kb = read_rss_kb(child.id());
    let _ = child.kill();
    let _ = child.wait();

    assert!(
        rss_kb > 0,
        "failed to read RSS for the running server process"
    );
    assert!(
        rss_kb < 30 * 1024,
        "idle RSS was {rss_kb} KB, over the 30 MB (FR-010) budget"
    );
}

#[cfg(target_os = "macos")]
fn read_rss_kb(pid: u32) -> u64 {
    let output = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .unwrap_or(0)
}

#[cfg(target_os = "linux")]
fn read_rss_kb(pid: u32) -> u64 {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap_or_default();
    status
        .lines()
        .find(|l| l.starts_with("VmRSS:"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

#[tokio::test]
async fn flow_run_all_aggregates_a_flow_containing_an_assert_visual_step() {
    let name = "run-all-visual";
    let baselines_dir = scratch_baselines_dir(name);
    let flows_dir = scratch_flows_dir(name);
    let (chroma, embedder) = shared_semantic_deps();
    let server = SeleniumMcpServer::new(
        driver_cache_dir(),
        scratch_artifacts_dir(name),
        flows_dir.clone(),
        format!("e2e_{}", name.replace('-', "_")),
    )
    .with_semantic_deps(chroma, embedder)
    .with_baselines_dir(baselines_dir.clone());

    // A single flow_run_all pass both captures the baseline
    // (update_baselines) and, on a second call, diffs against it -- proving
    // the aggregate suite runner exercises FlowStep::AssertVisual through
    // the exact same run_flow code path flow_run and the CLI use, not a
    // separate one.
    let flow = selenium_mcp::flow::Flow {
        name: "run-all-visual-check".to_string(),
        description: String::new(),
        tags: vec![],
        base_url: None,
        env: vec![],
        created_epoch: None,
        viewport_width: None,
        viewport_height: None,
        profile: None,
        steps: vec![
            selenium_mcp::flow::FlowStep::Navigate {
                url: fixture_url("login.html"),
            },
            selenium_mcp::flow::FlowStep::AssertVisual {
                baseline: "run-all-baseline".to_string(),
                threshold: Some(0.00001),
                masks: vec![],
            },
        ],
    };
    selenium_mcp::flow::save_flow(&flows_dir.join("run-all-visual-check.yaml"), &flow).unwrap();

    let captured = server
        .flow_run_all(Parameters(selenium_mcp::server::FlowRunAllArgs {
            tag: None,
            heal_write_back: false,
            update_baselines: true,
        }))
        .await
        .unwrap();
    assert_eq!(captured.is_error, Some(false));
    let captured_text = &captured.content[0].as_text().unwrap().text;
    let captured_json: serde_json::Value = serde_json::from_str(captured_text).unwrap();
    assert_eq!(
        captured_json["outcome"], "all_passed",
        "got: {captured_text}"
    );
    assert!(baselines_dir.join("run-all-baseline.png").exists());

    let asserted = server
        .flow_run_all(Parameters(selenium_mcp::server::FlowRunAllArgs {
            tag: None,
            heal_write_back: false,
            update_baselines: false,
        }))
        .await
        .unwrap();
    assert_eq!(asserted.is_error, Some(false));
    let asserted_text = &asserted.content[0].as_text().unwrap().text;
    let asserted_json: serde_json::Value = serde_json::from_str(asserted_text).unwrap();
    assert_eq!(
        asserted_json["outcome"], "all_passed",
        "got: {asserted_text}"
    );
    assert_eq!(asserted_json["reports"][0]["passed"], true);
}
