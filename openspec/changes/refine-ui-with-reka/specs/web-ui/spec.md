## MODIFIED Requirements

### Requirement: Documented visual design system

The product SHALL have a design system documented at `docs/development/design-system.md` covering: design principles (clean, airy, professional; layered depth; generous whitespace; color-with-intent status semantics; `rounded-lg`/`rounded-xl` radius; `shadow-sm`-only elevation), the token set (color, type, spacing), core primitives (buttons, inputs, selects, badges/status pills, cards, tables, modal/confirm, toast, empty state), and page patterns (form, list+filter, detail, inbox). Tokens SHALL be implemented as Tailwind v4 `@theme` CSS variables with dark as the default theme, and every documented token SHALL correspond to a real token in code. A `/design-system` gallery route SHALL render the primitives for reference, including interactive overlays and applicable hover, focus, error, disabled, and loading states. The refined system SHALL retain the existing top navigation, page layouts, three surface levels, blue primary accent, and semantic status colors while applying consistent typography, spacing, borders, and control states.

#### Scenario: Dark theme is structural

- **WHEN** the console renders any screen
- **THEN** components SHALL consume semantic tokens and never raw palette utilities
- **AND THEN** the default theme SHALL be dark without any toggle or inversion pass

#### Scenario: Documentation matches implementation

- **WHEN** a token or primitive is documented in design-system.md
- **THEN** it SHALL exist in the codebase at the documented path
- **AND THEN** the gallery route SHALL render each documented primitive

#### Scenario: Existing journeys retain their structure

- **WHEN** a user visits a refreshed list, form, detail, or inbox page
- **THEN** its navigation, content grouping, and available product actions SHALL remain unchanged
- **AND THEN** shared controls SHALL use the refined visual system without changing status meanings

#### Scenario: Gallery demonstrates interaction states

- **WHEN** a reviewer opens the gallery
- **THEN** it SHALL provide normal, error, disabled, and loading examples where applicable and keyboard-operable modal, confirmation, and notification demonstrations
- **AND THEN** controls SHALL expose visible hover and keyboard focus states during interaction

## ADDED Requirements

### Requirement: Keyboard-accessible modal dialogs

Modal dialogs SHALL expose an accessible name, move focus inside on opening, contain keyboard focus while open, and prevent interaction with background content. Ordinary dialogs SHALL support dismissal through Escape, the backdrop, and a labeled close control. On closing, focus SHALL return to the opening control when it still exists and is focusable, or to a logical focusable location on the current page otherwise. Dialog content and actions SHALL remain reachable when content exceeds the viewport.

#### Scenario: Keyboard navigation stays in the dialog

- **WHEN** a user opens a modal and navigates with Tab or Shift+Tab
- **THEN** focus SHALL remain inside the modal and its title SHALL provide its accessible name
- **AND THEN** background controls SHALL not be operable until the modal closes

#### Scenario: Dismissal restores focus

- **WHEN** a user dismisses an ordinary modal with Escape, its backdrop, or its close control
- **THEN** the modal SHALL close without invoking its primary action
- **AND THEN** focus SHALL return to the opener or a logical fallback if the opener is unavailable

#### Scenario: Small viewport or long content

- **WHEN** modal content exceeds the available viewport at a narrow width or increased zoom
- **THEN** the user SHALL be able to scroll to all content and actions without interacting with the background page

### Requirement: Safe asynchronous confirmations

Confirmation dialogs SHALL name the action, expose its explanatory content as an accessible description, and initially focus Cancel rather than the confirming action. An idle confirmation SHALL allow cancellation with Cancel or Escape but SHALL NOT dismiss from a backdrop click. While its caller reports the action as busy, the dialog SHALL remain open, indicate progress, prevent duplicate confirmation, and prevent user-initiated cancellation or dismissal. The caller SHALL remain responsible for completing or failing the operation and controlling whether the dialog stays open.

#### Scenario: Cancellation does not perform the action

- **WHEN** a user opens a confirmation and chooses Cancel or presses Escape before confirming
- **THEN** the dialog SHALL report cancellation without invoking the confirming action
- **AND THEN** a backdrop click alone SHALL neither cancel nor confirm

#### Scenario: Busy confirmation cannot be dismissed or repeated

- **WHEN** a confirming action is in progress and the caller marks the confirmation busy
- **THEN** repeated activation, Escape, backdrop clicks, and cancellation controls SHALL not confirm again or dismiss the dialog
- **AND THEN** the confirming control SHALL display a busy state

#### Scenario: Failed operation can be retried or cancelled

- **WHEN** the caller clears busy after a failure and leaves the dialog open
- **THEN** confirmation and cancellation controls SHALL become available again
- **AND THEN** the explanatory content SHALL remain available

### Requirement: Accessible transient notifications

Transient notifications SHALL preserve success, informational, and error messaging, announce new messages politely without moving focus, and provide a labeled keyboard-accessible dismiss control. Notifications SHALL expire after six seconds of unpaused display time. Expiry SHALL pause while the notification region is hovered or contains keyboard focus, or while the browser window is inactive, and resume when those conditions no longer apply. Notifications SHALL remain within the viewport at narrow widths.

#### Scenario: Message appears without stealing focus

- **WHEN** an operation produces a transient notification
- **THEN** the message SHALL be announced politely and shown with its semantic tone
- **AND THEN** focus SHALL remain on the user's current control

#### Scenario: Reading or interacting pauses expiry

- **WHEN** a user hovers the notification region, focuses a control in it, or switches away from the browser window
- **THEN** notifications SHALL remain visible while the pause condition applies
- **AND THEN** automatic dismissal SHALL resume after interaction ends and the window is active

#### Scenario: Manual dismissal removes only the selected message

- **WHEN** a user activates a notification's dismiss control
- **THEN** only that notification SHALL be removed
- **AND THEN** other queued notifications SHALL remain independently dismissible

### Requirement: Accessible shared form control states

Shared inputs and selects SHALL associate labels and displayed hints or errors with their controls. They SHALL expose invalid and disabled states to assistive technology and retain visible keyboard focus indicators. Loading buttons SHALL prevent activation and communicate busy state without losing their action label. Simple selects SHALL retain native keyboard and form behavior, including existing empty-string filter values and required placeholders.

#### Scenario: Field error is associated with its control

- **WHEN** a shared input or select displays an error
- **THEN** the control SHALL expose an invalid state and reference the error as its accessible description
- **AND THEN** its label SHALL remain associated with the control

#### Scenario: Empty filter value remains selectable

- **WHEN** a user selects an existing empty-valued filter option such as "All environments"
- **THEN** the select SHALL retain the empty value and the page SHALL retain its existing unfiltered behavior

#### Scenario: Disabled and loading controls do not activate

- **WHEN** a user attempts to activate a disabled control or a loading button
- **THEN** no action SHALL be invoked
- **AND THEN** a loading button SHALL retain its label and expose its busy state
