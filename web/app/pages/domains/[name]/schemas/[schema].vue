<script setup lang="ts">
import { ApiError } from '~/composables/useApi'
import { useSessionStore } from '~/stores/session'
import type { PromotionView, SchemaDetail, SubscriptionSummary } from '~/types/api'

definePageMeta({ middleware: 'auth' })

const route = useRoute()
const catalog = useCatalog()
const config = useKonductorConfig()
const session = useSessionStore()
const toast = useToast()

const domain = route.params.name as string
const schemaName = route.params.schema as string
const eventName = schemaName.split('@')[1] ?? schemaName

const schema = ref<SchemaDetail | null>(null)
const promotions = ref<Record<string, PromotionView | undefined>>({})
const subscribers = ref<SubscriptionSummary[] | null>(null)
const pending = ref(true)
const notFound = ref(false)
const forbidden = ref(false)
const loadFailed = ref(false)

const appendOpen = ref(false)
const confirmEnv = ref<string | null>(null)
const confirmBusy = ref(false)

const authoringEnv = computed(() => config.environments[0] ?? '')
const targetEnvs = computed(() => config.environments.slice(1))
const latestByEnv = computed(() => schema.value?.latestVersions ?? {})
const authoringLatest = computed(() => latestByEnv.value[authoringEnv.value] ?? 0)

// Promote and append affordances are hidden from the auditor role; the API
// still enforces the publisher binding on every mutation.
const canManage = computed(() => !session.globalRoles.includes('platform-auditor'))

// Pipeline nodes follow the configured order; a target the schema has not
// reached still renders so the gap is visible. The promote affordance is
// offered only where the immediate predecessor holds versions the target
// lacks — the same adjacency rule the API and workflow enforce.
const pipeline = computed(() => config.environments.map((env, index) => {
  const predecessor = index > 0 ? config.environments[index - 1] : undefined
  const predecessorLatest = predecessor === undefined ? 0 : (latestByEnv.value[predecessor] ?? 0)
  const version = latestByEnv.value[env] as number | undefined
  return {
    env,
    version,
    behind: Math.max(0, authoringLatest.value - (latestByEnv.value[env] ?? 0)),
    isAuthoring: env === authoringEnv.value,
    promotable: predecessor !== undefined && predecessorLatest > (version ?? 0),
  }
}))

const maxVersion = computed(() => Math.max(0, ...Object.values(latestByEnv.value)))

const historyRows = computed(() => {
  const rows: { version: number, reached: string[], awaiting: string[] }[] = []
  for (let v = maxVersion.value; v >= 1; v -= 1) {
    const reached = config.environments.filter(env => (latestByEnv.value[env] ?? 0) >= v)
    rows.push({ version: v, reached, awaiting: config.environments.filter(env => !reached.includes(env)) })
  }
  return rows
})

const historyColumns = [
  { key: 'version', label: 'Version' },
  { key: 'reached', label: 'Available in' },
  { key: 'awaiting', label: 'Awaiting promotion' },
]

const subscriberColumns = [
  { key: 'consumerDomain', label: 'Consumer' },
  { key: 'environments', label: 'Environments' },
]

function inFlight(view?: PromotionView): boolean {
  return view !== undefined && ['VALIDATING', 'REPLAYING', 'REPINNING'].includes(view.status)
}

const anyInFlight = computed(() => targetEnvs.value.some(env => inFlight(promotions.value[env])))

// Content inspection: pipeline nodes view the environment's latest; history
// rows pin the row's version. Read-only, so the auditor role keeps it.
const contentView = ref<{ environment: string, version?: number } | null>(null)

const activePromotionEnvs = computed(() => targetEnvs.value.filter(env => promotions.value[env] !== undefined))

async function loadSchema() {
  try {
    schema.value = await catalog.describeSchema(schemaName, { producerDomain: domain })
  } catch (error) {
    if (error instanceof ApiError && error.status === 404) {
      notFound.value = true
    } else if (error instanceof ApiError && error.status === 403) {
      forbidden.value = true
    } else {
      loadFailed.value = true
      toast.error(error instanceof ApiError ? error.message : 'Failed to load the schema')
    }
  } finally {
    pending.value = false
  }
}

