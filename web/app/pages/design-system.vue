<script setup lang="ts">
import type { TableColumn } from '~/components/NxTable.vue'

// Living reference for docs/development/design-system.md — every documented
// primitive renders here. Excluded from navigation; referenced by the doc.

const demoColumns: TableColumn[] = [
  { key: 'domainName', label: 'Domain' },
  { key: 'status', label: 'Status' },
  { key: 'ownerEmail', label: 'Owner' },
]
const demoRows = [
  { domainName: 'orders', status: 'ACTIVE', ownerEmail: 'orders-team@example.com' },
  { domainName: 'payments', status: 'PENDING_APPROVAL', ownerEmail: 'pay-team@example.com' },
]

const inputValue = ref('')
const selectValue = ref('')
const filterValue = ref('')
const statuses = ['ACTIVE', 'PENDING_APPROVAL', 'PROVISIONING', 'AWAITING_PROMOTION', 'FAILED', 'INACTIVE']
</script>

<template>
  <div class="mx-auto flex max-w-4xl flex-col gap-10">
    <header>
      <h1 class="text-2xl font-semibold">Design system</h1>
      <p class="mt-1 text-sm text-muted">
        Living gallery for the primitives documented in
        <code class="font-mono text-xs">docs/development/design-system.md</code>.
      </p>
    </header>

    <NxCard title="Buttons">
      <div class="flex flex-wrap items-center gap-3">
        <NxButton>Primary</NxButton>
        <NxButton variant="secondary">Secondary</NxButton>
        <NxButton variant="danger">Danger</NxButton>
        <NxButton variant="ghost">Ghost</NxButton>
        <NxButton size="sm">Small</NxButton>
        <NxButton loading>Loading</NxButton>
        <NxButton disabled>Disabled</NxButton>
      </div>
    </NxCard>

    <NxCard title="Inputs">
      <div class="grid max-w-md gap-4">
        <NxInput v-model="inputValue" label="Domain name" placeholder="orders" hint="Lowercase letters, digits, and hyphens." required />
        <NxInput v-model="inputValue" label="With error" error="Domain name is required." />
        <NxInput label="Disabled input" model-value="Read only while busy" disabled />
        <NxSelect
          v-model="selectValue"
          label="Owning team"
          placeholder="Choose a team"
          :options="[
            { value: 'team-payments', label: 'team-payments' },
            { value: 'team-orders', label: 'team-orders' },
          ]"
          required
        />
        <NxSelect v-model="filterValue" label="Environment filter" :options="[{ value: '', label: 'All environments' }, { value: 'dev', label: 'Development' }]" hint="An empty value means all environments." />
        <NxSelect label="Select with error" :options="[]" error="Choose an environment." />
        <NxSelect label="Disabled select" :options="[{ value: '', label: 'Unavailable' }]" disabled />
      </div>
    </NxCard>

    <NxCard title="Badges and status pills">
      <div class="flex flex-wrap items-center gap-2">
        <NxBadge tone="success">success</NxBadge>
        <NxBadge tone="info">info</NxBadge>
        <NxBadge tone="warning">warning</NxBadge>
        <NxBadge tone="danger">danger</NxBadge>
        <NxBadge tone="neutral">neutral</NxBadge>
        <span class="mx-2 border-l border-border" />
        <NxStatusPill v-for="status in statuses" :key="status" :status="status" />
      </div>
    </NxCard>

    <NxCard title="Table">
      <NxTable :columns="demoColumns" :items="demoRows">
        <template #cell-status="{ item }">
          <NxStatusPill :status="String(item.status)" />
        </template>
      </NxTable>
      <div class="mt-4">
        <NxTable :columns="demoColumns" :items="[]">
          <template #empty>
            <NxEmptyState title="Nothing here" body="Empty table cells render this slot." />
          </template>
        </NxTable>
      </div>
    </NxCard>

    <GalleryOverlays />
    <GalleryNotifications />

    <NxCard title="Empty state">
      <NxEmptyState
        title="No domains yet"
        body="Request a domain to start publishing events through the Konductor."
      >
        <template #actions>
          <NxButton>Request a domain</NxButton>
        </template>
      </NxEmptyState>
    </NxCard>
  </div>
</template>
