<script setup lang="ts">
import { ApiError } from '~/composables/useApi'
import type { SchemaSummary } from '~/types/api'

const props = defineProps<{ domain: string }>()

const catalog = useCatalog()
const toast = useToast()

const schemas = ref<SchemaSummary[]>([])
const forbidden = ref(false)
const pending = ref(true)

const formOpen = ref(false)
const formEventName = ref<string | undefined>(undefined)

// Producer scope: publishers see their own domain's schemas without any
// consumer entitlement (GET /schemas?domain=... with no consumerDomain).
async function load() {
  pending.value = true
  try {
    const response = await catalog.listSchemas({ producerDomain: props.domain })
    schemas.value = response.items ?? []
  } catch (error) {
    if (error instanceof ApiError && error.status === 403) {
      forbidden.value = true
    } else {
      toast.error(error instanceof ApiError ? error.message : 'Failed to load schemas')
    }
  } finally {
    pending.value = false
  }
}

function openRegister() {
  formEventName.value = undefined
  formOpen.value = true
}

function openAppend(schema: SchemaSummary) {
  formEventName.value = schema.schemaName?.split('@')[1] ?? schema.schemaName
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
      <NxButton v-if="!forbidden && !pending" size="sm" @click="openRegister">Register schema</NxButton>
    </template>

    <p v-if="pending" class="text-sm text-muted">Loading…</p>
    <p v-else-if="forbidden" class="text-sm text-muted">
      Publisher access is required to view this domain's schemas.
    </p>
    <p v-else-if="schemas.length === 0" class="text-sm text-muted">
      No schemas registered for this domain yet.
    </p>
    <ul v-else class="flex flex-col gap-2">
      <li v-for="schema in schemas" :key="schema.schemaName" class="flex items-center justify-between gap-4">
        <NuxtLink
          :to="`/catalog/${encodeURIComponent(schema.producerDomain ?? '')}/${encodeURIComponent(schema.schemaName ?? '')}`"
          class="truncate font-mono text-sm text-accent hover:underline"
        >
          {{ schema.schemaName }}
        </NuxtLink>
        <span class="flex shrink-0 items-center gap-2">
          <NxBadge v-for="(version, env) in schema.latestVersions" :key="env" tone="neutral">
            {{ env }} v{{ version }}
          </NxBadge>
          <NxButton variant="secondary" size="sm" @click="openAppend(schema)">New version</NxButton>
        </span>
      </li>
    </ul>

    <SchemaAuthoringForm
      :open="formOpen"
      :domain="domain"
      :event-name="formEventName"
      @close="formOpen = false"
      @saved="onSaved"
    />
  </NxCard>
</template>
