<script setup lang="ts">
import type { TableColumn } from '~/components/NxTable.vue'
import { ApiError } from '~/composables/useApi'
import type { MetadataEnvironmentState, MetadataRevision } from '~/types/api'

definePageMeta({ middleware: 'persona' })

const metadata = useMetadata()
const session = useSessionStore()

const environments = ref<MetadataEnvironmentState[]>([])
const revisions = ref<MetadataRevision[]>([])
const revisionsNextToken = ref<string | undefined>()
const pending = ref(true)
const pendingMore = ref(false)
const errorMessage = ref('')

/** Newest revision is the additive head the editor extends. */
const headRevision = computed(() => revisions.value[0] ?? null)

const environmentColumns: TableColumn[] = [
  { key: 'environment', label: 'Environment' },
  { key: 'activeRevision', label: 'Active revision' },
  { key: 'generation', label: 'Generation' },
  { key: 'activatedBy', label: 'Activated by' },
  { key: 'activatedAt', label: 'Activated at' },
]

const revisionColumns: TableColumn[] = [
  { key: 'revision', label: 'Revision' },
  { key: 'predecessor', label: 'Predecessor' },
  { key: 'definitionDigest', label: 'Definition digest' },
  { key: 'createdBy', label: 'Author' },
  { key: 'createdAt', label: 'Created' },
]

function when(timestamp?: string): string {
  if (!timestamp) return '—'
  return new Date(timestamp).toLocaleString()
}

async function loadMoreRevisions() {
  pendingMore.value = true
  try {
    const response = await metadata.listRevisions(revisionsNextToken.value)
    revisions.value.push(...(response.revisions ?? []))
    revisionsNextToken.value = response.nextToken
  } catch (error) {
    errorMessage.value = error instanceof ApiError ? error.message : 'Failed to load revisions'
  } finally {
    pendingMore.value = false
  }
}

onMounted(async () => {
  pending.value = true
  try {
    const [environmentList, revisionPage] = await Promise.all([
      metadata.listEnvironments(),
      metadata.listRevisions(),
    ])
    environments.value = environmentList.environments ?? []
    revisions.value = revisionPage.revisions ?? []
    revisionsNextToken.value = revisionPage.nextToken
  } catch (error) {
    errorMessage.value = error instanceof ApiError ? error.message : 'Failed to load metadata configuration'
  } finally {
    pending.value = false
  }
})
</script>

<template>
  <div class="flex flex-col gap-6">
    <header class="flex flex-wrap items-start justify-between gap-3">
      <div>
        <h1 class="text-2xl font-semibold">Metadata administration</h1>
        <p class="mt-1 text-sm text-muted">
          Platform-generated event metadata: immutable additive revisions and per-environment activation.
        </p>
      </div>
      <NxBadge :tone="session.isPlatformAdmin ? 'info' : 'neutral'">
        {{ session.isPlatformAdmin ? 'Read and write' : 'Read-only' }}
      </NxBadge>
    </header>

    <p v-if="errorMessage" role="alert" class="text-sm text-danger">{{ errorMessage }}</p>

    <MetadataDefaultsPanel />

    <MetadataDefinitionEditor
      v-if="session.isPlatformAdmin"
      :predecessor="headRevision"
    />
    <NxCard v-else title="Definition editor">
      <p class="text-sm text-muted">
        Configuration is read-only for auditors. Mutations require the
        platform-admin role; the API remains the enforcing boundary.
      </p>
    </NxCard>

    <NxCard title="Environments">
      <NxTable
        v-if="environments.length"
        :columns="environmentColumns"
        :items="(environments as unknown as Record<string, unknown>[])"
      >
        <template #cell-environment="{ item }">
          <span class="font-medium">{{ item.environment }}</span>
        </template>
        <template #cell-activeRevision="{ item }">
          <span class="inline-flex items-center gap-2">
            {{ item.activeRevision }}
            <NxBadge v-if="item.legacy" tone="warning">Legacy</NxBadge>
          </span>
        </template>
        <template #cell-generation="{ item }">{{ item.generation ?? '—' }}</template>
        <template #cell-activatedBy="{ item }">{{ item.activatedBy || '—' }}</template>
        <template #cell-activatedAt="{ item }">{{ when(String(item.activatedAt ?? '')) }}</template>
      </NxTable>
      <NxEmptyState
        v-else-if="!pending"
        title="No environments configured"
        body="Environment pointers appear once the platform's configured environments have active metadata."
      />
    </NxCard>

    <NxCard title="Revision history">
      <template #actions>
        <NxButton
          v-if="revisionsNextToken"
          variant="ghost"
          size="sm"
          :disabled="pendingMore"
          @click="loadMoreRevisions"
        >
          {{ pendingMore ? 'Loading…' : 'Load more' }}
        </NxButton>
      </template>
      <NxTable
        v-if="revisions.length"
        :columns="revisionColumns"
        :items="(revisions as unknown as Record<string, unknown>[])"
      >
        <template #cell-revision="{ item }">
          <span class="font-medium">M{{ item.revision }}</span>
        </template>
        <template #cell-predecessor="{ item }">M{{ item.predecessor }}</template>
        <template #cell-definitionDigest="{ item }">
          <span class="font-mono text-xs">{{ String(item.definitionDigest).slice(0, 12) }}</span>
        </template>
        <template #cell-createdBy="{ item }">{{ item.createdBy }}</template>
        <template #cell-createdAt="{ item }">{{ when(String(item.createdAt ?? '')) }}</template>
      </NxTable>
      <NxEmptyState
        v-else-if="!pending"
        title="No revisions yet"
        body="Saved configuration revisions are immutable and appear here, newest first."
      />
    </NxCard>
  </div>
</template>
