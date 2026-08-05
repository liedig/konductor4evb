# Readiness Rubric, Mandatory Behavior, and Evidence Templates

> **Spike methodology.** This rubric is frozen before results are collected (task 1.3). It explains how evaluation criteria inform the readiness decision; it does not assign arbitrary numerical weights that imply Nexus product priorities. The readiness decision (task 7.1) applies this rubric unchanged. Two independent reviewers (task 6.4) verify evidence against this rubric and record disagreements and resolutions.

## 1. Mandatory behavior (pass/fail)

Two behaviors are mandatory. Failing either fails the spike regardless of other evidence.

### 1.1 Mandatory correctness

The DynamoDB evaluator MUST return the expected decision for every frozen fixture scenario (S01 through S13). Specifically:

- No incorrect allow.
- No incorrect deny.
- No cross-tenant disclosure (an unauthorized principal receiving a resource it is not entitled to).
- No silently changed or omitted scenario. Any scenario the DynamoDB design cannot represent is recorded as an unsupported result, not silently dropped or approximated.
- The frozen manifest SHA-256 is unchanged from the one used to produce evidence.

A single mandatory-correctness failure produces a `conditional` readiness decision with the smallest scoped remediation required before production implementation (see `Evidence-based DynamoDB readiness decision` / `DynamoDB fails mandatory behavior`).

### 1.2 Mandatory non-disclosure

- An unauthorized `schema:discover` response MUST be indistinguishable from the corresponding unknown-resource response (S12a vs S12b): same status, same body shape, same error code, no timing channel that reveals existence.
- List decisions MUST filter unauthorized resources rather than return them as `denied` entries (S12c).
- IAM principal ARN audit provenance MUST NOT be usable as a resource-grant key and MUST NOT change the decision (S11c).

A non-disclosure failure is a mandatory-behavior failure.

## 2. Readiness rubric (informing criteria)

The criteria below inform readiness. Each is assessed as `support`, `concern`, or `unsupported`. `unsupported` is explicit evidence of a gap, never inferred as success.

| Criterion | What is assessed | Support | Concern | Unsupported |
|---|---|---|---|---|
| Latency | Warm/cold p50/p95/p99 vs provisional targets (methodology.md) | All provisional targets met or exceeded | One or more p99 targets missed but within 2x; explainable by cold start or tuning | p99 exceeds 2x target after documented tuning; sustained throughput unachievable |
| Cost | Dated cost worksheet at low/expected/high volumes | Monthly cost within an evidenced envelope at expected volume | Cost sensitive to an assumption (fan-out, grant count) not yet bounded | Cost unbounded or dominated by an unmetered component |
| Auditability | Reconstruct who/what/why from retained records without exposing tokens | Full reconstruction from audit + grant history | Reconstruction possible but requires cross-table correlation not in design | Cannot reconstruct grant change or decision rationale |
| Local testability | Common correctness suite runs without AWS credentials | Full suite runs locally; semantic gaps documented | Partial local run; cloud-only scenarios documented | Correctness suite requires AWS credentials for mandatory cases |
| Policy administration | Create/review/update/revoke/enumerate each binding and entitlement incl. concurrent changes | All operations via key-condition queries, no scans | Concurrent-change handling needs documented guard | An administration operation requires a scan or is not representable |
| Operational complexity | Services, deployable units, alarms, quotas, retries, failure modes, backup/recovery, operator steps | Inventory complete; operator steps bounded | Inventory complete but one failure mode needs custom logic | A required operational concern is unhandled |
| AWS account principal support | Grants by authenticated AWS account ID; equivalent decisions across IAM role/user provenance; ARN audit-only | Parity demonstrated (S11); ARN never selects grants | Parity demonstrated but provenance capture needs custom logic | Account-level granularity cannot be represented |
| Cognito integration | Trusted issuer/group mapping; reject forged/untrusted context; update group binding | Forged context denied (S11d); binding update works | Mapping works but issuer rotation needs custom logic | Trusted-issuer model cannot be represented |
| Hierarchy and inheritance | Global roles, group-to-role, role-to-domain, producer-domain-to-schema, exact-schema, overlapping grants | All hierarchy levels evaluated correctly | One level requires denormalization not yet evidenced | A hierarchy level cannot be represented additively |
| Immediate revocation | Timed denial + subscription disablement vs provisional targets | Both p95 targets met | Disablement p95 missed but bounded by fan-out | Decision invalidation or disablement unbounded |
| Asynchronous handling | Enqueue, delayed handling, revocation-before-consume, replay, incomplete, tampered | Deterministic denials acknowledged, not retried | One case needs custom guard | A case cannot be represented without token carriage |

### 2.1 Uncertainty treatment

