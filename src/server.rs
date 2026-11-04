use crate::action_log::ActionLog;
use crate::chroma::ChromaHandle;
use crate::embedding::EmbeddingEngine;
use crate::error::AppError;
use crate::flow::{
    interpolate, load_flow, save_flow, AssertCondition, Flow, FlowStep, MaskRect,
    ResolvedCondition, Target,
};
use crate::project::managed_path;
use crate::recording::{build_concat_file, clamp_fps, RecordingSlot};
use crate::report::{RunReport, StepOutcome};
use crate::session::SessionSlot;
use crate::visual::{diff_images, BaselineMetadata, Rect as VisualRect};
use image::GenericImageView;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, ContentBlock, Implementation, ProtocolVersion, ServerCapabilities, ServerConfig,
};
use rmcp::{tool, tool_handler, tool_router, ErrorData, ServerHandler};
use schemars::JsonSchema;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
use thirtyfour::prelude::*;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SessionStartArgs {
    /// Only "chrome" is supported in this milestone.
    #[serde(default = "default_browser")]
    pub browser: String,
    #[serde(default)]
    pub headless: bool,
    pub viewport_width: Option<u32>,
    pub viewport_height: Option<u32>,
    /// Persistent Chrome profile name. When set, launches Chrome with
    /// `--user-data-dir` pointed at this project's
    /// `.selenium-mcp/profiles/<name>/`, so cookies/localStorage/IndexedDB
    /// survive across separate session_start calls. Validated against
    /// `[a-zA-Z0-9_-]+` before use. Omitted preserves today's ephemeral
    /// profile behavior exactly.
    #[serde(default)]
    pub profile: Option<String>,
}

