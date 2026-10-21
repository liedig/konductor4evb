#!/usr/bin/env python3
"""Structural checks for ASL state machine definitions (run: `python3 scripts/tests/test_state_machines.py`).

No ASL linter is available locally and the scripts directory stays
stdlib-only, so this parses the regular 2-space-indented structure of the
hand-maintained definitions with a small indentation scanner. It catches the
failure classes that break deployments: dangling state references,
unreachable states, non-terminal Map processor states, and template
substitutions that drift from the placeholders the definition uses.
"""
import re
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent.parent

MACHINES = sorted(REPO.glob("event-streams/state-machines/*.asl.yaml")) + sorted(
    REPO.glob("domain-management/state-machines/*.asl.yaml")
)

NAME_RE = re.compile(r"^([A-Za-z][A-Za-z0-9_-]*):\s*$")
REF_RE = re.compile(r"^(Next|Default):\s*([A-Za-z][A-Za-z0-9_-]*)\s*$")
PROP_RE = re.compile(r"^([A-Za-z][A-Za-z0-9_-]*):")


def parse_machine(path):
    """Return (states, processors) where states maps name -> {refs, terminal}
    and processors maps outer state -> (inner states, inner refs)."""
    states = {}
    processors = {}
    current = None
    processor = None  # (outer name, inner states dict, inner current)
    inner_current = None
    for raw in path.read_text().splitlines():
        if not raw.strip() or raw.lstrip().startswith("#"):
            continue
        indent = len(raw) - len(raw.lstrip(" "))
        text = raw.strip()
        if indent == 0:
            if text.startswith("StartAt:"):
                states.setdefault("__start__", text.split(":", 1)[1].strip())
            current = None
            processor = None
            continue
        if processor is not None:
            outer, inner = processors[processor]
            if indent <= 4:
                processor = None
                inner_current = None
            else:
                if indent == 6 and text.startswith("StartAt:"):
                    inner.setdefault("__start__", text.split(":", 1)[1].strip())
                    continue
                if indent == 8 and NAME_RE.match(text):
                    inner_current = NAME_RE.match(text).group(1)
                    inner.setdefault(inner_current, {"refs": [], "terminal": False})
                    continue
                if indent >= 10 and inner_current:
                    ref = REF_RE.match(text)
                    if ref:
                        inner[inner_current]["refs"].append(ref.group(2))
                    if text.startswith("End: true"):
                        inner[inner_current]["terminal"] = True
                continue
        if indent == 2 and NAME_RE.match(text):
            current = NAME_RE.match(text).group(1)
            states.setdefault(current, {"refs": [], "terminal": False})
            continue
        if current is None:
            continue
        if indent == 4 and text.startswith("ItemProcessor:"):
            processors[current] = (current, {})
            processor = current
            continue
        if indent >= 4:
            ref = REF_RE.match(text)
            if ref:
                states[current]["refs"].append(ref.group(2))
            if indent == 4:
                if text.startswith("End: true"):
                    states[current]["terminal"] = True
                elif text.startswith("Type:") and text.split(":", 1)[1].strip() in (
                    "Succeed",
                    "Fail",
                ):
                    states[current]["terminal"] = True
    return states, processors


