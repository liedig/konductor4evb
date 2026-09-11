use chrono::{DateTime, NaiveDate, SecondsFormat, Utc};
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::error::ControlPlaneError;
use crate::metadata::definition::{
    GeneratedType, Generator, MetadataNode, ValidatedMetadataDefinition,
};

/// Trusted, platform-resolved context captured once per admission. The
/// admission instant is fixed at capture time and shared by every
/// `utc-timestamp` and `utc-date` field of that admission, so generation is a
/// pure function of the definition, this context, and the UUID source; tests
/// inject the instant instead of a global clock.
#[derive(Debug, Clone, PartialEq)]
pub struct GenerationContext {
    admission_instant: DateTime<Utc>,
    region: String,
    organisation: String,
    environment: String,
}

impl GenerationContext {
    /// Builds a context from trusted sources. A missing or empty source is a
    /// platform fault, not a producer error, and fails before any value is
    /// generated.
    pub fn new(
        admission_instant: DateTime<Utc>,
        region: impl Into<String>,
        organisation: impl Into<String>,
        environment: impl Into<String>,
    ) -> Result<Self, ControlPlaneError> {
        let region = region.into();
        let organisation = organisation.into();
        let environment = environment.into();
        for (source, value) in [
            ("processing region", &region),
            ("organisation", &organisation),
            ("environment", &environment),
        ] {
            if value.trim().is_empty() {
                return Err(generation_failure(format!(
                    "trusted {source} context is unavailable"
                )));
            }
        }
        Ok(Self {
            admission_instant,
            region,
            organisation,
            environment,
        })
    }

    pub fn admission_instant(&self) -> DateTime<Utc> {
        self.admission_instant
    }

    pub fn region(&self) -> &str {
        &self.region
    }

    pub fn organisation(&self) -> &str {
        &self.organisation
    }

    pub fn environment(&self) -> &str {
        &self.environment
    }
}

/// Source of per-admission UUID-v4 values. Injected in tests to keep
/// generation deterministic; production uses the platform RNG.
pub trait UuidSource: Send + Sync {
    /// Produces a canonical lowercase UUID-v4 string.
    fn uuid_v4(&self) -> String;
}

/// Production source backed by the platform RNG.
pub struct PlatformUuidSource;

impl UuidSource for PlatformUuidSource {
    fn uuid_v4(&self) -> String {
        Uuid::new_v4().to_string()
    }
}

/// Generates every configured extension value for one admission from the
/// trusted context. There is no channel for producer-supplied values: the
/// output contains exactly the configured fields. Each generated value is
/// checked against its derived constraint; a generator producing an invalid
/// value fails the admission rather than queueing a malformed event.
pub fn generate_extensions(
    definition: &ValidatedMetadataDefinition,
    context: &GenerationContext,
    uuid_source: &dyn UuidSource,
) -> Result<Map<String, Value>, ControlPlaneError> {
    let mut output = Map::new();
    generate_children(definition.children(), context, uuid_source, &mut output)?;
    Ok(output)
}

fn generate_children(
    children: &[MetadataNode],
    context: &GenerationContext,
    uuid_source: &dyn UuidSource,
    output: &mut Map<String, Value>,
) -> Result<(), ControlPlaneError> {
    for node in children {
        match node {
            MetadataNode::Field { name, generator } => {
                let value = generate_value(*generator, context, uuid_source)?;
                output.insert(name.clone(), Value::String(value));
            }
            MetadataNode::Object { name, children } => {
                let mut nested = Map::new();
                generate_children(children, context, uuid_source, &mut nested)?;
                output.insert(name.clone(), Value::Object(nested));
            }
        }
    }
    Ok(())
}

fn generate_value(
    generator: Generator,
    context: &GenerationContext,
    uuid_source: &dyn UuidSource,
) -> Result<String, ControlPlaneError> {
    let value = match generator {
        Generator::UuidV4 => uuid_source.uuid_v4(),
        Generator::UtcTimestamp => context
            .admission_instant
            .to_rfc3339_opts(SecondsFormat::Millis, true),
        Generator::UtcDate => context.admission_instant.format("%Y-%m-%d").to_string(),
        Generator::AwsRegion => context.region().to_string(),
        Generator::Organisation => context.organisation().to_string(),
        Generator::Environment => context.environment().to_string(),
    };
    validate_generated_value(generator, &value)?;
    Ok(value)
}

