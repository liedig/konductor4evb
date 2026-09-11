use crate::error::{ControlPlaneError, ValidationExceptionField, ValidationExceptionReason};
use crate::metadata::definition::{MetadataDefinition, MetadataNode, ValidatedMetadataDefinition};

/// Validates a candidate definition and confirms it is additive relative to
/// the latest saved organisation revision, returning the validated candidate
/// ready for persistence as a new immutable revision.
///
/// Additive means: every node present in the head is still present with the
/// exact same kind and generator, and the only changes are new fields or new
/// children on existing objects. Child ordering is not significant. No role
/// or request flag can bypass this check.
pub fn check_additive(
    candidate: MetadataDefinition,
    head: &ValidatedMetadataDefinition,
) -> Result<ValidatedMetadataDefinition, ControlPlaneError> {
    let validated = ValidatedMetadataDefinition::validate(candidate)?;
    let mut violations = Vec::new();
    let mut path = Vec::new();
    compare_children(
        head.children(),
        validated.children(),
        &mut path,
        &mut violations,
    );
    if violations.is_empty() {
        Ok(validated)
    } else {
        Err(additive_error(violations))
    }
}

fn compare_children(
    head: &[MetadataNode],
    candidate: &[MetadataNode],
    path: &mut Vec<String>,
    violations: &mut Vec<ValidationExceptionField>,
) {
    for head_node in head {
        let Some(candidate_node) = find_by_name(candidate, head_node.name()) else {
            violations.push(ValidationExceptionField {
                name: display_path(path, head_node.name()),
                message: "removes a previously saved field".to_string(),
            });
            continue;
        };
        match (head_node, candidate_node) {
            (
                MetadataNode::Field {
                    generator: head_generator,
                    ..
                },
                MetadataNode::Field {
                    generator: candidate_generator,
                    ..
                },
            ) => {
                if head_generator != candidate_generator {
                    violations.push(ValidationExceptionField {
                        name: display_path(path, head_node.name()),
                        message: format!(
                            "changes the generator of a previously saved field from {} to {}; \
                             existing fields cannot be redefined",
                            head_generator.as_str(),
                            candidate_generator.as_str()
                        ),
                    });
                }
            }
            (MetadataNode::Object { children, .. }, MetadataNode::Object { .. }) => {
                path.push(head_node.name().to_string());
                let candidate_children = match candidate_node {
                    MetadataNode::Object { children, .. } => children,
                    _ => unreachable!("kinds matched in the match arm above"),
                };
                compare_children(children, candidate_children, path, violations);
                path.pop();
            }
            (MetadataNode::Field { .. }, MetadataNode::Object { .. }) => {
                violations.push(kind_violation(path, head_node, "nested object"));
            }
            (MetadataNode::Object { .. }, MetadataNode::Field { .. }) => {
                violations.push(kind_violation(path, head_node, "generated field"));
            }
        }
    }
}

fn find_by_name<'a>(nodes: &'a [MetadataNode], name: &str) -> Option<&'a MetadataNode> {
    nodes.iter().find(|node| node.name() == name)
}

fn kind_violation(
    path: &[String],
    node: &MetadataNode,
    replacement: &str,
) -> ValidationExceptionField {
    ValidationExceptionField {
        name: display_path(path, node.name()),
        message: format!(
            "changes a previously saved {} into a {replacement}; existing nodes cannot be redefined",
            match node {
                MetadataNode::Field { .. } => "generated field",
                MetadataNode::Object { .. } => "nested object",
            }
        ),
    }
}

fn display_path(path: &[String], name: &str) -> String {
    if path.is_empty() {
        name.to_string()
    } else {
        format!("{}.{}", path.join("."), name)
    }
}

