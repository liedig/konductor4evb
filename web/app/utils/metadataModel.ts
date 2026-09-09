import type { MetadataDefinition, MetadataGenerator, MetadataNode } from '~/types/api'

/**
 * UI mirror of the server's bounded generator catalogue and additive
 * limits (konductor-common `metadata/definition.rs`, design.md decision 2).
 * Types and constraints are inferred by the platform and shown read-only;
 * the editor never accepts authored JSON Schema.
 */

export interface GeneratorCatalogueEntry {
  value: MetadataGenerator
  label: string
  constraint: string
  source: string
}

export const GENERATOR_CATALOGUE: GeneratorCatalogueEntry[] = [
  {
    value: 'uuid-v4',
    label: 'UUID v4',
    constraint: 'string in UUID v4 format',
    source: 'platform RNG, generated once per admitted event',
  },
  {
    value: 'utc-timestamp',
    label: 'UTC timestamp',
    constraint: 'UTC RFC 3339 timestamp with millisecond precision',
    source: 'one captured admission instant',
  },
  {
    value: 'utc-date',
    label: 'UTC date',
    constraint: 'UTC calendar date (YYYY-MM-DD)',
    source: 'the same captured admission instant',
  },
  {
    value: 'aws-region',
    label: 'AWS region',
    constraint: 'non-empty string',
    source: 'trusted processing region',
  },
  {
    value: 'organisation',
    label: 'Organisation',
    constraint: 'non-empty string',
    source: 'resolved platform organisation',
  },
  {
    value: 'environment',
    label: 'Environment',
    constraint: 'non-empty string',
    source: 'authorized publish API stage',
  },
]

export function catalogueEntry(generator: string): GeneratorCatalogueEntry | undefined {
  return GENERATOR_CATALOGUE.find(entry => entry.value === generator)
}

export const METADATA_LIMITS = {
  extensionNodesMax: 64,
  objectLevelsMax: 4,
  definitionBytesMax: 16 * 1024,
} as const

/** Reserved built-ins owned by the platform; never definable at the root. */
export const RESERVED_ROOT_FIELDS = [
  'schemaVersion',
  'idempotencyKey',
  'publishedAt',
  'metadataRevision',
] as const

/** Field name segments must match the server's `[A-Za-z][A-Za-z0-9_]{0,63}`. */
export const NAME_PATTERN = /^[A-Za-z][A-Za-z0-9_]{0,63}$/

const UNSAFE_KEYS = ['__proto__', 'prototype', 'constructor']

export interface DraftViolation {
  name: string
  message: string
}

/** Extension nodes (fields plus nested objects); built-ins never count. */
export function countNodes(nodes: MetadataNode[]): number {
  return nodes.reduce(
    (sum, node) => sum + 1 + (node.kind === 'object' ? countNodes(node.children ?? []) : 0),
    0,
  )
}

/** Deepest object nesting below the metadata root (0 = flat root fields). */
export function objectLevels(nodes: MetadataNode[]): number {
  return nodes.reduce((max, node) => {
    if (node.kind !== 'object') return max
    return Math.max(max, 1 + objectLevels(node.children ?? []))
  }, 0)
}

/** Compact serialization size; the preview response reports the authoritative count. */
export function definitionBytes(definition: MetadataDefinition): number {
  return new TextEncoder().encode(JSON.stringify(definition)).length
}

function validName(name: string, atRoot: boolean): string | undefined {
  if (UNSAFE_KEYS.includes(name)) return 'unsafe object key is not allowed at any level'
  if (atRoot && (RESERVED_ROOT_FIELDS as readonly string[]).includes(name)) {
    return 'reserved built-in metadata field cannot be redefined'
  }
  if (!NAME_PATTERN.test(name)) {
    return 'field name must match [A-Za-z][A-Za-z0-9_]{0,63} (1-64 characters, starting with a letter)'
  }
  return undefined
}

/**
 * Mirrors the server's definition validation: names, additive structure, and
 * limits. Returns bounded field-path violations for labeled editor errors.
 */
