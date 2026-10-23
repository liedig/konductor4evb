<script setup lang="ts">
import type { TableColumn } from '~/components/NxTable.vue'

const props = withDefaults(defineProps<{ domain: string, variant?: 'summary' | 'page' }>(), { variant: 'summary' })

const { subscriptions, forbidden, loadFailed, pending, load } = useOutboundSubscriptions(() => props.domain)

const SUMMARY_LIMIT = 5

// Newest subscription first, so a producer sees who started consuming
// most recently; undated rows keep their API order behind dated ones.
const ordered = computed(() =>
  [...subscriptions.value].sort((a, b) =>
    (b.createdAt ?? '').localeCompare(a.createdAt ?? '')
    || (a.consumerDomain ?? '').localeCompare(b.consumerDomain ?? '')
    || (a.schemaName ?? '').localeCompare(b.schemaName ?? '')))

const shown = computed(() =>
  props.variant === 'summary' ? ordered.value.slice(0, SUMMARY_LIMIT) : ordered.value)

const hasMore = computed(() => props.variant === 'summary' && subscriptions.value.length > SUMMARY_LIMIT)

const columns: TableColumn[] = [
  { key: 'consumerDomain', label: 'Consumer' },
  { key: 'schemaName', label: 'Schema' },
  { key: 'status', label: 'Status' },
  { key: 'createdAt', label: 'Created' },
]

onMounted(load)
</script>

<template>
  <header v-if="variant === 'page'" class="mb-6">
    <h1 class="text-2xl font-semibold">Outbound subscriptions</h1>
    <p class="mt-1 text-sm text-muted">
      Every domain consuming <span class="font-mono text-xs">{{ domain }}</span>'s schemas — the
      domain page shows only the five most recent.
    </p>
  </header>
  <NxCard :title="variant === 'summary' ? 'Outbound subscriptions' : undefined">
    <p v-if="pending" class="text-sm text-muted">Loading…</p>
    <p v-else-if="forbidden" class="text-sm text-muted">
      Publisher access is required to see who consumes this domain's schemas.
    </p>
    <p v-else-if="loadFailed" role="alert" class="text-sm text-danger">
      Subscriptions could not be loaded — refresh to try again.
    </p>
    <p v-else-if="subscriptions.length === 0" class="text-sm text-muted">
      No domain currently consumes this domain's schemas.
    </p>
    <div v-else class="flex flex-col gap-3">
      <NxTable :columns="columns" :items="(shown as unknown as Record<string, unknown>[])">
        <template #cell-schemaName="{ item }">
          <span class="font-mono text-xs">{{ item.schemaName }}</span>
        </template>
        <template #cell-status="{ item }">
          <NxStatusPill :status="String(item.status ?? '')" />
        </template>
        <template #cell-createdAt="{ item }">
          <span class="text-muted">{{ item.createdAt ? new Date(String(item.createdAt)).toLocaleDateString() : '—' }}</span>
        </template>
      </NxTable>
      <NuxtLink
        v-if="hasMore"
        :to="`/domains/${encodeURIComponent(domain)}/subscriptions`"
        class="self-start text-sm text-accent hover:underline"
      >
        View all subscriptions
      </NuxtLink>
    </div>
  </NxCard>
</template>
