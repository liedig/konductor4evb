<script setup lang="ts">
import { ApiError } from '~/composables/useApi'
import type { ComposedContract, SchemaDetail } from '~/types/api'

definePageMeta({ middleware: 'auth' })

const route = useRoute()
const catalog = useCatalog()
const toast = useToast()

const producerDomain = route.params.producer as string
const schemaName = route.params.schema as string
const consumerDomain = ref((route.query.consumer as string) ?? '')

const schema = ref<SchemaDetail | null>(null)
const uncovered = ref(false)
const errorMessage = ref('')
const pending = ref(true)
const metadataRevisions = ref<Record<string, number>>({})

const selectedEnvironment = ref('')
const contract = ref<ComposedContract | null>(null)
const contractPending = ref(false)
const contractError = ref('')

const subscribeOpen = ref(false)
const subscribing = ref(false)
const pollState = ref<'idle' | 'polling' | 'enabled' | 'failed' | 'settling'>('idle')
const failureReason = ref('')

async function load() {
  pending.value = true
  uncovered.value = false
  errorMessage.value = ''
  try {
    schema.value = await catalog.describeSchema(schemaName, {
      consumerDomain: consumerDomain.value || undefined,
      producerDomain,
    })
    await loadMetadataRevisions()
    if (!selectedEnvironment.value || !availableEnvironments.value.includes(selectedEnvironment.value)) {
      selectedEnvironment.value = availableEnvironments.value[0] ?? ''
    }
    await loadContract()
  } catch (error) {
    if (error instanceof ApiError && error.status === 403) {
      uncovered.value = true
      schema.value = null
    } else {
      errorMessage.value = error instanceof ApiError ? error.message : 'Failed to load the schema'
    }
  } finally {
    pending.value = false
  }
}

// The detail response carries business versions only; the producer-scoped
// catalog entry adds the independent metadata revision per environment.
// A consumer without catalog scope keeps the business-only badges.
async function loadMetadataRevisions() {
  // /schemas is paginated: keep paging until the schema is found or the
  // listing is exhausted, otherwise large producers silently lose the badges.
  try {
    let nextToken: string | undefined
    do {
      const listed = await catalog.listSchemas({ producerDomain, nextToken })
      const entry = (listed.items ?? []).find(item => item.schemaName === schemaName)
      if (entry) {
        metadataRevisions.value = entry.metadataRevisions ?? {}
        return
      }
      nextToken = listed.nextToken
    } while (nextToken)
    metadataRevisions.value = {}
  } catch {
    metadataRevisions.value = {}
  }
}

// One contract read per selection feeds both panels: the raw business
// document and the composed document always come from the same environment's
// current pair (design.md, section 9).
async function loadContract() {
  contract.value = null
  contractError.value = ''
  if (!selectedEnvironment.value) return
  contractPending.value = true
  try {
    contract.value = await catalog.getContract(schemaName, {
      domain: producerDomain,
      environment: selectedEnvironment.value,
      consumerDomain: consumerDomain.value || undefined,
    })
  } catch (error) {
    contractError.value = error instanceof ApiError ? error.message : 'Failed to load the contract'
  } finally {
    contractPending.value = false
  }
}

function selectEnvironment(env: string) {
  if (env === selectedEnvironment.value || contractPending.value) return
  selectedEnvironment.value = env
  loadContract()
}

async function confirmSubscribe() {
  if (!consumerDomain.value || !schema.value?.schemaName) return
  subscribing.value = true
  try {
    await catalog.subscribe({
      consumerDomain: consumerDomain.value,
      producerDomain,
      schemaName: schema.value.schemaName,
    })
    subscribeOpen.value = false
    pollSubscription(`${consumerDomain.value}.${producerDomain}.${schema.value.detailType}`)
  } catch (error) {
    toast.error(error instanceof ApiError ? error.message : 'Subscribe failed')
    subscribing.value = false
  }
}

// 202 flows poll on a short interval, then settle to a manual-refresh state
// (design.md, decision 6).
async function pollSubscription(subscriptionId: string) {
  pollState.value = 'polling'
  subscribing.value = false
  const deadline = Date.now() + 10_000
  while (Date.now() < deadline) {
    await new Promise(resolve => setTimeout(resolve, 2000))
    try {
      const detail = await catalog.describeSubscription(subscriptionId)
      if (detail.status === 'ACTIVE') {
        pollState.value = 'enabled'
        toast.success('Subscription is active')
        return
      }
      if (detail.status === 'FAILED') {
        pollState.value = 'failed'
        failureReason.value = detail.errorDetail
          ? `Activation failed: ${detail.errorDetail}`
          : 'Activation failed — see the subscription detail for the reason.'
        return
      }
    } catch {
      // transient read failures during activation: keep polling until deadline
    }
  }
  pollState.value = 'settling'
}

