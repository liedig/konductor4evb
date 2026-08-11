use jsonwebtoken::{decode, decode_header, Algorithm, Validation};
use lambda_runtime::Error;
use serde_json::Value;

use nexus_common::principal::TrustedPrincipal;

use crate::claims::AccessTokenClaims;
use crate::error::AuthorizerError;
use crate::jwks::KeyResolver;
use crate::policy::{build_output, http_method_from_arn, route_allowed, AuthorizerOutput, Effect};

/// Deployment configuration for the authorizer, sourced from environment.
pub struct AuthorizerConfig {
    pub issuer: String,
    pub client_id: String,
    pub jwks_uri: String,
}

impl AuthorizerConfig {
    pub fn from_env() -> Result<Self, Error> {
        let issuer = std::env::var("COGNITO_ISSUER").map_err(|_| "COGNITO_ISSUER is not set")?;
        let client_id =
            std::env::var("COGNITO_CLIENT_ID").map_err(|_| "COGNITO_CLIENT_ID is not set")?;
        let jwks_uri =
            std::env::var("COGNITO_JWKS_URI").map_err(|_| "COGNITO_JWKS_URI is not set")?;
        Ok(Self {
            issuer,
            client_id,
            jwks_uri,
        })
    }
}

#[derive(Debug)]
enum AuthFlow {
    Allow(AuthorizerOutput),
    Forbidden(AuthorizerOutput),
}

/// REQUEST authorizer entry point.
///
/// Valid tokens yield an execute-api policy (Allow or route-gated Deny). Any
/// token-validation failure maps to `Unauthorized` (401) and the reason is
/// logged without token material.
pub async fn handle<R: KeyResolver>(
    resolver: &R,
    config: &AuthorizerConfig,
    event: Value,
) -> Result<Value, Error> {
    match authorize(resolver, config, &event).await {
        Ok(AuthFlow::Allow(out)) | Ok(AuthFlow::Forbidden(out)) => {
            serde_json::to_value(out).map_err(Error::from)
        }
        Err(e) => {
            tracing::warn!(reason = %e, "oauth authorizer denied request");
            Err(Error::from("Unauthorized"))
        }
    }
}

async fn authorize<R: KeyResolver>(
    resolver: &R,
    config: &AuthorizerConfig,
    event: &Value,
) -> Result<AuthFlow, AuthorizerError> {
    let method_arn = event
        .get("methodArn")
        .and_then(Value::as_str)
        .ok_or_else(|| AuthorizerError::InvalidToken("missing methodArn".to_string()))?;
    let method = http_method_from_arn(method_arn).unwrap_or("").to_string();
    let token = extract_bearer(event)?;

    let header = decode_header(token).map_err(|e| AuthorizerError::InvalidToken(e.to_string()))?;
    let kid = header.kid.ok_or(AuthorizerError::MissingKeyId)?;
    let key = resolver.decoding_key(&kid).await?;

    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_issuer(&[config.issuer.as_str()]);
    // Cognito access tokens carry the client in `client_id`, not `aud`.
    validation.validate_aud = false;
    let claims = decode::<AccessTokenClaims>(token, &key, &validation)
        .map_err(|e| AuthorizerError::InvalidToken(e.to_string()))?
        .claims;
    claims.assert_for_client(&config.client_id)?;

    let (roles, team_groups) = claims.roles_and_team_groups();
    let effect = if route_allowed(&method, &roles, &team_groups) {
        Effect::Allow
    } else {
        Effect::Deny
    };
    let principal =
        TrustedPrincipal::oauth(claims.sub, claims.iss, claims.client_id, roles, team_groups);

    let output = build_output(&principal, method_arn, effect);
    Ok(match effect {
        Effect::Allow => AuthFlow::Allow(output),
        Effect::Deny => AuthFlow::Forbidden(output),
    })
}

