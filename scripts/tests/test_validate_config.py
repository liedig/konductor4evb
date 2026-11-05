#!/usr/bin/env python3
"""Tests for scripts/validate-config.py's drift guard (run: `python3 scripts/tests/test_validate_config.py`)."""

import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path

MODULE = Path(__file__).resolve().parent.parent / "validate-config.py"
spec = importlib.util.spec_from_file_location("validate_config", MODULE)
vc = importlib.util.module_from_spec(spec)
spec.loader.exec_module(vc)


def write_fixture(parameters):
    handle = tempfile.NamedTemporaryFile(
        mode="w", suffix=".json", delete=False
    )
    json.dump(
        {
            "Stacks": [
                {
                    "Parameters": [
                        {"ParameterKey": key, "ParameterValue": value}
                        for key, value in parameters.items()
                    ]
                }
            ]
        },
        handle,
    )
    handle.close()
    return handle.name


class DriftGuardTest(unittest.TestCase):
    def test_reordered_environment_list_is_refused(self):
        fixture = write_fixture({"Environments": "dev,staging,prod"})
        cfg = {"environments": ["staging", "dev", "prod"]}
        errors = vc.drift_guard(cfg, "konductor-core", "r", None, fixture, False)
        self.assertTrue(errors, "a reordered list must fail the drift guard")
        self.assertIn("fixed at setup", errors[0])

    def test_matching_environment_list_passes(self):
        fixture = write_fixture({"Environments": "dev,staging,prod"})
        cfg = {"environments": ["dev", "staging", "prod"]}
        self.assertEqual(vc.drift_guard(cfg, "konductor-core", "r", None, fixture, False), [])

    def test_fresh_installation_skips_the_guard(self):
        fixture = write_fixture({})
        cfg = {"environments": ["qa", "live"]}
        self.assertEqual(vc.drift_guard(cfg, "konductor-core", "r", None, fixture, False), [])


class BoolSettingTest(unittest.TestCase):
    def test_bool_settings_render_as_lowercase_parameter_values(self):
        cfg = dict(vc.DEFAULTS)
        cfg.update(
            {
                "organization": "acme",
                "aws_organization_arn": "arn:aws:organizations::123456789012:organization/o-abc123def4",
                "system_event_emission_enabled": True,
            }
        )
        overrides = vc.render_overrides(cfg)
        self.assertIn("SystemEventEmissionEnabled=true", overrides)

    def test_bool_settings_reject_string_values(self):
        import io
        from contextlib import redirect_stderr

        with tempfile.NamedTemporaryFile(mode="w", suffix=".toml", delete=False) as handle:
            handle.write(
                'organization = "acme"\n'
                'aws_organization_arn = "arn:aws:organizations::123456789012:organization/o-abc123def4"\n'
                'system_event_emission_enabled = "true"\n'
            )
            path = handle.name
        buffer = io.StringIO()
        with redirect_stderr(buffer):
            cfg = vc.load_config(path)
        self.assertIsNone(cfg)
        self.assertIn("must be a bool", buffer.getvalue())



class RenderStagesPublishThrottleTest(unittest.TestCase):
    """Every rendered stage keeps the /* control-plane throttle and gains the
    dedicated /~1publish POST throttle that references the parameters."""

    def test_rendered_stages_add_publish_method_throttle(self):
        block = vc.render_stages_block(["dev", "staging", "prod"])
        # 2 APIs x 2 rendered environments = 4 stages, each with both entries.
        self.assertEqual(block.count('ResourcePath: "/*"'), 4)
        self.assertEqual(block.count('ThrottlingRateLimit: 50'), 4)
        self.assertEqual(block.count('ThrottlingBurstLimit: 100'), 4)
        self.assertEqual(block.count('ResourcePath: "/~1publish"'), 4)
        self.assertEqual(block.count("HttpMethod: POST"), 4)
        self.assertEqual(
            block.count("ThrottlingRateLimit: { Ref: PublishThrottleRateLimit }"), 4
        )
        self.assertEqual(
            block.count("ThrottlingBurstLimit: { Ref: PublishThrottleBurstLimit }"), 4
        )

    def test_committed_rendered_block_matches_default_environments(self):
        template = vc.TEMPLATE_PATH.read_text()
        match = vc.RENDERED_STAGES_RE.search(template)
        self.assertIsNotNone(match, "rendered-stages markers missing from template")
        expected = (
            "  # konductor:rendered-stages-begin\n"
            + vc.render_stages_block(vc.DEFAULTS["environments"])
            + "  # konductor:rendered-stages-end\n"
        )
        self.assertEqual(
            match.group(0),
            expected,
            "committed rendered block drifted — run make render-stages",
        )


def write_toml(extra_lines):
    handle = tempfile.NamedTemporaryFile(mode="w", suffix=".toml", delete=False)
    handle.write(
        'organization = "acme"\n'
        'aws_organization_arn = "arn:aws:organizations::123456789012:organization/o-abc123def4"\n'
        + extra_lines
    )
    handle.close()
    return handle.name


