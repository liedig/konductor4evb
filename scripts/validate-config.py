#!/usr/bin/env python3
"""Preflight validation for the customer platform configuration.

Reads konductor-platform.toml, enforces the configuration contract
(docs/reference/configuration.md), checks deploy prerequisites (SES identity
verification, OAuth URL scheme rules), and guards against drift of
fixed-at-setup settings on an already-deployed stack.

With --render, prints the sam deploy --parameter-overrides value for the same
validated configuration, so deploy and validation cannot read different
values. All diagnostics go to stderr; --render stdout carries only the
overrides line.

Exit code is 0 when the configuration is valid, 1 otherwise.
"""

import argparse
import json
import re
import subprocess
import sys
import tomllib
from pathlib import Path

ORGANIZATION_RE = re.compile(r"^[a-z0-9]([a-z0-9-]{0,41}[a-z0-9])$")
ENVIRONMENT_RE = re.compile(r"^[a-z0-9][a-z0-9-]{0,31}$")
NAMESPACE_CHARSET_RE = re.compile(r"^([a-z0-9-]|\{org\}|\{env\}|\{domain\})+$")
AWS_ORG_ARN_RE = re.compile(
    r"^arn:aws[a-z-]*:organizations::\d{12}:organization/o-[a-z0-9]{10,32}$"
)
EMAIL_RE = re.compile(r"^[^@\s]+@[^@\s]+\.[^@\s]+$")
HTTPS_OR_LOCALHOST_RE = re.compile(r"^(https://\S+|http://localhost(:\d+)?(/\S*)?)$")

MAX_ENVIRONMENTS = 10  # API Gateway stages-per-API default quota
MAX_NAMESPACE_LENGTH = 128

REQUIRED_PLACEHOLDERS = ("{org}", "{env}", "{domain}")

DEFAULTS = {
    "environments": ["dev", "staging", "prod"],
    "namespace_pattern": "{org}-{env}-{domain}",
    "notification_sender_email": "",
    "platform_operator_email": "",
    "access_token_validity_minutes": 60,
    "system_event_emission_enabled": False,
    "oauth_callback_urls": ["http://localhost:3000/callback"],
    "oauth_logout_urls": ["http://localhost:3000/logout"],
}

# snake_case config key -> (template parameter name, lifecycle class)
SETTINGS = {
    "organization": ("Organization", "fixed"),
    "aws_organization_arn": ("AwsOrganizationArn", "fixed"),
    "environments": ("Environments", "fixed"),
    "namespace_pattern": ("NamespacePattern", "fixed"),
    "notification_sender_email": ("NotificationSenderEmail", "updateable"),
    "platform_operator_email": ("PlatformOperatorEmail", "updateable"),
    "access_token_validity_minutes": ("AccessTokenValidityMinutes", "updateable"),
    "system_event_emission_enabled": ("SystemEventEmissionEnabled", "updateable"),
    "oauth_callback_urls": ("OAuthCallbackUrls", "updateable"),
    "oauth_logout_urls": ("OAuthLogoutUrls", "updateable"),
}


def err(msg):
    print(f"ERROR: {msg}", file=sys.stderr)


def note(msg):
    print(f"  {msg}", file=sys.stderr)


