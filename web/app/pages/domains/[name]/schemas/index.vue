<script setup lang="ts">
import type { SchemaSummary } from '~/types/api'

definePageMeta({ middleware: 'auth' })

const route = useRoute()
const domain = computed(() => String(route.params.name))
const config = useKonductorConfig()

const { schemas, nextToken, forbidden, loadFailed, pending, load } = useDomainSchemas(() => domain.value)

// Environment badges follow the configured order, not the API payload's.
function orderedEnvs(schema: SchemaSummary): string[] {
  return orderEnvironments(Object.keys(schema.latestVersions ?? {}), config.environments)
}

onMounted(() => load())
</script>

<template>
  <div class="flex flex-col gap-6">
    <header>
      <h1 class="text-2xl font-semibold">Schemas</h1>
      <p class="mt-1 text-sm text-muted">
        Every schema registered by <span class="font-mono text-xs">{{ domain }}</span> — the
        domain page shows only the five most recently modified.
      </p>
    </header>

    <NxCard>
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
      <template v-else>
        <ul class="flex flex-col gap-2">
          <li v-for="schema in schemas" :key="schema.schemaName" class="flex items-start justify-between gap-4">
            <div class="min-w-0">
              <NuxtLink
                :to="`/domains/${encodeURIComponent(domain)}/schemas/${encodeURIComponent(schema.schemaName ?? '')}`"
                class="truncate font-mono text-sm text-accent hover:underline"
              >
                {{ schema.schemaName }}
              </NuxtLink>
              <p v-if="schema.lastModifiedAt" class="mt-0.5 text-xs text-faint">
                Modified {{ schema.lastModifiedAt.slice(0, 10) }}
              </p>
            </div>
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
        <div v-if="nextToken" class="mt-4 flex justify-center">
          <NxButton variant="ghost" :loading="pending" @click="load(false)">Load more</NxButton>
        </div>
      </template>
    </NxCard>
  </div>
</template>