fn default_browser() -> String {
    "chrome".to_string()
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NavigateArgs {
    pub url: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ClickArgs {
    pub target: Target,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TypeTextArgs {
    pub target: Target,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub clear_first: bool,
    /// Sensitive values are never recorded literally. Prefer env_var for replay.
    #[serde(default)]
    pub secret: bool,
    /// Read text from this process environment variable; save only its reference.
    pub env_var: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScreenshotArgs {
    pub name: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BaselineSaveArgs {
    pub name: String,
    pub target: Option<Target>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AssertVisualArgs {
    pub name: String,
    pub target: Option<Target>,
    #[serde(default)]
    pub masks: Vec<MaskRect>,
    pub threshold: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RecordStartArgs {
    pub fps: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RecordStopArgs {
    pub formats: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AssertThatArgs {
    pub that: AssertCondition,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FlowSaveArgs {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub base_url: Option<String>,
    #[serde(default)]
    pub env: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FlowNameArgs {
    pub name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProfileClearArgs {
    pub name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FlowRunArgs {
    pub name: String,
    #[serde(default)]
    pub headless: bool,
    /// When a step heals (concrete selector failed, semantic lookup found
    /// it), write the new selector back into the flow file. Default false —
    /// replay never silently rewrites a flow unless explicitly asked.
    #[serde(default)]
    pub heal_write_back: bool,
    /// When true, every assert_visual step captures and overwrites its
    /// baseline instead of asserting against it (design.md §11.1). Default
    /// false — replay never silently overwrites a baseline unless asked.
    #[serde(default)]
    pub update_baselines: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FlowRunAllArgs {
    pub tag: Option<String>,
    #[serde(default)]
    pub heal_write_back: bool,
    #[serde(default)]
    pub update_baselines: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FindSemanticArgs {
    pub query: String,
    pub top_k: Option<usize>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SemanticCandidate {
    pub css: String,
    pub score: Option<f32>,
    pub verified: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SemanticResolution {
    pub resolved: Option<Target>,
    pub candidates: Vec<SemanticCandidate>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RunHistoryArgs {
    pub flow: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct KnowledgeSearchArgs {
    pub query: String,
    pub top_k: Option<usize>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RunSummary {
    pub summary: String,
    pub flow_name: Option<String>,
    pub passed: Option<bool>,
    pub duration_ms: Option<u64>,
    pub started_epoch: Option<u64>,
    pub healed_count: Option<u64>,
    pub score: Option<f32>,
}

fn run_summary_from_match(m: &crate::chroma::ChromaMatch) -> RunSummary {
    let meta = m.metadata.as_ref();
    RunSummary {
        summary: m.document.clone().unwrap_or_default(),
        flow_name: meta
            .and_then(|v| v.get("flow_name"))
            .and_then(|v| v.as_str())
            .map(String::from),
        passed: meta.and_then(|v| v.get("passed")).and_then(|v| v.as_bool()),
        duration_ms: meta
            .and_then(|v| v.get("duration_ms"))
            .and_then(|v| v.as_u64()),
        started_epoch: meta
            .and_then(|v| v.get("started_epoch"))
            .and_then(|v| v.as_u64()),
        healed_count: meta
            .and_then(|v| v.get("healed_count"))
            .and_then(|v| v.as_u64()),
        score: m.distance,
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ChromaStatus {
    pub status: String,
    pub url: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct MemoryStats {
    pub chroma: ChromaStatus,
    pub elements_count: Option<u64>,
    pub runs_count: Option<u64>,
    pub cache_disk_usage_bytes: u64,
}

fn dir_size_bytes(path: &std::path::Path) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.filter_map(|e| e.ok()) {
            let entry_path = entry.path();
            if let Ok(meta) = entry.metadata() {
                if meta.is_dir() {
                    total += dir_size_bytes(&entry_path);
                } else {
                    total += meta.len();
                }
            }
        }
    }
    total
}

/// Diagnostic-only: never errors, reports `status: "disabled"` instead when
/// Chroma can't be reached or spawned (design.md — memory_stats degrades
/// gracefully rather than failing the tool call).
pub async fn compute_memory_stats(
    chroma: &crate::chroma::ChromaHandle,
    project_id: &str,
    home_cache_dir: &std::path::Path,
) -> MemoryStats {
    let cache_disk_usage_bytes = dir_size_bytes(home_cache_dir);
    let client = chroma.ensure_client().await.ok();
    let externally_configured = std::env::var("CHROMA_URL").is_ok();
    stats_from_client(
        client.as_ref(),
        externally_configured,
        project_id,
        cache_disk_usage_bytes,
    )
    .await
}

/// Pure of any connection attempt — `client: None` is exactly the
/// disabled/unreachable case, directly testable with no env var or network
/// involved.
async fn stats_from_client(
    client: Option<&crate::chroma::ChromaClient>,
    externally_configured: bool,
    project_id: &str,
    cache_disk_usage_bytes: u64,
) -> MemoryStats {
    let Some(client) = client else {
        return MemoryStats {
            chroma: ChromaStatus {
                status: "disabled".to_string(),
                url: None,
            },
            elements_count: None,
            runs_count: None,
            cache_disk_usage_bytes,
        };
    };

    let status = if externally_configured {
        "connected"
    } else {
        "spawned"
    };

    let elements_count = match client
        .get_or_create_collection(
            &crate::index::elements_collection_name(project_id),
            "cosine",
        )
        .await
    {
        Ok(id) => client.count(&id).await.ok(),
        Err(_) => None,
    };
    let runs_count = match client
        .get_or_create_collection(&crate::index::runs_collection_name(project_id), "cosine")
        .await
    {
        Ok(id) => client.count(&id).await.ok(),
        Err(_) => None,
    };

    MemoryStats {
        chroma: ChromaStatus {
            status: status.to_string(),
            url: Some(client.base_url().to_string()),
        },
        elements_count,
        runs_count,
        cache_disk_usage_bytes,
    }
}

pub(crate) fn now_epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock is before the Unix epoch")
        .as_secs()
}

fn list_flow_names(flows_dir: &std::path::Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(flows_dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let path = e.path();
            if path.extension().and_then(|s| s.to_str()) == Some("yaml") {
                let name = path.file_stem()?.to_str()?;
                managed_path(flows_dir, name, ".yaml").ok()?;
                Some(name.to_string())
            } else {
                None
            }
        })
        .collect();
    names.sort();
    names
}

#[derive(Clone)]
pub struct SeleniumMcpServer {
    session: SessionSlot,
    recording: RecordingSlot,
    action_log: ActionLog,
    driver_cache_dir: PathBuf,
    artifacts_dir: PathBuf,
    flows_dir: PathBuf,
    baselines_dir: PathBuf,
    profiles_dir: PathBuf,
    project_id: String,
    chroma: ChromaHandle,
    embedder: EmbeddingEngine,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl SeleniumMcpServer {
    pub fn new(
        driver_cache_dir: PathBuf,
        artifacts_dir: PathBuf,
        flows_dir: PathBuf,
        project_id: String,
    ) -> Self {
        let home_cache = driver_cache_dir
            .parent()
            .unwrap_or(&driver_cache_dir)
            .to_path_buf();
        // Mirrors project::ensure_layout's scheme: baselines/ is a sibling
        // of artifacts/ under the same .selenium-mcp/ directory.
        let baselines_dir = artifacts_dir
            .parent()
            .unwrap_or(&artifacts_dir)
            .join("baselines");
        let profiles_dir = artifacts_dir
            .parent()
            .unwrap_or(&artifacts_dir)
            .join("profiles");
        Self {
            session: SessionSlot::new(),
            recording: RecordingSlot::new(),
            action_log: ActionLog::default(),
            driver_cache_dir,
            artifacts_dir,
            flows_dir,
            baselines_dir,
            profiles_dir,
            project_id,
            chroma: ChromaHandle::new(
                home_cache.join("chroma"),
                crate::chroma::DEFAULT_IDLE_TIMEOUT,
            ),
            embedder: EmbeddingEngine::new(home_cache.join("models")),
            tool_router: Self::tool_router(),
        }
    }

    /// Test/injection hook: replace the lazily-constructed Chroma handle and
    /// embedder with shared instances (e.g. one Chroma process and one
    /// loaded model reused across an entire e2e test binary instead of one
    /// per server).
    pub fn with_semantic_deps(mut self, chroma: ChromaHandle, embedder: EmbeddingEngine) -> Self {
        self.chroma = chroma;
        self.embedder = embedder;
        self
    }

    /// Test/injection hook: override the derived baselines_dir. Production
    /// always derives it from artifacts_dir's parent (mirroring
    /// project::ensure_layout's real `.selenium-mcp/{artifacts,baselines}`
    /// nesting); test scratch dirs are flat, so that derivation would
    /// resolve to a directory shared across every test in the binary
    /// without this override.
    pub fn with_baselines_dir(mut self, baselines_dir: PathBuf) -> Self {
        self.baselines_dir = baselines_dir;
        self
    }

    /// Test/injection hook: override the derived profiles_dir. Production
    /// always derives it from artifacts_dir's parent (mirroring
    /// project::ensure_layout's real `.selenium-mcp/{artifacts,profiles}`
    /// nesting); test scratch dirs are flat, so that derivation would
    /// resolve to a directory shared across every test in the binary
    /// without this override.
    pub fn with_profiles_dir(mut self, profiles_dir: PathBuf) -> Self {
        self.profiles_dir = profiles_dir;
        self
    }

    /// Best-effort: semantic memory is a degradation-tolerant feature
    /// (design.md §10/§11) — a Chroma or embedding failure must never fail
    /// the navigate call itself.
    async fn index_current_page(&self) {
        let chroma = self.chroma.clone();
        let embedder = self.embedder.clone();
        let project_id = self.project_id.clone();
        let now = now_epoch_secs();
        let result = self
            .session
            .with_driver(|driver| async move {
                crate::index::index_page(&driver, &chroma, &embedder, &project_id, now).await
            })
            .await;
        if let Err(e) = result {
            tracing::warn!(error = %e, "page indexing skipped");
        }
    }

    #[tool(description = "Launch a browser session. Only one session may be active at a time.")]
    pub async fn session_start(
        &self,
        Parameters(args): Parameters<SessionStartArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        if args.browser != "chrome" {
            return Ok(CallToolResult::error(vec![ContentBlock::text(format!(
                "unsupported browser '{}' — only 'chrome' is supported in this milestone",
                args.browser
            ))]));
        }

        let mut caps = DesiredCapabilities::chrome();
        if args.headless {
            if let Err(e) = caps.set_headless() {
                return Ok(AppError::from(e).into_tool_result());
            }
        }
        if let (Some(w), Some(h)) = (args.viewport_width, args.viewport_height) {
            if let Err(e) = caps.add_arg(&format!("--window-size={w},{h}")) {
                return Ok(AppError::from(e).into_tool_result());
            }
        }
        if let Some(name) = &args.profile {
            if let Err(e) = crate::project::validate_profile_name(name) {
                return Ok(e.into_tool_result());
            }
            let profile_path = match managed_path(&self.profiles_dir, name, "") {
                Ok(path) => path,
                Err(e) => return Ok(e.into_tool_result()),
            };
            if let Err(e) = caps.add_arg(&format!("--user-data-dir={}", profile_path.display())) {
                return Ok(AppError::from(e).into_tool_result());
            }
        }

        let driver = match WebDriver::managed(caps)
            .cache_dir(self.driver_cache_dir.clone())
            .await
        {
            Ok(driver) => driver,
            Err(e) => return Ok(AppError::from(e).into_tool_result()),
        };

        match self.session.start(driver).await {
            Ok(()) => {
                self.action_log.clear();
                Ok(CallToolResult::success(vec![ContentBlock::text(
                    "browser session started",
                )]))
            }
            Err((app_err, driver)) => {
                let _ = driver.quit().await;
                Ok(app_err.into_tool_result())
            }
        }
    }

    #[tool(description = "Close the active browser session.")]
    pub async fn session_end(&self) -> Result<CallToolResult, ErrorData> {
        match self.session.end().await {
            Ok(()) => Ok(CallToolResult::success(vec![ContentBlock::text(
                "browser session ended",
            )])),
            Err(app_err) => Ok(app_err.into_tool_result()),
        }
    }

    #[tool(
        description = "Navigate the active browser session to a URL. Auto-indexes the page's interactive elements into semantic memory."
    )]
    pub async fn navigate(
        &self,
        Parameters(NavigateArgs { url }): Parameters<NavigateArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let result = do_navigate(&self.session, url.clone()).await;
        if result.is_ok() {
            self.action_log.append(FlowStep::Navigate { url });
            self.index_current_page().await;
        }
        Ok(match result {
            Ok(()) => CallToolResult::success(vec![ContentBlock::text("navigated")]),
            Err(e) => e.into_tool_result(),
        })
    }

    #[tool(description = "Click the element matching the given target.")]
    pub async fn click(
        &self,
        Parameters(ClickArgs { target }): Parameters<ClickArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let result = do_click(&self.session, &target).await;
        if result.is_ok() {
            self.action_log.append(FlowStep::Click { target });
        }
        Ok(match result {
            Ok(()) => CallToolResult::success(vec![ContentBlock::text("clicked")]),
            Err(e) => e.into_tool_result(),
        })
    }

    #[tool(description = "Type text into the element matching the given target.")]
    pub async fn type_text(
        &self,
        Parameters(TypeTextArgs {
            target,
            text,
            clear_first,
            secret,
            env_var,
        }): Parameters<TypeTextArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let (value, recorded_text) = if let Some(name) = &env_var {
            if !crate::flow::valid_env_name(name) || !text.is_empty() {
                return Ok(AppError::InvalidTarget("env_var must be a valid environment name and cannot be combined with literal text".into()).into_tool_result());
            }
            match std::env::var(name) {
                Ok(value) => (value, format!("{{env.{name}}}")),
                Err(_) => {
                    return Ok(AppError::InvalidTarget(
                        "required typing environment variable is unavailable".into(),
                    )
                    .into_tool_result())
                }
            }
        } else {
            (text, String::new())
        };
        let fallback_text = if env_var.is_none() && !secret {
            Some(value.clone())
        } else {
            None
        };
        let result = do_type_text(
            &self.session,
            &target,
            value,
            clear_first,
            secret || env_var.is_some(),
        )
        .await;
        if let Ok(detected_secret) = &result {
            let sensitive = secret || env_var.is_some() || *detected_secret;
            self.action_log.append(FlowStep::TypeText {
                target,
                text: if sensitive {
                    recorded_text
                } else {
                    fallback_text.unwrap_or_default()
                },
                clear_first,
                secret: sensitive,
            });
        }
        Ok(match result {
            Ok(_) => CallToolResult::success(vec![ContentBlock::text("typed")]),
            Err(e) => e.into_tool_result(),
        })
    }

    #[tool(
        description = "Assert a condition against the current page: element_visible, text_visible, url_matches, title_matches, or element_count. Failures return detail, not exceptions."
    )]
    pub async fn assert_that(
        &self,
        Parameters(AssertThatArgs { that }): Parameters<AssertThatArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let result = do_assert_that(&self.session, &that).await;
        match result {
            Ok(true) => {
                self.action_log.append(FlowStep::AssertThat { that });
                Ok(CallToolResult::success(vec![ContentBlock::text(
                    "assertion passed",
                )]))
            }
            Ok(false) => Ok(CallToolResult::error(vec![ContentBlock::text(
                "assertion failed",
            )])),
            Err(e) => Ok(e.into_tool_result()),
        }
    }

    #[tool(description = "Save every action recorded this session as a named YAML flow.")]
    pub async fn flow_save(
        &self,
        Parameters(FlowSaveArgs {
            name,
            description,
            tags,
            base_url,
            mut env,
        }): Parameters<FlowSaveArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let path = match managed_path(&self.flows_dir, &name, ".yaml") {
            Ok(path) => path,
            Err(e) => return Ok(e.into_tool_result()),
        };
        let result = self.action_log.save_with(|steps| {
            if steps.is_empty() {
                return Err(AppError::InvalidTarget(
                    "no actions recorded this session".into(),
                ));
            }
            for step in &steps {
                if let FlowStep::TypeText { text, .. } = step {
                    if let Some(name) = crate::flow::secret_env_name(text) {
                        if !env.iter().any(|n| n == name) {
                            env.push(name.into());
                        }
                    }
                }
            }
            let flow = Flow {
                name: name.clone(),
                description,
                tags,
                base_url,
                env,
                created_epoch: Some(now_epoch_secs()),
                viewport_width: None,
                viewport_height: None,
                profile: None,
                steps,
            };
            save_flow(&path, &flow)
        });
        match result {
            Ok(()) => Ok(CallToolResult::success(vec![ContentBlock::text(
                path.display().to_string(),
            )])),
            Err(e) => Ok(e.into_tool_result()),
        }
    }

    #[tool(description = "List saved flows in this project.")]
    pub async fn flow_list(&self) -> Result<CallToolResult, ErrorData> {
        let names = list_flow_names(&self.flows_dir);
        Ok(CallToolResult::success(vec![ContentBlock::text(
            names.join("\n"),
        )]))
    }

    #[tool(description = "Delete a saved flow by name.")]
    pub async fn flow_delete(
        &self,
        Parameters(FlowNameArgs { name }): Parameters<FlowNameArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let path = match managed_path(&self.flows_dir, &name, ".yaml") {
            Ok(path) => path,
            Err(e) => return Ok(e.into_tool_result()),
        };
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(CallToolResult::success(vec![ContentBlock::text("deleted")])),
            Err(e) => Ok(AppError::from(e).into_tool_result()),
        }
    }

    #[tool(
        description = "Delete a named persistent Chrome profile directory (created via session_start's or a flow's `profile` field). Succeeds whether or not the profile previously existed."
    )]
    pub async fn profile_clear(
        &self,
        Parameters(ProfileClearArgs { name }): Parameters<ProfileClearArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        if let Err(e) = crate::project::validate_profile_name(&name) {
            return Ok(e.into_tool_result());
        }
        let path = match managed_path(&self.profiles_dir, &name, "") {
            Ok(path) => path,
            Err(e) => return Ok(e.into_tool_result()),
        };
        match std::fs::remove_dir_all(&path) {
            Ok(()) => Ok(CallToolResult::success(vec![ContentBlock::text(format!(
                "profile '{name}' cleared"
            ))])),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Ok(CallToolResult::success(vec![ContentBlock::text(format!(
                    "no profile named '{name}' — nothing to clear"
                ))]))
            }
            Err(e) => Ok(AppError::from(e).into_tool_result()),
        }
    }

    #[tool(
        description = "Replay a saved flow: launches its own browser session, runs every step, and returns a pass/fail report. Selectors self-heal via semantic memory on failure (report-only unless heal_write_back is set)."
    )]
    pub async fn flow_run(
        &self,
        Parameters(FlowRunArgs {
            name,
            headless,
            heal_write_back,
            update_baselines,
        }): Parameters<FlowRunArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let path = match managed_path(&self.flows_dir, &name, ".yaml") {
            Ok(path) => path,
            Err(e) => return Ok(e.into_tool_result()),
        };
        let flow = match load_flow(&path) {
            Ok(f) => f,
            Err(e) => return Ok(e.into_tool_result()),
        };
        let report = run_flow(
            &flow,
            RunFlowOptions {
                driver_cache_dir: &self.driver_cache_dir,
                headless,
                chroma: &self.chroma,
                embedder: &self.embedder,
                project_id: &self.project_id,
                heal_write_back,
                flow_path: Some(&path),
                baselines_dir: &self.baselines_dir,
                artifacts_dir: &self.artifacts_dir,
                update_baselines,
                profiles_dir: &self.profiles_dir,
            },
        )
        .await;
        let summary = serde_json::to_string_pretty(&report)
            .unwrap_or_else(|_| "failed to serialize report".to_string());
        Ok(if report.passed {
            CallToolResult::success(vec![ContentBlock::text(summary)])
        } else {
            CallToolResult::error(vec![ContentBlock::text(summary)])
        })
    }

    #[tool(
        description = "Run every flow in this project (optionally tag-filtered) and return the aggregate report."
    )]
    pub async fn flow_run_all(
        &self,
        Parameters(FlowRunAllArgs {
            tag,
            heal_write_back,
            update_baselines,
        }): Parameters<FlowRunAllArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let suite = crate::cli::run_suite(crate::cli::RunSuiteOptions {
            flows_dir: &self.flows_dir,
            driver_cache_dir: &self.driver_cache_dir,
            chroma: &self.chroma,
            embedder: &self.embedder,
            project_id: &self.project_id,
            tag_filter: tag.as_deref(),
            flow_filter: None,
            heal_write_back,
            baselines_dir: &self.baselines_dir,
            artifacts_dir: &self.artifacts_dir,
            update_baselines,
            profiles_dir: &self.profiles_dir,
        })
        .await;
        let summary = serde_json::json!({
            "outcome": suite.outcome,
            "reports": suite.reports,
        });
        let text = serde_json::to_string_pretty(&summary)
            .unwrap_or_else(|_| "failed to serialize suite report".to_string());
        Ok(match suite.outcome {
            crate::cli::SuiteOutcome::AllPassed => {
                CallToolResult::success(vec![ContentBlock::text(text)])
            }
            _ => CallToolResult::error(vec![ContentBlock::text(text)]),
        })
    }

    #[tool(
        description = "Capture a PNG screenshot of the current viewport into the project's artifact directory."
    )]
    pub async fn screenshot(
        &self,
        Parameters(ScreenshotArgs { name }): Parameters<ScreenshotArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let name = name.unwrap_or_else(|| "screenshot".to_string());
        let path = match managed_path(&self.artifacts_dir, &name, ".png") {
            Ok(path) => path,
            Err(e) => return Ok(e.into_tool_result()),
        };
        let result = self
            .session
            .with_driver(|driver| {
                let path = path.clone();
                async move { driver.screenshot(&path).await.map_err(AppError::from) }
            })
            .await;
        Ok(match result {
            Ok(()) => CallToolResult::success(vec![ContentBlock::text(path.display().to_string())]),
            Err(e) => e.into_tool_result(),
        })
    }

    #[tool(
        description = "Capture a named visual baseline (full viewport, or an element if target is given) into the project's baselines directory."
    )]
    pub async fn baseline_save(
        &self,
        Parameters(BaselineSaveArgs { name, target }): Parameters<BaselineSaveArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        Ok(
            match save_baseline(&self.session, &self.baselines_dir, &name, target).await {
                Ok(path) => {
                    CallToolResult::success(vec![ContentBlock::text(path.display().to_string())])
                }
                Err(e) => e.into_tool_result(),
            },
        )
    }

    #[tool(
        description = "Perceptual diff of the current view (viewport, or an element if target is given) against a named baseline. Fails if the baseline was captured under different viewport/OS conditions, or if the diff ratio exceeds threshold (default 0.001). Writes a diff-image artifact on failure."
    )]
    pub async fn assert_visual(
        &self,
        Parameters(AssertVisualArgs {
            name,
            target,
            masks,
            threshold,
        }): Parameters<AssertVisualArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let threshold = threshold.unwrap_or(0.001);
        let outcome = match check_visual(
            &self.session,
            &self.baselines_dir,
            &name,
            target,
            &masks,
            threshold,
        )
        .await
        {
            Ok(o) => o,
            Err(e) => return Ok(e.into_tool_result()),
        };

        if let Some(reason) = &outcome.reason {
            let text = serde_json::json!({
                "passed": false,
                "reason": reason,
                "baseline": outcome.baseline_meta,
                "current": outcome.current_dims.map(|(w, h)| serde_json::json!({
                    "width": w, "height": h, "os": std::env::consts::OS
                })),
            });
            return Ok(CallToolResult::error(vec![ContentBlock::text(
                serde_json::to_string_pretty(&text).unwrap_or_default(),
            )]));
        }

        let mut result_json = serde_json::json!({
            "passed": outcome.passed,
            "ratio": outcome.ratio,
            "differing_pixels": outcome.differing_pixels,
            "compared_pixels": outcome.compared_pixels,
            "threshold": outcome.threshold,
        });

        if !outcome.passed {
            if let Some(diff_png) = &outcome.diff_png {
                let diff_path = match managed_path(
                    &self.artifacts_dir,
                    &format!("visual-diff-{name}-{}", now_epoch_secs()),
                    ".png",
                ) {
                    Ok(path) => path,
                    Err(e) => return Ok(e.into_tool_result()),
                };
                if std::fs::write(&diff_path, diff_png).is_ok() {
                    result_json["diff_image"] = serde_json::json!(diff_path.display().to_string());
                }
            }
        }

        let text = serde_json::to_string_pretty(&result_json).unwrap_or_default();
        Ok(if outcome.passed {
            CallToolResult::success(vec![ContentBlock::text(text)])
        } else {
            CallToolResult::error(vec![ContentBlock::text(text)])
        })
    }

    #[tool(
        description = "Find an element by natural-language description via semantic memory. Returns the first live-verified match plus ranked runner-ups with scores."
    )]
    pub async fn find_semantic(
        &self,
        Parameters(FindSemanticArgs { query, top_k }): Parameters<FindSemanticArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let top_k = top_k.unwrap_or(5);
        let chroma = self.chroma.clone();
        let embedder = self.embedder.clone();
        let project_id = self.project_id.clone();
        let result = self
            .session
            .with_driver(|driver| async move {
                resolve_semantic(&driver, &chroma, &embedder, &project_id, &query, top_k).await
            })
            .await;

        Ok(match result {
            Ok(resolution) => {
                let text = serde_json::to_string_pretty(&resolution)
                    .unwrap_or_else(|_| "failed to serialize resolution".to_string());
                if resolution.resolved.is_some() {
                    CallToolResult::success(vec![ContentBlock::text(text)])
                } else {
                    CallToolResult::error(vec![ContentBlock::text(text)])
                }
            }
            Err(e) => e.into_tool_result(),
        })
    }

    #[tool(
        description = "Recent flow run reports from the knowledge base, optionally filtered by flow name, newest first. Plain metadata lookup, no semantic ranking."
    )]
    pub async fn run_history(
        &self,
        Parameters(RunHistoryArgs { flow, limit }): Parameters<RunHistoryArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let limit = limit.unwrap_or(20);
        let client = match self.chroma.ensure_client().await {
            Ok(c) => c,
            Err(e) => return Ok(e.into_tool_result()),
        };
        let collection_id = match client
            .get_or_create_collection(
                &crate::index::runs_collection_name(&self.project_id),
                "cosine",
            )
            .await
        {
            Ok(id) => id,
            Err(e) => return Ok(e.into_tool_result()),
        };
        let where_clause = flow.map(|f| serde_json::json!({ "flow_name": f }));
        let got = match client.get(&collection_id, where_clause).await {
            Ok(g) => g,
            Err(e) => return Ok(e.into_tool_result()),
        };

        let mut summaries: Vec<RunSummary> =
            got.matches.iter().map(run_summary_from_match).collect();
        summaries.sort_by(|a, b| {
            b.started_epoch
                .unwrap_or(0)
                .cmp(&a.started_epoch.unwrap_or(0))
        });
        summaries.truncate(limit);

        let text = serde_json::to_string_pretty(&summaries)
            .unwrap_or_else(|_| "failed to serialize run history".to_string());
        Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
    }

    #[tool(
        description = "Semantic search over run reports, e.g. \"what usually breaks on the checkout flow\". Returns ranked run summaries."
    )]
    pub async fn knowledge_search(
        &self,
        Parameters(KnowledgeSearchArgs { query, top_k }): Parameters<KnowledgeSearchArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let top_k = top_k.unwrap_or(5);
        let client = match self.chroma.ensure_client().await {
            Ok(c) => c,
            Err(e) => return Ok(e.into_tool_result()),
        };
        let embedding = match self.embedder.embed_one(&query).await {
            Ok(e) => e,
            Err(e) => return Ok(e.into_tool_result()),
        };
        let collection_id = match client
            .get_or_create_collection(
                &crate::index::runs_collection_name(&self.project_id),
                "cosine",
            )
            .await
        {
            Ok(id) => id,
            Err(e) => return Ok(e.into_tool_result()),
        };
        let result = match client.query(&collection_id, &embedding, top_k, None).await {
            Ok(r) => r,
            Err(e) => return Ok(e.into_tool_result()),
        };

        let summaries: Vec<RunSummary> =
            result.matches.iter().map(run_summary_from_match).collect();
        let text = serde_json::to_string_pretty(&summaries)
            .unwrap_or_else(|_| "failed to serialize knowledge search results".to_string());
        Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
    }

    #[tool(
        description = "Chroma status, element/run collection sizes, and cache disk usage under ~/.selenium-mcp."
    )]
    pub async fn memory_stats(&self) -> Result<CallToolResult, ErrorData> {
        let home_cache = self
            .driver_cache_dir
            .parent()
            .unwrap_or(&self.driver_cache_dir);
        let stats = compute_memory_stats(&self.chroma, &self.project_id, home_cache).await;
        let text = serde_json::to_string_pretty(&stats)
            .unwrap_or_else(|_| "failed to serialize memory stats".to_string());
        Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
    }

    #[tool(description = "Begin frame capture for the active session. Default 3 fps, max 5 fps.")]
    pub async fn record_start(
        &self,
        Parameters(RecordStartArgs { fps }): Parameters<RecordStartArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let fps = match clamp_fps(fps) {
            Ok(fps) => fps,
            Err(e) => return Ok(e.into_tool_result()),
        };
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let run_dir = match managed_path(&self.artifacts_dir, &format!("recording-{unique}"), "") {
            Ok(path) => path,
            Err(e) => return Ok(e.into_tool_result()),
        };
        let frames_dir = match managed_path(&run_dir, "frames", "") {
            Ok(path) => path,
            Err(e) => return Ok(e.into_tool_result()),
        };
        let result = self
            .recording
            .start(self.session.clone(), run_dir, frames_dir, fps)
            .await;
        Ok(match result {
            Ok(()) => CallToolResult::success(vec![ContentBlock::text("recording started")]),
            Err(e) => e.into_tool_result(),
        })
    }

    #[tool(
        description = "Stop recording and encode the requested formats: \"mp4\", \"gif\", or both."
    )]
    pub async fn record_stop(
        &self,
        Parameters(RecordStopArgs { formats }): Parameters<RecordStopArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        if formats.is_empty() || formats.iter().any(|f| f != "mp4" && f != "gif") {
            return Ok(
                AppError::InvalidTarget("formats must contain mp4 and/or gif".into())
                    .into_tool_result(),
            );
        }
        let (run_dir, frames_dir, frame_count, timestamps, stop_elapsed) =
            match self.recording.stop().await {
                Ok(v) => v,
                Err(e) => return Ok(e.into_tool_result()),
            };

        if frame_count == 0 {
            return Ok(AppError::Ffmpeg(
                "no frames were captured — was the recording running long enough?".to_string(),
            )
            .into_tool_result());
        }

        let concat_path = match managed_path(&frames_dir, "concat", ".txt") {
            Ok(path) => path,
            Err(e) => return Ok(e.into_tool_result()),
        };
        let concat_contents = build_concat_file(frame_count, &timestamps, stop_elapsed);
        if let Err(e) = std::fs::write(&concat_path, concat_contents) {
            return Ok(AppError::from(e).into_tool_result());
        }

        let ffmpeg_bin_dir = self
            .driver_cache_dir
            .parent()
            .unwrap_or(&self.driver_cache_dir)
            .join("bin");
        let ffmpeg_path = match crate::ffmpeg::ensure_ffmpeg(&ffmpeg_bin_dir) {
            Ok(path) => path,
            // FR-011: ffmpeg missing (and download blocked) must never lose
            // the captured frames -- report where they landed instead of
            // just the ffmpeg failure, since remove_dir_all below never runs
            // on this path.
            Err(e) => {
                return Ok(CallToolResult::error(vec![ContentBlock::text(
                    frames_preserved_message(&e, &frames_dir),
                )]))
            }
        };

        let mut outputs = Vec::new();
        for format in &formats {
            let output_path = match managed_path(&run_dir, "demo", &format!(".{format}")) {
                Ok(path) => path,
                Err(e) => return Ok(e.into_tool_result()),
            };
            let encode_result = encode(&ffmpeg_path, &concat_path, format, &output_path).await;
            if let Err(e) = encode_result {
                return Ok(CallToolResult::error(vec![ContentBlock::text(
                    frames_preserved_message(&e, &frames_dir),
                )]));
            }
            outputs.push(output_path.display().to_string());
        }

        let _ = std::fs::remove_dir_all(&frames_dir);

        Ok(CallToolResult::success(vec![ContentBlock::text(
            outputs.join("\n"),
        )]))
    }
}

