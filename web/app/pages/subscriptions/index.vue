<script setup lang="ts">
import type { TableColumn } from '~/components/NxTable.vue'
import { ApiError } from '~/composables/useApi'
import type { DomainSummary, SubscriptionSummary } from '~/types/api'

definePageMeta({ middleware: 'auth' })

const domainsApi = useDomains()
const catalog = useCatalog()
const toast = useToast()

const ownedDomains = ref<DomainSummary[]>([])
const selectedDomain = ref('')
const subscriptions = ref<SubscriptionSummary[]>([])
const pending = ref(false)
const errorMessage = ref('')
const busyId = ref<string | null>(null)
const unsubscribeTarget = ref<SubscriptionSummary | null>(null)

const columns: TableColumn[] = [
  { key: 'schemaName', label: 'Schema' },
  { key: 'producerDomain', label: 'Producer' },
  { key: 'status', label: 'Status' },
  { key: 'environments', label: 'Environments' },
  { key: 'actions', label: '' },
]

async function loadOwnedDomains() {
  const response = await domainsApi.list()
  ownedDomains.value = (response.domains ?? []).filter(d => d.status === 'ACTIVE')
  if (ownedDomains.value.length > 0) {
    selectedDomain.value = ownedDomains.value[0]!.domainName
  }
}

async function load() {
  if (!selectedDomain.value) {
    subscriptions.value = []
    return
  }
  pending.value = true
  errorMessage.value = ''
  try {
    const response = await catalog.listSubscriptions({ domain: selectedDomain.value, direction: 'inbound' })
    subscriptions.value = (response.items ?? []).filter(s => s.status !== 'REMOVED')
  } catch (error) {
    errorMessage.value = error instanceof ApiError ? error.message : 'Failed to load subscriptions'
  } finally {
    pending.value = false
  }
}

async function setEnabled(item: SubscriptionSummary, enabled: boolean) {
  if (!item.subscriptionId || !item.consumerDomain) return
  busyId.value = item.subscriptionId
  try {
    const environments = Object.fromEntries(
      Object.keys(item.environments ?? {}).map(env => [env, { enabled }]),
    )
    await catalog.updateSubscriptionState({
      subscriptionId: item.subscriptionId,
      consumerDomain: item.consumerDomain,
      environments,
    })
    toast.success(`${enabled ? 'Resumed' : 'Paused'} ${item.schemaName}`)
    await load()
  } catch (error) {
    toast.error(error instanceof ApiError ? error.message : 'State update failed')
  } finally {
    busyId.value = null
  }
}

async function confirmUnsubscribe() {
  const target = unsubscribeTarget.value
  if (!target?.subscriptionId) return
  busyId.value = target.subscriptionId
  try {
    await catalog.unsubscribe(target.subscriptionId)
    toast.success(`Unsubscribed from ${target.schemaName} — removal is asynchronous`)
    unsubscribeTarget.value = null
    await load()
  } catch (error) {
    toast.error(error instanceof ApiError ? error.message : 'Unsubscribe failed')
  } finally {
    busyId.value = null
  }
}

function envSummary(item: SubscriptionSummary): string {
  const envs = item.environments ?? {}
  const parts = Object.entries(envs).map(([env, state]) =>
    `${env}: ${state.enabled === false ? 'paused' : (state.status ?? 'unknown').toLowerCase()}`,
  )
  return parts.join(' · ') || '—'
}

onMounted(async () => {
  try {
    await loadOwnedDomains()
  } catch {
    ownedDomains.value = []
  }
  await load()
})
watch(selectedDomain, load)
</script>

<template>
  <div class="flex flex-col gap-6">
    <header>
      <h1 class="text-2xl font-semibold">My subscriptions</h1>
      <p class="mt-1 text-sm text-muted">Inbound subscriptions for your domains.</p>
    </header>

    <div class="max-w-xs">
      <NxSelect
        v-model="selectedDomain"
        label="Consumer domain"
        placeholder="Select a domain"
        :options="ownedDomains.map(d => ({ value: d.domainName, label: d.domainName }))"
      />
    </div>

    <p v-if="errorMessage" role="alert" class="text-sm text-danger">{{ errorMessage }}</p>

    <NxEmptyState
      v-else-if="!pending && subscriptions.length === 0"
      title="No subscriptions"
      body="Subscribe from the catalog to start receiving events for this domain."
    >
      <template #actions>
        <NuxtLink to="/catalog"><NxButton variant="secondary">Open catalog</NxButton></NuxtLink>
      </template>
    </NxEmptyState>

    <NxTable v-else :columns="columns" :items="(subscriptions as unknown as Record<string, unknown>[])">
      <template #cell-schemaName="{ item }">
        <span class="font-mono text-xs">{{ item.schemaName }}</span>
      </template>
      <template #cell-status="{ item }">
        <div class="flex flex-col gap-1">
          <NxStatusPill :status="String(item.status ?? '')" />
          <span v-if="item.status === 'FAILED'" class="text-xs text-danger">activation failed</span>
        </div>
      </template>
      <template #cell-environments="{ item }">
        <span class="text-xs text-muted">{{ envSummary(item as unknown as SubscriptionSummary) }}</span>
      </template>
      <template #cell-actions="{ item }">
        <div class="flex justify-end gap-2">
          <template v-if="item.status === 'ACTIVE'">
            <NxButton
              variant="ghost"
              size="sm"
              :loading="busyId === item.subscriptionId"
              @click="setEnabled(item as unknown as SubscriptionSummary, false)"
            >
              Pause
            </NxButton>
            <NxButton
              variant="danger"
              size="sm"
              @click="unsubscribeTarget = item as unknown as SubscriptionSummary"
            >
              Unsubscribe
            </NxButton>
          </template>
          <NxButton
            v-else-if="item.status !== 'REMOVED'"
            variant="secondary"
            size="sm"
            :loading="busyId === item.subscriptionId"
            @click="setEnabled(item as unknown as SubscriptionSummary, true)"
          >
            Resume
          </NxButton>
        </div>
      </template>
    </NxTable>

    <NxConfirm
      :open="unsubscribeTarget !== null"
      :title="`Unsubscribe from ${unsubscribeTarget?.schemaName ?? ''}`"
      confirm-label="Unsubscribe"
      danger
      :busy="busyId !== null"
      @confirm="confirmUnsubscribe"
      @cancel="unsubscribeTarget = null"
    >
      Delivery stops and the subscription is removed asynchronously. You can subscribe again later while the entitlement still covers the schema.
    </NxConfirm>
  </div>
</template>