// Environments with a current schema version materialize at activation; the
// producer's remaining environments follow on promotion (awaiting promotion).
const availableEnvironments = computed(() => Object.keys(schema.value?.latestVersions ?? {}).sort())

function pretty(value?: string): string {
  if (!value) return ''
  try {
    return JSON.stringify(JSON.parse(value), null, 2)
  } catch {
    return value
  }
}

const prettyBusiness = computed(() => pretty(contract.value?.businessContent))
const prettyComposed = computed(() => pretty(contract.value?.composedDocument))
// Degradation path: when the contract read fails (for example a metadata
// 503, or an API stage predating the contract route), the raw business
// document from describe-schema still renders — labelled as the latest
// registered version, since describe content is not environment-scoped.
const prettyRegisteredContent = computed(() => pretty(schema.value?.content))

// Copy mirrors exactly what each panel renders: on the degradation path the
// business panel holds the registered document instead of contract content.
const contentCopyText = computed(() => contractError.value ? prettyRegisteredContent.value : prettyBusiness.value)
const { copied: contentCopied, copy: copyContent } = useCopyToClipboard()
const { copied: previewCopied, copy: copyPreview } = useCopyToClipboard()

// Entry points pass ?from=<internal path> so the back link returns to the
// referring page; default is the consumer catalog. Internal paths only:
// reject protocol-relative (//) and backslash forms — browsers normalize
// `/\evil.example` to an external authority.
const backHref = computed(() => {
  const from = route.query.from
  const internal = typeof from === 'string'
    && from.startsWith('/')
    && !from.startsWith('//')
    && !from.includes('\\')
  return internal ? from : '/catalog'
})
const backLabel = computed(() => {
  const match = backHref.value.match(/^\/domains\/([^/?]+)/)
  if (!match) return '← Catalog'
  const segment = match[1] ?? ''
  try {
    return `← ${decodeURIComponent(segment)} schemas`
  } catch {
    // Malformed percent-encoding in the from param: show it verbatim
    // rather than letting URIError break page rendering.
    return `← ${segment} schemas`
  }
})

onMounted(load)
watch(consumerDomain, load)
</script>