def load_config(path):
    try:
        raw = tomllib.loads(Path(path).read_text())
    except FileNotFoundError:
        err(f"{path} not found — copy konductor-platform.toml.example and fill in your values.")
        return None
    except tomllib.TOMLDecodeError as exc:
        err(f"{path} is not valid TOML: {exc}")
        return None

    errors = []
    for key in sorted(set(raw) - set(SETTINGS)):
        errors.append(f"{path}: unknown setting '{key}' — the contract defines: {', '.join(sorted(SETTINGS))}.")

    cfg = dict(DEFAULTS)
    cfg.update(raw)

    for key in ("organization", "aws_organization_arn"):
        if key not in raw:
            errors.append(f"{path}: '{key}' is required — it has no default.")

    for key, value in cfg.items():
        expected = list if key in ("environments", "oauth_callback_urls", "oauth_logout_urls") else (
            bool if key == "system_event_emission_enabled"
            else int if key == "access_token_validity_minutes" else str
        )
        if expected is bool:
            if not isinstance(value, bool):
                errors.append(f"{path}: '{key}' must be a bool (got {type(value).__name__} {value!r}).")
        elif not isinstance(value, expected) or (expected is int and isinstance(value, bool)):
            errors.append(f"{path}: '{key}' must be a {expected.__name__} (got {type(value).__name__} {value!r}).")

    for key in ("environments", "oauth_callback_urls", "oauth_logout_urls"):
        entries = cfg.get(key)
        if isinstance(entries, list) and any(not isinstance(e, str) for e in entries):
            errors.append(f"{path}: '{key}' entries must be strings.")

    if errors:
        for message in errors:
            err(message)
        return None
    return cfg


def validate_environments(envs):
    errors = []
    if isinstance(envs, list) and all(isinstance(e, str) for e in envs):
        if not 1 <= len(envs) <= MAX_ENVIRONMENTS:
            errors.append(
                f"environments has {len(envs)} entries — the contract allows 1-{MAX_ENVIRONMENTS} "
                "(API Gateway stages-per-API quota)."
            )
        for env in envs:
            if not ENVIRONMENT_RE.match(env):
                errors.append(
                    f"environments entry '{env}' does not match {ENVIRONMENT_RE.pattern} — 1-32 lowercase "
                    "letters, digits, or hyphens, starting with a letter or digit (the rule domain "
                    "registration enforces at runtime)."
                )
        seen, dupes = set(), set()
        for env in envs:
            (dupes if env in seen else seen).add(env)
        for dupe in sorted(dupes):
            errors.append(f"environments entry '{dupe}' is duplicated — entries must be unique.")
    return errors


def validate_contract(cfg):
    errors = []

    org = cfg["organization"]
    if isinstance(org, str) and not ORGANIZATION_RE.match(org):
        errors.append(
            f"organization '{org}' does not match {ORGANIZATION_RE.pattern} — 2-43 lowercase letters, "
            "digits, or hyphens, starting and ending with a letter or digit (bounded by the 64-character "
            "IAM role-name limit for <org>-konductor-trust-role)."
        )

    errors += validate_environments(cfg["environments"])

    pattern = cfg["namespace_pattern"]
    if isinstance(pattern, str):
        if len(pattern) > MAX_NAMESPACE_LENGTH:
            errors.append(
                f"namespace_pattern is {len(pattern)} characters — the contract allows at most "
                f"{MAX_NAMESPACE_LENGTH}."
            )
        if not NAMESPACE_CHARSET_RE.match(pattern):
            errors.append(
                "namespace_pattern may only contain the placeholders {org}, {env}, {domain} and "
                "lowercase letters, digits, or hyphens — substituted names must stay valid for the "
                "strictest services (SQS queue names)."
            )
        for placeholder in REQUIRED_PLACEHOLDERS:
            if placeholder not in pattern:
                errors.append(
                    f"namespace_pattern '{pattern}' is missing the {placeholder} placeholder — all of "
                    f"{', '.join(REQUIRED_PLACEHOLDERS)} are required so resource names stay unique "
                    "per organization, environment, and domain."
                )

    arn = cfg["aws_organization_arn"]
    if isinstance(arn, str) and not AWS_ORG_ARN_RE.match(arn):
        errors.append(
            f"aws_organization_arn '{arn}' is not a valid AWS Organizations ARN — expected "
            "arn:aws[a-z-]*:organizations::<12-digit-account>:organization/o-<id> "
            "(from the AWS Organizations console)."
        )
    return errors


