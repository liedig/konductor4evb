#!/usr/bin/env python3
"""Teardown script behavior tests (run: `python3 scripts/tests/test_teardown_domain.py`).

Runs scripts/teardown-domain.sh with a mocked `aws` CLI and a no-op `sleep` on
PATH (the scripts directory stays stdlib-only). The stub serves a canned domain
record and approval execution status, fails the namespace parameter delete or
the execution stop on request, and records every call to a log so tests can
assert whether the domain record was deleted.
"""
import json
import os
import stat
import subprocess
import tempfile
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent.parent
SCRIPT = REPO / "scripts/teardown-domain.sh"

AWS_STUB = """#!/usr/bin/env bash
set -euo pipefail
echo "$1 $2" >> "$MOCK_LOG"
case "$1 $2" in
  "dynamodb get-item")
    cat "$MOCK_DIR/item.json"
    ;;
  "cloudformation describe-stacks")
    echo "arn:aws:states:eu-west-1:123456789012:stateMachine:konductor-deprovision-env"
    ;;
  "cloudformation describe-stack-resource")
    case "$*" in
      *"--logical-resource-id ApprovalWorkflow"*)
        echo "arn:aws:states:eu-west-1:123456789012:stateMachine:konductor-domain-approval"
        ;;
      *)
        echo "unexpected stack resource: $*" >&2
        exit 1
        ;;
    esac
    ;;
  "stepfunctions stop-execution")
    echo "$*" >> "$MOCK_DIR/stop-execution.args"
    if [ -f "$MOCK_DIR/stop-error" ]; then
      cat "$MOCK_DIR/stop-error" >&2
      exit 254
    fi
    echo '{"stopDate": "2026-10-26T00:00:00+00:00"}'
    ;;
  "stepfunctions describe-execution")
    echo "$*" >> "$MOCK_DIR/describe-execution.args"
    cat "$MOCK_DIR/execution-status"
    ;;
  "ssm delete-parameter")
    if [ -f "$MOCK_DIR/ssm-error" ]; then
      cat "$MOCK_DIR/ssm-error" >&2
      exit 254
    fi
    echo '{}'
    ;;
  "dynamodb delete-item")
    echo '{}'
    ;;
  *)
    echo "unexpected aws call: $*" >&2
    exit 1
    ;;
esac
"""

PARAMETER_NOT_FOUND = (
    "An error occurred (ParameterNotFound) when calling the DeleteParameter operation: \n"
)
ACCESS_DENIED = (
    "An error occurred (AccessDeniedException) when calling the DeleteParameter "
    "operation: not authorized to perform ssm:DeleteParameter\n"
)
APPROVAL_EXECUTION_ARN = (
    "arn:aws:states:eu-west-1:123456789012:execution:"
    "konductor-domain-approval:approval-req-123"
)
EXECUTION_DOES_NOT_EXIST = (
    "An error occurred (ExecutionDoesNotExist) when calling the StopExecution "
    f"operation: Execution Does Not Exist: '{APPROVAL_EXECUTION_ARN}'\n"
)
STOP_ACCESS_DENIED = (
    "An error occurred (AccessDeniedException) when calling the StopExecution "
    "operation: not authorized to perform states:StopExecution\n"
)