async function loadPromotions() {
  await Promise.all(targetEnvs.value.map(async (env) => {
    try {
      const view = await catalog.getPromotion({ domain, eventName, environment: env })
      promotions.value = { ...promotions.value, [env]: view }
    } catch (error) {
      if (!(error instanceof ApiError && error.status === 404)) {
        toast.error(error instanceof ApiError ? error.message : `Failed to load the promotion status for ${env}`)
      }
    }
  }))
}

async function loadSubscribers() {
  try {
    const response = await catalog.listSubscriptions({ domain, direction: 'outbound', schema: schemaName })
    subscribers.value = (response.items ?? []).filter(s => s.status !== 'REMOVED' && s.schemaName === schemaName)
  } catch (error) {
    if (error instanceof ApiError && error.status === 403) {
      subscribers.value = null
    } else {
      toast.error(error instanceof ApiError ? error.message : 'Failed to load subscribers')
    }
  }
}

let pollTimer: ReturnType<typeof setTimeout> | undefined

function schedulePoll() {
  if (pollTimer !== undefined) return
  pollTimer = setTimeout(async () => {
    pollTimer = undefined
    await pollPromotions()
    if (anyInFlight.value) schedulePoll()
  }, 1500)
}

async function pollPromotions() {
  const before: Record<string, string | undefined> = {}
  for (const env of targetEnvs.value) before[env] = promotions.value[env]?.status
  await Promise.all(targetEnvs.value.filter(env => inFlight(promotions.value[env])).map(async (env) => {
    try {
      const view = await catalog.getPromotion({ domain, eventName, environment: env })
      promotions.value = { ...promotions.value, [env]: view }
    } catch {
      // A transient read failure must not stop the polling loop.
    }
  }))
  for (const env of targetEnvs.value) {
    const view = promotions.value[env]
    if (view && before[env] !== 'COMPLETED' && view.status === 'COMPLETED') {
      toast.success(`${eventName} is now at v${view.targetVersion ?? view.requestedVersion} in ${env}`)
      await loadSchema()
    }
  }
}

const confirmFrom = computed(() => (confirmEnv.value ? (latestByEnv.value[confirmEnv.value] ?? 0) : 0))
const confirmCount = computed(() => Math.max(0, authoringLatest.value - confirmFrom.value))
const confirmRepins = computed(() =>
  (subscribers.value ?? []).filter(s => confirmEnv.value && s.environments?.[confirmEnv.value]?.status === 'ACTIVE').length)

function askPromote(env: string) {
  confirmEnv.value = env
}

async function confirmPromote() {
  const env = confirmEnv.value
  if (!env) return
  confirmBusy.value = true
  try {
    const view = await catalog.requestPromotion({ domain, eventName, environment: env })
    promotions.value = { ...promotions.value, [env]: view }
    confirmEnv.value = null
    schedulePoll()
  } catch (error) {
    if (error instanceof ApiError && error.status === 409) {
      toast.error(`A promotion is already in progress for ${env} — its progress is shown below.`)
      confirmEnv.value = null
      await loadPromotions()
      schedulePoll()
    } else {
      toast.error(error instanceof ApiError ? error.message : 'The promotion request failed')
      confirmEnv.value = null
    }
  } finally {
    confirmBusy.value = false
  }
}

function phaseDetail(view: PromotionView): string {
  if (view.status === 'VALIDATING') return 'Comparing the version histories.'
  if (view.status === 'REPLAYING') {
    const done = view.replayedVersions.length
    return done > 0 ? `Replaying versions — ${done} written so far.` : 'Replaying versions.'
  }
  if (view.status === 'REPINNING') {
    return 'Re-pinning subscription rules — delivery continues while the widened rules drain (a few minutes by design).'
  }
  if (view.status === 'COMPLETED') {
    const repin = view.repin
    const suffix = repin && repin.matched > 0 ? `, ${repin.repinned} of ${repin.matched} subscriptions re-pinned` : ''
    return `Now at v${view.targetVersion ?? view.requestedVersion} in ${view.targetEnvironment}${suffix}.`
  }
  if (view.status === 'FAILED') {
    if (view.reason === 'DIVERGED') {
      return `Replay stopped: the version history in ${view.targetEnvironment} diverges from ${view.sourceEnvironment}. No versions were written — reconcile the target registry with the platform team before promoting again.`
    }
    if (view.reason === 'SKIPPED_ENVIRONMENT') {
      return `Refused: ${view.targetEnvironment} can only receive versions promoted from the environment immediately before it. Promote through each environment in order.`
    }
    if (view.reason === 'REPLAY_FAILED') {
      const lastReplayed = view.replayedVersions[view.replayedVersions.length - 1]
      const reached = lastReplayed !== undefined
        ? `${view.targetEnvironment} is now at v${lastReplayed}`
        : `no new versions were written to ${view.targetEnvironment}`
      return `Replay failed before reaching v${view.requestedVersion}: ${reached}. Fix the cause and promote again — already-replayed versions are skipped.`
    }
    return 'The promotion failed.'
  }
  return 'The promotion was cancelled.'
}