def validate_prerequisites(cfg):
    errors = []

    sender = cfg["notification_sender_email"]
    operator = cfg["platform_operator_email"]
    if isinstance(sender, str) and isinstance(operator, str):
        for key, value in (("notification_sender_email", sender), ("platform_operator_email", operator)):
            if value and not EMAIL_RE.match(value):
                errors.append(f"{key} '{value}' is not a valid email address.")
        if bool(sender) != bool(operator):
            errors.append(
                "notification_sender_email and platform_operator_email must be set together — "
                "notifications are enabled by setting both or disabled by leaving both empty."
            )

    for key in ("oauth_callback_urls", "oauth_logout_urls"):
        urls = cfg[key]
        if isinstance(urls, list):
            for url in urls:
                if isinstance(url, str) and not HTTPS_OR_LOCALHOST_RE.match(url):
                    errors.append(
                        f"{key} entry '{url}' must use HTTPS — plain HTTP is allowed only for "
                        "http://localhost[:port] during local development."
                    )

    minutes = cfg["access_token_validity_minutes"]
    if isinstance(minutes, int) and not isinstance(minutes, bool) and not 5 <= minutes <= 1440:
        errors.append(
            f"access_token_validity_minutes is {minutes} — the contract allows 5-1440 "
            "(the Amazon Cognito access-token validity bounds)."
        )
    return errors


def ses_verification_status(email, region, profile, ses_fixture):
    if ses_fixture is not None:
        return ses_fixture.get(email, "NOT_FOUND")
    cmd = [
        "aws", "sesv2", "get-email-identity",
        "--email-identity", email,
        "--region", region,
        "--output", "json",
    ]
    if profile:
        cmd += ["--profile", profile]
    try:
        result = subprocess.run(cmd, capture_output=True, text=True, check=False)
    except FileNotFoundError:
        return "AWS_CLI_MISSING"
    if result.returncode != 0:
        if "NotFoundException" in result.stderr:
            return "NOT_FOUND"
        return f"AWS_ERROR: {result.stderr.strip()}"
    return json.loads(result.stdout).get("VerificationStatus", "UNKNOWN")


def check_ses(cfg, region, profile, ses_fixture, skip_aws):
    sender = cfg["notification_sender_email"]
    operator = cfg["platform_operator_email"]
    if not sender and not operator:
        note("notifications disabled (notification_sender_email and platform_operator_email both empty)")
        return []
    if not sender or not operator:
        return []  # both-or-neither already reported by validate_prerequisites
    if skip_aws:
        note("SES verification status not checked (--skip-aws-checks)")
        return []

    errors = []
    for key, email in (("notification_sender_email", sender), ("platform_operator_email", operator)):
        status = ses_verification_status(email, region, profile, ses_fixture)
        if status == "SUCCESS":
            note(f"SES identity verified: {email}")
        elif status == "NOT_FOUND":
            errors.append(
                f"{key} '{email}' has no SES identity in region {region} — create it with "
                "'make deploy-notifications' and confirm the verification email before enabling "
                "notifications."
            )
        elif status and status.startswith("AWS_ERROR"):
            errors.append(f"could not check SES status for {key} '{email}': {status.removeprefix('AWS_ERROR: ')}")
        elif status == "AWS_CLI_MISSING":
            errors.append("could not check SES status: the aws CLI is not installed or not on PATH.")
        else:
            errors.append(
                f"{key} '{email}' SES verification status is {status} — confirm the verification email "
                f"(or run 'make deploy-notifications' first); status must be SUCCESS before enabling "
                "notifications."
            )
    return errors


def deployed_parameters(stack_name, region, profile, stack_fixture):
    if stack_fixture is not None:
        data = json.loads(Path(stack_fixture).read_text())
    else:
        cmd = [
            "aws", "cloudformation", "describe-stacks",
            "--stack-name", stack_name,
            "--region", region,
            "--output", "json",
        ]
        if profile:
            cmd += ["--profile", profile]
        try:
            result = subprocess.run(cmd, capture_output=True, text=True, check=False)
        except FileNotFoundError:
            return None, "the aws CLI is not installed or not on PATH"
        if result.returncode != 0:
            if "does not exist" in result.stderr or "ValidationError" in result.stderr:
                return {}, None
            return None, result.stderr.strip()
        data = json.loads(result.stdout)
    stacks = data.get("Stacks", [])
    if not stacks:
        return {}, None
    return {p["ParameterKey"]: p.get("ParameterValue", "") for p in stacks[0].get("Parameters", [])}, None