fn baseline_paths(
    baselines_dir: &std::path::Path,
    name: &str,
) -> Result<(PathBuf, PathBuf), AppError> {
    Ok((
        managed_path(baselines_dir, name, ".png")?,
        managed_path(baselines_dir, name, ".json")?,
    ))
}

/// FR-011: ffmpeg missing/failing must never lose the captured frames --
/// `record_stop` only deletes `frames_dir` after a successful encode, so on
/// any ffmpeg failure the frames are already preserved on disk; this just
/// makes sure the returned error says where.
fn frames_preserved_message(err: &AppError, frames_dir: &std::path::Path) -> String {
    format!(
        "{err} — captured frames are preserved at {}",
        frames_dir.display()
    )
}

async fn capture_png(session: &SessionSlot, target: Option<Target>) -> Result<Vec<u8>, AppError> {
    session
        .with_driver(|driver| async move {
            match target {
                Some(t) => {
                    let by = t.to_by()?;
                    let el = driver.find(by).await.map_err(AppError::from)?;
                    el.screenshot_as_png().await.map_err(AppError::from)
                }
                None => driver.screenshot_as_png().await.map_err(AppError::from),
            }
        })
        .await
}

/// Shared core behind the `baseline_save` tool and `FlowStep::AssertVisual`'s
/// `--update-baselines` path (design.md §9/§11.1 — a flow replays through
/// the same code path as live tools).
async fn save_baseline(
    session: &SessionSlot,
    baselines_dir: &std::path::Path,
    name: &str,
    target: Option<Target>,
) -> Result<PathBuf, AppError> {
    let (png_path, meta_path) = baseline_paths(baselines_dir, name)?;
    let png_bytes = capture_png(session, target).await?;
    let (width, height) = image::load_from_memory(&png_bytes)
        .map_err(|e| AppError::InvalidTarget(format!("failed to decode screenshot: {e}")))?
        .dimensions();
    let metadata = BaselineMetadata {
        width,
        height,
        os: std::env::consts::OS.to_string(),
    };

    std::fs::create_dir_all(baselines_dir).map_err(AppError::from)?;
    std::fs::write(&png_path, &png_bytes).map_err(AppError::from)?;
    let meta_json = serde_json::to_string_pretty(&metadata).unwrap_or_else(|_| "{}".to_string());
    std::fs::write(&meta_path, meta_json).map_err(AppError::from)?;
    Ok(png_path)
}

