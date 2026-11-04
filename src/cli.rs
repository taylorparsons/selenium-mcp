use crate::chroma::ChromaHandle;
use crate::embedding::EmbeddingEngine;
use crate::flow::load_flow;
use crate::report::RunReport;
use crate::server::{run_flow, RunFlowOptions};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SuiteOutcome {
    AllPassed,
    SomeFailed,
    Infra(String),
}

pub fn exit_code(outcome: &SuiteOutcome) -> i32 {
    match outcome {
        SuiteOutcome::AllPassed => 0,
        SuiteOutcome::SomeFailed => 1,
        SuiteOutcome::Infra(_) => 2,
    }
}

#[derive(Debug, Serialize)]
pub struct SuiteReport {
    pub reports: Vec<RunReport>,
    pub outcome: SuiteOutcome,
}

pub struct RunSuiteOptions<'a> {
    pub flows_dir: &'a Path,
    pub driver_cache_dir: &'a Path,
    pub chroma: &'a ChromaHandle,
    pub embedder: &'a EmbeddingEngine,
    pub project_id: &'a str,
    pub tag_filter: Option<&'a str>,
    pub flow_filter: Option<&'a str>,
    pub heal_write_back: bool,
    pub baselines_dir: &'a Path,
    pub artifacts_dir: &'a Path,
    pub update_baselines: bool,
    pub profiles_dir: &'a Path,
}

pub async fn run_suite(options: RunSuiteOptions<'_>) -> SuiteReport {
    let RunSuiteOptions {
        flows_dir,
        driver_cache_dir,
        chroma,
        embedder,
        project_id,
        tag_filter,
        flow_filter,
        heal_write_back,
        baselines_dir,
        artifacts_dir,
        update_baselines,
        profiles_dir,
    } = options;

    if let Err(e) = crate::project::reject_managed_symlinks(flows_dir) {
        return SuiteReport {
            reports: vec![],
            outcome: SuiteOutcome::Infra(e.to_string()),
        };
    }
    let entries = match std::fs::read_dir(flows_dir) {
        Ok(e) => e,
        Err(e) => {
            return SuiteReport {
                reports: vec![],
                outcome: SuiteOutcome::Infra(format!("can't read flows dir: {e}")),
            }
        }
    };

    let mut yaml_paths: Vec<_> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("yaml"))
        .collect();
    yaml_paths.sort();

    let mut reports = Vec::new();
    let mut any_failed = false;
    let mut infrastructure_error = None;

    for path in yaml_paths {
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        if let Err(e) = crate::project::managed_path(flows_dir, name, ".yaml") {
            return SuiteReport {
                reports,
                outcome: SuiteOutcome::Infra(e.to_string()),
            };
        }
        let flow = match load_flow(&path) {
            Ok(f) => f,
            Err(e) => {
                return SuiteReport {
                    reports,
                    outcome: SuiteOutcome::Infra(format!("malformed flow {}: {e}", path.display())),
                }
            }
        };

        if let Some(name) = flow_filter {
            if flow.name != name {
                continue;
            }
        }
        if let Some(tag) = tag_filter {
            if !flow.tags.iter().any(|t| t == tag) {
                continue;
            }
        }

        let report = run_flow(
            &flow,
            RunFlowOptions {
                driver_cache_dir,
                headless: true,
                chroma,
                embedder,
                project_id,
                heal_write_back,
                flow_path: Some(&path),
                baselines_dir,
                artifacts_dir,
                update_baselines,
                profiles_dir,
            },
        )
        .await;
        if let Some(reason) = infrastructure_failure(&report) {
            infrastructure_error = Some(reason);
        }
        if !report.passed {
            any_failed = true;
        }
        reports.push(report);
    }

    let outcome = if let Some(reason) = infrastructure_error {
        SuiteOutcome::Infra(reason)
    } else if any_failed {
        SuiteOutcome::SomeFailed
    } else {
        SuiteOutcome::AllPassed
    };
    SuiteReport { reports, outcome }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_passed_maps_to_zero() {
        assert_eq!(exit_code(&SuiteOutcome::AllPassed), 0);
    }

    #[test]
    fn some_failed_maps_to_one() {
        assert_eq!(exit_code(&SuiteOutcome::SomeFailed), 1);
    }

    #[test]
    fn infra_maps_to_two() {
        assert_eq!(exit_code(&SuiteOutcome::Infra("bad driver".to_string())), 2);
    }
}

// run_flow emits these setup outcomes before executing any user steps.
fn infrastructure_failure(report: &RunReport) -> Option<String> {
    let step = report.steps.first()?;
    if !step.passed && matches!(step.description.as_str(),
        "launch browser" | "configure headless chrome" | "configure fixed viewport" |
        "configure persistent profile" | "validate flow"
    ) {
        Some(step.error.clone().unwrap_or_else(|| step.description.clone()))
    } else {
        None
    }
}

#[cfg(test)]
mod infrastructure_tests {
    use super::*;
    use crate::report::StepOutcome;

    #[test]
    fn browser_launch_failure_is_infrastructure_but_assertion_is_not() {
        let mut report = RunReport {
            flow_name: "test".into(), passed: false, duration_ms: 0,
            steps: vec![StepOutcome {
                description: "launch browser".into(), passed: false,
                error: Some("driver unavailable".into()), healed: None,
            }],
        };
        assert_eq!(infrastructure_failure(&report).as_deref(), Some("driver unavailable"));
        report.steps[0].description = "assert_that".into();
        assert!(infrastructure_failure(&report).is_none());
    }
}