function envEntries(subscription: SubscriptionSummary): { env: string, status: string, pinned?: number, paused: boolean }[] {
  const entries = subscription.environments ?? {}
  return orderEnvironments(Object.keys(entries), config.environments).map(env => ({
    env,
    status: entries[env]?.status ?? 'PENDING',
    pinned: entries[env]?.pinnedVersion,
    paused: entries[env]?.enabled === false,
  }))
}

onMounted(async () => {
  await loadSchema()
  if (notFound.value || loadFailed.value) return
  await Promise.all([loadPromotions(), loadSubscribers()])
  if (anyInFlight.value) schedulePoll()
})

onBeforeUnmount(() => {
  if (pollTimer !== undefined) clearTimeout(pollTimer)
})
</script>

<template>
  <div class="flex flex-col gap-6">
    <header class="flex flex-wrap items-center justify-between gap-4">
      <div>
        <NuxtLink :to="`/domains/${encodeURIComponent(domain)}`" class="text-sm text-accent hover:underline">
          &larr; {{ domain }}
        </NuxtLink>
        <h1 class="mt-1 font-mono text-xl font-semibold">{{ schemaName }}</h1>
        <p class="mt-1 text-sm text-muted">
          Promotion pipeline, version history, and subscribers for this schema.
        </p>
      </div>
      <NxButton v-if="canManage && !pending && !notFound && !forbidden && !loadFailed" variant="secondary" @click="appendOpen = true">
        New version
      </NxButton>
    </header>

    <p v-if="pending" class="text-sm text-muted">Loading…</p>
    <NxEmptyState
      v-else-if="notFound"
      title="Schema not found"
      body="This schema does not exist for the domain, or you do not have access to it."
    >
      <template #actions>
        <NuxtLink :to="`/domains/${encodeURIComponent(domain)}`">
          <NxButton variant="secondary">Back to the domain</NxButton>
        </NuxtLink>
      </template>
    </NxEmptyState>
    <NxEmptyState
      v-else-if="forbidden"
      title="Publisher access required"
      body="You need a publisher binding on this domain to view this schema's pipeline, history, and promotions."
    >
      <template #actions>
        <NuxtLink :to="`/domains/${encodeURIComponent(domain)}`">
          <NxButton variant="secondary">Back to the domain</NxButton>
        </NuxtLink>
      </template>
    </NxEmptyState>
    <p v-else-if="loadFailed" role="alert" class="text-sm text-danger">The schema could not be loaded — refresh to try again.</p>

    <template v-else>
      <NxCard title="Promotion pipeline">
        <ol class="flex flex-wrap items-stretch gap-3">
          <li
            v-for="node in pipeline"
            :key="node.env"
            class="flex min-w-40 flex-col gap-2 rounded-lg border border-border bg-surface-raised p-3"
          >
            <div class="flex items-center gap-2">
              <span class="font-medium">{{ node.env }}</span>
              <NxBadge v-if="node.isAuthoring" tone="info">authoring</NxBadge>
            </div>
            <div class="flex items-center gap-2">
              <NxBadge v-if="node.version !== undefined" tone="neutral">v{{ node.version }}</NxBadge>
              <span v-else class="text-xs text-muted">not promoted yet</span>
              <NxBadge v-if="node.behind > 0" tone="warning">{{ node.behind }} behind</NxBadge>
            </div>
            <NxButton
              v-if="node.version !== undefined"
              variant="ghost"
              size="sm"
              :aria-label="`View schema content in ${node.env}`"
              @click="contentView = { environment: node.env }"
            >
              View content
            </NxButton>
            <NxButton
              v-if="canManage && node.promotable && !inFlight(promotions[node.env])"
              size="sm"
              @click="askPromote(node.env)"
            >
              Promote to {{ node.env }}
            </NxButton>
            <span v-else-if="inFlight(promotions[node.env])" class="text-xs text-muted">promotion in progress</span>
          </li>
        </ol>
      </NxCard>

      <NxCard v-if="activePromotionEnvs.length > 0" title="Promotions">
        <ul class="flex flex-col gap-3">
          <li v-for="env in activePromotionEnvs" :key="env" class="flex flex-col gap-1">
            <div class="flex items-center gap-2">
              <span class="font-medium">{{ promotions[env]?.sourceEnvironment }} → {{ env }}</span>
              <NxStatusPill :status="promotions[env]?.status ?? ''" />
            </div>
            <p class="text-sm text-muted">{{ phaseDetail(promotions[env]!) }}</p>
          </li>
        </ul>
      </NxCard>

      <NxCard title="Version history">
        <NxTable :columns="historyColumns" :items="(historyRows as unknown as Record<string, unknown>[])">
          <template #cell-version="{ item }">
            <span class="font-mono text-xs">v{{ item.version }}</span>
          </template>
          <template #cell-reached="{ item }">
            <span class="flex flex-wrap gap-1">
              <button
                v-for="env in (item.reached as string[])"
                :key="env"
                type="button"
                class="cursor-pointer"
                :aria-label="`View v${item.version} content in ${env}`"
                @click="contentView = { environment: env, version: item.version as number }"
              >
                <NxBadge tone="neutral">{{ env }}</NxBadge>
              </button>
            </span>
          </template>
          <template #cell-awaiting="{ item }">
            <span v-if="(item.awaiting as string[]).length > 0" class="flex flex-wrap gap-1">
              <NxBadge v-for="env in (item.awaiting as string[])" :key="env" tone="warning">{{ env }}</NxBadge>
            </span>
            <span v-else class="text-xs text-muted">everywhere</span>
          </template>
        </NxTable>
      </NxCard>

      <NxCard title="Subscribers">
        <p v-if="subscribers === null" class="text-sm text-muted">
          Publisher access is required to see who consumes this schema.
        </p>
        <p v-else-if="subscribers.length === 0" class="text-sm text-muted">
          No domain currently consumes this schema.
        </p>
        <NxTable v-else :columns="subscriberColumns" :items="(subscribers as unknown as Record<string, unknown>[])">
          <template #cell-consumerDomain="{ item }">
            <span class="font-medium">{{ item.consumerDomain }}</span>
          </template>
          <template #cell-environments="{ item }">
            <span class="flex flex-wrap gap-1">
              <template v-for="entry in envEntries(item as unknown as SubscriptionSummary)" :key="entry.env">
                <NxBadge :tone="entry.paused ? 'neutral' : 'info'">
                  {{ entry.env }}<template v-if="entry.pinned !== undefined">&nbsp;v{{ entry.pinned }}</template><template v-if="entry.paused">&nbsp;paused</template>
                </NxBadge>
              </template>
            </span>
          </template>
        </NxTable>
      </NxCard>
    </template>

    <SchemaContentViewer
      :open="contentView !== null"
      :schema-name="schemaName"
      :domain="domain"
      :environment="contentView?.environment ?? ''"
      :version="contentView?.version"
      @close="contentView = null"
    />

    <SchemaAuthoringForm
      :open="appendOpen"
      :domain="domain"
      :event-name="eventName"
      :append-schema-name="schemaName"
      @close="appendOpen = false"
      @saved="loadSchema"
    />

    <NxConfirm
      :open="confirmEnv !== null"
      :title="`Promote ${eventName} to ${confirmEnv}`"
      confirm-label="Promote"
      :busy="confirmBusy"
      @confirm="confirmPromote"
      @cancel="confirmEnv = null"
    >
      <p>
        This replays {{ confirmCount === 1 ? 'version' : 'versions' }}
        v{{ confirmFrom + 1 }}–v{{ authoringLatest }} from {{ authoringEnv }} into {{ confirmEnv }}
        under the same version numbers.
      </p>
      <p v-if="confirmRepins > 0" class="mt-2">
        {{ confirmRepins }} active {{ confirmRepins === 1 ? 'subscription' : 'subscriptions' }} in {{ confirmEnv }}
        will be re-pinned to the replayed version; event delivery continues while the rules drain.
      </p>
      <p v-else class="mt-2">No active subscriptions in {{ confirmEnv }} need re-pinning.</p>
    </NxConfirm>
  </div>
</template>
