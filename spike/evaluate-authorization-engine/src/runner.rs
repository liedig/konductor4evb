use chrono::Utc;
use serde_json::Value;

use crate::adapter::GrantAdapter;
use crate::contract::{Action, AuthContext, AuthorizationSubject, Decision, DenyReason, Resource};
use crate::evidence::{ScenarioResult, StepResult};
use crate::manifest::{Manifest, Scenario, Step};

pub struct ScenarioRunner<'a, A: GrantAdapter> {
    pub manifest: &'a Manifest,
    pub adapter: &'a A,
}

impl<'a, A: GrantAdapter> ScenarioRunner<'a, A> {
    pub fn new(manifest: &'a Manifest, adapter: &'a A) -> Self {
        Self { manifest, adapter }
    }

    pub async fn run_all(&self) -> Vec<ScenarioResult> {
        let mut results = Vec::with_capacity(self.manifest.scenarios.len());
        for scenario in &self.manifest.scenarios {
            results.push(self.run_scenario(scenario).await);
        }
        results
    }

    async fn run_scenario(&self, scenario: &Scenario) -> ScenarioResult {
        let mut step_results = Vec::new();
        if scenario.steps.is_empty() {
            step_results.push(self.run_step(scenario, &Step::default()).await);
        } else {
            for step in &scenario.steps {
                step_results.push(self.run_step(scenario, step).await);
            }
        }
        let passed = step_results.iter().all(|s| s.pass);
        ScenarioResult {
            scenario_id: scenario.id.clone(),
            scenario: scenario.scenario.clone(),
            spec_reference: scenario.spec_reference.clone(),
            step_results,
            passed,
            captured_at: Utc::now(),
        }
    }

    async fn run_step(&self, scenario: &Scenario, step: &Step) -> StepResult {
        let (subject, action, resource, context) = self.resolve_inputs(scenario, step);

        if let Some(grant_id) = step.revoke.as_ref() {
            let _ = self.adapter.revoke(grant_id).await;
        }
        if let Some(before_consume) = step.before_consume.as_ref() {
            let _ = self.adapter.revoke(&before_consume.revoke).await;
        }

        let expected_val = step
            .expected
            .as_ref()
            .or(scenario.expected.as_ref())
            .or(step.expected_handler_decision.as_ref())
            .or(step.expected_decision_after_revocation.as_ref())
            .cloned()
            .unwrap_or(Value::Null);

        let (observed, pass) = match (subject, action, resource, context) {
            (Some(subject), Some(action), Some(resource), Some(context)) => match resource {
                Resource::SchemaList { producer_domain } => {
                    let visible = self
                        .adapter
                        .discover_list(subject, producer_domain, context)
                        .await
                        .unwrap_or_default();
                    let expected_visible = parse_visible_schemas(&expected_val);
                    (Some(Decision::Allow), visible == expected_visible)
                }
                _ => {
                    let observed = self
                        .adapter
                        .decide(subject, action, resource, context)
                        .await
                        .ok();
                    let expected_decision =
                        parse_expected(&expected_val, step.deny_reason.as_ref());
                    let pass = observed
                        .as_ref()
                        .map(|observed| *observed == expected_decision)
                        .unwrap_or(false);
                    (observed, pass)
                }
            },
            _ => (None, false),
        };

        let command_id = context.map(|context| context.command_id.clone());
        StepResult {
            label: step.label.clone(),
            command_id,
            expected: expected_val,
            observed,
            pass,
            notes: step.notes.clone().or(scenario.notes.clone()),
        }
    }

    fn resolve_inputs<'b>(
        &self,
        scenario: &'b Scenario,
        step: &'b Step,
    ) -> (
        Option<&'b AuthorizationSubject>,
        Option<&'b Action>,
        Option<&'b Resource>,
        Option<&'b AuthContext>,
    ) {
        if let Some(enqueue) = step.enqueue.as_ref() {
            return (
                Some(&enqueue.authorization_subject),
                Some(&enqueue.action),
                Some(&enqueue.resource),
                Some(&enqueue.context),
            );
        }
        (
            step.authorization_subject
                .as_ref()
                .or(scenario.authorization_subject.as_ref()),
            step.action.as_ref().or(scenario.action.as_ref()),
            step.resource.as_ref().or(scenario.resource.as_ref()),
            step.context.as_ref().or(scenario.context.as_ref()),
        )
    }
}

fn parse_expected(value: &Value, deny_reason: Option<&DenyReason>) -> Decision {
    match value {
        Value::String(s) => match s.as_str() {
            "allow" => Decision::Allow,
            "deny" => Decision::Deny {
                reason: deny_reason.cloned().unwrap_or(DenyReason::Unknown),
            },
            "deny-as-unknown" => Decision::DenyAsUnknown,
            _ => Decision::Allow,
        },
        _ => Decision::Allow,
    }
}

fn parse_visible_schemas(value: &Value) -> Vec<String> {
    value
        .get("visibleSchemas")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}