/// Confirms a generated value satisfies its generator's derived constraint.
/// This guards against a faulty source (or a broken future generator)
/// queueing values the event's own contract would reject.
fn validate_generated_value(generator: Generator, value: &str) -> Result<(), ControlPlaneError> {
    let valid = match generator.generated_type() {
        GeneratedType::UuidV4 => is_uuid_v4(value),
        GeneratedType::UtcTimestamp => parse_rfc3339_millis_utc(value).is_some(),
        GeneratedType::UtcDate => NaiveDate::parse_from_str(value, "%Y-%m-%d").is_ok(),
        GeneratedType::NonEmptyString => !value.trim().is_empty(),
    };
    if valid {
        Ok(())
    } else {
        Err(generation_failure(format!(
            "generator '{}' produced a value that violates its own constraint",
            generator.as_str()
        )))
    }
}

fn is_uuid_v4(value: &str) -> bool {
    match Uuid::parse_str(value) {
        Ok(id) => id.get_version_num() == 4 && id.get_variant() == uuid::Variant::RFC4122,
        Err(_) => false,
    }
}

/// Accepts exactly the canonical UTC form with millisecond precision and a
/// `Z` suffix, matching the `publishedAt` representation.
fn parse_rfc3339_millis_utc(value: &str) -> Option<DateTime<Utc>> {
    let parsed = DateTime::parse_from_rfc3339(value).ok()?;
    let utc = parsed.with_timezone(&Utc);
    if utc.to_rfc3339_opts(SecondsFormat::Millis, true) == value {
        Some(utc)
    } else {
        None
    }
}