struct VisualCheckOutcome {
    passed: bool,
    reason: Option<String>,
    ratio: Option<f64>,
    differing_pixels: Option<u64>,
    compared_pixels: Option<u64>,
    threshold: f64,
    baseline_meta: Option<BaselineMetadata>,
    current_dims: Option<(u32, u32)>,
    diff_png: Option<Vec<u8>>,
}

/// Shared core behind the `assert_visual` tool and `FlowStep::AssertVisual`.
/// A viewport/OS mismatch against the baseline's recorded metadata is
/// reported as a distinct `reason` (design.md §11.2's `baseline_os_mismatch`
/// case) rather than a diff ratio — checked before any pixel comparison
/// runs, so a resolution/platform mismatch is never conflated with a real
/// visual regression.
async fn check_visual(
    session: &SessionSlot,
    baselines_dir: &std::path::Path,
    name: &str,
    target: Option<Target>,
    masks: &[MaskRect],
    threshold: f64,
) -> Result<VisualCheckOutcome, AppError> {
    let (png_path, meta_path) = baseline_paths(baselines_dir, name)?;

    let baseline_bytes = std::fs::read(&png_path).map_err(|_| {
        AppError::InvalidTarget(format!(
            "no baseline named '{name}' found — call baseline_save first"
        ))
    })?;
    let baseline_meta: BaselineMetadata = std::fs::read_to_string(&meta_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .ok_or_else(|| {
            AppError::InvalidTarget(format!(
                "baseline '{name}' is missing its metadata file — call baseline_save again"
            ))
        })?;

    let current_bytes = capture_png(session, target).await?;
    let (cw, ch) = image::load_from_memory(&current_bytes)
        .map_err(|e| AppError::InvalidTarget(format!("failed to decode screenshot: {e}")))?
        .dimensions();

    let current_os = std::env::consts::OS;
    if baseline_meta.width != cw || baseline_meta.height != ch || baseline_meta.os != current_os {
        return Ok(VisualCheckOutcome {
            passed: false,
            reason: Some("baseline captured under different conditions".to_string()),
            ratio: None,
            differing_pixels: None,
            compared_pixels: None,
            threshold,
            baseline_meta: Some(baseline_meta),
            current_dims: Some((cw, ch)),
            diff_png: None,
        });
    }

    let mask_rects: Vec<VisualRect> = masks
        .iter()
        .map(|m| VisualRect {
            x: m.x,
            y: m.y,
            w: m.w,
            h: m.h,
        })
        .collect();
    let outcome = diff_images(&baseline_bytes, &current_bytes, &mask_rects)
        .map_err(|e| AppError::InvalidTarget(e.to_string()))?;

    let passed = outcome.ratio <= threshold;
    Ok(VisualCheckOutcome {
        passed,
        reason: None,
        ratio: Some(outcome.ratio),
        differing_pixels: Some(outcome.differing_pixels),
        compared_pixels: Some(outcome.compared_pixels),
        threshold,
        baseline_meta: Some(baseline_meta),
        current_dims: Some((cw, ch)),
        diff_png: if passed { None } else { Some(outcome.diff_png) },
    })
}

async fn do_navigate(session: &SessionSlot, url: String) -> Result<(), AppError> {
    session
        .with_driver(|driver| async move { driver.goto(url).await.map_err(AppError::from) })
        .await
}

async fn do_click(session: &SessionSlot, target: &Target) -> Result<(), AppError> {
    let target = target.clone();
    session
        .with_driver(|driver| async move {
            let by = target.to_by()?;
            let element = driver.find(by).await.map_err(AppError::from)?;
            element.click().await.map_err(AppError::from)
        })
        .await
}

async fn do_type_text(
    session: &SessionSlot,
    target: &Target,
    text: String,
    clear_first: bool,
    force_secret: bool,
) -> Result<bool, AppError> {
    let target = target.clone();
    session
        .with_driver(|driver| async move {
            let by = target.to_by()?;
            let element = driver.find(by).await.map_err(AppError::from)?;
            let field_type = element.attr("type").await?.unwrap_or_default();
            let autocomplete = element.attr("autocomplete").await?.unwrap_or_default();
            let identity = format!("{} {} {}",
                element.attr("name").await?.unwrap_or_default(),
                element.attr("id").await?.unwrap_or_default(),
                element.attr("aria-label").await?.unwrap_or_default(),
            ).to_ascii_lowercase();
            let sensitive = force_secret || field_type.eq_ignore_ascii_case("password")
                || ["password", "passwd", "passphrase", "secret", "token", "apikey", "api_key", "api-key", "one-time-code", "otp", "cvv", "cvc"].iter().any(|word| identity.contains(word))
                || identity.split(|c: char| !c.is_ascii_alphanumeric()).any(|word| word == "pin")
                || autocomplete.to_ascii_lowercase().split_whitespace().any(|s| matches!(s, "current-password" | "new-password" | "one-time-code" | "cc-number" | "cc-csc"));
            if clear_first {
                element.clear().await.map_err(AppError::from)?;
            }
            element.send_keys(text).await.map_err(AppError::from)?;
            Ok(sensitive)
        })
        .await.map_err(|_| AppError::InvalidTarget("typing failed; verify the target and active session (driver details omitted to protect typed values)".into()))
}

/// Shared by the `find_semantic` tool and self-healing flow replay.
/// Embeds `query`, ranks candidates from the current URL's element index
/// (falling back to the whole project if the URL has no matches yet — see
/// design.md §8.2), and live-verifies each candidate against the current
/// DOM in ranked order.
pub async fn resolve_semantic(
    driver: &WebDriver,
    chroma: &crate::chroma::ChromaHandle,
    embedder: &crate::embedding::EmbeddingEngine,
    project_id: &str,
    query: &str,
    top_k: usize,
) -> Result<SemanticResolution, AppError> {
    if crate::chroma::memory_disabled() {
        return Err(AppError::ChromaUnavailable(
            "disabled by SELENIUM_MCP_DISABLE_MEMORY".into(),
        ));
    }
    let url = driver.current_url().await.map_err(AppError::from)?;
    let normalized = crate::index::normalize_url(url.as_str());

    let embedding = embedder.embed_one(query).await?;
    let client = chroma.ensure_client().await?;
    let collection_id = client
        .get_or_create_collection(
            &crate::index::elements_collection_name(project_id),
            "cosine",
        )
        .await?;

    let mut result = client
        .query(
            &collection_id,
            &embedding,
            top_k,
            Some(serde_json::json!({ "url": normalized })),
        )
        .await?;
    if result.matches.is_empty() {
        result = client
            .query(&collection_id, &embedding, top_k, None)
            .await?;
    }

    let mut candidates = Vec::new();
    let mut resolved: Option<Target> = None;
    for m in &result.matches {
        let Some(css) = m
            .metadata
            .as_ref()
            .and_then(|v| v.get("css"))
            .and_then(|v| v.as_str())
        else {
            continue;
        };
        let verified = match driver.find(By::Css(css)).await {
            Ok(el) => el.is_displayed().await.unwrap_or(false),
            Err(_) => false,
        };
        candidates.push(SemanticCandidate {
            css: css.to_string(),
            score: m.distance,
            verified,
        });
        if verified && resolved.is_none() {
            resolved = Some(Target {
                css: Some(css.to_string()),
                xpath: None,
                semantic: Some(query.to_string()),
            });
        }
    }

    Ok(SemanticResolution {
        resolved,
        candidates,
    })
}

pub struct RunFlowOptions<'a> {
    pub driver_cache_dir: &'a std::path::Path,
    pub headless: bool,
    pub chroma: &'a crate::chroma::ChromaHandle,
    pub embedder: &'a crate::embedding::EmbeddingEngine,
    pub project_id: &'a str,
    pub heal_write_back: bool,
    /// Some enables write-back on a successful heal; None (e.g. a flow that
    /// hasn't been saved to disk) disables it defensively even if
    /// heal_write_back is true.
    pub flow_path: Option<&'a std::path::Path>,
    pub baselines_dir: &'a std::path::Path,
    pub artifacts_dir: &'a std::path::Path,
    /// design.md §11.1 `--update-baselines`: an AssertVisual step captures
    /// and overwrites its baseline instead of diffing against it — the
    /// standard workflow after an intentional UI change.
    pub update_baselines: bool,
    pub profiles_dir: &'a std::path::Path,
}

