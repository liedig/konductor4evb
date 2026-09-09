//! Platform-owned event metadata: immutable additive revisions composed with
//! business schemas into exact event contracts.
//!
//! The built-in metadata fields (`schemaVersion`, `idempotencyKey`,
//! `publishedAt`, `metadataRevision`) are platform-owned and never
//! configurable. Everything else in this module deals with the extension
//! definitions a platform administrator can add on top of those defaults.

pub mod additive;
pub mod composition;
pub mod definition;
pub mod generation;
pub mod schema;
pub mod selector;
pub mod store;

pub use definition::{
    GeneratedType, Generator, MetadataDefinition, MetadataNode, MAX_DEFINITION_BYTES,
    MAX_EXTENSION_NODES, MAX_OBJECT_LEVELS, RESERVED_ROOT_FIELDS,
};
pub use selector::{ContractSelector, MetadataRevision};
