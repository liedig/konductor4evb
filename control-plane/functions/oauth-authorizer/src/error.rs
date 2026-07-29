use thiserror::Error;

/// Internal control-flow errors for the authorizer. Every variant maps to an
/// `Unauthorized` (401) response; the detail is logged (never including token
/// material) and never returned to the caller.
#[derive(Debug, Error)]
pub enum AuthorizerError {
    #[error("missing or malformed Authorization header")]
    MissingToken,

    #[error("token header has no key id")]
    MissingKeyId,

    #[error("token validation failed: {0}")]
    InvalidToken(String),

    #[error("token_use must be 'access'")]
    WrongTokenUse,

    #[error("token was issued for an unexpected client")]
    WrongClient,

    #[error("JWKS retrieval failed: {0}")]
    Jwks(String),
}
