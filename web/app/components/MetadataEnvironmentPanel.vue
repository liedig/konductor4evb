<script setup lang="ts">
import type { TableColumn } from '~/components/NxTable.vue'
import { ApiError } from '~/composables/useApi'
import type {
  MetadataActivationAudit,
  MetadataEnvironmentState,
  MetadataReadinessReport,
  MetadataRevision,
  SchemaSummary,
} from '~/types/api'

/**
 * Environment metadata activation surface: active pointer per environment,
 * explicit per-environment activation (readiness + timestamped affected-schema
 * inventory before an explicit confirmation), and the activation audit trail.
 * Observations are timestamped and never presented as proof of consumer
 * compatibility; the server re-runs authoritative checks on activation.
 */
const props = withDefaults(defineProps<{
  environments?: MetadataEnvironmentState[]
  revisions?: MetadataRevision[]
  admin?: boolean
  pending?: boolean
}>(), {
  environments: () => [],
  revisions: () => [],
  admin: false,
  pending: false,
})

const emit = defineEmits<{ changed: [] }>()

const metadata = useMetadata()
const catalog = useCatalog()
const toast = useToast()

const stateColumns: TableColumn[] = [
  { key: 'environment', label: 'Environment' },
  { key: 'activeRevision', label: 'Active revision' },
  { key: 'generation', label: 'Generation' },
  { key: 'activatedBy', label: 'Activated by' },
  { key: 'activatedAt', label: 'Activated at' },
  { key: 'actions', label: '' },
]

const auditColumns: TableColumn[] = [
  { key: 'occurredAt', label: 'Time' },
  { key: 'actor', label: 'Actor' },
  { key: 'transition', label: 'Transition' },
  { key: 'operationId', label: 'Operation' },
]

const prepareTarget = ref<MetadataEnvironmentState | null>(null)
const candidateRevision = ref<number | null>(null)
const readiness = ref<MetadataReadinessReport | null>(null)
const readinessAt = ref('')
const readinessPending = ref(false)
const readinessError = ref('')

const inventory = ref<SchemaSummary[]>([])
const inventoryNextToken = ref<string | undefined>()
const inventoryAt = ref('')
const inventoryPending = ref(false)
const inventoryError = ref('')

const confirmOpen = ref(false)
const activating = ref(false)
const activationError = ref('')
const staleState = ref(false)

const audits = ref<MetadataActivationAudit[]>([])
const auditsNextToken = ref<string | undefined>()
const auditsPending = ref(false)
const auditError = ref('')

/** Revisions available as candidates: 1..head, newest first. */
const candidateOptions = computed(() =>
  (props.revisions ?? [])
    .filter(revision => revision.revision >= 1)
    .map(revision => ({ value: String(revision.revision), label: `M${revision.revision}` })),
)

function when(timestamp?: string): string {
  if (!timestamp) return '—'
  return new Date(timestamp).toLocaleString()
}

async function loadAudits(nextToken?: string): Promise<void> {
  auditsPending.value = true
  try {
    const page = await metadata.listActivations(nextToken)
    if (nextToken) audits.value.push(...(page.activations ?? []))
    else audits.value = page.activations ?? []
    auditsNextToken.value = page.nextToken
  } catch (error) {
    auditError.value = error instanceof ApiError ? error.message : 'Failed to load activation history'
  } finally {
    auditsPending.value = false
  }
}

function prepareActivation(environment: MetadataEnvironmentState): void {
  prepareTarget.value = environment
  candidateRevision.value = props.revisions[0]?.revision ?? null
  readiness.value = null
  readinessAt.value = ''
  readinessError.value = ''
  resetInventory()
  loadInventory()
  staleState.value = false
  activationError.value = ''
}

async function checkReadiness(): Promise<void> {
  if (!prepareTarget.value || candidateRevision.value === null || readinessPending.value) return
  readinessPending.value = true
  readinessError.value = ''
  try {
    readiness.value = await metadata.readiness({
      revision: candidateRevision.value,
      environment: prepareTarget.value.environment,
    })
    readinessAt.value = new Date().toISOString()
  } catch (error) {
    readiness.value = null
    readinessError.value = error instanceof ApiError ? error.message : 'Readiness check failed'
  } finally {
    readinessPending.value = false
  }
}

function resetInventory(): void {
  inventory.value = []
  inventoryNextToken.value = undefined
  inventoryAt.value = ''
  inventoryError.value = ''
}

/**
 * Affected current-schema inventory for the prepared environment: the
 * catalog's current business versions and active metadata revisions. This
 * is an observation with a capture time, not a compatibility snapshot.
 */
