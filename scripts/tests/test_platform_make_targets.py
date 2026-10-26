#!/usr/bin/env python3
"""Tests for the Makefile's platform-account targets (run: `python3 scripts/tests/test_platform_make_targets.py`).

Recipes are rendered with `make -n`, so nothing is built and nothing reaches AWS.
"""

import os
import re
import shlex
import stat
import subprocess
import tempfile
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
REGION = "ap-southeast-2"
PROFILE = "konductor-platform"
NOTIFICATION_EMAILS = {
    "NOTIFICATION_SENDER_EMAIL": "sender@example.com",
    "PLATFORM_OPERATOR_EMAIL": "platform-team@example.com",
}
# Answers `aws configure get region [--profile <name>]` like the AWS CLI: the
# profile's configured Region, or exit 1 when the profile has none.
AWS_STUB = f"""#!/usr/bin/env bash
case "$*" in
  "configure get region") region="$CURRENT_PROFILE_REGION" ;;
  "configure get region --profile {PROFILE}") region="$PLATFORM_PROFILE_REGION" ;;
  *) echo "unexpected aws call: $*" >&2; exit 1 ;;
esac
[ -n "$region" ] && echo "$region"
"""


def rendered_commands(target, region=REGION, environment=None, **variables):
    inherited = {
        key: value
        for key, value in os.environ.items()
        if key not in ("PLATFORM_PROFILE", "AWS_REGION", "AWS_DEFAULT_REGION")
    }
    result = subprocess.run(
        [
            "make", "--no-print-directory", "-n", target,
            *([f"AWS_REGION={region}"] if region else []),
            *(f"{key}={value}" for key, value in variables.items()),
        ],
        cwd=REPO_ROOT,
        env={**inherited, **(environment or {})},
        capture_output=True,
        text=True,
        check=True,
    )
    script = result.stdout.replace("\\\n", " ")
    return [" ".join(part.split()) for part in re.split(r"[;\n]|\$\(|\)", script) if part.strip()]


def commands_starting(commands, prefix, *required_tokens):
    return [
        command
        for command in commands
        if command.startswith(prefix) and set(required_tokens) <= set(shlex.split(command))
    ]


def only(commands, prefix, *required_tokens):
    matches = commands_starting(commands, prefix, *required_tokens)
    assert len(matches) == 1, f"expected one '{prefix}' command, found {matches}"
    return matches[0]


def option_values(command, option):
    tokens = shlex.split(command)
    return [tokens[index + 1] for index in range(len(tokens) - 1) if tokens[index] == option]


class DeployCoreTest(unittest.TestCase):
    def test_deploys_to_the_region_and_profile_the_preflight_validated(self):
        commands = rendered_commands("deploy-core", PLATFORM_PROFILE=PROFILE)

        preflight = only(commands, "python3 scripts/validate-config.py", "--render")
        deploy = only(commands, "sam deploy")

        for command in (preflight, deploy):
            self.assertEqual(option_values(command, "--region"), [REGION], command)
            self.assertEqual(option_values(command, "--profile"), [PROFILE], command)

    def test_preflight_and_deploy_use_the_current_credentials_when_no_profile_is_set(self):
        commands = rendered_commands("deploy-core")

        preflight = only(commands, "python3 scripts/validate-config.py", "--render")
        deploy = only(commands, "sam deploy")

        for command in (preflight, deploy):
            self.assertEqual(option_values(command, "--profile"), [], command)


class DeployNotificationsTest(unittest.TestCase):
    def test_deploys_to_the_requested_region_and_profile(self):
        commands = rendered_commands("deploy-notifications", PLATFORM_PROFILE=PROFILE, **NOTIFICATION_EMAILS)

        deploy = only(commands, "sam deploy")

        self.assertEqual(option_values(deploy, "--region"), [REGION], deploy)
        self.assertEqual(option_values(deploy, "--profile"), [PROFILE], deploy)

    def test_uses_the_current_credentials_when_no_profile_is_set(self):
        deploy = only(rendered_commands("deploy-notifications", **NOTIFICATION_EMAILS), "sam deploy")

        self.assertEqual(option_values(deploy, "--region"), [REGION], deploy)
        self.assertEqual(option_values(deploy, "--profile"), [], deploy)


class StackTeardownTest(unittest.TestCase):
    TARGETS = ("teardown-core", "teardown-notifications")

    def test_deletes_in_the_requested_region_and_profile(self):
        for target in self.TARGETS:
            with self.subTest(target=target):
                delete = only(rendered_commands(target, PLATFORM_PROFILE=PROFILE), "aws cloudformation delete-stack")

                self.assertEqual(option_values(delete, "--region"), [REGION], delete)
                self.assertEqual(option_values(delete, "--profile"), [PROFILE], delete)

    def test_uses_the_current_credentials_when_no_profile_is_set(self):
        for target in self.TARGETS:
            with self.subTest(target=target):
                delete = only(rendered_commands(target), "aws cloudformation delete-stack")

                self.assertEqual(option_values(delete, "--region"), [REGION], delete)
                self.assertEqual(option_values(delete, "--profile"), [], delete)


