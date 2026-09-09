<script setup lang="ts">
import type { TableColumn } from '~/components/NxTable.vue'

/**
 * Protected defaults (design.md, decision 2): the built-in metadata block
 * every event carries. Configuration can neither define nor shadow these;
 * `idempotencyKey` is the one documented publisher-supplied exception.
 */
interface DefaultField {
  name: string
  type: string
  constraint: string
  source: string
}

const defaults: DefaultField[] = [
  {
    name: 'schemaVersion',
    type: 'integer',
    constraint: 'the event\u2019s stamped business schema version',
    source: 'resolved publisher business schema',
  },
  {
    name: 'idempotencyKey',
    type: 'string',
    constraint: 'publisher-supplied deduplication key',
    source: 'the supported publisher input exception',
  },
  {
    name: 'publishedAt',
    type: 'string',
    constraint: 'UTC RFC 3339 timestamp with millisecond precision',
    source: 'queue acceptance time, stamped at delivery',
  },
  {
    name: 'metadataRevision',
    type: 'integer',
    constraint: 'the event\u2019s stamped metadata revision',
    source: 'the environment\u2019s active revision',
  },
]

const columns: TableColumn[] = [
  { key: 'name', label: 'Field' },
  { key: 'type', label: 'Type' },
  { key: 'constraint', label: 'Validation' },
  { key: 'source', label: 'Source' },
  { key: 'locked', label: '' },
]
</script>

<template>
  <NxCard title="Protected defaults">
    <p class="mb-4 text-sm text-muted">
      Stamped on every event by the platform. Revisions are additive extensions only;
      these fields are immutable and mandatory.
    </p>
    <NxTable :columns="columns" :items="(defaults as unknown as Record<string, unknown>[])">
      <template #cell-name="{ item }">
        <span class="font-mono text-sm font-medium">{{ item.name }}</span>
      </template>
      <template #cell-locked>
        <NxBadge tone="neutral">Immutable</NxBadge>
      </template>
    </NxTable>
  </NxCard>
</template>
