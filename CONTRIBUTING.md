# Contributing to Konductor for Amazon EventBridge

## License

This project is licensed under the [Apache License 2.0](LICENSE). By contributing, you agree that your contributions will be licensed under the same license.

## Developer Certificate of Origin

This project uses the [Developer Certificate of Origin (DCO)](https://developercertificate.org/) to establish a clear contribution chain. All commits must include a `Signed-off-by` line:

```text
Signed-off-by: Your Name <your.email@example.com>
```

Add it automatically with `git commit --signoff` or `git commit -s`.

By signing off, you certify that you wrote the contribution or otherwise have the right to submit it under the project's license. See the [full DCO text](https://developercertificate.org/) for details.

## Getting Started

1. Fork the repository and create a feature branch from `main`
2. Follow the conventions in [AGENTS.md](AGENTS.md) for code style, testing, and git workflow
3. Run `make check` and `make test` before submitting a PR
4. Use [Conventional Commits](https://www.conventionalcommits.org/) for commit messages
5. Reference any relevant issues in your PR description

## Pull Request Checklist

- [ ] `cargo fmt --check` passes
- [ ] `cargo clippy --workspace -- -D warnings` passes
- [ ] `cargo test --workspace` passes
- [ ] New behavior was built test-first (red/green/refactor), and bug fixes include a reproducing test; see [Testing Strategy](AGENTS.md#testing-strategy)
- [ ] Commit messages follow Conventional Commits with `Signed-off-by`
- [ ] Docs updated if applicable, following the [documentation maintenance rules](docs/development/documentation.md)

## Issues and triage

- Open issues through one of the four forms: bug report, feature request, documentation, or question. Blank issues are turned off. If nothing else fits, use the question form.
- Issues are public. Remove account IDs, tokens, email addresses, and other secrets before you submit.
- Report security vulnerabilities privately, as described in [SECURITY.md](SECURITY.md), never in an issue.
- Each form applies its type label (`type:bug`, `type:feature`, `type:docs`, or `question`) and `status:needs-triage`. Forms can't turn a dropdown choice into a label, so during triage a maintainer applies the `area:*` label for the component you chose and a priority from `P0` (critical) to `P3` (low), then removes `status:needs-triage`. Labels are defined in [`.github/labels.yml`](.github/labels.yml).
- Maintainers who need a free-form issue create it with `gh issue create`, which doesn't go through the form chooser.

## Review and merging

Pull requests to `main` need a code owner's approval, with every review conversation resolved, and the jobs in [`.github/workflows/ci.yml`](.github/workflows/ci.yml) must pass. Each pull request is merged as one squashed commit that uses the pull request title as its subject, so give the pull request a Conventional Commits title. The rules are defined as rulesets in [`.github/rulesets/`](.github/rulesets/).