class TestStateMachines(unittest.TestCase):
    def test_state_targets_resolve_and_reach(self):
        self.assertTrue(MACHINES, "no state machine definitions found")
        for path in MACHINES:
            with self.subTest(machine=path.name):
                states, _ = parse_machine(path)
                start = states.pop("__start__")
                self.assertIn(start, states, f"StartAt {start} undefined")
                referenced = {start}
                for name, state in states.items():
                    for target in state["refs"]:
                        self.assertIn(target, states, f"{name} -> {target} undefined")
                        referenced.add(target)
                self.assertEqual(
                    referenced, set(states), "unreachable states (or missed by parse)"
                )
                self.assertTrue(
                    any(s["terminal"] for s in states.values()), "no terminal state"
                )

    def test_map_processors_terminate(self):
        for path in MACHINES:
            states, processors = parse_machine(path)
            for outer, (_, inner) in processors.items():
                with self.subTest(machine=path.name, processor=outer):
                    start = inner.pop("__start__", None)
                    self.assertIsNotNone(start, f"{outer}'s processor has no StartAt")
                    self.assertIn(start, inner, f"{start} undefined in {outer}")
                    referenced = {start}
                    for name, state in inner.items():
                        for target in state["refs"]:
                            self.assertIn(
                                target, inner, f"{outer}.{name} -> {target} undefined"
                            )
                            referenced.add(target)
                    self.assertEqual(referenced, set(inner), "unreachable inner states")
                    self.assertTrue(
                        any(s["terminal"] for s in inner.values()),
                        f"{outer}'s processor never ends",
                    )

    def test_no_yaml_anchors_or_aliases(self):
        # Step Functions' YAML parser does not expand anchors: an aliased
        # Retry arrives as a scalar and fails validation ("Expected ARRAY").
        alias = re.compile(r":\s*[*&][A-Za-z]")
        for path in MACHINES:
            with self.subTest(machine=path.name):
                offenders = [
                    line.strip()
                    for line in path.read_text().splitlines()
                    if alias.search(line)
                ]
                self.assertEqual(offenders, [], "YAML anchors/aliases are not expanded")

    def test_catchers_transition_with_next(self):
        # A Catcher supports Next, not End; "End: true" inside a Catch block
        # is rejected by the service's schema validation.
        for path in MACHINES:
            with self.subTest(machine=path.name):
                catch_indent = None
                for raw in path.read_text().splitlines():
                    if not raw.strip() or raw.lstrip().startswith("#"):
                        continue
                    indent = len(raw) - len(raw.lstrip(" "))
                    text = raw.strip()
                    if catch_indent is not None:
                        if indent <= catch_indent:
                            catch_indent = None
                        elif text.startswith("End:"):
                            self.fail(f"Catch entry uses End (use Next): {raw.strip()}")
                    if text == "Catch:":
                        catch_indent = indent

    def test_inner_state_names_unique_across_maps(self):
        # The service rejects duplicate state names even across separate Map
        # processors of the same machine (DUPLICATE_STATE_NAME).
        for path in MACHINES:
            with self.subTest(machine=path.name):
                _, processors = parse_machine(path)
                seen = {}
                for outer, (_, inner) in processors.items():
                    for name in inner:
                        if name == "__start__":
                            continue
                        self.assertNotIn(
                            name, seen, f"{name} in {outer} duplicates {seen.get(name)}"
                        )
                        seen[name] = outer

    def test_substitutions_match_template(self):
        template = (REPO / "konductor-core-stack.yaml").read_text()
        for path in MACHINES:
            with self.subTest(machine=path.name):
                used = set(re.findall(r"\$\{([A-Za-z0-9]+)\}", path.read_text()))
                block = re.search(
                    r"DefinitionUri: \./\S*" + re.escape(path.name)
                    + r"\n\s+DefinitionSubstitutions:\n((?:\s{8,}\S.*\n)+)",
                    template,
                )
                self.assertIsNotNone(block, f"{path.name} not wired in template")
                provided = set(
                    re.findall(r"^\s{8,}([A-Za-z0-9]+):", block.group(1), re.M)
                )
                self.assertEqual(used, provided, "used vs provided mismatch")


REPIN_MACHINE = REPO / "event-streams/state-machines/repin-schema-env.asl.yaml"
PROMOTE_MACHINE = REPO / "event-streams/state-machines/promote-schema.asl.yaml"
UPGRADE_PASSTHROUGH = 'upgrade: "{% $states.context.Map.Item.Value.upgrade %}"'


class TestUpgradeFactPropagation(unittest.TestCase):
    """Both re-pin workflows forward the per-entry upgrade facts the
    materialize step needs for its phase commits; without the passthrough
    the emission gate silently emits nothing."""

    def test_repin_schema_env_propagates_upgrade_facts(self):
        text = REPIN_MACHINE.read_text()
        self.assertIn('trigger: "AUTHORING"', text)
        self.assertEqual(text.count(UPGRADE_PASSTHROUGH), 2)
        self.assertIn('requestedVersion: "{% $states.input.version %}"', text)
        self.assertIn('startMetadataRevision: "{% $widen.metadataRevision %}"', text)

    def test_promote_schema_propagates_upgrade_facts(self):
        text = PROMOTE_MACHINE.read_text()
        self.assertIn('trigger: "PROMOTION"', text)
        self.assertEqual(text.count(UPGRADE_PASSTHROUGH), 2)
        self.assertIn('requestedVersion: "{% $states.input.requestedVersion %}"', text)
        self.assertIn('startMetadataRevision: "{% $widen.metadataRevision %}"', text)



PROVISION_DOMAIN_MACHINE = REPO / "domain-management/state-machines/provision-domain.asl.yaml"
CORE_TEMPLATE = REPO / "konductor-core-stack.yaml"


