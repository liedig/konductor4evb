## 1. Inventory

- [x] 1.1 Mechanically enumerate every DynamoDB write call site (`put_item`, `update_item`, `transact_write_items`) across the control-plane, domain-management, event-streams, and notifications function crates, crossed with EventBridge (`put_events`), SQS (`send_message`), SNS (`publish`), and Step Functions (`start_execution`) calls in the same handler path
- [x] 1.2 Manually trace each candidate handler to confirm the write/side-effect graph and document the existing recovery mechanism (hand-rolled idempotent continuation, queue redrive, or none); known starting points: `register.rs` (including the collision-preflight write), `update.rs`, `domain-state`, `finalize-approval` (the approval decide path), `subscriptions` subscribe/unsubscribe and `subscriptions/outcome.rs`, schema administration, notification triggers, the publish path, and the control-plane entitlement handlers (`entitlement-admin`, `binding-admin`, `entitlement-revocation-worker`, `entitlement-coverage-sweep`)

## 2. Classification

- [x] 2.1 Apply the six rubric questions from design.md to each inventoried call site and record the answers
- [x] 2.2 Classify each emitted artifact as change record or business event, proposing business-event names in the established `{Operation}Succeeded|Failed` style where translation is warranted
- [x] 2.3 Walk the failure modes of each multi-step handler: document the partial state a crash between each pair of steps leaves and whether current recovery covers it

## 3. Decisions

- [x] 3.1 Record one decision per handler (MIGRATE, STAY with reason, or ENVELOPE-DRIVEN) in `docs/development/decisions.md` using the fixed format from design.md
- [x] 3.2 Define the follow-up change list for every MIGRATE decision (expected candidates: the `register.rs` three-system write, `update.rs` lifecycle events, subscription outcome events), one scaffolded change name per migration, without implementing them
- [x] 3.3 Decide whether the rubric becomes an authoring-time checklist in AGENTS.md or PR review criteria, informed by how many existing handlers the audit caught

## 4. Validation

- [x] 4.1 Review the inventory against the codebase a second time to prove no write path was missed (including `konductor-common` helpers that write on behalf of handlers)
- [x] 4.2 Confirm the change made no production code modifications, run strict OpenSpec validation, and verify the decision record satisfies the proposal
