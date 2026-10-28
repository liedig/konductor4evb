#!/usr/bin/env python3
"""Drift tests for the repository's GitHub configuration files.

Run: `uv run --with pytest --with pyyaml pytest scripts/tests/test_repository_config.py`.
Labels, issue forms, rulesets, and workflows reference each other by name, and
GitHub silently accepts a reference to something that doesn't exist (a label
it then invents, a required check that never reports). These tests catch that
locally.
"""

import itertools
import json
import re
import unittest
from pathlib import Path

import yaml

REPO_ROOT = Path(__file__).resolve().parents[2]
GITHUB_DIR = REPO_ROOT / ".github"

AREA_LABELS = [
    "area:domain-mgmt",
    "area:schema-mgmt",
    "area:event-streams",
    "area:control-plane",
    "area:notifications",
    "area:trust",
    "area:web",
    "area:sdk",
    "area:infra",
    "area:ci",
    "area:deps",
    "area:docs",
]


def load_yaml(path):
    return yaml.safe_load(path.read_text())


def documentation_site_url():
    return re.search(r"^site_url:\s*(\S+)", (REPO_ROOT / "mkdocs.yml").read_text(), re.MULTILINE).group(1)


def label_definitions():
    return load_yaml(GITHUB_DIR / "labels.yml")


def defined_label_names():
    return {label["name"] for label in label_definitions()}


class LabelSetTest(unittest.TestCase):
    def test_label_names_are_unique(self):
        names = [label["name"] for label in label_definitions()]

        self.assertEqual(len(names), len(set(names)))

    def test_every_label_has_a_color_and_description(self):
        for label in label_definitions():
            with self.subTest(label=label["name"]):
                self.assertRegex(label["color"], r"^[0-9a-f]{6}$")
                self.assertTrue(label["description"].strip())

    def test_area_labels_cover_the_codebase(self):
        defined_areas = {name for name in defined_label_names() if name.startswith("area:")}

        self.assertEqual(defined_areas, set(AREA_LABELS))

    def test_question_label_is_defined(self):
        self.assertIn("question", defined_label_names())

    def test_header_describes_applying_labels_with_gh(self):
        header = (GITHUB_DIR / "labels.yml").read_text().split("\n- name:", 1)[0]

        self.assertIn("gh label create --force", header)
        self.assertNotIn("sync-labels", header)


class DependabotLabelsTest(unittest.TestCase):
    def setUp(self):
        self.updates = load_yaml(GITHUB_DIR / "dependabot.yml")["updates"]

    def test_every_dependabot_label_is_defined(self):
        defined = defined_label_names()

        for update in self.updates:
            with self.subTest(ecosystem=update["package-ecosystem"]):
                self.assertLessEqual(set(update["labels"]), defined)

    def test_dependency_updates_are_chores_labelled_by_area(self):
        for update in self.updates:
            ecosystem = update["package-ecosystem"]
            area = "area:ci" if ecosystem == "github-actions" else "area:deps"
            with self.subTest(ecosystem=ecosystem):
                self.assertEqual(update["labels"], ["type:chore", area])


class IssueChooserTest(unittest.TestCase):
    def setUp(self):
        self.config = load_yaml(GITHUB_DIR / "ISSUE_TEMPLATE" / "config.yml")
        self.links = self.config.get("contact_links", [])

    def test_blank_issues_are_disabled(self):
        self.assertIs(self.config["blank_issues_enabled"], False)

    def test_vulnerabilities_are_routed_to_private_reporting(self):
        security_links = [link for link in self.links if link["url"].endswith("/security/advisories/new")]

        self.assertEqual(len(security_links), 1)
        self.assertRegex(security_links[0]["about"], r"(?i)not .*public")

    def test_readers_are_routed_to_the_documentation_site(self):
        self.assertIn(documentation_site_url(), [link["url"] for link in self.links])


FORM_TYPE_LABELS = {
    "bug_report.yml": "type:bug",
    "feature_request.yml": "type:feature",
    "documentation.yml": "type:docs",
    "question.yml": "question",
}
FORM_ITEM_TYPES = {"markdown", "textarea", "input", "dropdown", "checkboxes"}
AREA_OPTION = re.compile(r"^[A-Z][^()]+ \((area:[a-z-]+)\)$")


def load_form(file_name):
    return load_yaml(GITHUB_DIR / "ISSUE_TEMPLATE" / file_name)


def form_fields(form):
    return {item["id"]: item for item in form["body"] if item["type"] != "markdown"}


def is_required(field):
    return field.get("validations", {}).get("required", False)


