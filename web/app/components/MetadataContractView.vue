<script setup lang="ts">
import { ApiError } from '~/composables/useApi'
import type { ComposedContract } from '~/types/api'

/**
 * Composed-contract inspection for one schema: the exact current pair for a
 * selected environment, or an explicitly selected historical pair. Historical
 * reads always carry both business version and metadata revision together —
 * the UI never substitutes the active metadata revision — and revision 0 is
 * the documented legacy selector for events predating metadata stamping.
 */
const props = withDefaults(defineProps<{
  schemaName: string
  producerDomain: string
  environments?: string[]
  consumerDomain?: string
  initialEnvironment?: string
}>(), {
  environments: () => [],
  consumerDomain: '',
  initialEnvironment: '',
})

const catalog = useCatalog()

const environment = ref(props.initialEnvironment || props.environments[0] || '')
const historical = ref(false)
const schemaVersionInput = ref('')
const metadataRevisionInput = ref('')
const selectorError = ref('')
const contract = ref<ComposedContract | null>(null)
const pending = ref(false)
const errorMessage = ref('')
const view = ref<'composed' | 'metadata' | 'business'>('composed')

watch(() => props.environments, (list) => {
  if (!environment.value && list.length) environment.value = list[0] ?? ''
}, { immediate: true })

function parseSelector(value: string): number | undefined {
  if (!/^\d+$/.test(value.trim())) return undefined
  return Number(value.trim())
}

/** All-or-nothing historical selectors; never a silent current-metadata pair. */
function validateSelectors(): { schemaVersion?: number, metadataRevision?: number } | null {
  selectorError.value = ''
  if (!historical.value) return {}
  const schemaVersion = parseSelector(schemaVersionInput.value)
  const metadataRevision = parseSelector(metadataRevisionInput.value)
  const hasVersion = schemaVersion !== undefined
  const hasRevision = metadataRevision !== undefined
  if (hasVersion !== hasRevision) {
    selectorError.value =
      'Historical inspection needs both the business version and the metadata revision (0 is the legacy selector).'
    return null
  }
  if (!hasVersion) {
    selectorError.value = 'Enter the business version and metadata revision, or switch back to the current pair.'
    return null
  }
  return { schemaVersion, metadataRevision }
}

async function load(): Promise<void> {
  const selectors = validateSelectors()
  if (!selectors || pending.value) return
  pending.value = true
  errorMessage.value = ''
  try {
    contract.value = await catalog.getContract(props.schemaName, {
      domain: props.producerDomain,
      environment: environment.value,
      consumerDomain: props.consumerDomain || undefined,
      schemaVersion: selectors.schemaVersion,
      metadataRevision: selectors.metadataRevision,
    })
  } catch (error) {
    contract.value = null
    errorMessage.value = error instanceof ApiError ? error.message : 'Failed to load the contract'
  } finally {
    pending.value = false
  }
}

function pretty(value?: string): string {
  if (!value) return ''
  try {
    return JSON.stringify(JSON.parse(value), null, 2)
  } catch {
    return value
  }
}

watch(environment, () => {
  contract.value = null
})
watch(historical, () => {
  selectorError.value = ''
})

onMounted(load)
</script>

<template>
  <div class="flex flex-col gap-4">
    <div class="flex flex-wrap items-end gap-3">
      <div class="w-44">
        <NxSelect
          v-model="environment"
          label="Environment"
          :options="environments.map(name => ({ value: name, label: name }))"
        />
      </div>
      <NxButton variant="secondary" size="sm" :disabled="pending" @click="load">
        {{ pending ? 'Loading…' : 'Load contract' }}
      </NxButton>
      <label class="flex items-center gap-2 text-sm text-muted">
        <input v-model="historical" type="checkbox">
        Historical pair
      </label>
    </div>

    <div v-if="historical" class="flex flex-wrap items-end gap-3">
      <div class="w-32">
        <NxInput
          v-model="schemaVersionInput"
          label="Business version"
          placeholder="2"
        />
      </div>
      <div class="w-32">
        <NxInput
          v-model="metadataRevisionInput"
          label="Metadata revision"
          placeholder="0 = legacy"
        />
      </div>
      <p class="max-w-md text-xs text-faint">
        Both selectors are required together; the active metadata revision is never substituted
        for a historical business version. Revision 0 is the legacy pre-stamping selector.
      </p>
    </div>
    <p v-if="selectorError" role="alert" class="text-sm text-danger">{{ selectorError }}</p>
    <p v-else-if="errorMessage" role="alert" class="text-sm text-danger">{{ errorMessage }}</p>

    <template v-if="contract">
      <div class="flex flex-wrap items-center gap-2">
        <NxBadge tone="info">{{ environment }} V{{ contract.schemaVersion }}</NxBadge>
        <NxBadge tone="neutral">{{ environment }} M{{ contract.metadataRevision }}</NxBadge>
        <NxBadge v-if="historical" tone="warning">Historical — explicit selection</NxBadge>
        <span v-if="contract.definitionDigest" class="font-mono text-xs text-faint">
          digest {{ contract.definitionDigest.slice(0, 12) }}
        </span>
      </div>

      <div class="flex gap-2">
        <NxButton
          v-for="option in (['composed', 'metadata', 'business'] as const)"
          :key="option"
          :variant="view === option ? 'primary' : 'ghost'"
          size="sm"
          @click="view = option"
        >
          {{ option === 'composed' ? 'Composed contract' : option === 'metadata' ? 'Inherited metadata' : 'Raw business' }}
        </NxButton>
      </div>

      <p v-if="view === 'composed'" class="text-xs text-faint">
        The full detail object ({metadata: &#123;…&#125;, data: &#123;…&#125;}) validates against this
        draft-4 document offline. Generated fields:
        <span v-if="contract.generators?.length" class="font-mono">
          {{ contract.generators.map(generator => generator.name).join(', ') }}
        </span>
        <span v-else>none for this revision.</span>
      </p>
      <pre v-if="view === 'composed'" class="max-h-80 overflow-auto rounded-lg border border-border bg-surface-raised p-4 text-xs">{{ pretty(contract.composedDocument) }}</pre>
      <p v-else-if="view === 'metadata'" class="text-xs text-faint">
        The platform-owned metadata block for M{{ contract.metadataRevision }}; read-only for every caller.
      </p>
      <pre v-if="view === 'metadata'" class="max-h-80 overflow-auto rounded-lg border border-border bg-surface-raised p-4 text-xs">{{ pretty(contract.metadataSchema) }}</pre>
      <p v-else class="text-xs text-faint">
        The raw business document, unchanged by metadata activation.
      </p>
      <pre v-if="view === 'business'" class="max-h-80 overflow-auto rounded-lg border border-border bg-surface-raised p-4 text-xs">{{ pretty(contract.businessContent) }}</pre>
    </template>

    <p v-else-if="!pending && !errorMessage && !selectorError" class="text-sm text-faint">
      Load the contract to inspect the exact pair this environment serves.
    </p>
  </div>
</template>
