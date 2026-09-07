use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Outcome {
    pub(crate) status: &'static str,
    pub(crate) environment: String,
    pub(crate) enabled: bool,
    pub(crate) rule_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) pinned_version: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) rule_arn: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<StepError>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StepError {
    pub(crate) failed_step: &'static str,
    pub(crate) cause: String,
}
