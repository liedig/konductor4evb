<script setup lang="ts">
import { ApiError } from '~/composables/useApi'
import { useSessionStore } from '~/stores/session'

definePageMeta({ middleware: 'auth' })

const session = useSessionStore()
const domainsApi = useDomains()
const toast = useToast()

const NAME_PATTERN = /^[a-z0-9][a-z0-9-]{0,62}[a-z0-9]$/
const ACCOUNT_PATTERN = /^[0-9]{12}$/
const EMAIL_PATTERN = /^[^\s@]+@[^\s@]+\.[^\s@]+$/

const form = reactive({
  domainName: '',
  owningTeam: '',
  ownerAccountId: '',
  ownerEmail: '',
  secondaryOwnerEmails: [''],
  description: '',
  tags: '',
})
const fieldErrors = reactive<Record<string, string>>({})
const busy = ref(false)
const accepted = ref<{ requestId?: string, message?: string } | null>(null)

const teamOptions = computed(() =>
  session.teamGroups.map(group => ({ value: group, label: group })),
)

// Pre-select the owning team when the caller belongs to exactly one (spec).
onMounted(() => {
  if (teamOptions.value.length === 1) {
    form.owningTeam = teamOptions.value[0]!.value
  }
})

function validate(): boolean {
  fieldErrors.domainName = NAME_PATTERN.test(form.domainName)
    ? ''
    : 'Lowercase letters, digits, and hyphens; must start and end with a letter or digit (max 64 chars).'
  fieldErrors.owningTeam = form.owningTeam ? '' : 'Choose the team that will own the domain.'
  fieldErrors.ownerAccountId = ACCOUNT_PATTERN.test(form.ownerAccountId) ? '' : 'A 12-digit AWS account ID.'
  fieldErrors.ownerEmail = EMAIL_PATTERN.test(form.ownerEmail) ? '' : 'A valid email address.'
  const secondaries = form.secondaryOwnerEmails.map(e => e.trim()).filter(Boolean)
  fieldErrors.secondaryOwnerEmails = secondaries.length === 0
    ? 'At least one secondary owner email is required.'
    : secondaries.every(e => EMAIL_PATTERN.test(e)) ? '' : 'Every secondary owner email must be valid.'
  return Object.values(fieldErrors).every(e => e === '')
}

function parseTags(): Record<string, string> | undefined {
  const entries = form.tags
    .split('\n')
    .map(line => line.trim())
    .filter(Boolean)
    .map(line => line.split('='))
  if (entries.length === 0) return undefined
  return Object.fromEntries(entries.filter(pair => pair.length === 2 && pair[0] && pair[1]))
}

async function submit() {
  if (busy.value || !validate()) return
  busy.value = true
  try {
    accepted.value = await domainsApi.register({
      domainName: form.domainName,
      owningTeam: form.owningTeam,
      ownerAccountId: form.ownerAccountId,
      ownerEmail: form.ownerEmail,
      secondaryOwnerEmails: form.secondaryOwnerEmails.map(e => e.trim()).filter(Boolean),
      description: form.description || undefined,
      tags: parseTags(),
    })
    toast.success('Registration request accepted')
  } catch (error) {
    if (error instanceof ApiError && error.fieldList.length > 0) {
      for (const field of error.fieldList) {
        if (field.name) fieldErrors[field.name] = field.message ?? 'Invalid'
      }
    } else {
      toast.error(error instanceof ApiError ? error.message : 'Registration request failed')
    }
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div class="mx-auto flex max-w-2xl flex-col gap-6">
    <header>
      <h1 class="text-2xl font-semibold">Request a domain</h1>
      <p class="mt-1 text-sm text-muted">
        Registrations are processed asynchronously and decided by the platform team.
      </p>
    </header>

    <NxCard v-if="accepted" title="Request accepted">
      <p class="text-sm text-muted">
        {{ accepted.message }} — request <code class="font-mono text-xs">{{ accepted.requestId }}</code>.
        The domain appears in your list as soon as it is recorded.
      </p>
      <template #actions>
        <NuxtLink to="/domains"><NxButton variant="secondary">Back to my domains</NxButton></NuxtLink>
      </template>
    </NxCard>

    <NxCard v-else>
      <form class="flex flex-col gap-4" @submit.prevent="submit">
        <NxInput
          v-model="form.domainName"
          label="Domain name"
          placeholder="payments"
          hint="Stable identifier; cannot be changed later."
          :error="fieldErrors.domainName"
          required
        />
        <NxSelect
          v-model="form.owningTeam"
          label="Owning team"
          :options="teamOptions"
          placeholder="Choose one of your teams"
          hint="Approval creates an owner binding for this team."
          :error="fieldErrors.owningTeam"
          required
        />
        <NxInput
          v-model="form.ownerAccountId"
          label="Owner AWS account ID"
          placeholder="111122223333"
          :error="fieldErrors.ownerAccountId"
          required
        />
        <NxInput
          v-model="form.ownerEmail"
          label="Owner email"
          type="email"
          :error="fieldErrors.ownerEmail"
          required
        />
        <div class="flex flex-col gap-2">
          <div v-for="(_, index) in form.secondaryOwnerEmails" :key="index" class="flex items-end gap-2">
            <NxInput
              v-model="form.secondaryOwnerEmails[index]"
              :label="index === 0 ? 'Secondary owner emails' : undefined"
              type="email"
              class="flex-1"
              :error="index === 0 ? fieldErrors.secondaryOwnerEmails : undefined"
              :required="index === 0"
            />
            <NxButton
              v-if="form.secondaryOwnerEmails.length > 1"
              variant="ghost"
              size="sm"
              @click="form.secondaryOwnerEmails.splice(index, 1)"
            >
              Remove
            </NxButton>
          </div>
          <div>
            <NxButton variant="ghost" size="sm" @click="form.secondaryOwnerEmails.push('')">
              Add secondary owner
            </NxButton>
          </div>
        </div>
        <NxInput v-model="form.description" label="Description" placeholder="Payments domain events" />
        <div class="flex flex-col gap-1.5">
          <label for="tags" class="text-sm font-medium text-muted">Tags</label>
          <textarea
            id="tags"
            v-model="form.tags"
            rows="3"
            placeholder="cost-center=1234"
            class="rounded-lg border border-border-strong bg-surface px-3 py-2 font-mono text-sm text-foreground placeholder:text-faint"
          />
          <p class="text-sm text-faint">One <code>key=value</code> pair per line.</p>
        </div>
        <div class="mt-2 flex justify-end gap-2">
          <NuxtLink to="/domains"><NxButton variant="secondary">Cancel</NxButton></NuxtLink>
          <NxButton type="submit" :loading="busy">Submit request</NxButton>
        </div>
      </form>
    </NxCard>
  </div>
</template>
