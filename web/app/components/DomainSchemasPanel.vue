<script setup lang="ts">
import type { SchemaSummary } from '~/types/api'

const props = defineProps<{ domain: string, namespace?: string }>()

const config = useKonductorConfig()

const { schemas, nextToken, forbidden, loadFailed, pending, load } = useDomainSchemas(() => props.domain)

const formOpen = ref(false)

const PANEL_LIMIT = 5

// Environment badges follow the configured order, not the API payload's.
function orderedEnvs(schema: SchemaSummary): string[] {
  return orderEnvironments(Object.keys(schema.latestVersions ?? {}), config.environments)
}

// The panel summarizes: the five most recently modified schemas of the
// loaded page. The catalog orders stably by name and pages at fifty, so a
// modification beyond the first page is reachable from the dedicated view
// one click away. Undated schemas sort last, then by name.
const recent = computed(() =>
  [...schemas.value].sort((a, b) =>
    (b.lastModifiedAt ?? '').localeCompare(a.lastModifiedAt ?? '')
    || (a.schemaName ?? '').localeCompare(b.schemaName ?? '')).slice(0, PANEL_LIMIT))

const hasMore = computed(() => schemas.value.length > PANEL_LIMIT || nextToken.value !== undefined)

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
      <!-- Registering is offered from the panel regardless of list state
           (spec: schema authoring journeys); per-schema version management
           lives on each producer schema page. -->
      <NxButton v-if="!forbidden && !loadFailed && !pending" size="sm" @click="openRegister">
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
    <div v-else class="flex flex-col gap-3">
      <ul class="flex flex-col gap-2">
        <li v-for="schema in recent" :key="schema.schemaName" class="flex items-start justify-between gap-4">
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
      <NuxtLink
        v-if="hasMore"
        :to="`/domains/${encodeURIComponent(domain)}/schemas`"
        class="self-start text-sm text-accent hover:underline"
      >
        View all schemas
      </NuxtLink>
    </div>

    <SchemaAuthoringForm
      :open="formOpen"
      :domain="domain"
      :namespace="namespace"
      @close="formOpen = false"
      @saved="onSaved"
    />
  </NxCard>
</template>