async function loadInventory(nextToken?: string): Promise<void> {
  if (!prepareTarget.value || inventoryPending.value) return
  inventoryPending.value = true
  inventoryError.value = ''
  try {
    const page = await catalog.listSchemas({
      environment: prepareTarget.value.environment,
      nextToken,
    })
    if (nextToken) inventory.value.push(...(page.items ?? []))
    else inventory.value = page.items ?? []
    inventoryNextToken.value = page.nextToken
    inventoryAt.value = new Date().toISOString()
  } catch (error) {
    inventoryError.value = error instanceof ApiError ? error.message : 'Failed to load affected schemas'
  } finally {
    inventoryPending.value = false
  }
}

function openConfirmation(): void {
  if (!prepareTarget.value || candidateRevision.value === null || activating.value) return
  activationError.value = ''
  confirmOpen.value = true
}

function closeConfirmation(): void {
  if (activating.value) return
  confirmOpen.value = false
}

async function activate(): Promise<void> {
  if (!prepareTarget.value || candidateRevision.value === null || activating.value) return
  activating.value = true
  activationError.value = ''
  try {
    const result = await metadata.activate({
      revision: candidateRevision.value,
      environment: prepareTarget.value.environment,
      expectedActiveRevision: prepareTarget.value.activeRevision,
      operationId: crypto.randomUUID(),
    })
    toast.success(
      result.alreadyActive
        ? `M${result.activeRevision} was already active for ${prepareTarget.value.environment}`
        : `${prepareTarget.value.environment} is now active at M${result.activeRevision}`,
    )
    confirmOpen.value = false
    prepareTarget.value = null
    emit('changed')
    loadAudits()
  } catch (error) {
    if (error instanceof ApiError && error.status === 409) {
      staleState.value = true
      activationError.value =
        'The environment moved since this screen was opened. Authoritative state reloaded — review the refreshed identity, pick the candidate again, and confirm once more.'
      confirmOpen.value = false
      emit('changed')
    } else {
      activationError.value = (error instanceof ApiError ? error.message : 'Activation failed')
    }
  } finally {
    activating.value = false
  }
}

/** Keeps an open prepare dialog's identity in step with authoritative state. */
watch(() => props.environments, (list) => {
  const target = prepareTarget.value
  if (!target) return
  const fresh = list?.find(environment => environment.environment === target.environment)
  if (fresh && fresh !== target) prepareTarget.value = fresh
}, { deep: true })

onMounted(() => {
  loadAudits()
})
</script>

