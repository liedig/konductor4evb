//! Shared DynamoDB vocabulary for the metadata configuration table: partition
//! constants and sort-key builders used by the bootstrap, administration,
//! and admission paths so every component addresses the same items.
//!
//! Item shapes:
//! - `REVISION` / `r{revision:010}`: immutable revision records.
//! - `HEAD` / `HEAD`: the global revision head.
//! - `ENV` / `{environment}`: per-environment active pointers (hot reads
//!   never share a partition with revision or audit traffic).
//! - `AUDIT` / `{unix-millis}#{operationId}`: immutable mutation/audit
//!   records, paginating chronologically.

pub const REVISION_PARTITION: &str = "REVISION";
pub const HEAD_PARTITION: &str = "HEAD";
pub const ENVIRONMENT_PARTITION: &str = "ENV";
pub const AUDIT_PARTITION: &str = "AUDIT";

pub const HEAD_SORT_KEY: &str = "HEAD";

/// Zero-padded so lexicographic sort order matches numeric revision order
/// for paginated history.
pub fn revision_sort_key(revision: u32) -> String {
    format!("r{revision:010}")
}

/// Audit sort key: activation/mutation time first so history paginates
/// chronologically; the operation id disambiguates same-millisecond writes.
pub fn audit_sort_key(unix_millis: i64, operation_id: &str) -> String {
    format!("{unix_millis}#{operation_id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revision_keys_pad_for_lexicographic_numeric_order() {
        let keys = [1, 2, 10, 100, 123456789].map(revision_sort_key).to_vec();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);
        assert_eq!(revision_sort_key(1), "r0000000001");
    }

    #[test]
    fn audit_keys_sort_chronologically_then_by_operation() {
        let a = audit_sort_key(1000, "op-b");
        let b = audit_sort_key(1001, "op-a");
        let c = audit_sort_key(1001, "op-b");
        assert!(a < b);
        assert!(b < c);
        assert_eq!(c, "1001#op-b");
    }
}