def drift_guard(cfg, stack_name, region, profile, stack_fixture, skip_aws):
    if skip_aws:
        note("drift guard skipped (--skip-aws-checks)")
        return []
    params, failure = deployed_parameters(stack_name, region, profile, stack_fixture)
    if failure:
        return [f"could not read deployed parameters for stack '{stack_name}': {failure}"]
    if params == {}:
        note(f"stack '{stack_name}' not deployed — fresh installation, no drift guard needed")
        return []

    def configured(parameter):
        key = next(k for k, v in SETTINGS.items() if v[0] == parameter)
        return render_value(cfg[key])

    errors = []
    for key, (parameter, lifecycle) in SETTINGS.items():
        if lifecycle != "fixed" or parameter not in params:
            continue
        deployed = params[parameter]
        wanted = configured(parameter)
        if deployed != wanted:
            errors.append(
                f"{key} is fixed at setup: deployed stack '{stack_name}' has {parameter}='{deployed}' "
                f"but the configuration requests '{wanted}'. Changing it on an existing installation "
                "means a new deployment — redeploy with the deployed value or stand up a new stack. "
                "See docs/reference/configuration.md."
            )
    return errors


# ---- Stage rendering -------------------------------------------------------
# The core template keeps a literal SAM-managed stage (the authoring
# environment, marked "# konductor:authoring-stage") and a rendered block of
# stage resources (between the "# konductor:rendered-stages" markers) for
# every other configured environment. The block is rendered inline rather
# than included via Fn::Transform: a Resources-level AWS::Include failed
# changeset creation ("S3 object's content should be valid Yaml/JSON") in the
# full template, so the deploy-time render edits the template directly.

REPO_ROOT = Path(__file__).resolve().parent.parent
TEMPLATE_PATH = REPO_ROOT / "konductor-core-stack.yaml"

AUTHORING_STAGE_RE = re.compile(
    r"^(\s*)StageName: \S+(\s*# konductor:authoring-stage\s*)$", re.M
)
EXPECTED_AUTHORING_MARKERS = 2  # KonductorOAuthApi and KonductorIamApi

ACCESS_LOG_FORMAT = (
    '{"requestId":"$context.requestId","ip":"$context.identity.sourceIp",'
    '"requestTime":"$context.requestTime","httpMethod":"$context.httpMethod",'
    '"resourcePath":"$context.resourcePath","status":"$context.status",'
    '"responseLength":"$context.responseLength",'
    '"integrationLatency":"$context.integrationLatency"}'
)

STAGE_APIS = [
    ("KonductorOAuthApi", "ApiAccessLogGroup"),
    ("KonductorIamApi", "IamApiAccessLogGroup"),
]

RENDERED_STAGES_RE = re.compile(
    r"  # konductor:rendered-stages-begin\n.*?  # konductor:rendered-stages-end\n",
    re.S,
)


def stage_logical_suffix(env):
    """Logical-ID-safe form of an environment name (`qa-blue` -> `QaBlue`)."""
    parts = [p for p in re.split(r"[^A-Za-z0-9]+", env) if p]
    return "".join(p[:1].upper() + p[1:] for p in parts)