fn extract_bearer(event: &Value) -> Result<&str, AuthorizerError> {
    let headers = event
        .get("headers")
        .and_then(Value::as_object)
        .ok_or(AuthorizerError::MissingToken)?;
    let value = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
        .and_then(|(_, v)| v.as_str())
        .ok_or(AuthorizerError::MissingToken)?;
    value
        .strip_prefix("Bearer ")
        .ok_or(AuthorizerError::MissingToken)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jwks::CachingResolver;
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use jsonwebtoken::{encode, DecodingKey, EncodingKey, Header};
    use rsa::pkcs1::EncodeRsaPrivateKey;
    use rsa::traits::PublicKeyParts;
    use rsa::{RsaPrivateKey, RsaPublicKey};
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    const ISSUER: &str = "https://cognito-idp.ap-southeast-2.amazonaws.com/ap-southeast-2_test";
    const CLIENT_ID: &str = "testclientid123";
    const GET_ARN: &str = "arn:aws:execute-api:ap-southeast-2:123456789012:api/v1/GET/domains";
    const POST_ARN: &str =
        "arn:aws:execute-api:ap-southeast-2:123456789012:api/v1/POST/domains/registrations/decide";

    struct TestKey {
        kid: String,
        private: RsaPrivateKey,
        decoding: DecodingKey,
    }

    fn generate_key(kid: &str) -> TestKey {
        let mut rng = rand::thread_rng();
        let private = RsaPrivateKey::new(&mut rng, 2048).unwrap();
        let public = RsaPublicKey::from(&private);
        let n = URL_SAFE_NO_PAD.encode(public.n().to_bytes_be());
        let e = URL_SAFE_NO_PAD.encode(public.e().to_bytes_be());
        let decoding = DecodingKey::from_rsa_components(&n, &e).unwrap();
        TestKey {
            kid: kid.to_string(),
            private,
            decoding,
        }
    }

    fn sign(key: &TestKey, claims: &Value) -> String {
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(key.kid.clone());
        let der = key.private.to_pkcs1_der().unwrap();
        let encoding = EncodingKey::from_rsa_der(der.as_bytes());
        encode(&header, claims, &encoding).unwrap()
    }

    fn future_exp(secs: u64) -> u64 {
        (SystemTime::now() + Duration::from_secs(secs))
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    fn valid_claims(groups: &[&str]) -> Value {
        json!({
            "sub": "user-abc-123",
            "iss": ISSUER,
            "exp": future_exp(3600),
            "iat": future_exp(0),
            "token_use": "access",
            "client_id": CLIENT_ID,
            "cognito:groups": groups,
            "scope": "openid profile email",
            "username": "user-abc-123"
        })
    }

    fn config() -> AuthorizerConfig {
        AuthorizerConfig {
            issuer: ISSUER.to_string(),
            client_id: CLIENT_ID.to_string(),
            jwks_uri: "https://example.invalid/jwks.json".to_string(),
        }
    }

    fn event(method_arn: &str, token: &str) -> Value {
        json!({
            "type": "REQUEST",
            "methodArn": method_arn,
            "headers": { "Authorization": format!("Bearer {token}") }
        })
    }

    struct StaticResolver {
        keys: Mutex<HashMap<String, DecodingKey>>,
    }

    impl StaticResolver {
        fn with_keys(keys: &[&TestKey]) -> Self {
            let map = keys
                .iter()
                .map(|k| (k.kid.clone(), k.decoding.clone()))
                .collect();
            Self {
                keys: Mutex::new(map),
            }
        }
    }

    impl KeyResolver for StaticResolver {
        async fn decoding_key(&self, kid: &str) -> Result<DecodingKey, AuthorizerError> {
            self.keys
                .lock()
                .unwrap()
                .get(kid)
                .cloned()
                .ok_or_else(|| AuthorizerError::Jwks("unknown kid".to_string()))
        }
    }

    #[tokio::test]
    async fn valid_admin_token_allowed_with_compact_context() {
        let key = generate_key("kid-1");
        let token = sign(&key, &valid_claims(&["platform-admin", "team-payments"]));
        let resolver = StaticResolver::with_keys(&[&key]);
        let out = handle(&resolver, &config(), event(GET_ARN, &token))
            .await
            .unwrap();
        assert_eq!(out["policyDocument"]["Statement"][0]["Effect"], "Allow");
        assert_eq!(out["principalId"], "user-abc-123");
        assert_eq!(out["context"]["authenticationMechanism"], "oauth");
        assert_eq!(out["context"]["globalRoles"], "[\"platform-admin\"]");
        assert_eq!(out["context"]["teamGroups"], "[\"team-payments\"]");
        assert_eq!(out["context"]["issuer"], ISSUER);
    }

    #[tokio::test]
    async fn auditor_allowed_on_read_but_forbidden_on_mutation() {
        let key = generate_key("kid-1");
        let token = sign(&key, &valid_claims(&["platform-auditor"]));
        let resolver = StaticResolver::with_keys(&[&key]);

        let read = handle(&resolver, &config(), event(GET_ARN, &token))
            .await
            .unwrap();
        assert_eq!(read["policyDocument"]["Statement"][0]["Effect"], "Allow");

        let write = handle(&resolver, &config(), event(POST_ARN, &token))
            .await
            .unwrap();
        assert_eq!(write["policyDocument"]["Statement"][0]["Effect"], "Deny");
    }

    #[tokio::test]
    async fn team_caller_is_allowed_through_to_resource_authorization() {
        let key = generate_key("kid-1");
        let token = sign(&key, &valid_claims(&["team-payments"]));
        let resolver = StaticResolver::with_keys(&[&key]);

        let read = handle(&resolver, &config(), event(GET_ARN, &token))
            .await
            .unwrap();
        assert_eq!(read["policyDocument"]["Statement"][0]["Effect"], "Allow");
        assert_eq!(read["context"]["teamGroups"], "[\"team-payments\"]");

        let write = handle(&resolver, &config(), event(POST_ARN, &token))
            .await
            .unwrap();
        assert_eq!(write["policyDocument"]["Statement"][0]["Effect"], "Allow");
    }

    #[tokio::test]
    async fn principal_without_any_group_is_forbidden() {
        let key = generate_key("kid-1");
        let token = sign(&key, &valid_claims(&[]));
        let resolver = StaticResolver::with_keys(&[&key]);
        let out = handle(&resolver, &config(), event(GET_ARN, &token))
            .await
            .unwrap();
        assert_eq!(out["policyDocument"]["Statement"][0]["Effect"], "Deny");
    }

    #[tokio::test]
    async fn expired_token_is_unauthorized() {
        let key = generate_key("kid-1");
        let mut claims = valid_claims(&["platform-admin"]);
        claims["exp"] = json!(1);
        let token = sign(&key, &claims);
        let resolver = StaticResolver::with_keys(&[&key]);
        assert!(handle(&resolver, &config(), event(GET_ARN, &token))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn wrong_issuer_is_unauthorized() {
        let key = generate_key("kid-1");
        let mut claims = valid_claims(&["platform-admin"]);
        claims["iss"] = json!("https://evil.example.com");
        let token = sign(&key, &claims);
        let resolver = StaticResolver::with_keys(&[&key]);
        assert!(handle(&resolver, &config(), event(GET_ARN, &token))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn id_token_use_is_unauthorized() {
        let key = generate_key("kid-1");
        let mut claims = valid_claims(&["platform-admin"]);
        claims["token_use"] = json!("id");
        let token = sign(&key, &claims);
        let resolver = StaticResolver::with_keys(&[&key]);
        assert!(handle(&resolver, &config(), event(GET_ARN, &token))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn wrong_client_is_unauthorized() {
        let key = generate_key("kid-1");
        let mut claims = valid_claims(&["platform-admin"]);
        claims["client_id"] = json!("some-other-client");
        let token = sign(&key, &claims);
        let resolver = StaticResolver::with_keys(&[&key]);
        assert!(handle(&resolver, &config(), event(GET_ARN, &token))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn bad_signature_is_unauthorized() {
        let signing_key = generate_key("kid-1");
        let other_key = generate_key("kid-1");
        let token = sign(&signing_key, &valid_claims(&["platform-admin"]));
        let resolver = StaticResolver::with_keys(&[&other_key]);
        assert!(handle(&resolver, &config(), event(GET_ARN, &token))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn unknown_key_id_is_unauthorized() {
        let key = generate_key("kid-unknown");
        let token = sign(&key, &valid_claims(&["platform-admin"]));
        let resolver = StaticResolver::with_keys(&[]);
        assert!(handle(&resolver, &config(), event(GET_ARN, &token))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn jwks_rotation_accepts_new_key_and_rejects_retired_key() {
        let old_key = generate_key("kid-old");
        let new_key = generate_key("kid-new");
        let current = Arc::new(Mutex::new(
            [("kid-old".to_string(), old_key.decoding.clone())]
                .into_iter()
                .collect::<HashMap<_, _>>(),
        ));
        let resolver = {
            let current = current.clone();
            CachingResolver::new(move || {
                let current = current.clone();
                async move { Ok(current.lock().unwrap().clone()) }
            })
        };

        let old_token = sign(&old_key, &valid_claims(&["platform-admin"]));
        assert!(handle(&resolver, &config(), event(GET_ARN, &old_token))
            .await
            .is_ok());

        current.lock().unwrap().clear();
        current
            .lock()
            .unwrap()
            .insert("kid-new".to_string(), new_key.decoding.clone());

        let new_token = sign(&new_key, &valid_claims(&["platform-admin"]));
        assert!(handle(&resolver, &config(), event(GET_ARN, &new_token))
            .await
            .is_ok());

        // The retired key is dropped from the refreshed set, so its token now fails.
        assert!(handle(&resolver, &config(), event(GET_ARN, &old_token))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn jwks_cache_refreshes_only_on_unknown_key_id() {
        let key = generate_key("kid-1");
        let fetches = Arc::new(AtomicUsize::new(0));
        let resolver = {
            let fetches = fetches.clone();
            let decoding = key.decoding.clone();
            CachingResolver::new(move || {
                let fetches = fetches.clone();
                let decoding = decoding.clone();
                async move {
                    fetches.fetch_add(1, Ordering::SeqCst);
                    Ok([("kid-1".to_string(), decoding)].into_iter().collect())
                }
            })
        };

        resolver.decoding_key("kid-1").await.unwrap();
        resolver.decoding_key("kid-1").await.unwrap();
        assert_eq!(
            fetches.load(Ordering::SeqCst),
            1,
            "known kid must use cache"
        );

        let _ = resolver.decoding_key("kid-2").await;
        assert_eq!(
            fetches.load(Ordering::SeqCst),
            2,
            "unknown kid triggers one refresh"
        );
    }

    #[tokio::test]
    async fn error_path_does_not_leak_token_material() {
        let key = generate_key("kid-1");
        let token = sign(&key, &valid_claims(&["platform-admin"]));
        let resolver = StaticResolver::with_keys(&[]);
        let err = authorize(&resolver, &config(), &event(GET_ARN, &token))
            .await
            .unwrap_err();
        assert!(!err.to_string().contains(&token));
        assert!(!err.to_string().contains("platform-admin"));
    }
}
