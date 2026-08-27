<script setup lang="ts">
import type { TableColumn } from '~/components/NxTable.vue'
import { ApiError } from '~/composables/useApi'
import type { DomainSummary } from '~/types/api'

definePageMeta({ middleware: 'auth' })

const domainsApi = useDomains()

const domains = ref<DomainSummary[]>([])
const nextToken = ref<string | undefined>()
const pending = ref(true)
const errorMessage = ref('')

const columns: TableColumn[] = [
  { key: 'domainName', label: 'Domain' },
  { key: 'status', label: 'Status' },
  { key: 'description', label: 'Description' },
  { key: 'ownerEmail', label: 'Owner' },
  { key: 'createdAt', label: 'Created' },
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

function formatDate(iso?: string) {
  return iso ? new Date(iso).toLocaleDateString() : '—'
}

// Detail pages authorize by owner binding, which only exists once a request
// is approved — pending and rejected rows render without a link.
function hasDetail(status?: string) {
  return status !== 'PENDING_APPROVAL' && status !== 'REJECTED'
}

onMounted(loadMore)
</script>

<template>
  <div class="flex flex-col gap-6">
    <header class="flex items-center justify-between gap-4">
      <div>
        <h1 class="text-2xl font-semibold">My domains</h1>
        <p class="mt-1 text-sm text-muted">Domains owned by your team groups, and requests awaiting review.</p>
      </div>
      <NuxtLink to="/domains/new">
        <NxButton>Request a domain</NxButton>
      </NuxtLink>
    </header>

    <p v-if="errorMessage" role="alert" class="text-sm text-danger">{{ errorMessage }}</p>

    <NxEmptyState
      v-if="!pending && domains.length === 0 && !errorMessage"
      title="No domains yet"
      body="Request a domain to start publishing and subscribing to events through the Konductor. Approval is a platform-team decision."
    >
      <template #actions>
        <NuxtLink to="/domains/new">
          <NxButton>Request a domain</NxButton>
        </NuxtLink>
      </template>
    </NxEmptyState>

    <template v-else>
      <NxTable :columns="columns" :items="(domains as unknown as Record<string, unknown>[])">
        <template #cell-domainName="{ item }">
          <NuxtLink
            v-if="hasDetail(item.status as string | undefined)"
            :to="`/domains/${item.domainName}`"
            class="font-medium text-accent hover:underline"
          >
            {{ item.domainName }}
          </NuxtLink>
          <span v-else class="font-medium">{{ item.domainName }}</span>
        </template>
        <template #cell-status="{ item }">
          <NxStatusPill :status="String(item.status ?? '')" />
        </template>
        <template #cell-description="{ item }">
          <span class="text-muted">{{ item.description || '—' }}</span>
        </template>
        <template #cell-createdAt="{ item }">
          <span class="text-muted">{{ formatDate(item.createdAt as string | undefined) }}</span>
        </template>
      </NxTable>
      <div v-if="nextToken" class="flex justify-center">
        <NxButton variant="ghost" :loading="pending" @click="loadMore">Load more</NxButton>
      </div>
      <p v-if="pending && domains.length === 0" class="text-sm text-muted">Loading…</p>
    </template>
  </div>
</template>
