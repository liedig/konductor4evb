# DynamoDB grant-engine spike—conditionally ready (review gates 6.4/7.3 closed by operator 2026-07-28; latency residual accepted)

[Documentation home](../README.md)

- Recorded date: 2026-07-28
- Change ID: `evaluate-authorization-engine`
- Area: Authorization
- Status: recorded decision; read later changes before applying it.

## Decision and rationale

Spike validated DynamoDB-backed grants as Konductor's initial authorization engine; Amazon Verified Permissions rejected as the initial engine (reevaluation triggers in the change `design.md`). Representative grant model: single grant table (role bindings, entitlements, audit records) + separate subscription store; consumer/producer GSI + `grant_id-index` revocation GSI + admin GSIs; strongly-consistent single-key reads for every decision (1-2 reads per decision); synchronous subscription reconciliation on revoke. Frozen 13-scenario suite (34 steps) passes in-memory and against real DynamoDB in ap-southeast-2; 0 errors across 2040 benchmark samples; 30/30 revocation trials correct; cost about USD 42/month expected (ap-southeast-2, audit-on-mutations-only).

**Condition**: latency targets are met only by in-Region projection (the spike measured from a remote client, so latencies are RTT-dominated); the two-reviewer (task 6.4) and independent (task 7.3) review gates were closed by operator direction on 2026-07-28, and an in-Region latency re-measurement remains as the accepted residual uncertainty (carried into the implementation change). Production controls to add: conditional writes/transactions for concurrent grant mutation, durable idempotency store for the async `command_id` contract, semantic audit items + time-ordered admin GSI, and a `schema:discover` fan-out cap. A separate OpenSpec change will implement the production authorization engine.

## Evidence

[OpenSpec change](../../openspec/changes/archive/2026-08-05-evaluate-authorization-engine/proposal.md). The recorded date above is not necessarily the archive or merge date.

## Current relevance

This is a spike, not the final production topology. Read [the grant engine](add-grant-engine.md) and [entitlements](add-subscriber-entitlements.md) for the later store split. The recorded latency uncertainty is not new validation evidence.

[Decision index](README.md)