def render_stages_block(envs):
    """The stage resources between the rendered-stages markers, indented as
    Resources-level entries (two spaces)."""
    suffixes = {}
    for env in envs:
        suffix = stage_logical_suffix(env)
        if suffix in suffixes:
            raise ValueError(
                f"environments '{suffixes[suffix]}' and '{env}' produce the same "
                "logical-ID suffix — rename one of them."
            )
        suffixes[suffix] = env
    lines = [
        "  # Generated by scripts/validate-config.py --render-stages from the",
        "  # validated environment list (konductor-platform.toml). Do not edit",
        "  # by hand.",
    ]
    for env in envs[1:]:
        suffix = stage_logical_suffix(env)
        for api, log_group in STAGE_APIS:
            lines += [
                "",
                f"  {api}{suffix}Stage:",
                "    Type: AWS::ApiGateway::Stage",
                "    DeletionPolicy: Delete",
                "    UpdateReplacePolicy: Delete",
                "    Properties:",
                f"      RestApiId: {{ Ref: {api} }}",
                f"      DeploymentId: {{ Ref: {api}.Deployment }}",
                f"      StageName: {env}",
                "      TracingEnabled: true",
                "      AccessLogSetting:",
                f"        DestinationArn: {{ Fn::GetAtt: [{log_group}, Arn] }}",
                f"        Format: '{ACCESS_LOG_FORMAT}'",
                "      MethodSettings:",
                '        - ResourcePath: "/*"',
                '          HttpMethod: "*"',
                "          ThrottlingBurstLimit: 100",
                "          ThrottlingRateLimit: 50",
            ]
    return "\n".join(lines) + "\n"


def render_template_stages(template_text, envs):
    """Splice the rendered stage block between the markers."""
    block = (
        "  # konductor:rendered-stages-begin\n"
        + render_stages_block(envs)
        + "  # konductor:rendered-stages-end\n"
    )
    rendered, count = RENDERED_STAGES_RE.subn(block, template_text)
    if count != 1:
        raise ValueError(
            "expected exactly one konductor:rendered-stages marker pair in "
            f"{TEMPLATE_PATH.name}, found {count}"
        )
    return rendered


def render_template_authoring_stage(template_text, envs):
    markers = AUTHORING_STAGE_RE.findall(template_text)
    if len(markers) != EXPECTED_AUTHORING_MARKERS:
        raise ValueError(
            f"expected {EXPECTED_AUTHORING_MARKERS} '# konductor:authoring-stage' markers "
            f"in {TEMPLATE_PATH.name}, found {len(markers)}"
        )
    return AUTHORING_STAGE_RE.sub(
        lambda m: f"{m.group(1)}StageName: {envs[0]}{m.group(2)}", template_text
    )


def stage_render_config(args):
    if Path(args.config).exists():
        return load_config(args.config)
    note(f"{args.config} not found — rendering stages with defaults: {DEFAULTS['environments']}")
    return dict(DEFAULTS)

def render_value(value):
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, list):
        return ",".join(value)
    return str(value)


def render_overrides(cfg):
    parts = []
    for key, (parameter, _) in SETTINGS.items():
        parts.append(f"{parameter}={render_value(cfg[key])}")
    return " ".join(parts)


