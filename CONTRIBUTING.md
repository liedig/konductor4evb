# Contributing to EventBridge Nexus

## License

This project is licensed under the [Apache License 2.0](LICENSE). By contributing, you agree that your contributions will be licensed under the same license.

## Developer Certificate of Origin

This project uses the [Developer Certificate of Origin (DCO)](https://developercertificate.org/) to establish a clear contribution chain. All commits must include a `Signed-off-by` line:

```
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
- [ ] Commit messages follow Conventional Commits with `Signed-off-by`
- [ ] Docs updated if applicable
