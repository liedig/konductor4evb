<script setup lang="ts">
import type { TableColumn } from '~/components/NxTable.vue'
import type { Entitlement } from '~/types/api'

const props = withDefaults(defineProps<{ domain: string, variant?: 'summary' | 'page' }>(), { variant: 'summary' })

const {
  entitlements,
  forbidden,
  loadFailed,
  pending,
  load,
  grantOpen,
  grantBusy,
  directoryEntries,
  scopeOptions,
  grantForm,
  openGrant,
  submitGrant,
  revokeTarget,
  revokeBusy,
  confirmRevoke,
} = useAccessGrants(() => props.domain)

const SUMMARY_LIMIT = 5

// Most recently granted first, so a producer sees the newest access on
// top; undated rows keep their API order behind dated ones.
const ordered = computed(() =>
  [...entitlements.value].sort((a, b) =>
    (b.createdAt ?? '').localeCompare(a.createdAt ?? '')
    || (a.consumerDomain ?? '').localeCompare(b.consumerDomain ?? '')
    || (a.scopePattern ?? '').localeCompare(b.scopePattern ?? '')))

const shown = computed(() =>
  props.variant === 'summary' ? ordered.value.slice(0, SUMMARY_LIMIT) : ordered.value)

const hasMore = computed(() => props.variant === 'summary' && entitlements.value.length > SUMMARY_LIMIT)

const columns: TableColumn[] = [
  { key: 'consumerDomain', label: 'Consumer domain' },
  { key: 'scopePattern', label: 'Scope' },
  { key: 'createdAt', label: 'Granted' },
  { key: 'actions', label: '' },
]

onMounted(load)
</script>

<template>
  <header v-if="variant === 'page'" class="mb-6">
    <h1 class="text-2xl font-semibold">Access grants</h1>
    <p class="mt-1 text-sm text-muted">
      Every consumer domain granted access to <span class="font-mono text-xs">{{ domain }}</span>'s
      schemas — the domain page shows only the five most recent. Grant and revoke work here too.
    </p>
  </header>
  <NxCard :title="variant === 'summary' ? 'Access grants' : undefined">
    <template #actions>
      <NxButton v-if="!forbidden" size="sm" @click="openGrant">Grant access</NxButton>
    </template>

    <p v-if="pending" class="text-sm text-muted">Loading…</p>
    <p v-else-if="forbidden" class="text-sm text-muted">
      Publisher access is required to manage this domain's grants.
    </p>
    <p v-else-if="loadFailed" role="alert" class="text-sm text-danger">
      Access grants could not be loaded — refresh to try again.
    </p>
    <p v-else-if="entitlements.length === 0" class="text-sm text-muted">
      No consumer domain has access to this domain's schemas yet.
    </p>
    <div v-else class="flex flex-col gap-3">
      <NxTable :columns="columns" :items="(shown as unknown as Record<string, unknown>[])">
        <template #cell-scopePattern="{ item }">
          <span class="font-mono text-xs">{{ item.scopePattern }}</span>
        </template>
        <template #cell-createdAt="{ item }">
          <span class="text-muted">{{ item.createdAt ? new Date(String(item.createdAt)).toLocaleDateString() : '—' }}</span>
        </template>
        <template #cell-actions="{ item }">
          <div class="flex justify-end">
            <NxButton variant="danger" size="sm" @click="revokeTarget = item as unknown as Entitlement">
              Revoke
            </NxButton>
          </div>
        </template>
      </NxTable>
      <NuxtLink
        v-if="hasMore"
        :to="`/domains/${encodeURIComponent(domain)}/grants`"
        class="self-start text-sm text-accent hover:underline"
      >
        View all grants
      </NuxtLink>
    </div>

    <NxModal :open="grantOpen" title="Grant access" @close="grantOpen = false">
      <div class="flex flex-col gap-4">
        <NxSelect
          v-model="grantForm.consumerDomain"
          label="Consumer domain"
          placeholder="Choose a domain"
          :options="directoryEntries.map(entry => ({ value: entry.domainName ?? '', label: entry.domainName ?? '' }))"
          required
        />
        <NxSelect
          v-model="grantForm.scopePattern"
          label="Covered schemas"
          placeholder="Choose coverage"
          :options="scopeOptions"
          hint="The wildcard covers future schemas too."
          required
        />
      </div>
      <template #actions>
        <NxButton variant="secondary" :disabled="grantBusy" @click="grantOpen = false">Cancel</NxButton>
        <NxButton
          :loading="grantBusy"
          :disabled="!grantForm.consumerDomain || !grantForm.scopePattern"
          @click="submitGrant"
        >
          Grant
        </NxButton>
      </template>
    </NxModal>

    <NxConfirm
      :open="revokeTarget !== null"
      :title="`Revoke access for ${revokeTarget?.consumerDomain ?? ''}`"
      confirm-label="Revoke"
      danger
      :busy="revokeBusy"
      @confirm="confirmRevoke"
      @cancel="revokeTarget = null"
    >
      Revoking removes coverage for <span class="font-mono text-xs">{{ revokeTarget?.scopePattern }}</span>.
      Active subscriptions it covers will be disabled asynchronously.
    </NxConfirm>
  </NxCard>
</template>
