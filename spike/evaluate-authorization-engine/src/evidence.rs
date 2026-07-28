use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::contract::Decision;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StepResult {
    pub label: Option<String>,
    pub command_id: Option<String>,
    pub expected: serde_json::Value,
    pub observed: Option<Decision>,
    pub pass: bool,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioResult {
    pub scenario_id: String,
    pub scenario: String,
    pub spec_reference: String,
    pub step_results: Vec<StepResult>,
    pub passed: bool,
    pub captured_at: DateTime<Utc>,
}
