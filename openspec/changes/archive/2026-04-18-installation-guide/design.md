## Context

The Konductor project has three deployable CloudFormation stacks (notifications, core, trust) but no unified installation guide. The README's "Getting Started" section is a brief three-step sketch that doesn't reflect the current three-stack deploy model, the optional notifications stack, or the smoke-test workflow. The `extract-notifications-stack` change landed the three-stack model; this change writes the guide that makes it installable by an end user.

The audience is a platform operator deploying Konductor for the first time into their own AWS environment (one platform account, at least one application account).

## Goals / Non-Goals

**Goals:**

- Write a runnable, end-to-end installation guide at `docs/installation.md`.
- Replace the README "Getting Started" section with a pointer to the guide.
- Cover all four deployment phases: prerequisites, notifications stack, core stack, trust stack.
- Include a smoke-test walkthrough that a first-time operator can follow.
- Include troubleshooting for common failure modes (SES sandbox, RAM share, trust role, build cache).
- Include uninstall instructions.

**Non-Goals:**

- Production hardening guidance (multi-account landing zone, SCPs, guardrails).
- CI/CD pipeline setup for Konductor itself.
- API authz setup (deferred to a future change).
- Alternate notification transport configuration (deferred).
- Detailed architecture explanation (that's `docs/development/architecture.md`).

## Decisions

### Decision 1: Single doc at `docs/installation.md`

One document covering the full install flow, rather than splitting across multiple files. The trust-stack guide (`docs/trust-stack-deployment.md`) remains as a Phase 3 detail reference.

**Rationale:** An installer following a single doc from start to finish is less likely to miss a step. Cross-referencing between docs creates gaps.

**Alternatives considered:**

- *Multi-doc split:* Would allow per-phase docs but increases navigation burden and risk of divergence.
- *README-only:* Would bloat the README beyond its purpose as a project overview.

### Decision 2: README Getting Started becomes a pointer

Replace the current "Prerequisites / Build and Test / Deploy" subsections in the README with a single link to `docs/installation.md`. Keep the README's architecture overview, API table, and lifecycle diagram — they serve a different purpose (project understanding, not step-by-step install).

**Rationale:** Duplicating install steps between README and installation.md guarantees drift.

### Decision 3: Smoke test uses `curl` (no auth)

API Gateway currently has IAM auth enabled but authz removal is deferred. The smoke test will note that IAM auth is active and show the `awscurl` command (SigV4-signed requests) as the path. If/when auth is removed, the guide updates to plain `curl`.

**Rationale:** The guide must be runnable as-is. Providing `awscurl` examples is the simplest way to make the smoke test work against the current API.

### Decision 4: SES sandbox callout is testing-only

The guide explicitly states that SES configuration is for testing against the sandbox, not production. It walks through the verification email flow without requesting SES production access.

**Rationale:** Production SES setup (DKIM, configuration sets, bounce handling) is a separate concern and would dilute the install guide's focus.

## Risks / Trade-offs

- **Risk:** The guide references stack names, parameter names, and API paths that may drift as the project evolves.
  - **Mitigation:** The guide uses the actual template parameter names and CloudFormation outputs, which are version-controlled alongside the code.

- **Risk:** The smoke test depends on IAM auth being present. If auth is removed in a future change, the guide must be updated.
  - **Mitigation:** The guide will note the auth dependency explicitly in the smoke-test section.

- **Trade-off:** The guide is AWS-specific and CLI-centric. Operators using Terraform, CDK, or the AWS Console will need to adapt the steps.
  - **Acceptable:** The project ships SAM/CloudFormation templates; the guide follows the same tooling.
