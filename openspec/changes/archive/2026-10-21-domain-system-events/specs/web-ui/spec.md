# Spec Delta

## ADDED Requirements

### Requirement: System events navigation and topic reference

The console SHALL provide a top-level System events navigation item with a dedicated icon. Its page SHALL list the caller's visible domains with each recorded system-events topic ARN as a read-only, copyable reference with inline copy confirmation, and SHALL link to the subscription setup guide. No event feed, topic administration, subscription management, or replay controls.

#### Scenario: Unread indicator signals new activity
- **WHEN** any visible domain's `systemEventsLastEventAt` is newer than the timestamp stored from the user's last System events visit
- **THEN** the navigation item SHALL display an unread indicator

#### Scenario: Visiting the page clears the indicator
- **WHEN** the user opens the System events page
- **THEN** the latest seen timestamp SHALL be stored for this browser
- **AND** the indicator SHALL clear until a newer event marker arrives

#### Scenario: Read state is per browser
- **WHEN** the same user opens the console in another browser
- **THEN** that browser SHALL compute the indicator from its own stored timestamp
- **AND** no server-side read state SHALL exist

#### Scenario: Copy succeeds
- **WHEN** the user activates a topic ARN's keyboard-accessible copy action
- **THEN** the exact ARN SHALL be copied
- **AND** success SHALL be confirmed inline without moving focus

#### Scenario: Topic is not configured
- **WHEN** a visible domain has no recorded `systemEventsTopicArn`
- **THEN** its entry SHALL explain that the topic requires platform provisioning or backfill
- **AND** it SHALL not fabricate an ARN or offer a copy action for a missing value

#### Scenario: Auditor can inspect without mutation controls
- **WHEN** a platform auditor opens the System events page
- **THEN** the topic references and copy actions SHALL be available
- **AND** the page SHALL contain no topic mutation, subscription-management, or replay controls

#### Scenario: Long ARN on a narrow viewport
- **WHEN** the page is viewed at a narrow supported viewport
- **THEN** ARNs and copy actions SHALL remain readable and operable without page-level horizontal overflow

#### Scenario: Domain detail no longer embeds the topic card
- **WHEN** a user opens a domain detail page
- **THEN** it SHALL NOT embed a System events topic section
- **AND** the topic reference SHALL live on the top-level System events page
