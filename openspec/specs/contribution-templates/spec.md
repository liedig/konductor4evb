# Capability: Contribution Templates

## Purpose

How external contributions enter the project: structured GitHub issue templates for bugs and features that capture diagnostic detail up front, and the three-axis labeling system (priority, type, area, status) used to triage them.

## Requirements

### Requirement: Bug report template

The repository SHALL provide a GitHub issue template for bug reports. The template SHALL prompt the reporter for structured information to aid diagnosis.

#### Scenario: Bug report issue created

- **WHEN** a contributor creates a new issue and selects the bug report template
- **THEN** the issue form SHALL prompt for: description of the bug, steps to reproduce, expected behaviour, actual behaviour, the affected component, and the commit or version in use
- **AND THEN** the form SHALL offer optional fields for the AWS region and for logs rendered as preformatted text
- **AND THEN** the issue SHALL be labelled `type:bug` and `status:needs-triage` automatically

#### Scenario: Reporter is warned to redact

- **WHEN** a contributor fills in the bug report form
- **THEN** the form SHALL instruct them to remove account IDs, tokens, email addresses, and other secrets from logs and descriptions before submitting

#### Scenario: Bug report does not ask for priority

- **WHEN** a contributor fills in the bug report form
- **THEN** the form SHALL NOT ask for a priority
- **AND THEN** a maintainer SHALL set the priority during triage

### Requirement: Feature request template

The repository SHALL provide a GitHub issue template for feature requests. The template SHALL prompt the requester for motivation and a description of the desired behaviour.

#### Scenario: Feature request issue created

- **WHEN** a contributor creates a new issue and selects the feature request template
- **THEN** the issue form SHALL prompt for: description of the feature, motivation (why this is needed), and the affected component
- **AND THEN** the form SHALL offer an optional field for alternatives considered
- **AND THEN** the issue SHALL be labelled `type:feature` and `status:needs-triage` automatically

#### Scenario: Feature request does not ask for priority

- **WHEN** a contributor fills in the feature request form
- **THEN** the form SHALL NOT ask for a priority

### Requirement: Pull request template

The repository SHALL provide a pull request template that prompts contributors for a summary, test plan, and checklist. The template SHALL be automatically populated when a new PR is created.

#### Scenario: PR created with template

- **WHEN** a contributor opens a new pull request
- **THEN** the PR description SHALL be pre-populated with the template containing sections for summary, test plan, and a checklist
- **AND THEN** the contributor SHALL fill in the sections before submitting

### Requirement: Documentation issue template

The repository SHALL provide a GitHub issue template for documentation problems, so readers of the published documentation site can report incorrect, missing, or unclear content.

#### Scenario: Documentation issue created

- **WHEN** a contributor creates a new issue and selects the documentation template
- **THEN** the issue form SHALL prompt for the affected page (URL or path) and what is wrong or missing
- **AND THEN** the issue SHALL be labelled `type:docs` and `status:needs-triage` automatically
- **AND THEN** the form SHALL NOT ask for a priority

### Requirement: Question issue template

The repository SHALL provide a GitHub issue template for usage questions that are neither bug reports nor feature requests.

#### Scenario: Question issue created

- **WHEN** a contributor creates a new issue and selects the question template
- **THEN** the issue form SHALL prompt for what they are trying to do and what they have already tried
- **AND THEN** the issue SHALL be labelled `question` and `status:needs-triage` automatically
- **AND THEN** the form SHALL NOT ask for a priority

### Requirement: Issue chooser routing

The repository's new-issue chooser SHALL offer only the structured issue forms, with blank issues disabled, and SHALL link to channels outside the issue tracker for vulnerability reports and documentation.

#### Scenario: Blank issue option is hidden

- **WHEN** a contributor opens the new-issue chooser in the GitHub web interface
- **THEN** the chooser SHALL list the bug report, feature request, documentation, and question forms
- **AND THEN** it SHALL NOT offer a blank issue option

#### Scenario: Vulnerability reporter is routed privately

- **WHEN** a contributor opens the new-issue chooser
- **THEN** the chooser SHALL include a link to the repository's private vulnerability reporting form, stating that vulnerabilities must not be reported in public issues

#### Scenario: Reader is routed to the documentation site

- **WHEN** a contributor opens the new-issue chooser
- **THEN** the chooser SHALL include a link to the published documentation site

### Requirement: Issue label set

The repository SHALL define its issue and pull request labels in `.github/labels.yml`. That file is the single source of truth for each label's name, color, and description, and every label applied automatically by an issue form or by Dependabot SHALL be defined there.

#### Scenario: Area labels cover the codebase

- **WHEN** a maintainer triages an issue or pull request
- **THEN** area labels SHALL exist for domain management, schema management, event streams, the control plane, notifications, trust, the web console, the SDK, infrastructure, CI, dependencies, and documentation

#### Scenario: Automatically applied labels exist

- **WHEN** an issue form or Dependabot applies a label to an issue or pull request
- **THEN** that label SHALL be defined in `.github/labels.yml`
- **AND THEN** Dependabot SHALL NOT fall back to its default labels

#### Scenario: Labels are applied to GitHub

- **WHEN** the repository's GitHub settings are applied, such as when it is made public
- **THEN** every label in `.github/labels.yml` SHALL be created with its defined color and description before issues are filed
