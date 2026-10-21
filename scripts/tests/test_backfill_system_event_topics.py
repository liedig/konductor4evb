#!/usr/bin/env python3
"""Backfill script behavior tests (run: `python3 scripts/tests/test_backfill_system_event_topics.py`).

Runs scripts/backfill-system-event-topics.sh with a mocked `aws` CLI on PATH
(the scripts directory stays stdlib-only). The stub serves canned scan pages,
answers Lambda ensure calls, and records invoke/update operations to a log so
tests can assert exactly which writes happened.
"""
import json
import os
import stat
import subprocess
import tempfile
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent.parent
SCRIPT = REPO / "scripts/backfill-system-event-topics.sh"

AWS_STUB = """#!/usr/bin/env bash
set -euo pipefail
case "$1 $2" in
  "cloudformation describe-stacks")
    echo "acme"
    ;;
  "dynamodb scan")
    page="scan-page1.json"
    for arg in "$@"; do
      if [ "$arg" = "page2" ]; then page="scan-page2.json"; fi
    done
    echo "scan $page" >> "$MOCK_LOG"
    cat "$MOCK_DIR/$page"
    ;;
  "lambda invoke")
    payload=""
    outfile=""
    prev=""
    for arg in "$@"; do
      if [ "$prev" = "--payload" ]; then payload="$arg"; fi
      prev="$arg"
    done
    outfile="$prev"
    domain=$(echo "$payload" | jq -r '.domainName')
    echo "invoke $domain" >> "$MOCK_LOG"
    echo "$payload" > "$MOCK_DIR/payload-$domain.json"
    if [ -f "$MOCK_DIR/failing-domains" ] && grep -qx "$domain" "$MOCK_DIR/failing-domains"; then
      echo '{"errorMessage": "simulated ensure failure"}' > "$outfile"
      echo '{"FunctionError": "Unhandled"}'
    else
      echo "{\\"topicArn\\": \\"arn:aws:sns:eu-west-1:999988887777:acme-$domain-system-events\\"}" > "$outfile"
      echo '{}'
    fi
    ;;
  "dynamodb update-item")
    domain=$(echo "$*" | grep -o 'acme-[a-z-]*-system-events' | head -1 | sed 's/^acme-//; s/-system-events$//')
    if [ -f "$MOCK_DIR/fail-first-update" ]; then
      rm "$MOCK_DIR/fail-first-update"
      echo "update-fail $domain" >> "$MOCK_LOG"
      exit 1
    fi
    echo "update $domain" >> "$MOCK_LOG"
    echo '{}'
    ;;
  *)
    echo "unexpected aws call: $*" >&2
    exit 1
    ;;
esac
"""

DOMAIN = "arn:aws:sns:eu-west-1:999988887777:acme-{}-system-events"


def item(name, status, with_topic=False):
    entry = {
        "domainName": {"S": name},
        "status": {"S": status},
        "ownerAccountId": {"S": "111122223333"},
        "ownerEmail": {"S": f"{name}@example.com"},
        "costCenter": {"S": "cc-1"},
    }
    if with_topic:
        entry["systemEventsTopicArn"] = {"S": DOMAIN.format(name)}
    return entry


