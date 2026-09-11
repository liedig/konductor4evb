<script setup lang="ts">
import { ApiError } from '~/composables/useApi'
import type {
  MetadataEnvironmentState,
  MetadataNode,
  MetadataPreviewResponse,
  MetadataRevision,
} from '~/types/api'

definePageMeta({ middleware: 'persona' })

const metadata = useMetadata()
const session = useSessionStore()
const toast = useToast()

const environments = ref<MetadataEnvironmentState[]>([])
const revisions = ref<MetadataRevision[]>([])
const revisionsNextToken = ref<string | undefined>()
const pending = ref(true)
const pendingMore = ref(false)
const errorMessage = ref('')

const editor = useTemplateRef<{ draft: MetadataNode[], valid: boolean, hasLocalChanges: boolean, reset: () => void } | null>('metadataEditor')

/** Newest revision is the additive head the editor extends. */
const headRevision = computed(() => revisions.value[0] ?? null)

// ---- Draft preview and save (platform-admin mutations) ----

const previewResult = ref<MetadataPreviewResponse | null>(null)
const previewPending = ref(false)
const previewError = ref('')
const savePending = ref(false)
const saveError = ref('')
const conflict = ref(false)

/** Authoritative refresh after activation: environments and revision history. */
async function refreshEnvironments(): Promise<void> {
  const [environmentList, revisionPage] = await Promise.all([
    metadata.listEnvironments(),
    metadata.listRevisions(),
  ])
  environments.value = environmentList.environments ?? []
  revisions.value = revisionPage.revisions ?? []
  revisionsNextToken.value = revisionPage.nextToken
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

async function previewDraft(): Promise<void> {
  if (!editor.value?.draft || previewPending.value || savePending.value) return
  previewPending.value = true
  previewError.value = ''
  try {
    previewResult.value = await metadata.preview({
      definition: { children: editor.value.draft },
    })
  } catch (error) {
    previewError.value = error instanceof ApiError ? error.message : 'Preview failed'
    previewResult.value = null
  } finally {
    previewPending.value = false
  }
}

async function saveRevision(): Promise<void> {
  const draft = editor.value?.draft
  if (!draft || !editor.value?.valid || savePending.value || previewPending.value) return
  if (!headRevision.value) {
    saveError.value = 'Revision 1 is created by deployment bootstrap; wait for the baseline to exist.'
    return
  }
  if (!editor.value.hasLocalChanges) {
    saveError.value = 'The draft matches the current head; a save would create an empty revision.'
    return
  }
  savePending.value = true
  saveError.value = ''
  try {
    const saved = await metadata.createRevision({
      expectedHead: headRevision.value.revision,
      operationId: crypto.randomUUID(),
      definition: { children: draft },
    })
    toast.success(`Revision M${saved.revision} saved; it is not activated`)
    previewResult.value = null
    conflict.value = false
    await refreshEnvironments()
    editor.value?.reset()
  } catch (error) {
    if (error instanceof ApiError && error.status === 409) {
      conflict.value = true
      await refreshEnvironments()
    } else {
      const fields = error instanceof ApiError && error.fieldList.length
        ? ` (${error.fieldList.map(field => `${field.name}: ${field.message}`).join('; ')})`
        : ''
      saveError.value = (error instanceof ApiError ? error.message : 'Save failed') + fields
    }
  } finally {
    savePending.value = false
  }
}

/** Conflict recovery: reload authoritative history, keep the local draft. */
async function reloadAfterConflict(): Promise<void> {
  if (savePending.value) return
  try {
    await refreshEnvironments()
    toast.info('History reloaded; your draft is kept for review')
  } catch (error) {
    errorMessage.value = error instanceof ApiError ? error.message : 'Failed to reload revisions'
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

    <template v-if="session.isPlatformAdmin">
      <MetadataDefinitionEditor ref="metadataEditor" :predecessor="headRevision" />

      <NxCard title="Draft actions">
        <div class="flex flex-wrap items-center gap-3">
          <NxButton
            variant="secondary"
            :disabled="previewPending || savePending"
            @click="previewDraft"
          >
            {{ previewPending ? 'Previewing…' : 'Preview draft' }}
          </NxButton>
          <NxButton
            variant="primary"
            :disabled="previewPending || savePending || !editor?.valid"
            @click="saveRevision"
          >
            {{ savePending ? 'Saving…' : 'Save new revision' }}
          </NxButton>
          <span v-if="editor?.hasLocalChanges" class="text-xs text-faint">Draft has unsaved changes</span>
          <span v-else class="text-xs text-faint">Draft matches the current head</span>
        </div>
        <p v-if="previewError" role="alert" class="mt-3 text-sm text-danger">{{ previewError }}</p>
        <p v-else-if="saveError" role="alert" class="mt-3 text-sm text-danger">{{ saveError }}</p>

        <div
          v-if="conflict"
          role="alert"
          class="mt-4 rounded-lg border border-warning bg-warning/10 px-4 py-3 text-sm text-warning"
        >
          <p>
            Another administrator saved a newer revision first. Your edits are kept
            locally — nothing of theirs was overwritten.
          </p>
          <div class="mt-2 flex items-center gap-2">
            <NxButton variant="secondary" size="sm" :disabled="savePending" @click="reloadAfterConflict">
              Reload latest history
            </NxButton>
            <span class="text-xs">Then review the difference in the history below before saving again.</span>
          </div>
        </div>
      </NxCard>

      <MetadataPreviewPanel :preview="previewResult" :pending="previewPending" />
    </template>

    <NxCard v-else title="Definition editor">
      <p class="text-sm text-muted">
        Configuration is read-only for auditors. Mutations require the
        platform-admin role; the API remains the enforcing boundary.
      </p>
    </NxCard>

    <MetadataEnvironmentPanel
      :environments="environments"
      :revisions="revisions"
      :admin="session.isPlatformAdmin"
      :pending="pending"
      @changed="refreshEnvironments"
    />

    <MetadataRevisionHistory
      :revisions="revisions"
      :next-token="revisionsNextToken"
      :pending-more="pendingMore"
      @load-more="loadMoreRevisions"
    />
  </div>
</template>
