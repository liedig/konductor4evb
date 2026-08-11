<script setup lang="ts">
import { ApiError } from '~/composables/useApi'
import { useSessionStore } from '~/stores/session'
import type { DomainSummary, SchemaSummary } from '~/types/api'

definePageMeta({ middleware: 'auth' })

const session = useSessionStore()
const domainsApi = useDomains()
const catalog = useCatalog()

const ownedDomains = ref<DomainSummary[]>([])
const consumerDomain = ref('')
const producerFilter = ref('')
const environmentFilter = ref('')

const schemas = ref<SchemaSummary[]>([])
const nextToken = ref<string | undefined>()
const pending = ref(false)
const errorMessage = ref('')

const activeOwned = computed(() => ownedDomains.value.filter(d => d.status === 'ACTIVE'))
const producerOptions = computed(() =>
  [...new Set(schemas.value.map(s => s.producerDomain).filter(Boolean))] as string[],
)
const environmentOptions = computed(() =>
  [...new Set(schemas.value.flatMap(s => Object.keys(s.latestVersions ?? {})))],
)

async function loadOwnedDomains() {
  try {
    const response = await domainsApi.list()
    ownedDomains.value = response.domains ?? []
    if (!session.isPlatformRole && activeOwned.value.length > 0) {
      consumerDomain.value = activeOwned.value[0]!.domainName
    }
  } catch {
    ownedDomains.value = []
  }
}

async function load(reset = true) {
  pending.value = true
  errorMessage.value = ''
  if (reset) {
    schemas.value = []
    nextToken.value = undefined
  }
  try {
    const response = await catalog.listSchemas({
      consumerDomain: consumerDomain.value || undefined,
      producerDomain: producerFilter.value || undefined,
      environment: environmentFilter.value || undefined,
      nextToken: nextToken.value,
    })
    schemas.value.push(...(response.items ?? []))
    nextToken.value = response.nextToken
  } catch (error) {
    errorMessage.value = error instanceof ApiError ? error.message : 'Failed to load the catalog'
  } finally {
    pending.value = false
  }
}

watch([consumerDomain, producerFilter, environmentFilter], () => load())

onMounted(async () => {
  await loadOwnedDomains()
  await load()
})

function detailHref(schema: SchemaSummary) {
  const query = consumerDomain.value ? `?consumer=${encodeURIComponent(consumerDomain.value)}` : ''
  return `/catalog/${encodeURIComponent(schema.producerDomain ?? '')}/${encodeURIComponent(schema.schemaName ?? '')}${query}`
}
</script>

<template>
  <div class="flex flex-col gap-6">
    <header>
      <h1 class="text-2xl font-semibold">Catalog</h1>
      <p class="mt-1 text-sm text-muted">Schemas your domains are entitled to consume.</p>
    </header>

    <div class="grid grid-cols-1 gap-3 sm:grid-cols-3">
      <NxSelect
        v-model="consumerDomain"
        label="Consumer domain"
        placeholder="Select a consumer domain"
        :options="activeOwned.map(d => ({ value: d.domainName, label: d.domainName }))"
      />
      <NxSelect
        v-model="producerFilter"
        label="Producer"
        :options="[{ value: '', label: 'All producers' }, ...producerOptions.map(p => ({ value: p, label: p }))]"
      />
      <NxSelect
        v-model="environmentFilter"
        label="Environment"
        :options="[{ value: '', label: 'All environments' }, ...environmentOptions.map(e => ({ value: e, label: e }))]"
      />
    </div>

    <p v-if="errorMessage" role="alert" class="text-sm text-danger">{{ errorMessage }}</p>

    <NxEmptyState
      v-else-if="!pending && schemas.length === 0"
      title="No schemas covered"
      body="This catalog lists schemas your consumer domain is entitled to. Ask the producer domain's team to grant access from their domain page."
    />

    <div v-else class="flex flex-col gap-3">
      <NuxtLink
        v-for="schema in schemas"
        :key="schema.schemaName"
        :to="detailHref(schema)"
        class="rounded-xl border border-border bg-surface p-4 shadow-sm transition-colors hover:border-border-strong"
      >
        <div class="flex items-center justify-between gap-4">
          <div class="min-w-0">
            <p class="truncate font-mono text-sm font-medium">{{ schema.schemaName }}</p>
            <p class="mt-0.5 text-sm text-muted">producer: {{ schema.producerDomain }}</p>
          </div>
          <div class="flex shrink-0 gap-2">
            <NxBadge v-for="(version, env) in schema.latestVersions" :key="env" tone="neutral">
              {{ env }} v{{ version }}
            </NxBadge>
          </div>
        </div>
      </NuxtLink>
      <div v-if="nextToken" class="flex justify-center">
        <NxButton variant="ghost" :loading="pending" @click="load(false)">Load more</NxButton>
      </div>
      <p v-if="pending" class="text-sm text-muted">Loading…</p>
    </div>
  </div>
</template>