export function validateDraft(nodes: MetadataNode[]): DraftViolation[] {
  const violations: DraftViolation[] = []
  let nodeCount = 0

  function walk(children: MetadataNode[], atRoot: boolean, path: string) {
    const seen = new Set<string>()
    for (const node of children) {
      nodeCount += 1
      const label = path ? `${path}.${node.name}` : node.name
      const nameError = validName(node.name, atRoot)
      if (nameError) violations.push({ name: label, message: nameError })
      if (seen.has(node.name)) {
        violations.push({ name: label, message: `duplicate name '${node.name}' among siblings` })
      }
      seen.add(node.name)
      if (node.kind === 'object') walk(node.children ?? [], false, label)
    }
  }

  walk(nodes, true, '')
  if (nodeCount > METADATA_LIMITS.extensionNodesMax) {
    violations.push({
      name: 'definition',
      message: `definition exceeds the maximum of ${METADATA_LIMITS.extensionNodesMax} extension nodes`,
    })
  }
  const levels = objectLevels(nodes)
  if (levels > METADATA_LIMITS.objectLevelsMax) {
    violations.push({
      name: 'definition',
      message: `object nesting exceeds the maximum of ${METADATA_LIMITS.objectLevelsMax} object levels`,
    })
  }
  const bytes = definitionBytes({ children: nodes })
  if (bytes > METADATA_LIMITS.definitionBytesMax) {
    violations.push({
      name: 'definition',
      message: `definition is ${bytes} bytes; the maximum is ${METADATA_LIMITS.definitionBytesMax}`,
    })
  }
  return violations
}

export function violationFor(
  violations: DraftViolation[],
  path: string,
): string | undefined {
  const exact = violations.find(v => v.name === path)
  if (exact) return exact.message
  return violations
    .filter(v => v.name.startsWith(`${path}.`) || v.name.startsWith(`${path} `))
    .map(v => `${v.name}: ${v.message}`)
    .join('; ') || undefined
}

/**
 * Deep-copies a saved revision's children into a fresh editable draft.
 * JSON round-trip instead of structuredClone: the saved tree arrives as a
 * reactive proxy, which structuredClone cannot clone, and definitions are
 * plain JSON documents by construction.
 */
export function draftFromSaved(children: MetadataNode[] | undefined): MetadataNode[] {
  return JSON.parse(JSON.stringify(children ?? [])) as MetadataNode[]
}

/**
 * Mutations the recursive node editor calls through the injected draft
 * mutator. `parent` is the enclosing object node, or null for the root.
 */
export interface DraftMutations {
  rename(node: MetadataNode, value: string): void
  setGenerator(node: MetadataNode, value: string): void
  addField(parent: MetadataNode | null): void
  addObject(parent: MetadataNode | null): void
  remove(node: MetadataNode): void
  violation(path: string): string | undefined
  isLocked(path: string): boolean
  busy(): boolean
}

/** Collects the dotted path of every node in an additive definition tree. */
function collectPaths(nodes: MetadataNode[], parent: string, into: Set<string>): void {
  for (const node of nodes) {
    const path = parent ? `${parent}.${node.name}` : node.name
    into.add(path)
    if (node.kind === 'object') collectPaths(node.children ?? [], path, into)
  }
}

/**
 * Dotted paths of nodes present in `current` but absent from `predecessor`:
 * what an additive revision adds. Saved revisions never change or remove
 * existing nodes, so additions are the only difference a diff needs to show.
 */
export function addedPaths(predecessor: MetadataNode[], current: MetadataNode[]): string[] {
  const existing = new Set<string>()
  collectPaths(predecessor, '', existing)
  const added: string[] = []
  const walk = (nodes: MetadataNode[], parent: string) => {
    for (const node of nodes) {
      const path = parent ? `${parent}.${node.name}` : node.name
      if (!existing.has(path)) added.push(path)
      if (node.kind === 'object') walk(node.children ?? [], path)
    }
  }
  walk(current, '')
  return added
}

/** Compact canonical JSON for read-only display of definitions and schemas. */
export function canonicalJson(value: unknown): string {
  return JSON.stringify(value, null, 2)
}
