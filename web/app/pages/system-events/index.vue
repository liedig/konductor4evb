<script setup lang="ts">
import { ApiError } from '~/composables/useApi'
import type { DomainDetail, DomainSummary } from '~/types/api'

definePageMeta({ middleware: 'auth' })

const SYSTEM_EVENTS_GUIDE_URL = 'https://liedig.github.io/konductor4evb/guides/system-events/'

const { list, describe } = useDomains()
const { markSeen, refresh } = useSystemEventsUnread()
const toast = useToast()

interface SystemEventsEntry {
  summary: DomainSummary
  detail: DomainDetail | null
}

const loading = ref(true)
const errorMessage = ref('')
const entries = ref<SystemEventsEntry[]>([])

async function load() {
  loading.value = true
  errorMessage.value = ''
  try {
    const response = await list()
    const summaries = (response.domains ?? []).filter((d) => d.domainName)
    const details = await Promise.allSettled(
      summaries.map((d) => describe(d.domainName)),
    )
    entries.value = summaries.map((summary, i) => ({
      summary,
      detail: details[i]?.status === 'fulfilled' ? details[i].value : null,
    }))
    return summaries
  } catch (error) {
    errorMessage.value = error instanceof ApiError ? error.message : 'Failed to load system events'
    toast.error(errorMessage.value)
    entries.value = []
    return [] as DomainSummary[]
  } finally {
    loading.value = false
  }
}

onMounted(async () => {
  const summaries = await load()
  // The newest marker the page renders is what this browser counts as seen.
  const newest = summaries
    .map((d) => d.systemEventsLastEventAt)
    .filter((m): m is string => Boolean(m))
    .sort()
    .at(-1)
  markSeen(newest ?? null)
  refresh()
})

const { copied: copiedArn, copy: copyTopicArn } = useCopyToClipboard()
const copiedName = ref<string | null>(null)

async function copyArn(name: string, arn: string) {
  await copyTopicArn(arn)
  copiedName.value = copiedArn.value ? name : null
}

function formatTimestamp(value?: string) {
  return value ? new Date(value).toLocaleString() : null
}
</script>

<template>
  <div class="flex flex-col gap-4">
    <div>
      <h1 class="text-2xl font-semibold">System events</h1>
      <p class="mt-1 text-sm text-muted">
        Each domain has one platform-managed SNS topic carrying its system events
        across all environments. Subscribe SQS queues, Lambda functions, or email
        addresses from your accounts (the domain owner is subscribed by default and
        must confirm the SNS email); an SNS filter policy on the
        <code>environment</code> attribute selects a single environment. Setup steps
        are in the
        <a
          :href="SYSTEM_EVENTS_GUIDE_URL"
          target="_blank"
          rel="noopener"
          class="underline decoration-dotted underline-offset-2 hover:text-accent"
        >system events guide</a>.
      </p>
    </div>

    <p v-if="loading" class="text-sm text-muted">Loading…</p>
    <p v-else-if="errorMessage" role="alert" class="text-sm text-danger">{{ errorMessage }}</p>
    <NxEmptyState
      v-else-if="entries.length === 0"
      title="No domains"
      body="No domains are visible to you. Register a domain or ask a platform administrator about access."
    />
    <template v-else>
      <NxCard v-for="entry in entries" :key="entry.summary.domainName" :title="entry.summary.domainName">
        <template #actions>
          <NxButton
            v-if="entry.detail?.systemEventsTopicArn"
            variant="ghost"
            size="sm"
            :aria-label="`Copy system events topic ARN for ${entry.summary.domainName}`"
            @click="copyArn(entry.summary.domainName, entry.detail.systemEventsTopicArn)"
          >
            {{ copiedName === entry.summary.domainName ? 'Copied' : 'Copy' }}
          </NxButton>
        </template>
        <p v-if="!entry.detail" class="text-sm text-muted">
          Couldn't load this domain's details — refresh the page to retry.
        </p>
        <div v-else-if="entry.detail.systemEventsTopicArn" class="flex flex-col gap-2">
          <p class="font-mono text-xs break-all">{{ entry.detail.systemEventsTopicArn }}</p>
          <p v-if="entry.detail.systemEventsLastEventAt" class="text-xs text-faint">
            Last system event: {{ entry.detail.systemEventsLastEventType ?? 'unknown type' }},
            {{ formatTimestamp(entry.detail.systemEventsLastEventAt) }}
          </p>
        </div>
        <p v-else class="text-sm text-muted">
          No system-events topic is configured for this domain yet — it appears here
          after platform provisioning or the topic backfill runs. Ask a platform
          administrator.
        </p>
      </NxCard>
    </template>
  </div>
</template>