class IssueFormsTest(unittest.TestCase):
    def test_issue_template_directory_holds_exactly_the_four_forms(self):
        templates = {path.name for path in (GITHUB_DIR / "ISSUE_TEMPLATE").glob("*.yml")}

        self.assertEqual(templates, set(FORM_TYPE_LABELS) | {"config.yml"})

    def test_each_form_is_a_valid_issue_form(self):
        for file_name in FORM_TYPE_LABELS:
            form = load_form(file_name)
            with self.subTest(form=file_name):
                self.assertTrue(form["name"].strip())
                self.assertTrue(form["description"].strip())
                ids = [item["id"] for item in form["body"] if "id" in item]
                self.assertEqual(len(ids), len(set(ids)))
                for item in form["body"]:
                    self.assertIn(item["type"], FORM_ITEM_TYPES)
                    if item["type"] == "markdown":
                        self.assertTrue(item["attributes"]["value"].strip())
                        continue
                    self.assertRegex(item["id"], r"^[a-z][a-z0-9_-]*$")
                    self.assertTrue(item["attributes"]["label"].strip())
                    if item["type"] == "dropdown":
                        self.assertTrue(item["attributes"]["options"])

    def test_each_form_applies_its_type_and_triage_labels(self):
        defined = defined_label_names()

        for file_name, type_label in FORM_TYPE_LABELS.items():
            labels = load_form(file_name)["labels"]
            with self.subTest(form=file_name):
                self.assertEqual(sorted(labels), sorted([type_label, "status:needs-triage"]))
                self.assertLessEqual(set(labels), defined)

    def test_no_form_asks_for_a_priority(self):
        for file_name in FORM_TYPE_LABELS:
            with self.subTest(form=file_name):
                for field_id, field in form_fields(load_form(file_name)).items():
                    self.assertNotIn("priority", field_id.lower())
                    self.assertNotIn("priority", field["attributes"]["label"].lower())

    def test_component_dropdowns_list_exactly_the_area_labels(self):
        for file_name in FORM_TYPE_LABELS:
            component = form_fields(load_form(file_name))["component"]
            with self.subTest(form=file_name):
                self.assertEqual(component["type"], "dropdown")
                options = component["attributes"]["options"]
                matches = [AREA_OPTION.match(option) for option in options]
                self.assertTrue(all(matches), options)
                self.assertEqual([match.group(1) for match in matches], AREA_LABELS)

    def assert_fields(self, file_name, required, optional=()):
        fields = form_fields(load_form(file_name))
        for field_id in required:
            with self.subTest(form=file_name, field=field_id):
                self.assertIn(field_id, fields)
                self.assertTrue(is_required(fields[field_id]))
        for field_id in optional:
            with self.subTest(form=file_name, field=field_id):
                self.assertIn(field_id, fields)
                self.assertFalse(is_required(fields[field_id]))
        return fields

    def test_bug_form_collects_diagnostics(self):
        fields = self.assert_fields(
            "bug_report.yml",
            required=["description", "steps", "expected", "actual", "component", "version"],
            optional=["region", "logs"],
        )

        self.assertEqual(fields["logs"]["attributes"]["render"], "shell")

    def test_bug_form_warns_reporters_to_redact(self):
        markdown = " ".join(
            item["attributes"]["value"] for item in load_form("bug_report.yml")["body"] if item["type"] == "markdown"
        )

        for term in ("account IDs", "tokens", "email addresses", "secrets"):
            with self.subTest(term=term):
                self.assertIn(term, markdown)

    def test_feature_form_collects_motivation(self):
        self.assert_fields(
            "feature_request.yml",
            required=["description", "motivation", "component"],
            optional=["alternatives"],
        )

    def test_documentation_form_collects_page_and_problem(self):
        self.assert_fields("documentation.yml", required=["page", "problem"])

    def test_question_form_collects_goal_and_attempts(self):
        self.assert_fields("question.yml", required=["goal", "tried"])



class PullRequestTemplateTest(unittest.TestCase):
    def test_label_guidance_lists_every_area_label_in_order(self):
        template = (GITHUB_DIR / "pull_request_template.md").read_text()

        self.assertEqual(re.findall(r"`(area:[a-z-]+)`", template), AREA_LABELS)


MAIN_BRANCH_CONDITION = {"ref_name": {"include": ["refs/heads/main"], "exclude": []}}
GITHUB_ACTIONS_APP_ID = 15368


def load_ruleset(name):
    return json.loads((GITHUB_DIR / "rulesets" / f"{name}.json").read_text())


def rules_by_type(ruleset):
    return {rule["type"]: rule.get("parameters", {}) for rule in ruleset["rules"]}


def load_workflow(file_name):
    return load_yaml(GITHUB_DIR / "workflows" / file_name)


def workflow_triggers(workflow):
    # PyYAML reads the bare key `on` as the boolean True.
    return workflow.get("on", workflow.get(True))


def job_check_names(job_id, job):
    name = job.get("name", job_id)
    matrix = job.get("strategy", {}).get("matrix")
    if not matrix:
        return {name}
    if not isinstance(matrix, dict) or "${{" in name or {"include", "exclude"} & matrix.keys():
        raise ValueError(f"{job_id}: only static names over a plain list matrix are supported")
    # GitHub appends the combination's values to a matrix job's static name.
    return {f"{name} ({', '.join(map(str, values))})" for values in itertools.product(*matrix.values())}