<template>
  <div class="flex flex-col gap-6">
    <header>
      <NuxtLink :to="backHref" class="text-sm text-muted hover:text-foreground">{{ backLabel }}</NuxtLink>
      <div class="mt-2 flex items-center justify-between gap-4">
        <h1 class="font-mono text-xl font-semibold break-all">{{ schemaName }}</h1>
        <NxButton v-if="schema && consumerDomain" @click="subscribeOpen = true">
          Subscribe {{ consumerDomain }}
        </NxButton>
      </div>
    </header>

    <p v-if="pending" class="text-sm text-muted">Loading…</p>

    <NxEmptyState
      v-else-if="uncovered"
      title="Not covered"
      :body="`${consumerDomain || 'Your domains'} hold no entitlement covering this schema. Ask the producer domain's team to grant access.`"
    />

    <p v-else-if="errorMessage" role="alert" class="text-sm text-danger">{{ errorMessage }}</p>

    <template v-else-if="schema">
      <NxCard title="Environment">
        <p class="mb-3 text-xs text-faint">
          Select an environment to load the contract it currently serves.
        </p>
        <div class="flex flex-wrap gap-2" role="group" aria-label="Environment">
          <button
            v-for="env in availableEnvironments"
            :key="env"
            type="button"
            :aria-pressed="selectedEnvironment === env"
            class="flex items-center gap-2 rounded-lg border px-3 py-1.5 text-sm transition-colors"
            :class="selectedEnvironment === env
              ? 'border-accent bg-accent/10 text-foreground'
              : 'border-border bg-surface text-muted hover:border-accent/50 hover:text-foreground'"
            @click="selectEnvironment(env)"
          >
            <span class="font-medium">{{ env }}</span>
            <NxBadge tone="info">v{{ schema.latestVersions?.[env] }}</NxBadge>
            <NxBadge v-if="metadataRevisions[env] !== undefined" tone="neutral">
              M{{ metadataRevisions[env] }}
            </NxBadge>
          </button>
        </div>
      </NxCard>

      <div v-if="availableEnvironments.length === 0" class="text-sm text-muted">
        This schema has no registered versions in any environment yet.
      </div>

      <div v-else class="flex flex-col gap-6">
        <NxCard title="Schema content">
          <template #actions>
            <NxButton
              variant="ghost"
              size="sm"
              aria-label="Copy schema content JSON"
              :disabled="!contentCopyText"
              @click="copyContent(contentCopyText)"
            >
              {{ contentCopied ? 'Copied' : 'Copy' }}
            </NxButton>
          </template>
          <template v-if="contractError">
            <p class="mb-3 text-xs text-faint">
              The latest registered business schema — the per-environment contract could not
              be loaded, so this is the highest version across environments.
            </p>
            <pre v-if="prettyRegisteredContent" class="max-h-[36rem] overflow-auto rounded-lg border border-border bg-background p-4 font-mono text-xs">{{ prettyRegisteredContent }}</pre>
            <p v-else class="text-sm text-muted">No registered content.</p>
          </template>
          <template v-else>
            <p class="mb-3 text-xs text-faint">
              The business schema published by {{ producerDomain }} at
              {{ selectedEnvironment }} v{{ contract?.schemaVersion }}.
            </p>
            <p v-if="contractPending" class="text-sm text-muted">Loading…</p>
            <pre v-else-if="contract" class="max-h-[36rem] overflow-auto rounded-lg border border-border bg-background p-4 font-mono text-xs">{{ prettyBusiness }}</pre>
          </template>
        </NxCard>

        <NxCard title="Schema preview">
          <template #actions>
            <NxButton
              variant="ghost"
              size="sm"
              aria-label="Copy composed contract JSON"
              :disabled="!prettyComposed"
              @click="copyPreview(prettyComposed)"
            >
              {{ previewCopied ? 'Copied' : 'Copy' }}
            </NxButton>
          </template>
          <p v-if="contractError" role="alert" class="text-sm text-danger">{{ contractError }}</p>
          <template v-else>
            <p class="mb-3 text-xs text-faint">
              The contract events in {{ selectedEnvironment }} validate against right now —
              business v{{ contract?.schemaVersion }}, metadata M{{ contract?.metadataRevision }}<template v-if="contract?.definitionDigest">, digest {{ contract.definitionDigest.slice(0, 12) }}</template>.
              The metadata block is platform-owned and read-only<template v-if="contract?.generators?.length">; generated fields:
                <span class="font-mono">{{ contract.generators.map(generator => generator.name).join(', ') }}</span>
              </template>.
            </p>
            <p v-if="contractPending" class="text-sm text-muted">Loading…</p>
            <pre v-else-if="contract" class="max-h-[36rem] overflow-auto rounded-lg border border-border bg-background p-4 font-mono text-xs">{{ prettyComposed }}</pre>
          </template>
        </NxCard>
      </div>

      <NxCard v-if="pollState !== 'idle'" title="Subscription activation">
        <p v-if="pollState === 'polling'" class="text-sm text-info">Subscription accepted — activating…</p>
        <p v-else-if="pollState === 'enabled'" class="text-sm text-success">Subscription is ENABLED.</p>
        <p v-else-if="pollState === 'failed'" class="text-sm text-danger">{{ failureReason }}</p>
        <p v-else class="text-sm text-warning">
          Still enabling after the polling window — check My subscriptions for the final state.
        </p>
        <template #actions>
          <NuxtLink to="/subscriptions"><NxButton variant="ghost" size="sm">My subscriptions</NxButton></NuxtLink>
        </template>
      </NxCard>
    </template>

    <NxModal :open="subscribeOpen" title="Confirm subscription" @close="subscribeOpen = false">
      <p class="text-sm text-muted">
        Subscribe <span class="font-medium text-foreground">{{ consumerDomain }}</span> to
        <span class="font-mono text-xs text-foreground">{{ schemaName }}</span>?
        The command is processed asynchronously; activation is tracked after submission.
      </p>
      <p class="mt-3 text-sm text-muted">
        Materializes now in:
        <span class="font-medium text-foreground">{{ availableEnvironments.join(', ') }}</span>.
        Environments the producer adds the schema to later follow automatically on promotion —
        no re-subscribe needed.
      </p>
      <template #actions>
        <NxButton variant="secondary" :disabled="subscribing" @click="subscribeOpen = false">Cancel</NxButton>
        <NxButton :loading="subscribing" @click="confirmSubscribe">Subscribe</NxButton>
      </template>
    </NxModal>
  </div>
</template>
