use std::collections::HashSet;

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
            self.seed_scenario(scenario).await;
            results.push(self.run_scenario(scenario).await);
        }
        results
    }

    async fn seed_scenario(&self, scenario: &Scenario) {
        let grants: Vec<_> = self
            .manifest
            .baseline_grant_seeds
            .iter()
            .filter(|g| scenario.seed_grants.iter().any(|id| id == &g.grant_id))
            .cloned()
            .collect();
        let subscriptions: Vec<_> = self
            .manifest
            .baseline_subscription_seeds
            .iter()
            .filter(|s| {
                scenario
                    .seed_subscriptions
                    .iter()
                    .any(|id| id == &s.subscription_id)
            })
            .cloned()
            .collect();
        let _ = self.adapter.seed(&grants, &subscriptions).await;

        let growth =
            parse_growth_schemas(self.manifest.topology.note_on_producer_growth.as_deref());
        let register_growth = scenario
            .precondition
            .as_deref()
            .map(|p| p.contains("registered AFTER"))
            .unwrap_or(false);
        for producer in &self.manifest.topology.producer_domains {
            for schema in &producer.schemas {
                let is_growth = growth.iter().any(|(d, s)| d == &producer.id && s == schema);
                if !is_growth || register_growth {
                    let _ = self.adapter.register_schema(&producer.id, schema).await;
                }
            }
        }
    }

    async fn run_scenario(&self, scenario: &Scenario) -> ScenarioResult {
        let tested: HashSet<String> = scenario
            .steps
            .iter()
            .flat_map(|s| s.applicable_grants.iter().cloned())
            .collect();
        let mut current: HashSet<String> = scenario
            .seed_grants
            .iter()
            .filter(|g| tested.contains(*g))
            .cloned()
            .collect();

        let mut step_results = Vec::new();
        if scenario.steps.is_empty() {
            step_results.push(self.run_step(scenario, &Step::default()).await);
        } else {
            for step in &scenario.steps {
                if !tested.is_empty() {
                    let target: HashSet<String> = step.applicable_grants.iter().cloned().collect();
                    for grant_id in current.difference(&target) {
                        let _ = self.adapter.revoke(grant_id).await;
                    }
                    current = target;
                }
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

        let (observed, mut pass) = match (subject, action, resource, context) {
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
                    let deny_without_reason = step.deny_reason.is_none()
                        && matches!(&expected_val, Value::String(s) if s == "deny");
                    let pass = observed
                        .as_ref()
                        .map(|observed| {
                            if deny_without_reason {
                                matches!(observed, Decision::Deny { .. })
                            } else {
                                *observed == expected_decision
                            }
                        })
                        .unwrap_or(false);
                    (observed, pass)
                }
            },
            _ => (None, false),
        };

        if let Some(expected_sub) = step.expected_subscription_state.as_ref() {
            let actual = self
                .adapter
                .subscription_state(&expected_sub.subscription_id)
                .await
                .unwrap_or_else(|_| "unknown".to_string());
            pass = pass && actual == expected_sub.state;
        }

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

fn parse_growth_schemas(note: Option<&str>) -> Vec<(String, String)> {
    let Some(note) = note else {
        return Vec::new();
    };
    note.split_whitespace()
        .filter_map(|token| {
            let (domain, schema) = token.split_once("::")?;
            let schema =
                schema.trim_end_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_');
            Some((domain.to_string(), schema.to_string()))
        })
        .collect()
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
