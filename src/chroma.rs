use crate::error::AppError;
use serde::Deserialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::process::Child;
use tokio::sync::Mutex;

const TENANT: &str = "default_tenant";
const DATABASE: &str = "default_database";
pub const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(600);

struct ChromaState {
    base_url: Option<String>,
    child: Option<Child>,
    last_used: Instant,
    reaper_running: bool,
}

/// Lazily connects to (or spawns) a Chroma server. Resolution order per
/// call: `CHROMA_URL` env var (external server, never spawned or reaped) ->
/// reuse an already-spawned instance -> spawn `chroma run` on a free port ->
/// spawn via `uvx --from chromadb chroma run` -> an actionable error.
#[derive(Clone)]
pub struct ChromaHandle {
    cache_dir: PathBuf,
    idle_timeout: Duration,
    state: Arc<Mutex<ChromaState>>,
}

impl ChromaHandle {
    pub fn new(cache_dir: PathBuf, idle_timeout: Duration) -> Self {
        Self {
            cache_dir,
            idle_timeout,
            state: Arc::new(Mutex::new(ChromaState {
                base_url: None,
                child: None,
                last_used: Instant::now(),
                reaper_running: false,
            })),
        }
    }

    pub async fn ensure_client(&self) -> Result<ChromaClient, AppError> {
        if memory_disabled() {
            return Err(AppError::ChromaUnavailable(
                "disabled by SELENIUM_MCP_DISABLE_MEMORY".into(),
            ));
        }
        if let Ok(url) = std::env::var("CHROMA_URL") {
            return Ok(ChromaClient::new(url));
        }

        let mut state = self.state.lock().await;
        state.last_used = Instant::now();

        if let Some(child) = state.child.as_mut() {
            if matches!(child.try_wait(), Ok(Some(_))) {
                state.base_url = None;
                state.child = None;
            }
        }

        if state.base_url.is_none() {
            let (url, child) = spawn_chroma(&self.cache_dir).await?;
            state.base_url = Some(url);
            state.child = Some(child);
            if !state.reaper_running {
                state.reaper_running = true;
                self.spawn_reaper();
            }
        }

        Ok(ChromaClient::new(state.base_url.clone().unwrap()))
    }

    /// Explicitly kills any spawned Chroma child process. `Child`'s
    /// `kill_on_drop(true)` only fires on a normal Drop, which never
    /// happens if the caller terminates via `std::process::exit()` (as the
    /// CLI test runner does) -- without this, a spawned sidecar leaks past
    /// process exit instead of ever idling out. A no-op when Chroma was
    /// never spawned (never spawned when `CHROMA_URL` is set, or when no
    /// tool in this run ever touched semantic memory).
    pub async fn shutdown(&self) {
        let mut state = self.state.lock().await;
        if let Some(mut child) = state.child.take() {
            let _ = child.kill().await;
        }
        state.base_url = None;
    }

    fn spawn_reaper(&self) {
        let state = self.state.clone();
        let idle_timeout = self.idle_timeout;
        let check_interval = (idle_timeout / 4)
            .max(Duration::from_millis(500))
            .min(Duration::from_secs(30));
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(check_interval).await;
                let mut guard = state.lock().await;
                if guard.child.is_none() {
                    guard.reaper_running = false;
                    return;
                }
                if guard.last_used.elapsed() >= idle_timeout {
                    if let Some(mut child) = guard.child.take() {
                        let _ = child.kill().await;
                    }
                    guard.base_url = None;
                    guard.reaper_running = false;
                    return;
                }
            }
        });
    }
}

pub fn memory_disabled() -> bool {
    std::env::var("SELENIUM_MCP_DISABLE_MEMORY")
        .map(|value| matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false)
}

async fn spawn_chroma(cache_dir: &Path) -> Result<(String, Child), AppError> {
    let data_dir = cache_dir.join("chroma");
    std::fs::create_dir_all(&data_dir).map_err(AppError::from)?;

    let direct = try_spawn("chroma", &[], &data_dir);
    let (url, child) = match direct {
        Ok(pair) => pair,
        Err(direct_err) => try_spawn("uvx", &["--from", "chromadb==1.5.9", "chroma"], &data_dir)
            .map_err(|uvx_err| {
                AppError::ChromaUnavailable(format!(
                    "no Chroma server found: `chroma` CLI failed ({direct_err}) and \
                     `uvx --from chromadb chroma` fallback also failed ({uvx_err}). \
                     Install chromadb (pip install chromadb) or set CHROMA_URL to an \
                     already-running Chroma server."
                ))
            })?,
    };

    wait_for_heartbeat(&url).await?;
    Ok((url, child))
}