fn generation_failure(reason: String) -> ControlPlaneError {
    ControlPlaneError::MetadataGenerationFailed { reason }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::definition::MetadataDefinition;

    struct ScriptedUuidSource;

    impl UuidSource for ScriptedUuidSource {
        fn uuid_v4(&self) -> String {
            "0f14d0ab-9605-4d62-bc3f-abcdef123456".to_string()
        }
    }

    struct FaultyUuidSource;

    impl UuidSource for FaultyUuidSource {
        fn uuid_v4(&self) -> String {
            "not-a-uuid".to_string()
        }
    }

    fn field(name: &str, generator: Generator) -> MetadataNode {
        MetadataNode::Field {
            name: name.to_string(),
            generator,
        }
    }

    fn object(name: &str, children: Vec<MetadataNode>) -> MetadataNode {
        MetadataNode::Object {
            name: name.to_string(),
            children,
        }
    }

    fn context() -> GenerationContext {
        GenerationContext::new(
            DateTime::from_timestamp_millis(1_785_407_400_123).unwrap(),
            "us-east-1",
            "acme",
            "staging",
        )
        .unwrap()
    }

    fn definition(children: Vec<MetadataNode>) -> ValidatedMetadataDefinition {
        ValidatedMetadataDefinition::validate(MetadataDefinition { children }).unwrap()
    }

    #[test]
    fn empty_definition_generates_no_extensions() {
        let output =
            generate_extensions(&definition(vec![]), &context(), &ScriptedUuidSource).unwrap();
        assert!(output.is_empty());
    }

    #[test]
    fn output_contains_exactly_the_configured_fields() {
        let definition = definition(vec![
            field("eventId", Generator::UuidV4),
            object("correlation", vec![field("traceDate", Generator::UtcDate)]),
        ]);
        let output = generate_extensions(&definition, &context(), &ScriptedUuidSource).unwrap();
        assert_eq!(output.len(), 2);
        assert!(output.contains_key("eventId"));
        let nested = output.get("correlation").unwrap().as_object().unwrap();
        assert_eq!(nested.len(), 1);
        assert!(nested.contains_key("traceDate"));
        // Built-ins are stamped by the envelope, never by extension
        // generation; nothing else may appear.
        for reserved in [
            "schemaVersion",
            "idempotencyKey",
            "publishedAt",
            "metadataRevision",
        ] {
            assert!(!output.contains_key(reserved));
        }
    }

    #[test]
    fn platform_source_produces_uuid_v4_values() {
        let definition = definition(vec![field("eventId", Generator::UuidV4)]);
        let output = generate_extensions(&definition, &context(), &PlatformUuidSource).unwrap();
        let value = output.get("eventId").unwrap().as_str().unwrap();
        assert!(is_uuid_v4(value), "platform UUID source produced {value}");
    }

    #[test]
    fn generation_is_deterministic_for_identical_inputs() {
        let definition = definition(vec![
            field("eventId", Generator::UuidV4),
            field("emittedAt", Generator::UtcTimestamp),
            field("emitDate", Generator::UtcDate),
        ]);
        let first = generate_extensions(&definition, &context(), &ScriptedUuidSource).unwrap();
        let second = generate_extensions(&definition, &context(), &ScriptedUuidSource).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn timestamp_and_date_share_the_captured_admission_instant() {
        let definition = definition(vec![
            field("emittedAt", Generator::UtcTimestamp),
            field("emitDate", Generator::UtcDate),
            object("extra", vec![field("emitDate2", Generator::UtcDate)]),
        ]);
        let output = generate_extensions(&definition, &context(), &ScriptedUuidSource).unwrap();
        assert_eq!(output["emittedAt"], "2026-07-30T10:30:00.123Z");
        assert_eq!(output["emitDate"], "2026-07-30");
        assert_eq!(output["extra"]["emitDate2"], "2026-07-30");
    }

    #[test]
    fn trusted_sources_flow_to_their_fields() {
        let definition = definition(vec![
            field("region", Generator::AwsRegion),
            field("org", Generator::Organisation),
            field("stage", Generator::Environment),
        ]);
        let output = generate_extensions(&definition, &context(), &ScriptedUuidSource).unwrap();
        assert_eq!(output["region"], "us-east-1");
        assert_eq!(output["org"], "acme");
        assert_eq!(output["stage"], "staging");
    }

    #[test]
    fn missing_trusted_context_fails_with_the_source_named() {
        let instant = DateTime::from_timestamp_millis(1_785_407_400_123).unwrap();
        for (region, organisation, environment, missing) in [
            ("", "acme", "staging", "processing region"),
            ("us-east-1", "", "staging", "organisation"),
            ("us-east-1", "acme", "", "environment"),
        ] {
            let err =
                GenerationContext::new(instant, region, organisation, environment).unwrap_err();
            let ControlPlaneError::MetadataGenerationFailed { reason } = err else {
                panic!("expected MetadataGenerationFailed for {missing}");
            };
            assert!(reason.contains(missing), "reason: {reason}");
        }
    }

    #[test]
    fn a_faulty_uuid_source_fails_generation() {
        let definition = definition(vec![field("eventId", Generator::UuidV4)]);
        let err = generate_extensions(&definition, &context(), &FaultyUuidSource).unwrap_err();
        match err {
            ControlPlaneError::MetadataGenerationFailed { reason } => {
                assert!(reason.contains("uuid-v4"), "reason: {reason}");
            }
            other => panic!("expected MetadataGenerationFailed, got {other}"),
        }
    }

    #[test]
    fn rfc3339_check_rejects_non_millisecond_forms() {
        assert!(parse_rfc3339_millis_utc("2026-07-30T10:30:00.123Z").is_some());
        // Different precision, offset, or garbage all fail the exact form.
        assert!(parse_rfc3339_millis_utc("2026-07-30T10:30:00Z").is_none());
        assert!(
            parse_rfc3339_millis_utc("2026-07-30T10:30:00.123456Z").is_none(),
            "microsecond precision must not pass as milliseconds"
        );
        assert!(parse_rfc3339_millis_utc("2026-07-30T10:30:00.123+02:00").is_none());
        assert!(parse_rfc3339_millis_utc("garbage").is_none());
    }

    #[test]
    fn uuid_v4_shape_check_rejects_wrong_versions_and_variants() {
        assert!(is_uuid_v4("0f14d0ab-9605-4d62-bc3f-abcdef123456"));
        // UUID v1 shape: valid UUID, wrong version.
        assert!(!is_uuid_v4("0f14d0ab-9605-1d62-bc3f-abcdef123456"));
        assert!(!is_uuid_v4("0f14d0ab-9605-4d62-bc3f-abcdef12345"));
        assert!(!is_uuid_v4("not-a-uuid"));
    }
}
