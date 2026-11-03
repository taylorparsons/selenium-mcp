use crate::chroma::ChromaHandle;
use crate::embedding::EmbeddingEngine;
use crate::error::AppError;
use crate::index::runs_collection_name;
use crate::project::hex_prefix;
use crate::report::RunReport;
use sha2::{Digest, Sha256};

pub fn run_id(flow_name: &str, started_epoch: u64) -> String {
    let mut hasher = Sha256::new();
    hasher.update(flow_name.as_bytes());
    hasher.update(started_epoch.to_string().as_bytes());
    let digest = hasher.finalize();
    hex_prefix(&digest, 16)
}

/// Natural-language summary a `knowledge_search` embedding is built from —
/// design.md §8.3: "what usually breaks on the checkout flow" should match
/// on wording like this, not raw structured fields.
pub fn summarize_run(report: &RunReport) -> String {
    if report.passed {
        format!(
            "{}: PASSED in {}ms, {}/{} steps",
            report.flow_name,
            report.duration_ms,
            report.steps.iter().filter(|s| s.passed).count(),
            report.steps.len(),
        )
    } else {
        match report.steps.iter().enumerate().find(|(_, s)| !s.passed) {
            Some((i, step)) => format!(
                "{}: FAILED at step {} ({}): {}",
                report.flow_name,
                i + 1,
                step.description,
                step.error.as_deref().unwrap_or("unknown error"),
            ),
            None => format!("{}: FAILED", report.flow_name),
        }
    }
}

/// Best-effort — a Chroma outage must never fail a flow run (FR-011). Only
/// called for runs that actually executed steps; infra-level failures
/// (missing env var, browser launch failure) never reach `run_flow`'s step
/// loop and are not recorded here — they're a "the run never started"
/// condition, not a flow outcome to search over.
pub async fn record_run_report(
    chroma: &ChromaHandle,
    embedder: &EmbeddingEngine,
    project_id: &str,
    report: &RunReport,
    started_epoch: u64,
) -> Result<(), AppError> {
    if crate::chroma::memory_disabled() {
        return Ok(());
    }
    let healed_count = report.steps.iter().filter(|s| s.healed.is_some()).count();
    let summary = summarize_run(report);
    let embedding = embedder.embed_one(&summary).await?;

    let client = chroma.ensure_client().await?;
    let collection_id = client
        .get_or_create_collection(&runs_collection_name(project_id), "cosine")
        .await?;

    let id = run_id(&report.flow_name, started_epoch);
    let metadata = serde_json::json!({
        "flow_name": report.flow_name,
        "passed": report.passed,
        "duration_ms": report.duration_ms,
        "started_epoch": started_epoch,
        "healed_count": healed_count as u64,
    });

    client
        .upsert(&collection_id, &[id], &[embedding], &[summary], &[metadata])
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::StepOutcome;

    fn report(flow_name: &str, passed: bool, steps: Vec<StepOutcome>) -> RunReport {
        RunReport {
            flow_name: flow_name.to_string(),
            passed,
            steps,
            duration_ms: 1234,
        }
    }

    #[test]
    fn run_id_is_stable_and_sixteen_hex_chars() {
        let a = run_id("login-happy-path", 1_700_000_000);
        let b = run_id("login-happy-path", 1_700_000_000);
        let c = run_id("login-happy-path", 1_700_000_001);

        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.len(), 16);
        assert!(a.chars().all(|ch| ch.is_ascii_hexdigit()));
    }

    #[test]
    fn summarize_run_describes_a_pass() {
        let r = report(
            "login-happy-path",
            true,
            vec![
                StepOutcome {
                    description: "navigate".to_string(),
                    passed: true,
                    error: None,
                    healed: None,
                },
                StepOutcome {
                    description: "click".to_string(),
                    passed: true,
                    error: None,
                    healed: None,
                },
            ],
        );
        assert_eq!(
            summarize_run(&r),
            "login-happy-path: PASSED in 1234ms, 2/2 steps"
        );
    }

    #[test]
    fn summarize_run_names_the_first_failing_step() {
        let r = report(
            "login-happy-path",
            false,
            vec![
                StepOutcome {
                    description: "navigate".to_string(),
                    passed: true,
                    error: None,
                    healed: None,
                },
                StepOutcome {
                    description: "click submit button".to_string(),
                    passed: false,
                    error: Some("element not found".to_string()),
                    healed: None,
                },
            ],
        );
        assert_eq!(
            summarize_run(&r),
            "login-happy-path: FAILED at step 2 (click submit button): element not found"
        );
    }
}
