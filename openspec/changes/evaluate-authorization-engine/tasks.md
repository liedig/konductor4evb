## 1. Freeze Evaluation Method

- [ ] 1.1 Create the DynamoDB fixture manifest covering every specified principal, role, domain, schema, entitlement, additive-grant, non-disclosure, revocation, and asynchronous command scenario
- [ ] 1.2 Record expected decisions, representative list filtering and unknown-resource responses, workload profiles, sample sizes, provisional measurement targets, AWS Region, time box, client settings, and a hash of the frozen fixtures, clearly labeling these values as spike methodology
- [ ] 1.3 Publish mandatory correctness and non-disclosure behavior, the readiness rubric, uncertainty treatment, production-blocker criteria, and evidence templates before collecting results

## 2. Build Disposable DynamoDB Harness

- [ ] 2.1 Build the shared spike runner with Cognito-group and IAM-authenticated AWS account fixtures without changing production Nexus code
- [ ] 2.2 Build the DynamoDB grant adapter and isolated table/index fixture needed to evaluate grants without scans
- [ ] 2.3 Add local correctness execution for the DynamoDB adapter, documenting any cloud-service semantic gaps and retaining configuration with secrets removed

## 3. Verify Common Authorization Semantics

- [ ] 3.1 Run and retain evidence for `platform-admin`, `platform-auditor`, Cognito group bindings, domain-scoped publishers, and domain-scoped subscribers
- [ ] 3.2 Run and retain evidence for producer-domain entitlements, exact-schema entitlements, producer growth after grant (domain-wide consumer gains the new schema without mutation, exact-schema consumer does not), additive overlapping grants, and removal of one overlapping grant
- [ ] 3.3 Run and retain Cognito-group/AWS-account parity, same-account IAM role/user provenance, forged group context, default-deny, unknown-versus-unauthorized response, and unauthorized list-filtering evidence, confirming IAM principal ARN is audit-only
- [ ] 3.4 Record every incorrect, unsupported, omitted, or translated scenario and determine whether the DynamoDB design satisfies mandatory correctness and non-disclosure behavior

## 4. Measure Latency and Cost

- [ ] 4.1 Run the predeclared warm and cold-start-path sample sizes per principal type under sequential and target-throughput profiles
- [ ] 4.2 Retain raw samples and report sample count, errors, retries, throttles, throughput, and p50, p95, and p99 latency against the provisional spike targets
- [ ] 4.3 Build the dated cost worksheet for the predeclared low, expected, and high monthly decision volumes, itemizing requests, storage, administration, audit, monitoring, and revocation processing

## 5. Measure Revocation and Asynchronous Handling

- [ ] 5.1 Implement the disposable revocation exercise that re-evaluates affected active subscriptions, preserves overlapping access, and simulates disabling every no-longer-authorized materialization
- [ ] 5.2 Run the predeclared number of revocation trials and retain separate p50, p95, maximum, and failure measures for first denied decision and complete subscription disablement
- [ ] 5.3 Exercise Cognito-group and IAM-authenticated AWS account commands across enqueue, delayed handling, revocation-before-consume, replay, incomplete context, and tampered context, confirming deterministic denials are acknowledged rather than retried
- [ ] 5.4 Compare immediate-revocation and asynchronous-handling evidence against the provisional spike targets and draft evidence-based production SLO recommendations

## 6. Complete Operational Exercises

- [ ] 6.1 Complete and retain the audit reconstruction and local-testability exercises for the DynamoDB design
- [ ] 6.2 Complete and retain the policy administration and operational complexity exercises, including concurrent changes, quotas, failures, backup/recovery, alarms, and operator steps
- [ ] 6.3 Complete and retain the AWS account principal, Cognito integration, and hierarchy/inheritance exercises for the DynamoDB design
- [ ] 6.4 Have two reviewers independently verify qualitative evidence against the frozen readiness rubric, record disagreements and resolutions, and link every conclusion to raw evidence

## 7. Confirm Readiness and Close the Spike

- [ ] 7.1 Apply mandatory correctness and non-disclosure behavior plus the frozen readiness rubric and publish a readiness decision for DynamoDB-backed grants
- [ ] 7.2 Document AVP as the rejected alternative, DynamoDB evidence, recommended production SLOs, assumptions, trade-offs, residual risks, conditional remediation if needed, and reevaluation triggers
- [ ] 7.3 Obtain independent review confirming scenario coverage, evidence traceability, unchanged readiness rubric, and satisfaction of every specification requirement
- [ ] 7.4 Destroy all tagged sandbox resources, verify cleanup, and retain sanitized evidence and the final decision under this change
- [ ] 7.5 Update `docs/development/decisions.md` with the spike recommendation and open a separate OpenSpec change for any production authorization implementation