- Unsupported or unmeasured behavior is recorded explicitly as `unsupported` and is never inferred as successful.
- Assumptions behind cost and latency projections are retained (principal mix, read/write ratio, grant count, subscription fan-out, item sizes, consistency mode, retries, log retention). Sensitivity to each assumption is noted.
- Where evidence is qualitative, the record states the exercise, observed result, and the reviewer's reasoning; disagreements between the two reviewers are recorded with resolution.
- No arbitrary numerical weight is assigned across criteria. The readiness decision narrates how support, concern, and unsupported findings combine.

### 2.2 Production-blocker criteria

A finding is a **production-blocking constraint** if any of the following hold:

1. Mandatory correctness or non-disclosure fails (Section 1).
2. A criterion is `unsupported` for a behavior that the agreed Nexus authorization model requires (e.g., immediate revocation cannot be bounded; account-level granularity cannot be represented).
3. Latency or revocation p99 exceeds 2x the provisional target after documented tuning of the representative grant model.
4. Maintaining the subscription reverse index or additive re-evaluation is operationally unsustainable at the evidenced volumes (operator steps unbounded, quotas exceeded without mitigation).
5. A product requirement for explicit deny, delegation, or condition logic emerges that the additive grant model cannot express.

A production-blocking constraint produces a `conditional` readiness decision documenting the remediation required before production implementation (see `DynamoDB has a production-blocking constraint`). Otherwise, satisfying mandatory behavior with no production-blocking constraint produces a `ready` decision (see `DynamoDB satisfies mandatory behavior`).

## 3. Evidence templates

Every retained result uses the template below. Templates live alongside the evidence they describe.

### 3.1 Correctness trial evidence

```
scenarioId:           Sxx-...
manifestSha256:       ec4efb25...
executedAt:           <RFC3339>
region:               us-east-1
expected:             <allow|deny|deny-as-unknown|...>
observed:             <decision>
result:               <pass|fail|unsupported>
translationNotes:     <storage-specific translation or unsupported reason; "none" if direct>
evidenceRef:          <path to raw output>
```

A `fail` or `unsupported` entry triggers Section 1 mandatory review and is listed in the readiness decision.

### 3.2 Latency benchmark evidence

```
profile:              <sequential|target-throughput>
principalType:        <cognito-group|aws-account>
warmSamples:          <n>
coldSamples:          <n>
errors:               <n>
retries:              <n>
throttles:            <n>
throughputDecPerSec:  <n>
p50_ms:               <n>
p95_ms:               <n>
p99_ms:               <n>
coldP99_ms:           <n>
provisionalTargetAssessment: <met|missed|exceeded-2x>
rawSamplesRef:        <path to raw samples>
executedAt:           <RFC3339>
region:               us-east-1
manifestSha256:       ec4efb25...
```

### 3.3 Cost worksheet evidence

```
priceDate:            <YYYY-MM-DD of public prices>
region:               us-east-1
volumeProfile:        <low|expected|high>
monthlyDecisions:     <n>
itemizedMonthlyCost:
  requests:           <USD>
  storage:            <USD>
  grantAdministration:<USD>
  audit:              <USD>
  monitoring:         <USD>
  revocationProcessing:<USD>
  total:              <USD>
assumptions:
  principalMix:       <text>
  readWriteRatio:     <text>
  grantCount:         <n>
  subscriptionFanOut: <n>
  itemSizes:          <text>
  consistencyMode:    <text>
  retries:            <text>
  logRetention:       <text>
formulaeRef:          <path to worksheet>
```

### 3.4 Revocation benchmark evidence

```
trials:               <n>
timeToFirstDeniedDecision_ms: { p50, p95, max, failures }
timeToSubscriptionDisablement_ms: { p50, p95, max, failures }
provisionalTargetAssessment: <met|missed|exceeded-2x>
reverseIndexMechanism: <text describing how affected subscriptions are found>
rawTrialsRef:         <path to raw trials>
executedAt:           <RFC3339>
region:               us-east-1
manifestSha256:       ec4efb25...
```

### 3.5 Operational exercise evidence

```
criterion:            <auditability|localTestability|policyAdministration|operationalComplexity|awsAccountPrincipal|cognitoIntegration|hierarchyAndInheritance|immediateRevocation|asynchronousHandling>
exercise:             <text>
setup:                <text>
steps:                <text>
observedResult:       <text>
failureBehavior:      <text>
requiredCustomLogic:  <text or "none">
operatorEffort:       <text>
requiredAwsServices:  <text>
rubricAssessment:     <support|concern|unsupported>
reasoning:            <text>
evidenceRef:          <path to raw output>
```

## 4. Two-reviewer verification (task 6.4)

Two reviewers independently:

1. Confirm every mandatory behavior result (Section 1) against the correctness evidence.
2. Confirm every criterion assessment (Section 2) links to a retained evidence reference.
3. Confirm the frozen manifest SHA-256 and readiness rubric are unchanged from the pre-measurement freeze.
4. Record disagreements and resolutions in `evidence/review-verification.md`.

The readiness decision (task 7.1) is not published until both reviewers sign off or disagreements are resolved and recorded.
