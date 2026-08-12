//! The composite resource grammar for subscribe-family authorization:
//! `{consumerDomain}/{producerDomain}/{eventName}`, constructed by typed
//! helpers at call sites and parsed by the grant engine with fail-closed
//! semantics.

use std::fmt;

/// A failure to construct or parse a composite subscription resource.
#[derive(Debug, thiserror::Error)]
pub enum SubscriptionResourceError {
    #[error("subscription resource must be exactly three '/'-separated segments: {0:?}")]
    WrongSegmentCount(String),
    #[error("subscription resource segments must be non-empty: {0:?}")]
    EmptySegment(String),
    #[error("key segment must not contain '#': {0:?}")]
    UnsafeKeySegment(String),
}

/// A parsed `{consumerDomain}/{producerDomain}/{eventName}` composite resource.
///
/// Domain names cannot contain `/`, so exactly three segments always parse;
/// anything else is malformed and the engine denies. `#` is rejected so a
/// segment can never forge a store-key boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubscriptionResource {
    consumer: String,
    producer: String,
    event: String,
}

impl SubscriptionResource {
    /// Builds a composite resource from typed segments, rejecting empty or
    /// unkeyable segments at construction.
    pub fn new(
        consumer: impl Into<String>,
        producer: impl Into<String>,
        event: impl Into<String>,
    ) -> Result<Self, SubscriptionResourceError> {
        let consumer = consumer.into();
        let producer = producer.into();
        let event = event.into();
        for segment in [&consumer, &producer, &event] {
            if segment.is_empty() {
                return Err(SubscriptionResourceError::EmptySegment(format!(
                    "{consumer}/{producer}/{event}"
                )));
            }
            if segment.contains('#') {
                return Err(SubscriptionResourceError::UnsafeKeySegment(segment.clone()));
            }
        }
        Ok(Self {
            consumer,
            producer,
            event,
        })
    }

    /// Parses a composite resource string, rejecting anything that is not
    /// exactly three non-empty segments.
    pub fn parse(resource: &str) -> Result<Self, SubscriptionResourceError> {
        let segments: Vec<&str> = resource.split('/').collect();
        if segments.len() != 3 {
            return Err(SubscriptionResourceError::WrongSegmentCount(
                resource.to_string(),
            ));
        }
        Self::new(segments[0], segments[1], segments[2])
    }

    /// The consumer domain segment.
    pub fn consumer_domain(&self) -> &str {
        &self.consumer
    }

    /// The producer domain segment.
    pub fn producer_domain(&self) -> &str {
        &self.producer
    }

    /// The event (schema) name segment.
    pub fn event_name(&self) -> &str {
        &self.event
    }
}

impl fmt::Display for SubscriptionResource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}/{}", self.consumer, self.producer, self.event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructor_renders_three_segments() {
        let resource = SubscriptionResource::new("checkout", "orders", "OrderCreated").unwrap();
        assert_eq!(resource.to_string(), "checkout/orders/OrderCreated");
        assert_eq!(resource.consumer_domain(), "checkout");
        assert_eq!(resource.producer_domain(), "orders");
        assert_eq!(resource.event_name(), "OrderCreated");
    }

    #[test]
    fn parse_round_trips_through_display() {
        let resource = SubscriptionResource::new("checkout", "orders", "OrderCreated").unwrap();
        let parsed = SubscriptionResource::parse(&resource.to_string()).unwrap();
        assert_eq!(parsed, resource);
    }

    #[test]
    fn parse_accepts_charset_punctuation() {
        let parsed = SubscriptionResource::parse("checkout-2/orders.prod/Order_Created.v2");
        assert!(parsed.is_ok());
    }

    #[test]
    fn parse_rejects_wrong_segment_counts() {
        for input in [
            "",
            "checkout",
            "checkout/orders",
            "checkout/orders/OrderCreated/extra",
            "/",
            "//",
            "///",
        ] {
            assert!(
                matches!(
                    SubscriptionResource::parse(input),
                    Err(SubscriptionResourceError::WrongSegmentCount(_))
                        | Err(SubscriptionResourceError::EmptySegment(_))
                ),
                "must reject {input:?}"
            );
        }
    }

    #[test]
    fn parse_rejects_empty_segments() {
        for input in [
            "/orders/OrderCreated",
            "checkout//OrderCreated",
            "checkout/orders/",
        ] {
            assert!(
                matches!(
                    SubscriptionResource::parse(input),
                    Err(SubscriptionResourceError::EmptySegment(_))
                ),
                "must reject {input:?}"
            );
        }
    }

    #[test]
    fn segments_reject_hash_delimiter() {
        assert!(matches!(
            SubscriptionResource::new("check#out", "orders", "OrderCreated"),
            Err(SubscriptionResourceError::UnsafeKeySegment(_))
        ));
        assert!(matches!(
            SubscriptionResource::parse("checkout/orders/Order#Created"),
            Err(SubscriptionResourceError::UnsafeKeySegment(_))
        ));
    }

    #[test]
    fn constructor_rejects_empty_segments() {
        for (c, p, e) in [
            ("", "orders", "OrderCreated"),
            ("checkout", "", "OrderCreated"),
            ("checkout", "orders", ""),
        ] {
            assert!(
                matches!(
                    SubscriptionResource::new(c, p, e),
                    Err(SubscriptionResourceError::EmptySegment(_))
                ),
                "must reject empty segment in {c}/{p}/{e}"
            );
        }
    }
}