/// Tries the concrete selector first; on failure, if the target also
/// carries a `semantic` description, re-resolves it via `resolve_semantic`
/// against the live DOM (design.md §8.2). Never returns an error itself —
/// any internal failure (Chroma down, embedding unavailable) just means no
/// healing happens, and the caller's own action against the original
/// target produces the real, meaningful error.
async fn resolve_with_healing(
    session: &SessionSlot,
    chroma: &crate::chroma::ChromaHandle,
    embedder: &crate::embedding::EmbeddingEngine,
    project_id: &str,
    target: &Target,
) -> (Target, Option<crate::report::HealedEvent>) {
    let concrete_ok = session
        .with_driver(|driver| {
            let target = target.clone();
            async move {
                let by = match target.to_by() {
                    Ok(by) => by,
                    Err(_) => return Ok(false),
                };
                Ok::<bool, AppError>(driver.find(by).await.is_ok())
            }
        })
        .await
        .unwrap_or(false);

    if concrete_ok {
        return (target.clone(), None);
    }
    let Some(query) = target.semantic.clone() else {
        return (target.clone(), None);
    };

    let resolution = session
        .with_driver(|driver| async move {
            resolve_semantic(&driver, chroma, embedder, project_id, &query, 5).await
        })
        .await;

    match resolution {
        Ok(SemanticResolution {
            resolved: Some(healed_target),
            ..
        }) => {
            let old_selector = target
                .css
                .clone()
                .or_else(|| target.xpath.clone())
                .unwrap_or_else(|| "(semantic only, no prior selector)".to_string());
            let new_selector = healed_target.css.clone().unwrap_or_default();
            (
                healed_target,
                Some(crate::report::HealedEvent {
                    old_selector,
                    new_selector,
                }),
            )
        }
        _ => (target.clone(), None),
    }
}

