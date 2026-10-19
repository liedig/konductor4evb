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


if __name__ == "__main__":
    unittest.main()
