<script setup lang="ts">
import { ApiError } from '~/composables/useApi'
import type { PendingRegistration } from '~/types/api'

definePageMeta({ middleware: 'persona' })

const approvals = useApprovals()
const toast = useToast()

const registrations = ref<PendingRegistration[]>([])
const pending = ref(true)
const errorMessage = ref('')
const busyName = ref<string | null>(null)

const rejectTarget = ref<PendingRegistration | null>(null)
const justification = ref('')
const justificationError = ref('')

function age(createdAt?: string): string {
  if (!createdAt) return '—'
  const seconds = Math.max(0, (Date.now() - new Date(createdAt).getTime()) / 1000)
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h`
  return `${Math.floor(seconds / 86400)}d`
}

async function load() {
  pending.value = true
  try {
    const response = await approvals.list()
    registrations.value = response.registrations ?? []
  } catch (error) {
    errorMessage.value = error instanceof ApiError ? error.message : 'Failed to load pending registrations'
  } finally {
    pending.value = false
  }
}

async function decide(registration: PendingRegistration, decision: 'ACCEPT' | 'REJECT', reason?: string) {
  if (!registration.taskToken || !registration.domainName || busyName.value) return
  busyName.value = registration.domainName
  try {
    await approvals.decide({
      taskToken: registration.taskToken,
      decision,
      justification: reason,
    })
    registrations.value = registrations.value.filter(r => r.domainName !== registration.domainName)
    toast.success(`${decision === 'ACCEPT' ? 'Approved' : 'Rejected'} ${registration.domainName}`)
  } catch (error) {
    // Expired/used task tokens and conflicts surface descriptively.
    toast.error(error instanceof ApiError ? `${error.message} (${error.status})` : 'Decision failed')
  } finally {
    busyName.value = null
  }
}

function openReject(registration: PendingRegistration) {
  justification.value = ''
  justificationError.value = ''
  rejectTarget.value = registration
}

function submitReject() {
  if (!justification.value.trim()) {
    justificationError.value = 'Justification is required for rejections.'
    return
  }
  const target = rejectTarget.value
  rejectTarget.value = null
  if (target) decide(target, 'REJECT', justification.value.trim())
}

onMounted(load)
</script>

<template>
  <div class="flex flex-col gap-6">
    <header>
      <h1 class="text-2xl font-semibold">Pending approvals</h1>
      <p class="mt-1 text-sm text-muted">Domain registrations awaiting a platform decision.</p>
    </header>

    <p v-if="errorMessage" role="alert" class="text-sm text-danger">{{ errorMessage }}</p>
    <p v-else-if="pending" class="text-sm text-muted">Loading…</p>

    <NxEmptyState
      v-else-if="registrations.length === 0"
      title="Inbox zero"
      body="No domain registrations are waiting for a decision."
    />

    <div v-else class="flex flex-col gap-4">
      <NxCard v-for="registration in registrations" :key="registration.domainName">
        <div class="flex items-start justify-between gap-4">
          <div class="min-w-0">
            <div class="flex items-center gap-3">
              <h2 class="font-mono text-base font-semibold">{{ registration.domainName }}</h2>
              <NxBadge tone="neutral">{{ age(registration.createdAt) }} old</NxBadge>
            </div>
            <p class="mt-1 text-sm text-muted">{{ registration.description || 'No description' }}</p>
            <dl class="mt-3 grid grid-cols-1 gap-x-8 gap-y-1 text-sm sm:grid-cols-2">
              <div class="flex gap-2">
                <dt class="text-muted">Owning team</dt>
                <dd><NxBadge tone="info">{{ registration.owningTeam ?? '—' }}</NxBadge></dd>
              </div>
              <div class="flex gap-2">
                <dt class="text-muted">Account</dt>
                <dd class="font-mono text-xs">{{ registration.ownerAccountId }}</dd>
              </div>
              <div class="flex gap-2">
                <dt class="text-muted">Owner</dt>
                <dd>{{ registration.ownerEmail }}</dd>
              </div>
              <div class="flex gap-2">
                <dt class="text-muted">Secondary</dt>
                <dd>{{ registration.secondaryOwnerEmails?.join(', ') || '—' }}</dd>
              </div>
            </dl>
          </div>
          <div class="flex shrink-0 gap-2">
            <NxButton
              :loading="busyName === registration.domainName"
              @click="decide(registration, 'ACCEPT')"
            >
              Approve
            </NxButton>
            <NxButton
              variant="danger"
              :disabled="busyName === registration.domainName"
              @click="openReject(registration)"
            >
              Reject
            </NxButton>
          </div>
        </div>
      </NxCard>
    </div>

    <NxModal
      :open="rejectTarget !== null"
      :title="`Reject ${rejectTarget?.domainName ?? ''}`"
      @close="rejectTarget = null"
    >
      <div class="flex flex-col gap-1.5">
        <label for="justification" class="text-sm font-medium text-muted">
          Justification <span class="text-danger">*</span>
        </label>
        <textarea
          id="justification"
          v-model="justification"
          rows="4"
          class="rounded-lg border bg-surface px-3 py-2 text-sm text-foreground"
          :class="justificationError ? 'border-danger' : 'border-border-strong'"
        />
        <p v-if="justificationError" class="text-sm text-danger">{{ justificationError }}</p>
      </div>
      <template #actions>
        <NxButton variant="secondary" @click="rejectTarget = null">Cancel</NxButton>
        <NxButton variant="danger" @click="submitReject">Reject registration</NxButton>
      </template>
    </NxModal>
  </div>
</template>