/// Matches the viewport every fixture/e2e test in this project already
/// captures baselines at (design.md §11.2's "fixed viewport" requirement) --
/// a flow can override it via `viewport_width`/`viewport_height`.
const DEFAULT_FLOW_VIEWPORT: (u32, u32) = (1280, 800);

pub async fn run_flow(flow: &Flow, options: RunFlowOptions<'_>) -> RunReport {
    let started = std::time::Instant::now();
    let started_epoch = now_epoch_secs();
    let session = SessionSlot::new();
    let mut env_map = HashMap::new();
    let mut steps_out = Vec::new();

    let validation = (|| -> Result<(), AppError> {
        crate::flow::validate_flow(flow)?;
        crate::project::reject_managed_symlinks(options.artifacts_dir)?;
        if let Some(profile) = &flow.profile {
            managed_path(options.profiles_dir, profile, "")?;
        }
        for step in &flow.steps {
            if let FlowStep::AssertVisual { baseline, .. } = step {
                baseline_paths(options.baselines_dir, baseline)?;
            }
        }
        Ok(())
    })();
    if let Err(e) = validation {
        return RunReport {
            flow_name: flow.name.clone(),
            passed: false,
            steps: vec![StepOutcome {
                description: "validate flow".into(),
                passed: false,
                error: Some(e.to_string()),
                healed: None,
            }],
            duration_ms: started.elapsed().as_millis() as u64,
        };
    }

    for name in &flow.env {
        match std::env::var(name) {
            Ok(v) => {
                env_map.insert(name.clone(), v);
            }
            Err(_) => {
                steps_out.push(StepOutcome {
                    description: format!("resolve required env var {name}"),
                    passed: false,
                    error: Some(format!("required env var {name} was not set")),
                    healed: None,
                });
                return RunReport {
                    flow_name: flow.name.clone(),
                    passed: false,
                    steps: steps_out,
                    duration_ms: started.elapsed().as_millis() as u64,
                };
            }
        }
    }

    let mut caps = DesiredCapabilities::chrome();
    if options.headless {
        if let Err(e) = caps.set_headless() {
            steps_out.push(StepOutcome {
                description: "configure headless chrome".to_string(),
                passed: false,
                error: Some(e.to_string()),
                healed: None,
            });
            return RunReport {
                flow_name: flow.name.clone(),
                passed: false,
                steps: steps_out,
                duration_ms: started.elapsed().as_millis() as u64,
            };
        }
    }
    // design.md §11.2: visual steps require a fixed viewport. Unlike
    // session_start (an interactive live session), a replayed flow has no
    // caller-supplied viewport per call, so this always sets one -- a
    // flow-level override if given, else DEFAULT_FLOW_VIEWPORT -- rather
    // than leaving window size to whatever Chrome's OS/window-manager
    // default happens to be, which would make assert_visual steps
    // non-deterministic against a baseline captured at a known size.
    let (viewport_width, viewport_height) = (
        flow.viewport_width.unwrap_or(DEFAULT_FLOW_VIEWPORT.0),
        flow.viewport_height.unwrap_or(DEFAULT_FLOW_VIEWPORT.1),
    );
    if let Err(e) = caps.add_arg(&format!("--window-size={viewport_width},{viewport_height}")) {
        steps_out.push(StepOutcome {
            description: "configure fixed viewport".to_string(),
            passed: false,
            error: Some(e.to_string()),
            healed: None,
        });
        return RunReport {
            flow_name: flow.name.clone(),
            passed: false,
            steps: steps_out,
            duration_ms: started.elapsed().as_millis() as u64,
        };
    }
    if let Some(name) = &flow.profile {
        if let Err(e) = crate::project::validate_profile_name(name) {
            steps_out.push(StepOutcome {
                description: "configure persistent profile".to_string(),
                passed: false,
                error: Some(e.to_string()),
                healed: None,
            });
            return RunReport {
                flow_name: flow.name.clone(),
                passed: false,
                steps: steps_out,
                duration_ms: started.elapsed().as_millis() as u64,
            };
        }
        let profile_path = match managed_path(options.profiles_dir, name, "") {
            Ok(path) => path,
            Err(e) => {
                return RunReport {
                    flow_name: flow.name.clone(),
                    passed: false,
                    steps: vec![StepOutcome {
                        description: "configure persistent profile".into(),
                        passed: false,
                        error: Some(e.to_string()),
                        healed: None,
                    }],
                    duration_ms: started.elapsed().as_millis() as u64,
                }
            }
        };
        if let Err(e) = caps.add_arg(&format!("--user-data-dir={}", profile_path.display())) {
            steps_out.push(StepOutcome {
                description: "configure persistent profile".to_string(),
                passed: false,
                error: Some(e.to_string()),
                healed: None,
            });
            return RunReport {
                flow_name: flow.name.clone(),
                passed: false,
                steps: steps_out,
                duration_ms: started.elapsed().as_millis() as u64,
            };
        }
    }
    let driver = match WebDriver::managed(caps)
        .cache_dir(options.driver_cache_dir.to_path_buf())
        .await
    {
        Ok(d) => d,
        Err(e) => {
            steps_out.push(StepOutcome {
                description: "launch browser".to_string(),
                passed: false,
                error: Some(e.to_string()),
                healed: None,
            });
            return RunReport {
                flow_name: flow.name.clone(),
                passed: false,
                steps: steps_out,
                duration_ms: started.elapsed().as_millis() as u64,
            };
        }
    };
    if let Err((_, driver)) = session.start(driver).await {
        let _ = driver.quit().await;
    }

    let mut all_passed = true;
    let mut healed_targets: Vec<(usize, String)> = Vec::new();
    for (idx, step) in flow.steps.iter().enumerate() {
        let (description, outcome, healed): (
            String,
            Result<(), AppError>,
            Option<crate::report::HealedEvent>,
        ) = match step {
            FlowStep::Navigate { url } => {
                match interpolate(url, flow.base_url.as_deref(), &env_map) {
                    Ok(resolved) => {
                        let nav_result = do_navigate(&session, resolved).await;
                        if nav_result.is_ok() {
                            // Best-effort, same as the live `navigate`
                            // tool: self-healing (below) needs a fresh
                            // index to query against, especially on a
                            // clean CI machine with no prior live
                            // session. Must never fail the step itself
                            // (FR-011 — flows still run with concrete
                            // selectors when Chroma is down).
                            let now = now_epoch_secs();
                            let index_result = session
                                .with_driver(|driver| async move {
                                    crate::index::index_page(
                                        &driver,
                                        options.chroma,
                                        options.embedder,
                                        options.project_id,
                                        now,
                                    )
                                    .await
                                })
                                .await;
                            if let Err(e) = index_result {
                                tracing::warn!(error = %e, "page indexing skipped during flow_run");
                            }
                        }
                        (format!("navigate to {url}"), nav_result, None)
                    }
                    Err(e) => (format!("navigate to {url}"), Err(e), None),
                }
            }
            FlowStep::Click { target } => {
                let (resolved_target, healed) = resolve_with_healing(
                    &session,
                    options.chroma,
                    options.embedder,
                    options.project_id,
                    target,
                )
                .await;
                (
                    format!("click {target:?}"),
                    do_click(&session, &resolved_target).await,
                    healed,
                )
            }
            FlowStep::TypeText {
                target,
                text,
                clear_first,
                secret,
            } => {
                let (resolved_target, healed) = resolve_with_healing(
                    &session,
                    options.chroma,
                    options.embedder,
                    options.project_id,
                    target,
                )
                .await;
                match interpolate(text, flow.base_url.as_deref(), &env_map) {
                    Ok(value) => (
                        format!("type [redacted] into {target:?}"),
                        do_type_text(&session, &resolved_target, value, *clear_first, *secret)
                            .await
                            .map(|_| ()),
                        healed,
                    ),
                    Err(e) => (format!("type [redacted] into {target:?}"), Err(e), None),
                }
            }
            FlowStep::AssertThat { that } => {
                let description = format!("assert {that:?}");
                let (resolved_that, healed) = match &that.element_visible {
                    Some(target) => {
                        let (resolved_target, healed) = resolve_with_healing(
                            &session,
                            options.chroma,
                            options.embedder,
                            options.project_id,
                            target,
                        )
                        .await;
                        let mut resolved_that = that.clone();
                        resolved_that.element_visible = Some(resolved_target);
                        (resolved_that, healed)
                    }
                    None => (that.clone(), None),
                };
                match do_assert_that(&session, &resolved_that).await {
                    Ok(true) => (description, Ok(()), healed),
                    Ok(false) => (
                        description,
                        Err(AppError::InvalidTarget("assertion failed".to_string())),
                        healed,
                    ),
                    Err(e) => (description, Err(e), healed),
                }
            }
            FlowStep::AssertVisual {
                baseline,
                threshold,
                masks,
            } => {
                let description = format!("assert_visual {baseline}");
                if options.update_baselines {
                    let outcome =
                        save_baseline(&session, options.baselines_dir, baseline, None).await;
                    (description, outcome.map(|_| ()), None)
                } else {
                    let threshold = threshold.unwrap_or(0.001);
                    let outcome = check_visual(
                        &session,
                        options.baselines_dir,
                        baseline,
                        None,
                        masks,
                        threshold,
                    )
                    .await;
                    let result = match outcome {
                        Ok(o) if o.passed => Ok(()),
                        Ok(o) => {
                            let mut msg = match (&o.reason, &o.baseline_meta, o.current_dims) {
                                (Some(reason), Some(baseline_meta), Some((cw, ch))) => format!(
                                    "{reason} (baseline: {}x{}/{}, current: {cw}x{ch}/{})",
                                    baseline_meta.width,
                                    baseline_meta.height,
                                    baseline_meta.os,
                                    std::env::consts::OS
                                ),
                                (Some(reason), ..) => reason.clone(),
                                _ => format!(
                                    "visual diff ratio {:.6} exceeds threshold {:.6}",
                                    o.ratio.unwrap_or(0.0),
                                    o.threshold
                                ),
                            };
                            if let Some(diff_png) = &o.diff_png {
                                if let Ok(diff_path) = managed_path(
                                    options.artifacts_dir,
                                    &format!("visual-diff-{baseline}-{}", now_epoch_secs()),
                                    ".png",
                                ) {
                                    if std::fs::write(&diff_path, diff_png).is_ok() {
                                        msg.push_str(&format!(
                                            " (diff image: {})",
                                            diff_path.display()
                                        ));
                                    }
                                }
                            }
                            Err(AppError::InvalidTarget(msg))
                        }
                        Err(e) => Err(e),
                    };
                    (description, result, None)
                }
            }
        };

        let passed = outcome.is_ok();
        if !passed {
            all_passed = false;
        }
        if let Some(h) = &healed {
            healed_targets.push((idx, h.new_selector.clone()));
        }
        steps_out.push(StepOutcome {
            description,
            passed,
            error: outcome.err().map(|e| e.to_string()),
            healed,
        });
    }

    let _ = session.end().await;

    if options.heal_write_back && !healed_targets.is_empty() {
        if let Some(path) = options.flow_path {
            let mut updated = flow.clone();
            for (idx, new_css) in &healed_targets {
                match &mut updated.steps[*idx] {
                    FlowStep::Click { target } => target.css = Some(new_css.clone()),
                    FlowStep::TypeText { target, .. } => target.css = Some(new_css.clone()),
                    FlowStep::AssertThat { that } => {
                        if let Some(t) = &mut that.element_visible {
                            t.css = Some(new_css.clone());
                        }
                    }
                    FlowStep::Navigate { .. } | FlowStep::AssertVisual { .. } => {}
                }
            }
            let _ = save_flow(path, &updated);
        }
    }

    let report = RunReport {
        flow_name: flow.name.clone(),
        passed: all_passed,
        steps: steps_out,
        duration_ms: started.elapsed().as_millis() as u64,
    };

    if let Err(e) = crate::knowledge::record_run_report(
        options.chroma,
        options.embedder,
        options.project_id,
        &report,
        started_epoch,
    )
    .await
    {
        tracing::warn!(error = %e, "run report was not recorded in the knowledge base");
    }

    report
}