class PublishPathSettingsTest(unittest.TestCase):
    SETTINGS = {
        "publish_throttle_rate_limit": ("PublishThrottleRateLimit", 2000),
        "publish_throttle_burst_limit": ("PublishThrottleBurstLimit", 1000),
        "publish_admission_concurrency_cap": ("PublishAdmissionConcurrencyCap", 0),
        "publish_delivery_max_concurrency": ("PublishDeliveryMaxConcurrency", 50),
        "domain_liveness_cache_seconds": ("DomainLivenessCacheSeconds", 300),
    }

    def valid_cfg(self, **overrides):
        cfg = dict(vc.DEFAULTS)
        cfg.update(
            {
                "organization": "acme",
                "aws_organization_arn": "arn:aws:organizations::123456789012:organization/o-abc123def4",
            }
        )
        cfg.update(overrides)
        return cfg

    def test_settings_are_updateable_and_render_under_parameter_names(self):
        cfg = self.valid_cfg(
            publish_throttle_rate_limit=3000,
            publish_throttle_burst_limit=1500,
            publish_admission_concurrency_cap=25,
            publish_delivery_max_concurrency=100,
            domain_liveness_cache_seconds=60,
        )
        overrides = vc.render_overrides(cfg)
        for key, (parameter, default) in self.SETTINGS.items():
            with self.subTest(setting=key):
                self.assertEqual(vc.SETTINGS[key], (parameter, "updateable"))
                self.assertIn(f"{parameter}={cfg[key]}", overrides)
        defaults = vc.render_overrides(self.valid_cfg())
        for key, (parameter, default) in self.SETTINGS.items():
            self.assertIn(f"{parameter}={default}", defaults)

    def test_non_integer_values_are_rejected(self):
        for key in self.SETTINGS:
            with self.subTest(setting=key):
                path = write_toml(f'{key} = "2000"\n')
                self.assertIsNone(vc.load_config(path))

    def test_boolean_values_are_rejected(self):
        for key in self.SETTINGS:
            with self.subTest(setting=key):
                path = write_toml(f"{key} = true\n")
                self.assertIsNone(vc.load_config(path))

    def test_out_of_range_values_are_rejected(self):
        cases = {
            "publish_throttle_rate_limit": (0, "1"),
            "publish_throttle_burst_limit": (-1, "0"),
            "publish_admission_concurrency_cap": (-1, "0"),
            "publish_delivery_max_concurrency": (1, "2-1000"),
            "domain_liveness_cache_seconds": (901, "0-900"),
        }
        for key, (bad, allowed) in cases.items():
            with self.subTest(setting=key):
                errors = vc.validate_publish_path(self.valid_cfg(**{key: bad}))
                self.assertTrue(errors, f"{key}={bad} must be rejected")
                self.assertIn(key, errors[0])
                self.assertIn(allowed, errors[0])
        extra = {
            "publish_delivery_max_concurrency": 1001,
            "domain_liveness_cache_seconds": -1,
        }
        for key, bad in extra.items():
            with self.subTest(setting=key):
                self.assertTrue(vc.validate_publish_path(self.valid_cfg(**{key: bad})))

    def test_boundary_values_are_accepted(self):
        cfg = self.valid_cfg(
            publish_throttle_rate_limit=1,
            publish_throttle_burst_limit=0,
            publish_admission_concurrency_cap=0,
            publish_delivery_max_concurrency=2,
            domain_liveness_cache_seconds=900,
        )
        self.assertEqual(vc.validate_publish_path(cfg), [])
        cfg["publish_delivery_max_concurrency"] = 1000
        cfg["domain_liveness_cache_seconds"] = 0
        self.assertEqual(vc.validate_publish_path(cfg), [])


