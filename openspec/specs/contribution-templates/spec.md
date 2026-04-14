# Capability: Contribution Templates

## Purpose
TBD

## Requirements

### Requirement: Bug report template

The repository SHALL provide a GitHub issue template for bug reports. The template SHALL prompt the reporter for structured information to aid diagnosis.

#### Scenario: Bug report issue created

- **WHEN** a contributor creates a new issue and selects the bug report template
- **THEN** the issue form SHALL prompt for: description of the bug, steps to reproduce, expected behaviour, and actual behaviour
- **AND THEN** the issue SHALL be labelled `bug` automatically

### Requirement: Feature request template

The repository SHALL provide a GitHub issue template for feature requests. The template SHALL prompt the requester for motivation and a description of the desired behaviour.

#### Scenario: Feature request issue created

- **WHEN** a contributor creates a new issue and selects the feature request template
- **THEN** the issue form SHALL prompt for: description of the feature and motivation (why this is needed)
- **AND THEN** the issue SHALL be labelled `enhancement` automatically

### Requirement: Blank issue escape hatch

The repository SHALL allow contributors to create blank issues without using a template, for cases that do not fit the structured templates.

#### Scenario: Blank issue created

- **WHEN** a contributor creates a new issue and selects the blank issue option
- **THEN** GitHub SHALL open a standard blank issue form with no pre-populated structure

### Requirement: Pull request template

The repository SHALL provide a pull request template that prompts contributors for a summary, test plan, and checklist. The template SHALL be automatically populated when a new PR is created.

#### Scenario: PR created with template

- **WHEN** a contributor opens a new pull request
- **THEN** the PR description SHALL be pre-populated with the template containing sections for summary, test plan, and a checklist
- **AND THEN** the contributor SHALL fill in the sections before submitting
