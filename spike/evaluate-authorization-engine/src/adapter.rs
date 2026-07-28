use std::future::Future;

use thiserror::Error;

use crate::contract::{Action, AuthContext, AuthorizationSubject, Decision, Resource};
use crate::manifest::{GrantSeed, SubscriptionSeed};

#[derive(Debug, Error)]
pub enum AdapterError {
    #[error("seed failed: {0}")]
    Seed(String),
    #[error("decision failed: {0}")]
    Decision(String),
    #[error("revoke failed: {0}")]
    Revoke(String),
    #[error("schema registration failed: {0}")]
    SchemaRegistration(String),
    #[error("subscription state lookup failed: {0}")]
    SubscriptionState(String),
    #[error("discover list failed: {0}")]
    DiscoverList(String),
}

/// Engine-neutral grant adapter seam. The DynamoDB adapter (task 2.2) implements
/// this trait; the spike runner is generic over `A: GrantAdapter`. Methods return
/// `Send` futures so the runner can spawn concurrent work in the throughput profile.
pub trait GrantAdapter {
    fn seed(
        &self,
        grants: &[GrantSeed],
        subscriptions: &[SubscriptionSeed],
    ) -> impl Future<Output = Result<(), AdapterError>> + Send;

    fn decide(
        &self,
        subject: &AuthorizationSubject,
        action: &Action,
        resource: &Resource,
        context: &AuthContext,
    ) -> impl Future<Output = Result<Decision, AdapterError>> + Send;

    fn revoke(&self, grant_id: &str) -> impl Future<Output = Result<(), AdapterError>> + Send;

    fn register_schema(
        &self,
        producer_domain: &str,
        schema: &str,
    ) -> impl Future<Output = Result<(), AdapterError>> + Send;

    fn subscription_state(
        &self,
        subscription_id: &str,
    ) -> impl Future<Output = Result<String, AdapterError>> + Send;

    fn discover_list(
        &self,
        subject: &AuthorizationSubject,
        producer_domain: &str,
        context: &AuthContext,
    ) -> impl Future<Output = Result<Vec<String>, AdapterError>> + Send;
}