class ThrottleBudgetTest(unittest.TestCase):
    """The throttle budget check warns (never fails) when one stage at its
    publish limit plus every stage's control-plane limit exceeds the account
    API Gateway throttle, and always notes the all-stages totals."""

    def cfg(self, **overrides):
        cfg = dict(vc.DEFAULTS)
        cfg.update(
            {
                "organization": "acme",
                "aws_organization_arn": "arn:aws:organizations::123456789012:organization/o-abc123def4",
            }
        )
        cfg.update(overrides)
        return cfg

    def run_budget(self, cfg, quota):
        import io
        from contextlib import redirect_stderr

        buffer = io.StringIO()
        with redirect_stderr(buffer):
            vc.throttle_budget(cfg, quota)
        return buffer.getvalue()

    def test_defaults_fit_the_default_quota_without_warning(self):
        output = self.run_budget(self.cfg(), 10000)
        self.assertNotIn("WARNING", output)
        for expected in ("19,950", "21,900", "10,000", "5,000"):
            self.assertIn(expected, output)

    def test_lower_quota_triggers_rate_warning(self):
        output = self.run_budget(self.cfg(), 2500)
        self.assertIn("WARNING", output)
        self.assertIn("publish_throttle_rate_limit", output)
        self.assertIn("x 3 environments", output)  # the per-method control-plane ceiling
        self.assertIn("9,950", output)
        self.assertIn("2,500", output)
        self.assertNotIn("publish_throttle_burst_limit", output)

    def test_raised_publish_rate_triggers_rate_warning(self):
        output = self.run_budget(self.cfg(publish_throttle_rate_limit=9600), 10000)
        self.assertIn("WARNING", output)
        self.assertIn("publish_throttle_rate_limit", output)
        self.assertIn("17,550", output)
        self.assertIn("10,000", output)

    def test_publish_burst_alone_triggers_burst_warning(self):
        output = self.run_budget(self.cfg(publish_throttle_burst_limit=6000), 10000)
        self.assertIn("WARNING", output)
        self.assertIn("publish_throttle_burst_limit", output)
        self.assertIn("6,000", output)
        self.assertIn("5,000", output)
        self.assertNotIn("publish_throttle_rate_limit", output)

    def test_warning_preserves_exit_code_and_render_output(self):
        import subprocess

        path = write_toml("")
        result = subprocess.run(
            [
                sys.executable,
                str(MODULE),
                path,
                "--region", "us-east-1",
                "--skip-aws-checks",
                "--quota-fixture", "2500",
                "--render",
            ],
            capture_output=True,
            text=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("WARNING", result.stderr)
        self.assertEqual(result.stdout, vc.render_overrides(self.cfg()) + "\n")

    def test_applied_quota_is_preferred_over_the_aws_default(self):
        from unittest import mock

        calls = []

        def fake_run(cmd, **kwargs):
            calls.append(cmd)
            result = mock.Mock()
            result.returncode = 0
            result.stdout = json.dumps({"Quota": {"Value": 7500.0}})
            return result

        with mock.patch.object(vc.subprocess, "run", side_effect=fake_run):
            quota, quota_note = vc.account_throttle_quota("us-east-1", None, None, False)
        self.assertEqual(quota, 7500)
        self.assertIsNone(quota_note)
        subcommands = [cmd[2] for cmd in calls]
        self.assertIn("get-service-quota", subcommands)
        self.assertNotIn("get-aws-default-service-quota", subcommands)

    def test_failed_lookup_falls_back_to_the_documented_default(self):
        from unittest import mock

        def fake_run(cmd, **kwargs):
            result = mock.Mock()
            result.returncode = 1
            result.stdout = ""
            result.stderr = "AccessDenied"
            return result

        with mock.patch.object(vc.subprocess, "run", side_effect=fake_run):
            quota, quota_note = vc.account_throttle_quota("us-east-1", None, None, False)
        self.assertEqual(quota, 10000)
        self.assertIn("10,000", quota_note)

    def test_skip_aws_checks_falls_back_to_the_documented_default(self):
        quota, quota_note = vc.account_throttle_quota("us-east-1", None, None, True)
        self.assertEqual(quota, 10000)
        self.assertIn("10,000", quota_note)

    def test_quota_fixture_wins(self):
        quota, quota_note = vc.account_throttle_quota("us-east-1", None, 2500, False)
        self.assertEqual(quota, 2500)
        self.assertIsNone(quota_note)

    def test_the_aws_default_quota_fallback_is_disclosed(self):
        from unittest import mock

        def fake_run(cmd, **kwargs):
            result = mock.Mock()
            if "get-service-quota" in cmd:
                result.returncode = 1
                result.stdout = ""
                result.stderr = "AccessDenied"
            else:
                result.returncode = 0
                result.stdout = json.dumps({"Quota": {"Value": 2500.0}})
            return result

        with mock.patch.object(vc.subprocess, "run", side_effect=fake_run):
            quota, quota_note = vc.account_throttle_quota("us-east-1", None, None, False)
        self.assertEqual(quota, 2500)
        self.assertIsNotNone(quota_note)
        self.assertIn("AWS default", quota_note)


class ControlPlaneMethodCountTest(unittest.TestCase):
    """The budget counts API Gateway method buckets, and the wildcard stage
    throttle applies to every method independently. The counts must track the
    OpenAPI contracts, which define each stage's methods."""

    def test_method_bucket_constants_match_the_openapi_contracts(self):
        import yaml

        methods = ("get", "post", "put", "patch", "delete", "head", "options")
        contracts = (
            ("api/openapi-oauth.yaml", vc.OAUTH_CONTROL_PLANE_METHODS),
            ("api/openapi-iam.yaml", vc.IAM_CONTROL_PLANE_METHODS),
        )
        repo_root = MODULE.resolve().parent.parent
        for name, constant in contracts:
            with self.subTest(contract=name):
                doc = yaml.safe_load((repo_root / name).read_text())
                operations = [
                    method
                    for path, item in doc["paths"].items()
                    for method in item
                    if method in methods and not (path == "/publish" and method == "post")
                ]
                self.assertEqual(
                    len(operations),
                    constant,
                    f"{name} control-plane operation count drifted; update the "
                    "OAUTH_CONTROL_PLANE_METHODS / IAM_CONTROL_PLANE_METHODS constants",
                )

if __name__ == "__main__":
    unittest.main()
