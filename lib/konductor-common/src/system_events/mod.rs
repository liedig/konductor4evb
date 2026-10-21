//! Typed models for Konductor system events: the public platform-to-domain
//! notification contract published on the system bus and routed to
//! per-domain system-events SNS topics.
//!
//! The canonical wire definitions are the checked-in draft-4 schemas under
//! `api/system-events/`; these models mirror them and the module tests
//! validate serialized fixtures against those documents.

mod attrs;
mod event;
mod identity;
mod models;
mod snapshot;
pub mod store;
mod subscriptions;
pub mod topic;

pub use event::{SnsAttribute, SnsAttributeDataType, SystemEvent};
pub use identity::{idempotency_key, UpgradePhase};
pub use models::{
    ContractSelector, ContractSelectors, PartialReason, SystemEventAudience, SystemEventData,
    SystemEventDetail, SystemEventMetadata, SystemEventType, SystemResource, SystemResourceType,
    UpgradeOutcome, UpgradeTrigger, VersionRange, SYSTEM_EVENT_CONTRACT_VERSION,
    SYSTEM_EVENT_SOURCE,
};
pub use snapshot::{SnapshotScope, UpgradeEventSnapshot};
pub use subscriptions::{
    ensure_email_subscription, reconcile_owner_email, remove_email_subscription,
    OwnerEmailReconciliation,
};
pub use topic::{
    delete_if_owned, ensure, expected_topic_arn, subscribe_policy, topic_name, TopicError,
};
