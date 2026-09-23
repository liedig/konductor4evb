# Contributor guide

[Documentation home](../README.md)

Use this sequence when changing or reviewing the repository:

1. Read [AGENTS.md](../../AGENTS.md) for shared conventions.
2. Find the affected topic through the [architecture overview](../architecture/README.md)
   or [documentation home](../README.md).
3. Read the relevant [capability requirement](../../openspec/specs/) and
   [decision records](../decisions/README.md), including later decisions.
4. Check [known discrepancies](../reference/documentation-discrepancies.md).
   Do not resolve unrelated conflicts as part of a structural or editorial change.
5. Run the relevant checks and update the affected topic, not a monolithic overview.

## Working references

| Area | Guidance |
|---|---|
| Code locations | [Repository map](repository.md) |
| Rust and browser validation | [Testing](testing.md) |
| Handler writes and durability | [Side-effects records](side-effects/README.md) |
| Console development | [Console workflow](console.md) |
| UI tokens and primitives | [Design system](design-system.md) |
| Vue application structure | [Vue conventions](vue.md) |
| TypeScript and tooling | [TypeScript conventions](typescript.md) |
| Documentation changes | [Maintenance rules](documentation.md) |

Use the [contribution guide](../../CONTRIBUTING.md) for pull requests and sign-off.
Operational rollout procedures belong in [operations](../operations/README.md).
