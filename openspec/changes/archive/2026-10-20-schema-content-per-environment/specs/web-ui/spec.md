# Spec Delta

## ADDED Requirements

### Requirement: Schema content inspection

On the producer schema page, every pipeline node whose environment holds the schema SHALL offer a view affordance that opens a modal showing that environment's latest version content, and each version-history row SHALL offer the same for every environment that version has reached. The modal SHALL show the content pretty-printed with a copy affordance that confirms inline, name the environment and version in its title, and follow the dialog contract: focus returns to the opener on close. Environments where the schema is absent SHALL offer no affordance. A failed content load SHALL surface as a toast and leave the modal dismissible.

#### Scenario: Publisher inspects staging's content

- **WHEN** a publisher activates the view affordance on the staging pipeline node
- **THEN** a modal SHALL open showing staging's latest version content, pretty-printed, titled with the environment and version

#### Scenario: Content is copyable

- **WHEN** the content modal is open
- **THEN** a copy action SHALL place the shown content on the clipboard and confirm inline

#### Scenario: Version history inspects the reached environment

- **WHEN** a publisher activates the view affordance for staging on the v2 history row
- **THEN** the modal SHALL show staging's v2 content, not the environment's latest

#### Scenario: Environment without the schema

- **WHEN** a pipeline node's environment has never received the schema
- **THEN** that node SHALL offer no view affordance

#### Scenario: Auditor inspects read-only

- **WHEN** a platform auditor activates the view affordance
- **THEN** the modal SHALL open with the same content and no mutation affordances

#### Scenario: Load failure is surfaced

- **WHEN** the content request fails
- **THEN** the console SHALL report the failure through a toast and the modal SHALL remain dismissible with focus restored to the opener