class ValidateConfigTest(unittest.TestCase):
    def test_uses_the_current_credentials_when_no_profile_is_set(self):
        preflight = only(rendered_commands("validate-config"), "python3 scripts/validate-config.py")

        self.assertEqual(option_values(preflight, "--region"), [REGION], preflight)
        self.assertEqual(option_values(preflight, "--profile"), [], preflight)


class DeployTrustTest(unittest.TestCase):
    def test_reads_core_outputs_with_the_platform_profile_and_deploys_with_the_app_profile(self):
        commands = rendered_commands("deploy-trust", PLATFORM_PROFILE=PROFILE, APP_PROFILE="domain-account")

        lookups = commands_starting(commands, "aws cloudformation describe-stacks")
        deploy = only(commands, "aws cloudformation deploy")

        self.assertEqual(len(lookups), 3, lookups)
        for lookup in lookups:
            self.assertEqual(option_values(lookup, "--region"), [REGION], lookup)
            self.assertEqual(option_values(lookup, "--profile"), [PROFILE], lookup)
        self.assertEqual(option_values(deploy, "--profile"), ["domain-account"], deploy)

    def test_reads_core_outputs_with_the_current_credentials_when_no_profile_is_set(self):
        commands = rendered_commands("deploy-trust", APP_PROFILE="domain-account")

        lookups = commands_starting(commands, "aws cloudformation describe-stacks")

        self.assertEqual(len(lookups), 3, lookups)
        for lookup in lookups:
            self.assertEqual(option_values(lookup, "--profile"), [], lookup)


class RegionDefaultTest(unittest.TestCase):
    """Without AWS_REGION, targets resolve the Region in the AWS CLI's order."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        stub = Path(self.tmp.name) / "aws"
        stub.write_text(AWS_STUB)
        stub.chmod(stub.stat().st_mode | stat.S_IEXEC)
        self.environment = {
            "PATH": f"{self.tmp.name}:{os.environ['PATH']}",
            "CURRENT_PROFILE_REGION": "us-west-2",
            "PLATFORM_PROFILE_REGION": "eu-central-1",
        }

    def tearDown(self):
        self.tmp.cleanup()

    def deploy_regions(self, region=None, **variables):
        commands = rendered_commands(
            "deploy-notifications",
            region=region,
            environment=self.environment,
            **NOTIFICATION_EMAILS,
            **variables,
        )
        return option_values(only(commands, "sam deploy"), "--region")

    def test_aws_default_region_comes_before_any_profile_region(self):
        self.environment["AWS_DEFAULT_REGION"] = "eu-west-1"

        for variables in ({}, {"PLATFORM_PROFILE": PROFILE}):
            with self.subTest(**variables):
                self.assertEqual(self.deploy_regions(**variables), ["eu-west-1"])

    def test_deploy_core_preflights_and_deploys_in_aws_default_region(self):
        self.environment["AWS_DEFAULT_REGION"] = "eu-west-1"

        commands = rendered_commands("deploy-core", region=None, environment=self.environment)

        preflight = only(commands, "python3 scripts/validate-config.py", "--render")
        deploy = only(commands, "sam deploy")
        for command in (preflight, deploy):
            self.assertEqual(option_values(command, "--region"), ["eu-west-1"], command)

    def test_an_explicit_aws_region_overrides_aws_default_region(self):
        self.environment["AWS_DEFAULT_REGION"] = "eu-west-1"

        with self.subTest(source="command line"):
            self.assertEqual(self.deploy_regions(region=REGION), [REGION])
        with self.subTest(source="environment"):
            self.environment["AWS_REGION"] = REGION
            self.assertEqual(self.deploy_regions(), [REGION])

    def test_falls_back_to_the_platform_profile_region(self):
        self.assertEqual(self.deploy_regions(PLATFORM_PROFILE=PROFILE), ["eu-central-1"])

    def test_falls_back_to_the_current_profile_region_without_a_platform_profile(self):
        self.assertEqual(self.deploy_regions(), ["us-west-2"])

    def test_falls_back_to_us_east_1_when_no_region_is_configured(self):
        self.environment["CURRENT_PROFILE_REGION"] = ""

        self.assertEqual(self.deploy_regions(), ["us-east-1"])


if __name__ == "__main__":
    unittest.main()
