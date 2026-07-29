//! Template tests for the Cognito interactive-identity foundation (Section 2).

mod support;

use support::{at, boolean, core_template, int, mapping, props, resource, seq, text};

#[test]
fn cognito_identity_resources_exist() {
    let doc = core_template();
    for (logical_id, expected_type) in [
        ("NexusUserPool", "AWS::Cognito::UserPool"),
        ("NexusUserPoolDomain", "AWS::Cognito::UserPoolDomain"),
        ("NexusUserPoolClient", "AWS::Cognito::UserPoolClient"),
        ("NexusPlatformAdminGroup", "AWS::Cognito::UserPoolGroup"),
        ("NexusPlatformAuditorGroup", "AWS::Cognito::UserPoolGroup"),
    ] {
        assert_eq!(
            text(at(&resource(&doc, logical_id), "Type")),
            expected_type,
            "{logical_id} should be a {expected_type}"
        );
    }
}

#[test]
fn controlled_groups_use_exact_global_role_names() {
    let doc = core_template();
    let admin = resource(&doc, "NexusPlatformAdminGroup");
    let auditor = resource(&doc, "NexusPlatformAuditorGroup");
    assert_eq!(text(at(props(admin), "GroupName")), "platform-admin");
    assert_eq!(text(at(props(auditor), "GroupName")), "platform-auditor");
}

#[test]
fn app_client_is_confidential_free_public_client() {
    let doc = core_template();
    let client = resource(&doc, "NexusUserPoolClient");
    let p = props(client);
    assert!(
        !boolean(at(p, "GenerateSecret")),
        "browser client must not embed a client secret"
    );
    assert_eq!(text(at(p, "PreventUserExistenceErrors")), "ENABLED");
    assert!(boolean(at(p, "AllowedOAuthFlowsUserPoolClient")));
}

#[test]
fn app_client_uses_authorization_code_flow_and_standard_scopes() {
    let doc = core_template();
    let client = resource(&doc, "NexusUserPoolClient");
    let p = props(client);
    let flows: Vec<&str> = seq(at(p, "AllowedOAuthFlows")).iter().map(text).collect();
    assert_eq!(
        flows,
        vec!["code"],
        "only authorization-code flow with PKCE"
    );
    let scopes: Vec<&str> = seq(at(p, "AllowedOAuthScopes")).iter().map(text).collect();
    for scope in ["openid", "email", "profile"] {
        assert!(scopes.contains(&scope), "missing scope {scope}");
    }
}

#[test]
fn access_token_lifetime_is_bounded_and_configurable() {
    let doc = core_template();
    let client = resource(&doc, "NexusUserPoolClient");
    let p = props(client);
    assert_eq!(
        text(at(p, "AccessTokenValidity")),
        "AccessTokenValidityMinutes",
        "access-token validity must come from the bounded parameter"
    );
    let units = at(p, "TokenValidityUnits");
    assert_eq!(text(at(units, "AccessToken")), "minutes");

    let params = at(&doc, "Parameters");
    let lifetime = at(params, "AccessTokenValidityMinutes");
    assert_eq!(int(at(lifetime, "MinValue")), 5);
    assert_eq!(int(at(lifetime, "MaxValue")), 1440);
    assert_eq!(int(at(lifetime, "Default")), 60);
}

#[test]
fn client_is_bound_to_the_user_pool_and_urls_are_parameterized() {
    let doc = core_template();
    let client = resource(&doc, "NexusUserPoolClient");
    let p = props(client);
    assert_eq!(text(at(p, "UserPoolId")), "NexusUserPool");
    assert_eq!(text(at(p, "CallbackURLs")), "OAuthCallbackUrls");
    assert_eq!(text(at(p, "LogoutURLs")), "OAuthLogoutUrls");
}

#[test]
fn issuer_and_client_configuration_are_exported() {
    let doc = core_template();
    let outputs = at(&doc, "Outputs");
    for key in [
        "CognitoUserPoolId",
        "CognitoUserPoolIssuer",
        "CognitoUserPoolClientId",
        "CognitoJwksUri",
        "CognitoOAuthDomain",
    ] {
        assert!(mapping(outputs).get(key).is_some(), "missing output {key}");
    }
}

#[test]
fn oauth_authorizer_function_is_least_privilege_and_configured() {
    let doc = core_template();
    let func = resource(&doc, "OAuthAuthorizerFunction");
    let p = props(func);
    assert_eq!(
        text(at(p, "CodeUri")),
        "./control-plane/functions/oauth-authorizer"
    );
    let vars = at(&at(p, "Environment"), "Variables");
    for key in ["COGNITO_ISSUER", "COGNITO_CLIENT_ID", "COGNITO_JWKS_URI"] {
        assert!(mapping(vars).get(key).is_some(), "missing env var {key}");
    }
    assert!(
        !mapping(p).contains_key("Policies"),
        "authorizer must have no IAM policies (it only fetches JWKS over HTTPS)"
    );
}
