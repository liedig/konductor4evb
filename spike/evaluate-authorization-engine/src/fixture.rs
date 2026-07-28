use crate::contract::{AuthorizationSubject, PrincipalType};
use crate::manifest::{GrantSeed, Manifest};

pub fn cognito_issuer(manifest: &Manifest) -> &str {
    &manifest.topology.cognito_issuer
}

pub fn cognito_group_subject(manifest: &Manifest, group: &str) -> AuthorizationSubject {
    AuthorizationSubject::cognito_group(cognito_issuer(manifest), group)
}

pub fn aws_account_subject(account_id: &str) -> AuthorizationSubject {
    AuthorizationSubject::aws_account(account_id)
}

pub fn selected_grants<'a>(manifest: &'a Manifest, grant_ids: &[String]) -> Vec<&'a GrantSeed> {
    manifest
        .baseline_grant_seeds
        .iter()
        .filter(|g| grant_ids.iter().any(|id| id == &g.grant_id))
        .collect()
}

pub fn consumer_via_group(subject: &AuthorizationSubject) -> Option<(&str, &str)> {
    match (&subject.kind, &subject.consumer_domain, &subject.via) {
        (PrincipalType::ConsumerDomain, Some(domain), Some(via)) => {
            via.group.as_deref().map(|group| (domain.as_str(), group))
        }
        _ => None,
    }
}