async fn do_assert_that(
    session: &SessionSlot,
    condition: &AssertCondition,
) -> Result<bool, AppError> {
    let resolved = condition.resolve()?;
    match resolved {
        ResolvedCondition::ElementVisible(target) => {
            let target = target.clone();
            session
                .with_driver(|driver| async move {
                    let by = target.to_by()?;
                    match driver.find(by).await {
                        Ok(el) => el.is_displayed().await.map_err(AppError::from),
                        Err(_) => Ok(false),
                    }
                })
                .await
        }
        ResolvedCondition::TextVisible(text) => {
            let text = text.to_string();
            session
                .with_driver(|driver| async move {
                    let body = driver.find(By::Css("body")).await.map_err(AppError::from)?;
                    let visible = body.text().await.map_err(AppError::from)?;
                    Ok(visible.contains(&text))
                })
                .await
        }
        ResolvedCondition::UrlMatches(pattern) => {
            let pattern = pattern.to_string();
            session
                .with_driver(|driver| async move {
                    let url = driver.current_url().await.map_err(AppError::from)?;
                    Ok(url.as_str().contains(&pattern))
                })
                .await
        }
        ResolvedCondition::TitleMatches(pattern) => {
            let pattern = pattern.to_string();
            session
                .with_driver(|driver| async move {
                    let title = driver.title().await.map_err(AppError::from)?;
                    Ok(title.contains(&pattern))
                })
                .await
        }
        ResolvedCondition::ElementCount { target, count } => {
            let target = target.clone();
            session
                .with_driver(|driver| async move {
                    let by = target.to_by()?;
                    let found = driver.find_all(by).await.map_err(AppError::from)?;
                    Ok(found.len() == count)
                })
                .await
        }
    }
}

