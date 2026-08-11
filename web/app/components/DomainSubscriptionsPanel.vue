<script setup lang="ts">
import type { TableColumn } from '~/components/NxTable.vue'
import { ApiError } from '~/composables/useApi'
import type { SubscriptionSummary } from '~/types/api'

const props = defineProps<{ domain: string }>()

const catalog = useCatalog()

const subscriptions = ref<SubscriptionSummary[]>([])
const forbidden = ref(false)
const pending = ref(true)

const columns: TableColumn[] = [
  { key: 'consumerDomain', label: 'Consumer' },
  { key: 'schemaName', label: 'Schema' },
  { key: 'status', label: 'Status' },
]

onMounted(async () => {
  try {
    const response = await catalog.listSubscriptions({ domain: props.domain, direction: 'outbound' })
    subscriptions.value = (response.items ?? []).filter(s => s.status !== 'REMOVED')
  } catch (error) {
    if (error instanceof ApiError && error.status === 403) {
      forbidden.value = true
    }
  } finally {
    pending.value = false
  }
})
</script>

<template>
  <NxCard title="Outbound subscriptions">
    <p v-if="pending" class="text-sm text-muted">Loading…</p>
    <p v-else-if="forbidden" class="text-sm text-muted">
      Publisher access is required to see who consumes this domain's schemas.
    </p>
    <p v-else-if="subscriptions.length === 0" class="text-sm text-muted">
      No domain currently consumes this domain's schemas.
    </p>
    <NxTable v-else :columns="columns" :items="(subscriptions as unknown as Record<string, unknown>[])">
      <template #cell-schemaName="{ item }">
        <span class="font-mono text-xs">{{ item.schemaName }}</span>
      </template>
      <template #cell-status="{ item }">
        <NxStatusPill :status="String(item.status ?? '')" />
      </template>
    </NxTable>
  </NxCard>
</template>