class TeardownCase(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        root = Path(self.tmp.name)
        self.mock_dir = root / "mock"
        self.mock_dir.mkdir()
        self.log = root / "calls.log"
        self.log.touch()
        bin_dir = root / "bin"
        bin_dir.mkdir()
        for name, body in (("aws", AWS_STUB), ("sleep", "#!/usr/bin/env bash\nexit 0\n")):
            stub = bin_dir / name
            stub.write_text(body)
            stub.chmod(stub.stat().st_mode | stat.S_IEXEC)
        self.env = {
            **os.environ,
            "PATH": f"{bin_dir}:{os.environ['PATH']}",
            "AWS_REGION": "eu-west-1",
            "DOMAIN_NAME": "smoke",
            "MOCK_DIR": str(self.mock_dir),
            "MOCK_LOG": str(self.log),
        }
        self.write_never_provisioned_record()
        self.report_execution_status("ABORTED")

    def tearDown(self):
        self.tmp.cleanup()

    def write_record(self, **attributes):
        record = {
            "Item": {
                "domainName": {"S": "smoke"},
                "ownerAccountId": {"S": "111122223333"},
                **attributes,
            }
        }
        (self.mock_dir / "item.json").write_text(json.dumps(record))

    def write_never_provisioned_record(self):
        self.write_record(status={"S": "REJECTED"})

    def write_pending_record(self, request_id="req-123"):
        attributes = {"status": {"S": "PENDING_APPROVAL"}}
        if request_id:
            attributes["requestId"] = {"S": request_id}
        self.write_record(**attributes)

    def report_execution_status(self, status):
        (self.mock_dir / "execution-status").write_text(f"{status}\n")

    def fail_parameter_delete_with(self, message):
        (self.mock_dir / "ssm-error").write_text(message)

    def fail_stop_with(self, message):
        (self.mock_dir / "stop-error").write_text(message)

    def execution_arn_sent_to(self, operation):
        args_file = self.mock_dir / f"{operation}.args"
        self.assertTrue(args_file.exists(), f"the script never called {operation}")
        args = args_file.read_text().split()
        return args[args.index("--execution-arn") + 1]

    def run_script(self):
        return subprocess.run(
            ["bash", str(SCRIPT)], env=self.env, capture_output=True, text=True
        )

    def calls(self):
        return [line for line in self.log.read_text().splitlines() if line.strip()]


class TestTeardownNamespaceParameter(TeardownCase):
    def test_existing_parameter_is_deleted_with_the_record(self):
        result = self.run_script()

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("ssm delete-parameter", self.calls())
        self.assertIn("dynamodb delete-item", self.calls())

    def test_missing_parameter_still_deletes_the_record(self):
        self.fail_parameter_delete_with(PARAMETER_NOT_FOUND)

        result = self.run_script()

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(
            "dynamodb delete-item",
            self.calls(),
            "never-provisioned domains and re-runs after a partial teardown "
            "have no parameter left to delete",
        )

    def test_other_parameter_errors_keep_the_record(self):
        self.fail_parameter_delete_with(ACCESS_DENIED)

        result = self.run_script()

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("AccessDeniedException", result.stderr)
        self.assertNotIn(
            "dynamodb delete-item",
            self.calls(),
            "the record must survive so a re-run can finish the teardown",
        )


class TestTeardownApprovalWorkflow(TeardownCase):
    def test_pending_domain_stops_its_approval_workflow_before_the_record_goes(self):
        self.write_pending_record()

        result = self.run_script()

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.execution_arn_sent_to("stop-execution"), APPROVAL_EXECUTION_ARN)
        self.assertEqual(
            self.execution_arn_sent_to("describe-execution"), APPROVAL_EXECUTION_ARN
        )
        calls = self.calls()
        self.assertLess(
            calls.index("stepfunctions describe-execution"),
            calls.index("dynamodb delete-item"),
            "the record goes only after the execution is confirmed stopped",
        )

    def test_execution_still_running_keeps_the_record(self):
        self.write_pending_record()
        self.report_execution_status("RUNNING")

        result = self.run_script()

        self.assertNotEqual(result.returncode, 0)
        self.assertIn(APPROVAL_EXECUTION_ARN, result.stderr)
        self.assertNotIn(
            "dynamodb delete-item",
            self.calls(),
            "a live approval workflow writes the record back when it rejects or times out",
        )

    def test_failed_stop_keeps_the_record(self):
        self.write_pending_record()
        self.fail_stop_with(STOP_ACCESS_DENIED)

        result = self.run_script()

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("AccessDeniedException", result.stderr)
        self.assertNotIn("dynamodb delete-item", self.calls())

    def test_missing_execution_still_deletes_the_record(self):
        self.write_pending_record()
        self.fail_stop_with(EXECUTION_DOES_NOT_EXIST)

        result = self.run_script()

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("stepfunctions stop-execution", self.calls())
        self.assertIn(
            "dynamodb delete-item",
            self.calls(),
            "a registration whose workflow never started has nothing to stop",
        )

    def test_pending_record_without_request_id_is_kept(self):
        self.write_pending_record(request_id=None)

        result = self.run_script()

        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn(
            "dynamodb delete-item",
            self.calls(),
            "without the request id the script cannot find the execution to stop",
        )

    def test_decided_domain_leaves_step_functions_alone(self):
        result = self.run_script()

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            [call for call in self.calls() if call.startswith("stepfunctions")], []
        )


if __name__ == "__main__":
    unittest.main()
