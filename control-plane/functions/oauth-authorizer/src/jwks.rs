use std::collections::HashMap;
use std::sync::Arc;

use jsonwebtoken::jwk::{AlgorithmParameters, JwkSet};
use jsonwebtoken::DecodingKey;
use tokio::sync::RwLock;

use crate::error::AuthorizerError;

/// Resolves the public key for a JWT `kid`. Abstracted so the validation flow
/// can be tested with in-memory keys (including rotation) without HTTP.
pub trait KeyResolver {
    fn decoding_key(
        &self,
        kid: &str,
    ) -> impl std::future::Future<Output = Result<DecodingKey, AuthorizerError>> + Send;
}

/// Builds a `kid` -> key map from a JWKS document, ignoring non-RSA keys.
/// Pure and independently testable.
pub fn keys_from_jwks(jwks: &JwkSet) -> Result<HashMap<String, DecodingKey>, AuthorizerError> {
    let mut keys = HashMap::new();
    for jwk in &jwks.keys {
        let kid = jwk
            .common
            .key_id
            .clone()
            .ok_or_else(|| AuthorizerError::Jwks("JWKS key is missing a key id".to_string()))?;
        if let AlgorithmParameters::RSA(rsa) = &jwk.algorithm {
            let key = DecodingKey::from_rsa_components(&rsa.n, &rsa.e)
                .map_err(|e| AuthorizerError::Jwks(format!("invalid RSA key material: {e}")))?;
            keys.insert(kid, key);
        }
    }
    Ok(keys)
}

/// Fetches and decodes the user pool's JWKS document over HTTPS.
pub async fn fetch_jwks(
    http: &reqwest::Client,
    jwks_uri: &str,
) -> Result<HashMap<String, DecodingKey>, AuthorizerError> {
    let jwks = http
        .get(jwks_uri)
        .send()
        .await
        .map_err(|e| AuthorizerError::Jwks(format!("request failed: {e}")))?
        .json::<JwkSet>()
        .await
        .map_err(|e| AuthorizerError::Jwks(format!("invalid JWKS document: {e}")))?;
    keys_from_jwks(&jwks)
}

/// A [`KeyResolver`] that caches the decoded JWKS key set and refreshes only
/// when a token references an unknown `kid`, so Cognito key rotation does not
/// require redeployment.
///
/// The cache is bounded by the (small) JWKS document size, and any retrieval or
/// parse failure is a fail-closed error. The fetch source is injected, which
/// keeps the caching/refresh behavior testable without HTTP.
pub struct CachingResolver<F> {
    fetch: F,
    cache: Arc<RwLock<HashMap<String, DecodingKey>>>,
}

impl<F, Fut> CachingResolver<F>
where
    F: Fn() -> Fut + Send + Sync,
    Fut: std::future::Future<Output = Result<HashMap<String, DecodingKey>, AuthorizerError>> + Send,
{
    pub fn new(fetch: F) -> Self {
        Self {
            fetch,
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl<F, Fut> KeyResolver for CachingResolver<F>
where
    F: Fn() -> Fut + Send + Sync,
    Fut: std::future::Future<Output = Result<HashMap<String, DecodingKey>, AuthorizerError>> + Send,
{
    async fn decoding_key(&self, kid: &str) -> Result<DecodingKey, AuthorizerError> {
        {
            let cache = self.cache.read().await;
            if let Some(key) = cache.get(kid) {
                return Ok(key.clone());
            }
        }

        tracing::debug!(kid, "JWKS cache miss, refreshing key set");
        let keys = (self.fetch)().await?;
        let resolved = keys.get(kid).cloned();
        let mut cache = self.cache.write().await;
        *cache = keys;
        resolved.ok_or_else(|| AuthorizerError::Jwks("unknown key id after refresh".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jwks_json(kids: &[&str]) -> serde_json::Value {
        let keys: Vec<serde_json::Value> = kids
            .iter()
            .map(|kid| {
                serde_json::json!({
                    "kty": "RSA",
                    "kid": kid,
                    "use": "sig",
                    "alg": "RS256",
                    "n": "0vx7agoebGcQSuuPiLJXZptN9nndrQmbXEps2aiAFbWhM78LhWx4cbbfAAtVT86zwu1RK7aPFFxuhDR1L6tSoc_BJECPebWKRXjBZCiFV4n3oknjhMstn64tZ_2W-5JsGY4Hc5n9yBXArwl93lqt7_RN5w6Cf0h4QyQ5v-65YGjQR0_FDW2QvzqY368QQMicAtaSqzs8KJZgnYb9c7d0zgdAZHzu6qMQvRL5hajrn1n91CbOpbISD08qNLyrdkt-bFTWhAI4vMQFh6WeZu0fM4lFd2NcRwr3XPksINHaQ-G_xBniIqbw0Ls1jF44-csFCur-kEgU8awapJzKnqDKgw",
                    "e": "AQAB"
                })
            })
            .collect();
        serde_json::json!({ "keys": keys })
    }

    #[test]
    fn builds_keys_indexed_by_kid() {
        let jwks: JwkSet = serde_json::from_value(jwks_json(&["key-1", "key-2"])).unwrap();
        let keys = keys_from_jwks(&jwks).unwrap();
        assert!(keys.contains_key("key-1"));
        assert!(keys.contains_key("key-2"));
    }

    #[test]
    fn fails_closed_when_key_id_missing() {
        let mut doc = jwks_json(&["key-1"]);
        doc["keys"][0].as_object_mut().unwrap().remove("kid");
        let jwks: JwkSet = serde_json::from_value(doc).unwrap();
        assert!(keys_from_jwks(&jwks).is_err());
    }
}
