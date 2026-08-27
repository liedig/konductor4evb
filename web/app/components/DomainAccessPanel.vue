<script setup lang="ts">
import type { TableColumn } from '~/components/NxTable.vue'
import { ApiError } from '~/composables/useApi'
import type { DirectoryEntry, Entitlement, SchemaSummary } from '~/types/api'

const props = defineProps<{ domain: string }>()

const api = useApi()
const domainsApi = useDomains()
const catalog = useCatalog()
const toast = useToast()

const entitlements = ref<Entitlement[]>([])
const forbidden = ref(false)
const pending = ref(true)

const grantOpen = ref(false)
const grantBusy = ref(false)
const directoryEntries = ref<DirectoryEntry[]>([])
const producerSchemas = ref<SchemaSummary[]>([])
const grantForm = reactive({ consumerDomain: '', scopePattern: '' })

const revokeTarget = ref<Entitlement | null>(null)
const revokeBusy = ref(false)

const columns: TableColumn[] = [
  { key: 'consumerDomain', label: 'Consumer domain' },
  { key: 'scopePattern', label: 'Scope' },
  { key: 'createdAt', label: 'Granted' },
  { key: 'actions', label: '' },
]

async function load() {
  pending.value = true
  try {
    const response = await api.get<{ entitlements?: Entitlement[] }>('/entitlements', { producerDomain: props.domain })
    entitlements.value = response.entitlements ?? []
  } catch (error) {
    if (error instanceof ApiError && error.status === 403) {
      forbidden.value = true
    } else {
      toast.error(error instanceof ApiError ? error.message : 'Failed to load access grants')
    }
  } finally {
    pending.value = false
  }
}

async function openGrant() {
  grantForm.consumerDomain = ''
  grantForm.scopePattern = ''
  grantOpen.value = true
  const [directory, schemas] = await Promise.all([
    domainsApi.directory().catch(() => [] as DirectoryEntry[]),
    catalog.listSchemas({ producerDomain: props.domain }).catch(() => ({ items: [] as SchemaSummary[] })),
  ])
  directoryEntries.value = directory.filter(d => d.domainName !== props.domain)
  producerSchemas.value = schemas.items ?? []
}

const scopeOptions = computed(() => [
  { value: '*', label: '* — entire domain, including future schemas' },
  ...producerSchemas.value.map((s) => {
    const detailType = s.schemaName?.split('@')[1] ?? s.schemaName ?? ''
    return { value: detailType, label: `${detailType} — exact schema` }
  }),
])

async function submitGrant() {
  if (!grantForm.consumerDomain || !grantForm.scopePattern) return
  grantBusy.value = true
  try {
    await api.post('/entitlements', {
      consumerDomain: grantForm.consumerDomain,
      producerDomain: props.domain,
      scopePattern: grantForm.scopePattern,
    })
    toast.success(`Access granted to ${grantForm.consumerDomain}`)
    grantOpen.value = false
    await load()
  } catch (error) {
    toast.error(error instanceof ApiError ? error.message : 'Grant failed')
  } finally {
    grantBusy.value = false
  }
}

async function confirmRevoke() {
  const target = revokeTarget.value
  if (!target?.entitlementId) return
  revokeBusy.value = true
  try {
    await api.del(`/entitlements/${encodeURIComponent(target.entitlementId)}`)
    toast.success(`Access revoked for ${target.consumerDomain}`)
    revokeTarget.value = null
    await load()
  } catch (error) {
    toast.error(error instanceof ApiError ? error.message : 'Revoke failed')
  } finally {
    revokeBusy.value = false
  }
}

onMounted(load)
</script>

<template>
  <NxCard title="Access grants">
    <template #actions>
      <NxButton v-if="!forbidden" size="sm" @click="openGrant">Grant access</NxButton>
    </template>

    <p v-if="pending" class="text-sm text-muted">Loading…</p>
    <p v-else-if="forbidden" class="text-sm text-muted">
      Publisher access is required to manage this domain's grants.
    </p>
    <p v-else-if="entitlements.length === 0" class="text-sm text-muted">
      No consumer domain has access to this domain's schemas yet.
    </p>
    <NxTable v-else :columns="columns" :items="(entitlements as unknown as Record<string, unknown>[])">
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

    <NxModal :open="grantOpen" title="Grant access" @close="grantOpen = false">
      <div class="flex flex-col gap-4">
        <NxSelect
          v-model="grantForm.consumerDomain"
          label="Consumer domain"
          placeholder="Choose a domain"
          :options="directoryEntries.map(d => ({ value: d.domainName ?? '', label: d.domainName ?? '' }))"
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
