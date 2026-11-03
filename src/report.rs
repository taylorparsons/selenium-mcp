use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct HealedEvent {
    pub old_selector: String,
    pub new_selector: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StepOutcome {
    pub description: String,
    pub passed: bool,
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub healed: Option<HealedEvent>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunReport {
    pub flow_name: String,
    pub passed: bool,
    pub steps: Vec<StepOutcome>,
    pub duration_ms: u64,
}
