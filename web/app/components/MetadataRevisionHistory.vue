<script setup lang="ts">
import type { TableColumn } from '~/components/NxTable.vue'
import type { MetadataNode, MetadataRevision } from '~/types/api'

/**
 * Immutable revision history, newest first, with per-revision inspection:
 * the canonical definition, the exact compiled schema, and the additive
 * difference versus its predecessor. Revisions cannot be edited here —
 * a new revision is created from the editor instead.
 */
const props = withDefaults(defineProps<{
  revisions?: MetadataRevision[]
  nextToken?: string | undefined
  pendingMore?: boolean
}>(), {
  revisions: () => [],
  nextToken: undefined,
  pendingMore: false,
})

const emit = defineEmits<{ loadMore: [] }>()

const columns: TableColumn[] = [
  { key: 'revision', label: 'Revision' },
  { key: 'predecessor', label: 'Predecessor' },
  { key: 'definitionDigest', label: 'Definition digest' },
  { key: 'createdBy', label: 'Author' },
  { key: 'createdAt', label: 'Created' },
  { key: 'actions', label: '' },
]

const inspectTarget = ref<MetadataRevision | null>(null)

function when(timestamp?: string): string {
  if (!timestamp) return '—'
  return new Date(timestamp).toLocaleString()
}

/** The saved predecessor of the inspected revision, for the additive diff. */
function predecessorOf(revision: MetadataRevision): MetadataNode[] {
  const predecessor = (props.revisions ?? []).find(r => r.revision === revision.predecessor)
  return predecessor?.definition?.children ?? []
}

function addedFields(revision: MetadataRevision): string[] {
  return addedPaths(predecessorOf(revision), revision.definition?.children ?? [])
}

function inspectLabel(revision: MetadataRevision): string {
  return `Inspect revision M${revision.revision}`
}
</script>

<template>
  <NxCard title="Revision history">
    <template #actions>
      <NxButton
        v-if="nextToken"
        variant="ghost"
        size="sm"
        :disabled="pendingMore"
        @click="emit('loadMore')"
      >
        {{ pendingMore ? 'Loading…' : 'Load more' }}
      </NxButton>
    </template>

    <NxTable
      v-if="revisions.length"
      :columns="columns"
      :items="(revisions as unknown as Record<string, unknown>[])"
    >
      <template #cell-revision="{ item }">
        <span class="font-medium">M{{ item.revision }}</span>
      </template>
      <template #cell-predecessor="{ item }">
        {{ item.predecessor ? `M${item.predecessor}` : '—' }}
      </template>
      <template #cell-definitionDigest="{ item }">
        <span class="font-mono text-xs">{{ String(item.definitionDigest).slice(0, 12) }}</span>
      </template>
      <template #cell-createdBy="{ item }">{{ item.createdBy }}</template>
      <template #cell-createdAt="{ item }">{{ when(String(item.createdAt ?? '')) }}</template>
      <template #cell-actions="{ item }">
        <NxButton
          variant="ghost"
          size="sm"
          :aria-label="inspectLabel(item as unknown as MetadataRevision)"
          @click="inspectTarget = item as unknown as MetadataRevision"
        >
          Inspect
        </NxButton>
      </template>
    </NxTable>

    <NxEmptyState
      v-else
      title="No revisions yet"
      body="Saved configuration revisions are immutable and appear here, newest first."
    />

    <NxModal
      :open="!!inspectTarget"
      :title="inspectTarget ? `Revision M${inspectTarget.revision}` : ''"
      @close="inspectTarget = null"
    >
      <div v-if="inspectTarget" class="flex max-h-[70vh] flex-col gap-4 overflow-y-auto pr-1">
        <dl class="grid grid-cols-2 gap-2 text-sm">
          <dt class="text-muted">Predecessor</dt>
          <dd>M{{ inspectTarget.predecessor || '—' }}</dd>
          <dt class="text-muted">Author</dt>
          <dd>{{ inspectTarget.createdBy }}</dd>
          <dt class="text-muted">Created</dt>
          <dd>{{ when(inspectTarget.createdAt) }}</dd>
          <dt class="text-muted">Definition digest</dt>
          <dd class="font-mono text-xs">{{ String(inspectTarget.definitionDigest).slice(0, 24) }}</dd>
          <dt class="text-muted">Compiler format</dt>
          <dd>{{ inspectTarget.compilerFormat }}</dd>
        </dl>

        <div>
          <h3 class="mb-2 text-sm font-medium text-muted">Added versus predecessor</h3>
          <ul v-if="addedFields(inspectTarget).length" class="flex list-disc flex-col gap-1 pl-5 text-sm">
            <li v-for="field in addedFields(inspectTarget)" :key="field" class="font-mono text-xs">
              {{ field }}
            </li>
          </ul>
          <p v-else class="text-sm text-faint">
            Nothing added — the baseline revision.
          </p>
        </div>

        <div>
          <h3 class="mb-2 text-sm font-medium text-muted">Canonical definition</h3>
          <pre class="overflow-x-auto rounded-lg border border-border bg-surface-raised p-3 text-xs">{{ canonicalJson(inspectTarget.definition) }}</pre>
        </div>

        <div v-if="inspectTarget.metadataSchema">
          <h3 class="mb-2 text-sm font-medium text-muted">Compiled metadata schema</h3>
          <pre class="overflow-x-auto rounded-lg border border-border bg-surface-raised p-3 text-xs">{{ canonicalJson(inspectTarget.metadataSchema) }}</pre>
        </div>
      </div>
      <template #actions>
        <NxButton variant="secondary" size="sm" @click="inspectTarget = null">Close</NxButton>
      </template>
    </NxModal>
  </NxCard>
</template>
