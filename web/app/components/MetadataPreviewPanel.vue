<script setup lang="ts">
import type { TableColumn } from '~/components/NxTable.vue'
import type { MetadataPreviewResponse } from '~/types/api'

/**
 * Synthetic draft preview (POST /metadata/preview): the example block,
 * compiled schema, and generator catalogue the server would apply. Preview
 * is read-only — it saves nothing, publishes nothing, and moves no pointer.
 */
withDefaults(defineProps<{
  preview?: MetadataPreviewResponse | null
  pending?: boolean
  environment?: string
}>(), {
  preview: null,
  pending: false,
  environment: '',
})

const generatorColumns: TableColumn[] = [
  { key: 'path', label: 'Field' },
  { key: 'generator', label: 'Generator' },
  { key: 'source', label: 'Source' },
  { key: 'constraint', label: 'Validation' },
]
</script>

<template>
  <NxCard title="Draft preview">
    <template #actions>
      <NxBadge tone="warning">Not saved, not published, not activated</NxBadge>
    </template>

    <p v-if="pending" class="text-sm text-muted">Previewing…</p>

    <template v-else-if="preview">
      <p class="mb-4 text-sm text-muted">
        The draft would become revision M{{ preview.candidateRevision ?? '—'
        }}{{ environment ? ` for the ${environment} environment context` : '' }}.
        Synthetic generated values below; nothing is registered or emitted.
      </p>

      <div class="flex flex-col gap-4">
        <div v-if="preview.generators?.length">
          <h3 class="mb-2 text-sm font-medium text-muted">Generated fields</h3>
          <NxTable
            :columns="generatorColumns"
            :items="(preview.generators as unknown as Record<string, unknown>[])"
          >
            <template #cell-path="{ item }">
              <span class="font-mono text-xs">{{ item.path }}</span>
            </template>
          </NxTable>
        </div>

        <div v-if="preview.example">
          <h3 class="mb-2 text-sm font-medium text-muted">Synthetic example metadata block</h3>
          <pre class="overflow-x-auto rounded-lg border border-border bg-surface-raised p-4 text-xs">{{ canonicalJson(preview.example) }}</pre>
        </div>

        <div v-if="preview.metadataSchema">
          <h3 class="mb-2 text-sm font-medium text-muted">Compiled metadata schema</h3>
          <pre class="max-h-72 overflow-auto rounded-lg border border-border bg-surface-raised p-4 text-xs">{{ canonicalJson(preview.metadataSchema) }}</pre>
        </div>

        <p v-if="preview.bounds" class="text-xs text-faint">
          Bounds: {{ preview.bounds.extensionNodesUsed }}/{{ preview.bounds.extensionNodesMax }}
          extension nodes, up to {{ preview.bounds.objectLevelsMax }} object levels,
          {{ preview.bounds.definitionBytesUsed }}/{{ preview.bounds.definitionBytesMax }} definition bytes.
        </p>
      </div>
    </template>

    <p v-else class="text-sm text-faint">
      Preview the draft to see the exact metadata schema, generated values, and bounds
      the next revision would apply. Preview never mutates configuration.
    </p>
  </NxCard>
</template>
