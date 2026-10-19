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


if __name__ == "__main__":
    unittest.main()