class BackfillCase(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        root = Path(self.tmp.name)
        self.mock_dir = root / "mock"
        self.mock_dir.mkdir()
        self.log = root / "calls.log"
        self.log.touch()
        bin_dir = root / "bin"
        bin_dir.mkdir()
        stub = bin_dir / "aws"
        stub.write_text(AWS_STUB)
        stub.chmod(stub.stat().st_mode | stat.S_IEXEC)
        self.env = {
            **os.environ,
            "PATH": f"{bin_dir}:{os.environ['PATH']}",
            "AWS_REGION": "eu-west-1",
            "MOCK_DIR": str(self.mock_dir),
            "MOCK_LOG": str(self.log),
        }

    def tearDown(self):
        self.tmp.cleanup()

    def write_pages(self, page1, page2=None):
        self._write("scan-page1.json", {"Items": page1, "NextToken": "page2" if page2 is not None else None})
        self._write("scan-page2.json", {"Items": page2 or []})

    def _write(self, name, doc):
        if "NextToken" in doc and doc["NextToken"] is None:
            del doc["NextToken"]
        (self.mock_dir / name).write_text(json.dumps(doc))

    def run_script(self, **extra_env):
        return subprocess.run(
            ["bash", str(SCRIPT)],
            env={**self.env, **extra_env},
            capture_output=True,
            text=True,
        )

    def operations(self):
        return [
            line.split(" ", 1)
            for line in self.log.read_text().splitlines()
            if line.strip()
        ]

    def updates(self):
        return [rest for op, rest in self.operations() if op == "update"]

    def invokes(self):
        return [rest for op, rest in self.operations() if op == "invoke"]


class TestBackfillSystemEventTopics(BackfillCase):
    def test_selects_active_and_inactive_without_topic_and_paginates(self):
        self.write_pages(
            [
                item("alpha", "ACTIVE"),
                item("gamma", "PROVISIONING"),
                item("delta", "ACTIVE", with_topic=True),
            ],
            [item("beta", "INACTIVE")],
        )

        result = self.run_script()

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("Found 2 eligible", result.stdout)
        self.assertEqual(sorted(self.updates()), ["alpha", "beta"])
        self.assertEqual(sorted(self.invokes()), ["alpha", "beta"])
        scans = [rest for op, rest in self.operations() if op == "scan"]
        self.assertEqual(scans, ["scan-page1.json", "scan-page2.json"])

    def test_ensure_payload_carries_the_owner_email(self):
        self.write_pages([item("alpha", "ACTIVE")])

        result = self.run_script()

        self.assertEqual(result.returncode, 0, result.stderr)
        payload = json.loads((self.mock_dir / "payload-alpha.json").read_text())
        self.assertEqual(payload["action"], "ensure")
        self.assertEqual(payload["ownerEmail"], "alpha@example.com")

    def test_domain_without_owner_email_fails_visibly_without_subscribing(self):
        broken = item("broken", "ACTIVE")
        del broken["ownerEmail"]
        self.write_pages([broken, item("alpha", "ACTIVE")])

        result = self.run_script()

        self.assertEqual(result.returncode, 1)
        self.assertIn("broken: FAILED (record has no ownerEmail", result.stderr)
        self.assertEqual(self.invokes(), ["alpha"])
        self.assertEqual(self.updates(), ["alpha"])

    def test_dry_run_performs_zero_writes(self):
        self.write_pages([item("alpha", "ACTIVE")])

        result = self.run_script(DRY_RUN="1")

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("DRY RUN", result.stdout)
        self.assertEqual(self.invokes(), [])
        self.assertEqual(self.updates(), [])

    def test_per_domain_failure_is_reported_and_others_continue(self):
        self.write_pages([item("alpha", "ACTIVE")], [item("beta", "INACTIVE")])
        (self.mock_dir / "failing-domains").write_text("beta\n")

        result = self.run_script()

        self.assertEqual(result.returncode, 1)
        self.assertIn("beta: FAILED", result.stderr)
        self.assertEqual(self.updates(), ["alpha"])
        self.assertEqual(sorted(self.invokes()), ["alpha", "beta"])

    def test_domain_selection_limits_scope(self):
        self.write_pages([item("alpha", "ACTIVE"), item("beta", "ACTIVE")])

        result = self.run_script(DOMAINS="beta")

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.updates(), ["beta"])
        self.assertEqual(self.invokes(), ["beta"])

    def test_interrupted_run_converges_on_rerun(self):
        # First run: the topic is ensured but the record write fails, as if
        # the operator interrupted between the two. The rerun must re-verify
        # the topic (idempotent ensure) and complete the write.
        self.write_pages([item("alpha", "ACTIVE")])
        (self.mock_dir / "fail-first-update").touch()

        first = self.run_script()
        self.assertEqual(first.returncode, 1)
        self.assertEqual(self.invokes(), ["alpha"])
        self.assertEqual(self.updates(), [])

        second = self.run_script()
        self.assertEqual(second.returncode, 0, second.stderr)
        self.assertEqual(self.invokes(), ["alpha", "alpha"])
        self.assertEqual(self.updates(), ["alpha"])


if __name__ == "__main__":
    unittest.main()