async fn encode(
    ffmpeg_path: &std::path::Path,
    concat_path: &std::path::Path,
    format: &str,
    output_path: &std::path::Path,
) -> Result<(), AppError> {
    let ffmpeg_path = ffmpeg_path.to_path_buf();
    let concat_path = concat_path.to_path_buf();
    let output_path = output_path.to_path_buf();
    let format = format.to_string();

    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let mut cmd = ffmpeg_sidecar::command::FfmpegCommand::new_with_path(&ffmpeg_path);
        cmd.arg("-y")
            .arg("-f")
            .arg("concat")
            .arg("-safe")
            .arg("0")
            .input(concat_path.to_string_lossy().to_string());

        match format.as_str() {
            "mp4" => {
                cmd.arg("-fps_mode")
                    .arg("vfr")
                    .codec_video("libx264")
                    .pix_fmt("yuv420p")
                    .arg("-vf")
                    .arg("scale=trunc(iw/2)*2:trunc(ih/2)*2");
            }
            "gif" => {
                cmd.arg("-vf").arg(
                    "fps=8,scale=960:-1:flags=lanczos,split[s0][s1];[s0]palettegen[p];[s1][p]paletteuse",
                );
            }
            other => {
                return Err(AppError::InvalidTarget(format!(
                    "unsupported format '{other}' — use \"mp4\" or \"gif\""
                )))
            }
        }
        cmd.output(output_path.to_string_lossy().to_string());

        let mut child = cmd
            .spawn()
            .map_err(|e| AppError::Ffmpeg(format!("failed to spawn ffmpeg: {e}")))?;
        let status = child
            .wait()
            .map_err(|e| AppError::Ffmpeg(format!("ffmpeg wait failed: {e}")))?;
        if !status.success() {
            return Err(AppError::Ffmpeg(format!(
                "ffmpeg exited with {status} encoding {format}"
            )));
        }
        Ok(())
    })
    .await
    .map_err(|e| AppError::Ffmpeg(format!("encode task panicked: {e}")))?
}

#[tool_handler(router = self.tool_router.clone())]
impl ServerHandler for SeleniumMcpServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::from_build_env())
            .with_protocol_version(ProtocolVersion::V_2024_11_05)
            .with_instructions(
                "Drives a real browser like a human tester. Start with session_start, \
                 then navigate/click/type_text/screenshot, then session_end."
                    .to_string(),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stats_from_client_reports_disabled_with_no_client_no_network_no_env() {
        let stats = stats_from_client(None, false, "some-project", 12345).await;

        assert_eq!(stats.chroma.status, "disabled");
        assert!(stats.chroma.url.is_none());
        assert!(stats.elements_count.is_none());
        assert!(stats.runs_count.is_none());
        assert_eq!(stats.cache_disk_usage_bytes, 12345);
    }

    #[test]
    fn frames_preserved_message_names_the_frames_dir_alongside_the_ffmpeg_error() {
        let err = AppError::Ffmpeg("ffmpeg not found and download failed".to_string());
        let frames_dir = std::path::Path::new("/tmp/some-run/frames");

        let message = frames_preserved_message(&err, frames_dir);

        assert!(message.contains("ffmpeg not found and download failed"));
        assert!(message.contains("/tmp/some-run/frames"));
    }
}
