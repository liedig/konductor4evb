## 1. Installation Guide

- [x] 1.1 Create `docs/installation.md` with overview section (three-stack model, account diagram)
- [x] 1.2 Write prerequisites section (local toolchain: Rust, cargo-lambda, zig, SAM CLI, AWS CLI; AWS: platform account, application account, Organization ARN, optional SES)
- [x] 1.3 Write Phase 1 — Deploy notifications stack (optional) with `aws cloudformation deploy` command and SES verification walkthrough
- [x] 1.4 Write Phase 2 — Deploy Nexus core stack with `make check`, `make test`, `make build`, `sam deploy` commands, required parameter table, and stack output capture
- [x] 1.5 Write Phase 3 — Onboard application account with trust-stack deploy command, link to `docs/trust-stack-deployment.md` for detail
- [x] 1.6 Write Phase 4 — Smoke test with domain registration, approval, ACTIVE verification, and resource checks in both accounts
- [x] 1.7 Write troubleshooting section (SES sandbox, RAM share, trust role AssumeRole, SAM build cache)
- [x] 1.8 Write uninstall section (delete domains, delete stacks, manual cleanup of retained resources)

## 2. README Update

- [x] 2.1 Replace the "Getting Started" section (Prerequisites, Build and Test, Deploy subsections) with a pointer to `docs/installation.md`

## 3. Documentation Updates

- [x] 3.1 Update `docs/development/decisions.md` with the single-installation-doc decision
- [x] 3.2 Update `docs/development/architecture.md` to add `docs/installation.md` to the cross-references table

## 4. Verification

- [x] 4.1 Confirm `docs/installation.md` covers all four deployment phases
- [x] 4.2 Confirm README Getting Started section points to installation guide and has no duplicated steps
