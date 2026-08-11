<script setup lang="ts">
import { ApiError } from '~/composables/useApi'
import type { SchemaSummary } from '~/types/api'

const props = defineProps<{ domain: string }>()

const catalog = useCatalog()

const schemas = ref<SchemaSummary[]>([])
const forbidden = ref(false)
const pending = ref(true)

// Producer scope: publishers see their own domain's schemas without any
// consumer entitlement (GET /schemas?domain=... with no consumerDomain).
onMounted(async () => {
  try {
    const response = await catalog.listSchemas({ producerDomain: props.domain })
    schemas.value = response.items ?? []
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
  <NxCard title="Schemas">
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
        <span class="flex shrink-0 gap-2">
          <NxBadge v-for="(version, env) in schema.latestVersions" :key="env" tone="neutral">
            {{ env }} v{{ version }}
          </NxBadge>
        </span>
      </li>
    </ul>
  </NxCard>
</template>
