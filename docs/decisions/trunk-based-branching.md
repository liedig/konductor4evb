# Retired `develop`; trunk-based on `main`—supersedes the gitflow decision in `github-ci-templates`

[Documentation home](../README.md)

- Recorded date: 2026-09-07
- Change ID: `trunk-based-branching`
- Area: CI / DevEx
- Status: recorded decision; read later changes before applying it.

## Decision and rationale

The repo ran half a git-flow: conventions (short-lived feature branches, CONTRIBUTING fork-from-`main`) and actual PR flow were trunk-based while only Dependabot targeted `develop`, producing recurring main/develop divergence with manual sync toil, plus Dependabot security alerts tracked against the default branch while fixes queued on `develop`. `develop` retired; Dependabot targets the default branch; CI push triggers run on `main` only; the `ci-pipeline` spec's trigger requirement updated to match. Rationale: a continuously deployed, pre-production, single-maintainer project has no multi-release support matrix that would justify a permanent integration branch; release points will be tags once the product matures.

## Evidence

Migrated from the decision log at commit `c7e2bd6`. No matching OpenSpec change directory was present during the reorganization.

[Decision index](README.md)
