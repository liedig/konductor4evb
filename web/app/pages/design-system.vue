<script setup lang="ts">
import type { TableColumn } from '~/components/NxTable.vue'

// Living reference for docs/development/design-system.md — every documented
// primitive renders here. Excluded from navigation; referenced by the doc.
const toast = useToast()

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
const modalOpen = ref(false)
const confirmOpen = ref(false)
const statuses = ['ACTIVE', 'PENDING_APPROVAL', 'PROVISIONING', 'FAILED', 'INACTIVE']
</script>

<template>
  <main class="mx-auto flex max-w-4xl flex-col gap-10 px-6 py-10">
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

    <NxCard title="Modal and confirm">
      <div class="flex gap-3">
        <NxButton variant="secondary" @click="modalOpen = true">Open modal</NxButton>
        <NxButton variant="danger" @click="confirmOpen = true">Open confirm</NxButton>
      </div>
      <NxModal :open="modalOpen" title="Modal" @close="modalOpen = false">
        <p class="text-sm text-muted">Dialog content goes here. Escape or the backdrop closes it.</p>
        <template #actions>
          <NxButton variant="secondary" @click="modalOpen = false">Close</NxButton>
        </template>
      </NxModal>
      <NxConfirm
        :open="confirmOpen"
        title="Disable domain"
        confirm-label="Disable"
        danger
        @confirm="confirmOpen = false"
        @cancel="confirmOpen = false"
      >
        Disabling rejects new publish and subscription activity. Existing subscriptions are retained.
      </NxConfirm>
    </NxCard>

    <NxCard title="Toast">
      <div class="flex gap-3">
        <NxButton variant="secondary" @click="toast.success('Registration request accepted')">Success</NxButton>
        <NxButton variant="secondary" @click="toast.error('AccessDenied: not an owner of this domain')">Error</NxButton>
      </div>
    </NxCard>

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
  </main>
</template>
