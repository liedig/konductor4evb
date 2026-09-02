<script setup lang="ts">
import { ApiError } from '~/composables/useApi'
import type { SchemaDetail } from '~/types/api'

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

const subscribeOpen = ref(false)
const subscribing = ref(false)
const pollState = ref<'idle' | 'polling' | 'enabled' | 'failed' | 'settling'>('idle')
const failureReason = ref('')

async function load() {
  pending.value = true
  uncovered.value = false
  try {
    schema.value = await catalog.describeSchema(schemaName, {
      consumerDomain: consumerDomain.value || undefined,
      producerDomain,
    })
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
        failureReason.value = 'Activation failed — see the subscription detail for the reason.'
        return
      }
    } catch {
      // transient read failures during activation: keep polling until deadline
    }
  }
  pollState.value = 'settling'
}

const prettyContent = computed(() => {
  if (!schema.value?.content) return ''
  try {
    return JSON.stringify(JSON.parse(schema.value.content), null, 2)
  } catch {
    return schema.value.content
  }
})

// Entry points pass ?from=<internal path> so the back link returns to the
// referring page; default is the consumer catalog. Internal paths only.
const backHref = computed(() => {
  const from = route.query.from
  return typeof from === 'string' && from.startsWith('/') && !from.startsWith('//') ? from : '/catalog'
})
const backLabel = computed(() => {
  const match = backHref.value.match(/^\/domains\/([^/?]+)/)
  return match ? `← ${decodeURIComponent(match[1] ?? '')} schemas` : '← Catalog'
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
      <NxCard title="Latest version per environment">
        <div class="flex flex-wrap gap-2">
          <NxBadge v-for="(version, env) in schema.latestVersions" :key="env" tone="info">
            {{ env }} v{{ version }}
          </NxBadge>
        </div>
      </NxCard>

      <NxCard title="Schema content">
        <pre class="overflow-x-auto rounded-lg border border-border bg-background p-4 font-mono text-xs">{{ prettyContent }}</pre>
      </NxCard>

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
      <template #actions>
        <NxButton variant="secondary" :disabled="subscribing" @click="subscribeOpen = false">Cancel</NxButton>
        <NxButton :loading="subscribing" @click="confirmSubscribe">Subscribe</NxButton>
      </template>
    </NxModal>
  </div>
</template>