class TestSystemTopicLifecycle(unittest.TestCase):
    """The provisioning workflow configures the domain's system-events topic
    once before environment provisioning, records the ARN only in the ACTIVE
    commit, and cleans up the owned topic on every failure path."""

    def setUp(self):
        self.text = PROVISION_DOMAIN_MACHINE.read_text()
        self.states, _ = parse_machine(PROVISION_DOMAIN_MACHINE)

    def test_topic_is_ensured_before_environment_provisioning(self):
        self.assertIn("EnsureSystemTopic", self.states)
        self.assertIn("EnsureRAMResourceShare", self.states["EnsureSystemTopic"]["refs"])
        self.assertIn("EnsureSystemTopic", self.states["TagNamespaceParameter"]["refs"])
        for field in [
            "domainName",
            "ownerAccountId",
            "environmentAccounts",
            "costCenter",
            "ownerEmail",
        ]:
            self.assertRegex(self.text, rf"        {field}: ", f"{field} passed to ensure")

    def test_active_commit_records_the_topic_arn(self):
        self.assertIn("systemEventsTopicArn = :topic", self.text)
        self.assertIn('S: "{% $systemEventsTopicArn %}"', self.text)

    def test_domain_level_failures_cleanup_the_topic(self):
        self.assertIn("DeleteSystemTopicDomainLevel", self.states)
        for source in [
            "CreateNamespaceParameter",
            "TagNamespaceParameter",
            "EnsureSystemTopic",
            "EnsureRAMResourceShare",
            "CreateRAMResourceShare",
            "AssociateParameterToExistingShare",
        ]:
            self.assertIn(
                "DeleteSystemTopicDomainLevel",
                self.states[source]["refs"],
                f"{source} failure must route through topic cleanup",
            )
        self.assertIn(
            "EmitProvisioningFailedDomainLevel",
            self.states["DeleteSystemTopicDomainLevel"]["refs"],
        )

    def test_rollback_path_cleans_up_and_reports_compensation_errors(self):
        self.assertIn(
            "DeleteSystemTopicAfterRollback", self.states["RollbackSucceededEnvs"]["refs"]
        )
        self.assertIn(
            "NoteSystemTopicCleanupFailure",
            self.states["DeleteSystemTopicAfterRollback"]["refs"],
            "a failed cleanup still emits the failure event with a recorded error",
        )
        self.assertIn("System topic cleanup failed: ", self.text)

    def test_delete_uses_the_deterministic_managed_topic_arn(self):
        count = self.text.count(
            "topicArn: \"{% 'arn:${AWS::Partition}:sns:${AWS::Region}:${AWS::AccountId}:'"
        )
        self.assertEqual(count, 2, "both cleanup states compute the deterministic ARN")

    def test_template_wires_manager_function_and_invoke_permission(self):
        template = CORE_TEMPLATE.read_text()
        self.assertIn("SystemTopicManagerFunction:", template)
        self.assertIn("konductor-system-topic-manager", template)
        self.assertIn(
            "SystemTopicManagerFunctionArn: !GetAtt SystemTopicManagerFunction.Arn",
            template,
        )
        self.assertIn("lambda:InvokeFunction", template)
        self.assertIn("${Organization}-*-system-events", template)
        manager = re.search(
            r"SystemTopicManagerFunction:.*?(?=\n  [A-Za-z])", template, re.S
        ).group(0)
        for action in [
            "sns:CreateTopic",
            "sns:DeleteTopic",
            "sns:SetTopicAttributes",
            "sns:ListTagsForResource",
            "sns:TagResource",
            "sns:UntagResource",
        ]:
            self.assertIn(action, manager)



class TestSystemTopicRetagAndTeardown(unittest.TestCase):
    """Retag covers the recorded domain topic, disable retains it, and only
    explicit ownership-checked teardown removes it."""

    def test_retag_skips_domains_without_a_recorded_topic(self):
        text = (REPO / "domain-management/state-machines/retag-domain.asl.yaml").read_text()
        self.assertIn('Condition: "{% $exists($domain.systemEventsTopicArn) %}"', text)
        self.assertIn('Resource: "arn:aws:states:::aws-sdk:sns:tagResource"', text)
        self.assertIn('ResourceArn: "{% $domain.systemEventsTopicArn.S %}"', text)
        self.assertIn('{"Key": "cost-center", "Value": $domain.costCenter.S}', text)

    def test_per_environment_cleanup_never_touches_the_domain_topic(self):
        deprovision = (REPO / "domain-management/state-machines/deprovision-env.asl.yaml").read_text()
        self.assertNotIn("sns:", deprovision)
        self.assertNotIn("system-events", deprovision)

    def test_teardown_deletes_the_topic_through_the_ownership_checked_manager(self):
        script = (REPO / "scripts/teardown-domain.sh").read_text()
        self.assertIn("systemEventsTopicArn.S", script)
        self.assertIn("konductor-system-topic-manager", script)
        self.assertIn('"delete"', script.replace("'", '"'))
        self.assertNotIn("aws sns delete-topic", script)

if __name__ == "__main__":
    unittest.main()
