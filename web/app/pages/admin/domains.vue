<script setup lang="ts">
import type { TableColumn } from '~/components/NxTable.vue'
import { ApiError } from '~/composables/useApi'
import type { DomainSummary } from '~/types/api'

definePageMeta({ middleware: 'persona' })

const domainsApi = useDomains()
const toast = useToast()

const domains = ref<DomainSummary[]>([])
const nextToken = ref<string | undefined>()
const pending = ref(true)
const errorMessage = ref('')

const stateTarget = ref<{ name: string, action: 'enable' | 'disable' } | null>(null)
const stateBusy = ref(false)

const columns: TableColumn[] = [
  { key: 'domainName', label: 'Domain' },
  { key: 'status', label: 'Status' },
  { key: 'ownerAccountId', label: 'Account' },
  { key: 'ownerEmail', label: 'Owner' },
  { key: 'actions', label: '' },
]

async function loadMore() {
  pending.value = true
  try {
    const response = await domainsApi.list(nextToken.value)
    domains.value.push(...(response.domains ?? []))
    nextToken.value = response.nextToken
  } catch (error) {
    errorMessage.value = error instanceof ApiError ? error.message : 'Failed to load domains'
  } finally {
    pending.value = false
  }
}

async function confirmStateChange() {
  if (!stateTarget.value) return
  stateBusy.value = true
  const { name, action } = stateTarget.value
  try {
    const result = await domainsApi.setState(name, action)
    toast.success(`${name} is now ${result.status}`)
    const entry = domains.value.find(d => d.domainName === name)
    if (entry && result.status) entry.status = result.status
    stateTarget.value = null
  } catch (error) {
    toast.error(error instanceof ApiError ? error.message : 'State change failed')
  } finally {
    stateBusy.value = false
  }
}

onMounted(loadMore)
</script>

<template>
  <div class="flex flex-col gap-6">
    <header>
      <h1 class="text-2xl font-semibold">All domains</h1>
      <p class="mt-1 text-sm text-muted">Platform view of every registered domain.</p>
    </header>

    <p v-if="errorMessage" role="alert" class="text-sm text-danger">{{ errorMessage }}</p>

    <NxTable :columns="columns" :items="(domains as unknown as Record<string, unknown>[])">
      <template #cell-domainName="{ item }">
        <NuxtLink :to="`/domains/${item.domainName}`" class="font-medium text-accent hover:underline">
          {{ item.domainName }}
        </NuxtLink>
      </template>
      <template #cell-status="{ item }">
        <NxStatusPill :status="String(item.status ?? '')" />
      </template>
      <template #cell-ownerAccountId="{ item }">
        <span class="font-mono text-xs">{{ item.ownerAccountId }}</span>
      </template>
      <template #cell-actions="{ item }">
        <NxButton
          v-if="item.status === 'ACTIVE'"
          variant="danger"
          size="sm"
          @click="stateTarget = { name: String(item.domainName), action: 'disable' }"
        >
          Disable
        </NxButton>
        <NxButton
          v-else-if="item.status === 'INACTIVE'"
          variant="secondary"
          size="sm"
          @click="stateTarget = { name: String(item.domainName), action: 'enable' }"
        >
          Enable
        </NxButton>
      </template>
      <template #empty>
        <NxEmptyState title="No domains" body="No domain has been registered yet." />
      </template>
    </NxTable>

    <div v-if="nextToken" class="flex justify-center">
      <NxButton variant="ghost" :loading="pending" @click="loadMore">Load more</NxButton>
    </div>

    <NxConfirm
      :open="stateTarget !== null"
      :title="stateTarget?.action === 'disable' ? `Disable ${stateTarget?.name}` : `Enable ${stateTarget?.name}`"
      :confirm-label="stateTarget?.action === 'disable' ? 'Disable' : 'Enable'"
      :danger="stateTarget?.action === 'disable'"
      :busy="stateBusy"
      @confirm="confirmStateChange"
      @cancel="stateTarget = null"
    >
      <template v-if="stateTarget?.action === 'disable'">
        Disabling rejects new publish commands and new subscription activations for this domain.
        Existing subscriptions, entitlements, and infrastructure are retained.
      </template>
      <template v-else>
        Enabling resumes publishing and subscription activation for this domain.
      </template>
    </NxConfirm>
  </div>
</template>