def workflow_check_names(workflow):
    return set().union(*(job_check_names(job_id, job) for job_id, job in workflow["jobs"].items()))


def is_path_filtered(workflow):
    triggers = workflow_triggers(workflow)
    if not isinstance(triggers, dict):
        return False
    return any(
        isinstance(trigger, dict) and ({"paths", "paths-ignore"} & trigger.keys())
        for trigger in triggers.values()
    )


class WorkflowCheckNamesTest(unittest.TestCase):
    def test_plain_jobs_report_their_name_or_id(self):
        workflow = {"jobs": {"check": {"name": "Check"}, "lint": {}}}

        self.assertEqual(workflow_check_names(workflow), {"Check", "lint"})

    def test_matrix_jobs_report_one_check_per_combination(self):
        workflow = {"jobs": {"sdk": {"name": "Python SDK", "strategy": {"matrix": {"python-version": ["3.11", "3.14"]}}}}}

        self.assertEqual(workflow_check_names(workflow), {"Python SDK (3.11)", "Python SDK (3.14)"})

    def test_multi_axis_matrix_names_list_values_in_key_order(self):
        workflow = {"jobs": {"t": {"name": "T", "strategy": {"matrix": {"os": ["a", "b"], "py": ["1"]}}}}}

        self.assertEqual(workflow_check_names(workflow), {"T (a, 1)", "T (b, 1)"})

    def test_unsupported_matrix_shapes_fail_loudly(self):
        for job in (
            {"name": "T (${{ matrix.py }})", "strategy": {"matrix": {"py": ["1"]}}},
            {"name": "T", "strategy": {"matrix": {"py": ["1"], "include": [{"py": "2"}]}}},
        ):
            with self.subTest(job=job), self.assertRaises(ValueError):
                workflow_check_names({"jobs": {"t": job}})


class MainReviewRulesetTest(unittest.TestCase):
    def setUp(self):
        self.ruleset = load_ruleset("main-review")

    def test_targets_main(self):
        self.assertEqual(self.ruleset["name"], "main-review")
        self.assertEqual(self.ruleset["target"], "branch")
        self.assertEqual(self.ruleset["enforcement"], "active")
        self.assertEqual(self.ruleset["conditions"], MAIN_BRANCH_CONDITION)

    def test_requires_code_owner_approved_squash_merges(self):
        rules = rules_by_type(self.ruleset)

        self.assertEqual(set(rules), {"pull_request"})
        review = rules["pull_request"]
        self.assertEqual(review["required_approving_review_count"], 1)
        self.assertIs(review["require_code_owner_review"], True)
        self.assertIs(review["dismiss_stale_reviews_on_push"], True)
        self.assertIs(review["required_review_thread_resolution"], True)
        self.assertEqual(review["allowed_merge_methods"], ["squash"])

    def test_only_repository_admins_bypass_and_only_when_merging(self):
        self.assertEqual(
            self.ruleset["bypass_actors"],
            [{"actor_id": 5, "actor_type": "RepositoryRole", "bypass_mode": "pull_request"}],
        )


class MainIntegrityRulesetTest(unittest.TestCase):
    def setUp(self):
        self.ruleset = load_ruleset("main-integrity")
        self.rules = rules_by_type(self.ruleset)
        self.checks = self.rules["required_status_checks"]["required_status_checks"]

    def test_targets_main_without_bypass(self):
        self.assertEqual(self.ruleset["name"], "main-integrity")
        self.assertEqual(self.ruleset["target"], "branch")
        self.assertEqual(self.ruleset["enforcement"], "active")
        self.assertEqual(self.ruleset["conditions"], MAIN_BRANCH_CONDITION)
        self.assertEqual(self.ruleset["bypass_actors"], [])

    def test_keeps_history_linear_and_undeletable(self):
        self.assertEqual(
            set(self.rules),
            {"required_status_checks", "required_linear_history", "non_fast_forward", "deletion"},
        )

    def test_required_checks_are_the_ci_jobs(self):
        ci = load_workflow("ci.yml")

        self.assertFalse(is_path_filtered(ci))
        self.assertEqual(sorted(check["context"] for check in self.checks), sorted(workflow_check_names(ci)))

    def test_required_checks_come_from_github_actions(self):
        for check in self.checks:
            with self.subTest(check=check["context"]):
                self.assertEqual(check["integration_id"], GITHUB_ACTIONS_APP_ID)

    def test_checks_that_may_not_report_are_not_required(self):
        may_not_report = set(workflow_check_names(load_workflow("droid-review.yml")))
        for path in (GITHUB_DIR / "workflows").glob("*.yml"):
            workflow = load_yaml(path)
            if is_path_filtered(workflow):
                may_not_report |= workflow_check_names(workflow)

        self.assertFalse({check["context"] for check in self.checks} & may_not_report)

    def test_branches_need_not_be_up_to_date(self):
        self.assertIs(self.rules["required_status_checks"]["strict_required_status_checks_policy"], False)


if __name__ == "__main__":
    unittest.main()