def init_from_stack(stack_name, region, profile, stack_fixture):
    """Build a konductor-platform.toml skeleton from deployed stack parameters."""
    params, failure = deployed_parameters(stack_name, region, profile, stack_fixture)
    if failure:
        err(f"could not read deployed parameters for stack '{stack_name}': {failure}")
        return None
    if params == {}:
        err(f"stack '{stack_name}' not deployed in region {region} — nothing to import.")
        return None

    cfg = {}
    for key, (parameter, _) in SETTINGS.items():
        raw = params.get(parameter)
        if raw is None:
            cfg[key] = DEFAULTS.get(key)
            continue
        default = DEFAULTS.get(key)
        if isinstance(default, bool):
            cfg[key] = raw == "true"
        elif isinstance(default, list):
            cfg[key] = [entry for entry in raw.split(",") if entry]
        elif isinstance(default, int):
            cfg[key] = int(raw)
        else:
            cfg[key] = raw

    lines = [
        "# Customer platform configuration for Konductor.",
        "# Generated from the deployed '%s' stack. Contract: docs/reference/configuration.md." % stack_name,
        "",
    ]
    for key in SETTINGS:
        value = cfg.get(key)
        if value is None:
            lines.append(f"# {key} = <required — no value found on the deployed stack>")
        elif isinstance(value, list):
            lines.append(f"{key} = [{', '.join(json.dumps(entry) for entry in value)}]")
        else:
            lines.append(f"{key} = {json.dumps(value)}")
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("config", nargs="?", default="konductor-platform.toml")
    parser.add_argument("--render", action="store_true",
                        help="print the sam --parameter-overrides value on stdout")
    parser.add_argument("--render-stages", action="store_true",
                        help="re-render the template's rendered-stages block and authoring "
                             "StageName from the validated environment list")
    parser.add_argument("--check-stages", action="store_true",
                        help="fail if the template's rendered stages or authoring StageName "
                             "drifted from the configured environment list")
    parser.add_argument("--region", default=None)
    parser.add_argument("--profile", default=None)
    parser.add_argument("--stack-name", default="konductor-core")
    parser.add_argument("--stack-fixture", default=None,
                        help="read deployed parameters from a recorded describe-stacks JSON file")
    parser.add_argument("--ses-fixture", default=None,
                        help="read SES statuses from a JSON object mapping email to VerificationStatus")
    parser.add_argument("--skip-aws-checks", action="store_true",
                        help="skip SES and drift-guard checks (offline validation only; not for deploy)")
    parser.add_argument("--init-from-stack", action="store_true",
                        help="print a configuration skeleton built from the deployed stack's parameters")
    args = parser.parse_args()

    region = args.region or subprocess.run(
        ["aws", "configure", "get", "region"], capture_output=True, text=True
    ).stdout.strip() or "us-east-1"

    if args.init_from_stack:
        skeleton = init_from_stack(args.stack_name, region, args.profile, args.stack_fixture)
        if skeleton is None:
            sys.exit(1)
        print(skeleton)
        return

    if args.render_stages or args.check_stages:
        cfg = stage_render_config(args)
        if cfg is None:
            sys.exit(1)
        env_errors = validate_environments(cfg["environments"])
        if env_errors:
            for message in env_errors:
                err(message)
            sys.exit(1)
        envs = cfg["environments"]
        try:
            rendered_template = render_template_stages(
                render_template_authoring_stage(TEMPLATE_PATH.read_text(), envs),
                envs,
            )
        except ValueError as exc:
            err(str(exc))
            sys.exit(1)
        if args.check_stages:
            if rendered_template != TEMPLATE_PATH.read_text():
                err(f"{TEMPLATE_PATH.name} stage rendering is stale for environments {envs}")
                print("stage drift — run scripts/validate-config.py --render-stages", file=sys.stderr)
                sys.exit(1)
            print("stages match the configured environment list", file=sys.stderr)
            return
        if rendered_template != TEMPLATE_PATH.read_text():
            TEMPLATE_PATH.write_text(rendered_template)
            note(f"rendered stages for {envs} into {TEMPLATE_PATH.name}")
        return

    cfg = load_config(args.config)
    if cfg is None:
        sys.exit(1)

    ses_fixture = json.loads(Path(args.ses_fixture).read_text()) if args.ses_fixture else None

    errors = []
    errors += validate_contract(cfg)
    errors += validate_prerequisites(cfg)
    errors += check_ses(cfg, region, args.profile, ses_fixture, args.skip_aws_checks)
    errors += drift_guard(cfg, args.stack_name, region, args.profile,
                          args.stack_fixture, args.skip_aws_checks)

    if errors:
        for message in errors:
            err(message)
        print(f"configuration invalid: {len(errors)} problem(s) in {args.config}", file=sys.stderr)
        sys.exit(1)

    print(f"configuration valid: {args.config}", file=sys.stderr)
    if args.render:
        print(render_overrides(cfg))


if __name__ == "__main__":
    main()