fn additive_error(violations: Vec<ValidationExceptionField>) -> ControlPlaneError {
    let summary = violations
        .iter()
        .map(|v| format!("{}: {}", v.name, v.message))
        .collect::<Vec<_>>()
        .join("; ");
    ControlPlaneError::ValidationException {
        message: format!("metadata revision is not additive: {summary}"),
        reason: ValidationExceptionReason::FieldValidationFailed,
        field_list: Some(violations),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::definition::Generator;

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

    fn field_list_of(err: ControlPlaneError) -> Vec<ValidationExceptionField> {
        match err {
            ControlPlaneError::ValidationException { field_list, .. } => {
                field_list.unwrap_or_default()
            }
            other => panic!("expected ValidationException, got {other}"),
        }
    }

    fn head() -> ValidatedMetadataDefinition {
        ValidatedMetadataDefinition::validate(MetadataDefinition {
            children: vec![
                field("eventId", Generator::UuidV4),
                object("correlation", vec![field("traceId", Generator::UuidV4)]),
            ],
        })
        .unwrap()
    }

    #[test]
    fn adding_a_new_field_passes() {
        let candidate = MetadataDefinition {
            children: vec![
                field("eventId", Generator::UuidV4),
                object("correlation", vec![field("traceId", Generator::UuidV4)]),
                field("emittedAt", Generator::UtcTimestamp),
            ],
        };
        assert!(check_additive(candidate, &head()).is_ok());
    }

    #[test]
    fn adding_children_to_an_existing_object_passes() {
        let candidate = MetadataDefinition {
            children: vec![
                field("eventId", Generator::UuidV4),
                object(
                    "correlation",
                    vec![
                        field("traceId", Generator::UuidV4),
                        field("traceDate", Generator::UtcDate),
                    ],
                ),
            ],
        };
        assert!(check_additive(candidate, &head()).is_ok());
    }

    #[test]
    fn adding_a_whole_new_object_tree_passes() {
        let candidate = MetadataDefinition {
            children: vec![
                field("eventId", Generator::UuidV4),
                object("correlation", vec![field("traceId", Generator::UuidV4)]),
                object(
                    "deployment",
                    vec![
                        field("region", Generator::AwsRegion),
                        object("context", vec![field("stage", Generator::Environment)]),
                    ],
                ),
            ],
        };
        assert!(check_additive(candidate, &head()).is_ok());
    }

    #[test]
    fn child_ordering_is_not_a_change() {
        let candidate = MetadataDefinition {
            children: vec![
                object("correlation", vec![field("traceId", Generator::UuidV4)]),
                field("eventId", Generator::UuidV4),
            ],
        };
        assert!(check_additive(candidate, &head()).is_ok());
    }

    #[test]
    fn removing_a_field_rejects_with_its_path() {
        let candidate = MetadataDefinition {
            children: vec![object(
                "correlation",
                vec![field("traceId", Generator::UuidV4)],
            )],
        };
        let err = check_additive(candidate, &head()).unwrap_err();
        let violations = field_list_of(err);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].name, "eventId");
        assert!(violations[0].message.contains("removes"));
    }

    #[test]
    fn removing_a_nested_field_rejects_with_a_bounded_dotted_path() {
        let candidate = MetadataDefinition {
            children: vec![
                field("eventId", Generator::UuidV4),
                object("correlation", vec![]),
            ],
        };
        let err = check_additive(candidate, &head()).unwrap_err();
        let violations = field_list_of(err);
        assert_eq!(violations[0].name, "correlation.traceId");
    }

    #[test]
    fn changing_a_generator_rejects_with_the_old_and_new_identity() {
        let candidate = MetadataDefinition {
            children: vec![
                field("eventId", Generator::UtcTimestamp),
                object("correlation", vec![field("traceId", Generator::UuidV4)]),
            ],
        };
        let err = check_additive(candidate, &head()).unwrap_err();
        let violations = field_list_of(err);
        assert_eq!(violations[0].name, "eventId");
        assert!(violations[0].message.contains("uuid-v4"));
        assert!(violations[0].message.contains("utc-timestamp"));
    }

    #[test]
    fn field_to_object_kind_change_is_rejected() {
        let candidate = MetadataDefinition {
            children: vec![
                object("eventId", vec![field("inner", Generator::UtcDate)]),
                object("correlation", vec![field("traceId", Generator::UuidV4)]),
            ],
        };
        let err = check_additive(candidate, &head()).unwrap_err();
        assert!(field_list_of(err)[0]
            .message
            .contains("cannot be redefined"));
    }

    #[test]
    fn object_to_field_kind_change_is_rejected() {
        let candidate = MetadataDefinition {
            children: vec![
                field("eventId", Generator::UuidV4),
                field("correlation", Generator::AwsRegion),
            ],
        };
        let err = check_additive(candidate, &head()).unwrap_err();
        assert!(field_list_of(err)[0]
            .message
            .contains("cannot be redefined"));
    }

    #[test]
    fn emptying_the_definition_reports_every_removal() {
        let err = check_additive(MetadataDefinition::default(), &head()).unwrap_err();
        assert_eq!(field_list_of(err).len(), 2);
    }

    #[test]
    fn every_removal_in_a_deep_tree_is_reported_with_its_full_path() {
        let deep_head = ValidatedMetadataDefinition::validate(MetadataDefinition {
            children: vec![object(
                "level1",
                vec![
                    object("level2", vec![field("leafA", Generator::UtcDate)]),
                    object("level3", vec![field("leafB", Generator::UtcDate)]),
                ],
            )],
        })
        .unwrap();
        let candidate = MetadataDefinition {
            children: vec![object(
                "level1",
                vec![
                    object("level2", vec![]),
                    object("level3", vec![object("newChild", vec![])]),
                ],
            )],
        };
        let err = check_additive(candidate, &deep_head).unwrap_err();
        let violations = field_list_of(err);
        let paths: Vec<&str> = violations.iter().map(|v| v.name.as_str()).collect();
        assert!(paths.contains(&"level1.level2.leafA"));
        assert!(paths.contains(&"level1.level3.leafB"));
    }

    #[test]
    fn any_valid_candidate_is_additive_against_the_empty_baseline() {
        let baseline =
            ValidatedMetadataDefinition::validate(MetadataDefinition::default()).unwrap();
        let candidate = MetadataDefinition {
            children: vec![field("eventId", Generator::UuidV4)],
        };
        assert!(check_additive(candidate, &baseline).is_ok());
    }

    #[test]
    fn reserved_field_shadowing_is_rejected_before_comparison() {
        // Shadowing a reserved built-in cannot reach the additive comparison:
        // candidate structural validation rejects it first with its path.
        let candidate = MetadataDefinition {
            children: vec![
                field("eventId", Generator::UuidV4),
                object("correlation", vec![field("traceId", Generator::UuidV4)]),
                field("publishedAt", Generator::UtcTimestamp),
            ],
        };
        let err = check_additive(candidate, &head()).unwrap_err();
        let violations = field_list_of(err);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].name, "publishedAt");
        assert!(violations[0].message.contains("reserved"));
    }

    #[test]
    fn structurally_invalid_candidates_are_rejected_before_comparison() {
        let candidate = MetadataDefinition {
            children: vec![
                field("eventId", Generator::UuidV4),
                field("bad name", Generator::AwsRegion),
            ],
        };
        let err = check_additive(candidate, &head()).unwrap_err();
        assert!(field_list_of(err)[0].name == "bad name");
    }
}
