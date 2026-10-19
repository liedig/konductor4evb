<script setup lang="ts">
import { ApiError } from '~/composables/useApi'
import type { SchemaSummary } from '~/types/api'

const props = defineProps<{ domain: string, namespace?: string }>()

const catalog = useCatalog()
const toast = useToast()
const config = useKonductorConfig()

const schemas = ref<SchemaSummary[]>([])
const forbidden = ref(false)
const loadFailed = ref(false)
const pending = ref(true)

const formOpen = ref(false)

// Environment badges follow the configured order, not the API payload's.
function orderedEnvs(schema: SchemaSummary): string[] {
  return orderEnvironments(Object.keys(schema.latestVersions ?? {}), config.environments)
}

// Producer scope: publishers see their own domain's schemas without any
// consumer entitlement (GET /schemas?domain=... with no consumerDomain).
async function load() {
  pending.value = true
  forbidden.value = false
  loadFailed.value = false
  try {
    const response = await catalog.listSchemas({ producerDomain: props.domain })
    schemas.value = response.items ?? []
  } catch (error) {
    if (error instanceof ApiError && error.status === 403) {
      forbidden.value = true
    } else {
      loadFailed.value = true
      toast.error(error instanceof ApiError ? error.message : 'Failed to load schemas')
    }
  } finally {
    pending.value = false
  }
}

function openRegister() {
  formOpen.value = true
}

async function onSaved() {
  await load()
}

onMounted(load)
</script>

<template>
  <NxCard title="Schemas">
    <template #actions>
      <!-- Registration starts here only until the first schema exists; after
           that, schema management lives on each producer schema page. -->
      <NxButton v-if="!forbidden && !loadFailed && !pending && schemas.length === 0" size="sm" @click="openRegister">
        Register schema
      </NxButton>
    </template>

    <p v-if="pending" class="text-sm text-muted">Loading…</p>
    <p v-else-if="forbidden" class="text-sm text-muted">
      Publisher access is required to view this domain's schemas.
    </p>
    <p v-else-if="loadFailed" role="alert" class="text-sm text-danger">
      Schemas could not be loaded — refresh to try again.
    </p>
    <p v-else-if="schemas.length === 0" class="text-sm text-muted">
      No schemas registered for this domain yet.
    </p>
    <ul v-else class="flex flex-col gap-2">
      <li v-for="schema in schemas" :key="schema.schemaName" class="flex items-center justify-between gap-4">
        <NuxtLink
          :to="`/domains/${encodeURIComponent(domain)}/schemas/${encodeURIComponent(schema.schemaName ?? '')}`"
          class="truncate font-mono text-sm text-accent hover:underline"
        >
          {{ schema.schemaName }}
        </NuxtLink>
        <span class="flex shrink-0 items-center gap-2">
          <template v-for="env in orderedEnvs(schema)" :key="env">
            <NxBadge tone="neutral">{{ env }} v{{ schema.latestVersions?.[env] }}</NxBadge>
            <NxBadge v-if="schema.metadataRevisions?.[env] !== undefined" tone="info">
              {{ env }} M{{ schema.metadataRevisions[env] }}
            </NxBadge>
          </template>
        </span>
      </li>
    </ul>

    <SchemaAuthoringForm
      :open="formOpen"
      :domain="domain"
      :namespace="namespace"
      @close="formOpen = false"
      @saved="onSaved"
    />
  </NxCard>
</template>