<template>
  <div class="flex flex-col gap-6">
    <NxCard title="Environments">
      <NxTable
        v-if="environments.length"
        :columns="stateColumns"
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
        <template #cell-actions="{ item }">
          <NxButton
            v-if="admin"
            variant="ghost"
            size="sm"
            :aria-label="`Prepare activation for ${item.environment}`"
            @click="prepareActivation(item as unknown as MetadataEnvironmentState)"
          >
            Prepare activation
          </NxButton>
        </template>
      </NxTable>
      <NxEmptyState
        v-else-if="!pending"
        title="No environments configured"
        body="Environment pointers appear once the platform's configured environments have active metadata."
      />
    </NxCard>

    <NxModal
      :open="!!prepareTarget"
      :title="prepareTarget ? `Prepare activation — ${prepareTarget.environment}` : ''"
      @close="prepareTarget = null"
    >
      <div v-if="prepareTarget" class="flex max-h-[70vh] w-[42rem] flex-col gap-4 overflow-y-auto pr-1">
        <dl class="grid grid-cols-2 gap-2 text-sm">
          <dt class="text-muted">Environment</dt>
          <dd class="font-medium">{{ prepareTarget.environment }}</dd>
          <dt class="text-muted">Current active revision</dt>
          <dd>M{{ prepareTarget.activeRevision }}{{ prepareTarget.legacy ? ' (legacy baseline)' : '' }}</dd>
        </dl>

        <NxSelect
          :model-value="candidateRevision === null ? '' : String(candidateRevision)"
          label="Candidate revision"
          :options="candidateOptions"
          hint="Activation moves this environment forward only; intermediate additive revisions may be skipped."
          @update:model-value="candidateRevision = Number($event)"
        />

        <div class="flex items-center gap-2">
          <NxButton
            variant="secondary"
            size="sm"
            :disabled="candidateRevision === null || readinessPending"
            @click="checkReadiness"
          >
            {{ readinessPending ? 'Checking…' : 'Check readiness' }}
          </NxButton>
          <span v-if="readinessAt" class="text-xs text-faint">Observed {{ when(readinessAt) }}</span>
        </div>
        <p v-if="readinessError" role="alert" class="text-sm text-danger">{{ readinessError }}</p>
        <div v-if="readiness" class="rounded-lg border border-border bg-surface-raised p-4 text-sm">
          <p>
            Definition recomplies exactly:
            <strong>{{ readiness.checks?.definitionRecompilesExactly ? 'yes' : 'no' }}</strong>
          </p>
          <p>
            Generated values satisfy the contract:
            <strong>{{ readiness.checks?.generatedValuesSatisfyContract ? 'yes' : 'no' }}</strong>
          </p>
          <p>
            Example metadata size:
            <strong>{{ readiness.checks?.exampleMetadataBytes ?? '—' }} bytes</strong>
            of a <strong>{{ readiness.checks?.sizeBudgetBytes ?? '—' }}</strong> byte budget
          </p>
        </div>

        <div>
          <div class="mb-2 flex items-center justify-between">
            <h3 class="text-sm font-medium text-muted">Affected current schemas</h3>
            <span v-if="inventoryAt" class="text-xs text-faint">Observed {{ when(inventoryAt) }}</span>
          </div>
          <p v-if="inventoryPending" class="text-sm text-muted">Loading…</p>
          <p v-else-if="inventoryError" role="alert" class="text-sm text-danger">{{ inventoryError }}</p>
          <p v-else-if="!inventory.length" class="text-sm text-faint">
            No current schemas publish to this environment.
          </p>
          <ul v-else class="flex flex-col gap-1">
            <li
              v-for="schema in inventory"
              :key="schema.schemaName"
              class="flex items-center justify-between gap-4 rounded border border-border px-3 py-2 text-sm"
            >
              <span class="font-mono text-xs">{{ schema.schemaName }}</span>
              <span class="text-xs text-muted">
                business V{{ schema.latestVersions?.[prepareTarget.environment] ?? '—' }}
                · metadata M{{ schema.metadataRevisions?.[prepareTarget.environment] ?? '—' }}
              </span>
            </li>
          </ul>
          <NxButton
            v-if="inventoryNextToken"
            variant="ghost"
            size="sm"
            class="mt-2"
            :disabled="inventoryPending"
            @click="loadInventory(inventoryNextToken)"
          >
            Load more schemas
          </NxButton>
        </div>

        <p v-if="staleState" role="alert" class="rounded-lg border border-warning bg-warning/10 px-4 py-3 text-sm text-warning">
          {{ activationError }}
        </p>
        <p v-else-if="activationError" role="alert" class="text-sm text-danger">{{ activationError }}</p>

        <div class="flex items-center justify-end gap-2">
          <NxButton variant="secondary" size="sm" :disabled="activating" @click="prepareTarget = null">
            Close
          </NxButton>
          <NxButton variant="primary" size="sm" :disabled="activating" @click="openConfirmation">
            Continue to activation
          </NxButton>
        </div>
      </div>
    </NxModal>

    <MetadataActivationConfirm
      :open="confirmOpen"
      :environment="prepareTarget?.environment ?? ''"
      :current-revision="prepareTarget?.activeRevision ?? 0"
      :candidate-revision="candidateRevision ?? 0"
      :busy="activating"
      :already-acknowledged="staleState"
      @confirm="activate"
      @close="closeConfirmation"
    />

    <NxCard title="Activation history">
      <template #actions>
        <NxButton
          v-if="auditsNextToken"
          variant="ghost"
          size="sm"
          :disabled="auditsPending"
          @click="loadAudits(auditsNextToken)"
        >
          {{ auditsPending ? 'Loading…' : 'Load more' }}
        </NxButton>
      </template>
      <p v-if="auditError" role="alert" class="text-sm text-danger">{{ auditError }}</p>
      <NxTable
        v-if="audits.length"
        :columns="auditColumns"
        :items="(audits as unknown as Record<string, unknown>[])"
      >
        <template #cell-occurredAt="{ item }">{{ when(String(item.occurredAt ?? '')) }}</template>
        <template #cell-actor="{ item }">{{ item.actor }}</template>
        <template #cell-transition="{ item }">
          M{{ item.beforeRevision }} → M{{ item.afterRevision }}
        </template>
        <template #cell-operationId="{ item }">
          <span class="font-mono text-xs">{{ String(item.operationId ?? '').slice(0, 8) }}</span>
        </template>
      </NxTable>
      <NxEmptyState
        v-else-if="!auditsPending"
        title="No activations recorded"
        body="Every committed activation lands here with its actor and transition."
      />
    </NxCard>
  </div>
</template>