fn try_spawn(
    program: &str,
    prefix_args: &[&str],
    data_dir: &Path,
) -> Result<(String, Child), String> {
    let port = free_port().map_err(|e| e.to_string())?;
    let url = format!("http://127.0.0.1:{port}");
    let mut cmd = tokio::process::Command::new(program);
    for a in prefix_args {
        cmd.arg(a);
    }
    cmd.arg("run")
        .arg("--path")
        .arg(data_dir)
        .arg("--host")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(port.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let child = cmd.spawn().map_err(|e| format!("{program}: {e}"))?;
    Ok((url, child))
}

fn free_port() -> Result<u16, AppError> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(AppError::from)?;
    let port = listener.local_addr().map_err(AppError::from)?.port();
    drop(listener);
    Ok(port)
}

async fn wait_for_heartbeat(base_url: &str) -> Result<(), AppError> {
    let client = reqwest::Client::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(resp) = client
            .get(format!("{base_url}/api/v2/heartbeat"))
            .send()
            .await
        {
            if resp.status().is_success() {
                return Ok(());
            }
        }
        if Instant::now() >= deadline {
            return Err(AppError::ChromaUnavailable(format!(
                "Chroma at {base_url} did not respond to a heartbeat within 10s"
            )));
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

#[derive(Clone)]
pub struct ChromaClient {
    http: reqwest::Client,
    base_url: String,
}

#[derive(Debug, Clone)]
pub struct ChromaMatch {
    pub id: String,
    pub document: Option<String>,
    pub metadata: Option<Value>,
    pub distance: Option<f32>,
}

#[derive(Debug, Clone, Default)]
pub struct ChromaQueryResult {
    pub matches: Vec<ChromaMatch>,
}

#[derive(Debug, Deserialize)]
struct ChromaQueryResponse {
    ids: Vec<Vec<String>>,
    #[serde(default)]
    documents: Option<Vec<Vec<Option<String>>>>,
    #[serde(default)]
    metadatas: Option<Vec<Vec<Option<Value>>>>,
    #[serde(default)]
    distances: Option<Vec<Vec<Option<f32>>>>,
}

impl ChromaQueryResult {
    fn from_response(raw: ChromaQueryResponse) -> Self {
        let ids = raw.ids.into_iter().next().unwrap_or_default();
        let mut documents = raw
            .documents
            .and_then(|d| d.into_iter().next())
            .unwrap_or_default();
        let mut metadatas = raw
            .metadatas
            .and_then(|m| m.into_iter().next())
            .unwrap_or_default();
        let mut distances = raw
            .distances
            .and_then(|d| d.into_iter().next())
            .unwrap_or_default();
        documents.resize(ids.len(), None);
        metadatas.resize(ids.len(), None);
        distances.resize(ids.len(), None);

        let matches = ids
            .into_iter()
            .zip(documents)
            .zip(metadatas)
            .zip(distances)
            .map(|(((id, document), metadata), distance)| ChromaMatch {
                id,
                document,
                metadata,
                distance,
            })
            .collect();
        Self { matches }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ChromaGetResult {
    pub matches: Vec<ChromaMatch>,
}

#[derive(Debug, Deserialize)]
struct ChromaGetResponse {
    ids: Vec<String>,
    #[serde(default)]
    documents: Option<Vec<Option<String>>>,
    #[serde(default)]
    metadatas: Option<Vec<Option<Value>>>,
}

impl ChromaGetResult {
    fn from_response(raw: ChromaGetResponse) -> Self {
        let mut documents = raw.documents.unwrap_or_default();
        let mut metadatas = raw.metadatas.unwrap_or_default();
        documents.resize(raw.ids.len(), None);
        metadatas.resize(raw.ids.len(), None);
        let matches = raw
            .ids
            .into_iter()
            .zip(documents)
            .zip(metadatas)
            .map(|((id, document), metadata)| ChromaMatch {
                id,
                document,
                metadata,
                distance: None,
            })
            .collect();
        Self { matches }
    }
}

impl ChromaClient {
    pub fn new(base_url: String) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url,
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn collections_url(&self) -> String {
        format!(
            "{}/api/v2/tenants/{TENANT}/databases/{DATABASE}/collections",
            self.base_url
        )
    }

    fn collection_url(&self, collection_id: &str, suffix: &str) -> String {
        format!(
            "{}/api/v2/tenants/{TENANT}/databases/{DATABASE}/collections/{collection_id}/{suffix}",
            self.base_url
        )
    }

    pub async fn get_or_create_collection(
        &self,
        name: &str,
        space: &str,
    ) -> Result<String, AppError> {
        let body = serde_json::json!({
            "name": name,
            "get_or_create": true,
            "configuration": { "hnsw": { "space": space } },
        });
        let resp = self
            .http
            .post(self.collections_url())
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::Chroma(e.to_string()))?;
        let resp = check_status(resp).await?;
        let value: Value = resp
            .json()
            .await
            .map_err(|e| AppError::Chroma(e.to_string()))?;
        value
            .get("id")
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| AppError::Chroma("collection response missing id".to_string()))
    }

    pub async fn upsert(
        &self,
        collection_id: &str,
        ids: &[String],
        embeddings: &[Vec<f32>],
        documents: &[String],
        metadatas: &[Value],
    ) -> Result<(), AppError> {
        let body = serde_json::json!({
            "ids": ids,
            "embeddings": embeddings,
            "documents": documents,
            "metadatas": metadatas,
        });
        let resp = self
            .http
            .post(self.collection_url(collection_id, "upsert"))
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::Chroma(e.to_string()))?;
        check_status(resp).await?;
        Ok(())
    }

    pub async fn query(
        &self,
        collection_id: &str,
        embedding: &[f32],
        n_results: usize,
        where_clause: Option<Value>,
    ) -> Result<ChromaQueryResult, AppError> {
        let mut body = serde_json::json!({
            "query_embeddings": [embedding],
            "n_results": n_results,
            "include": ["documents", "metadatas", "distances"],
        });
        if let Some(w) = where_clause {
            body["where"] = w;
        }
        let resp = self
            .http
            .post(self.collection_url(collection_id, "query"))
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::Chroma(e.to_string()))?;
        let resp = check_status(resp).await?;
        let raw: ChromaQueryResponse = resp
            .json()
            .await
            .map_err(|e| AppError::Chroma(e.to_string()))?;
        Ok(ChromaQueryResult::from_response(raw))
    }

    pub async fn get(
        &self,
        collection_id: &str,
        where_clause: Option<Value>,
    ) -> Result<ChromaGetResult, AppError> {
        let mut body = serde_json::json!({
            "include": ["documents", "metadatas"],
        });
        if let Some(w) = where_clause {
            body["where"] = w;
        }
        let resp = self
            .http
            .post(self.collection_url(collection_id, "get"))
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::Chroma(e.to_string()))?;
        let resp = check_status(resp).await?;
        let raw: ChromaGetResponse = resp
            .json()
            .await
            .map_err(|e| AppError::Chroma(e.to_string()))?;
        Ok(ChromaGetResult::from_response(raw))
    }

    pub async fn delete(
        &self,
        collection_id: &str,
        ids: Option<&[String]>,
    ) -> Result<(), AppError> {
        let body = match ids {
            Some(ids) => serde_json::json!({ "ids": ids }),
            None => serde_json::json!({}),
        };
        let resp = self
            .http
            .post(self.collection_url(collection_id, "delete"))
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::Chroma(e.to_string()))?;
        check_status(resp).await?;
        Ok(())
    }

    pub async fn count(&self, collection_id: &str) -> Result<u64, AppError> {
        let resp = self
            .http
            .get(self.collection_url(collection_id, "count"))
            .send()
            .await
            .map_err(|e| AppError::Chroma(e.to_string()))?;
        let resp = check_status(resp).await?;
        resp.json::<u64>()
            .await
            .map_err(|e| AppError::Chroma(e.to_string()))
    }
}

async fn check_status(resp: reqwest::Response) -> Result<reqwest::Response, AppError> {
    if resp.status().is_success() {
        Ok(resp)
    } else {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        Err(AppError::Chroma(format!("HTTP {status}: {text}")))
    }
}
