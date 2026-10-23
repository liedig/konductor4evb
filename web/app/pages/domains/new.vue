<script setup lang="ts">
import { ApiError } from '~/composables/useApi'
import { useSessionStore } from '~/stores/session'

definePageMeta({ middleware: 'auth' })

const session = useSessionStore()
const domainsApi = useDomains()
const toast = useToast()
const config = useKonductorConfig()

const NAME_PATTERN = /^[a-z0-9][a-z0-9-]{0,62}[a-z0-9]$/
const ACCOUNT_PATTERN = /^[0-9]{12}$/
const EMAIL_PATTERN = /^[^\s@]+@[^\s@]+\.[^\s@]+$/

const form = reactive({
  domainName: '',
  owningTeam: '',
  ownerEmail: '',
  secondaryOwnerEmails: [''],
  description: '',
  costCenter: '',
})
// One required account field per platform-configured environment; the
// user pins every environment explicitly instead of a default owner
// account plus sparse overrides.
const environmentAccounts = reactive<Record<string, string>>({})
for (const environment of config.environments) {
  environmentAccounts[environment] = ''
}
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
  fieldErrors.ownerEmail = EMAIL_PATTERN.test(form.ownerEmail) ? '' : 'A valid email address.'
  const secondaries = form.secondaryOwnerEmails.map(e => e.trim()).filter(Boolean)
  fieldErrors.secondaryOwnerEmails = secondaries.length === 0
    ? 'At least one secondary owner email is required.'
    : secondaries.every(e => EMAIL_PATTERN.test(e)) ? '' : 'Every secondary owner email must be valid.'
  fieldErrors.costCenter = validateCostCenter()
  for (const environment of config.environments) {
    const value = (environmentAccounts[environment] ?? '').trim()
    fieldErrors[`account-${environment}`] = ACCOUNT_PATTERN.test(value)
      ? ''
      : 'A 12-digit AWS account ID.'
  }
  return Object.values(fieldErrors).every(e => e === '')
}

// The register API still requires a single owner account (the domain
// record's `by-owner` index and the trust-stack advisory key on it); the
// form derives it from the authoring environment's account, so teams
// never see or choose it.
function ownerAccountPayload(): string {
  const authoring = config.environments[0]
  return authoring ? environmentAccounts[authoring]!.trim() : ''
}

function environmentAccountsPayload(): Record<string, string> {
  return Object.fromEntries(config.environments.map(environment =>
    [environment, environmentAccounts[environment]!.trim()],
  ))
}

const MAX_COST_CENTER = 256

function validateCostCenter(): string {
  const v = form.costCenter.trim()
  if (v.length > MAX_COST_CENTER) return `At most ${MAX_COST_CENTER} characters.`
  // eslint-disable-next-line no-control-regex
  if (/[\x00-\x1f]/.test(v)) return 'No control characters.'
  return ''
}

async function submit() {
  if (busy.value || !validate()) return
  busy.value = true
  try {
    accepted.value = await domainsApi.register({
      domainName: form.domainName,
      owningTeam: form.owningTeam,
      ownerAccountId: ownerAccountPayload(),
      ownerEmail: form.ownerEmail,
      secondaryOwnerEmails: form.secondaryOwnerEmails.map(e => e.trim()).filter(Boolean),
      description: form.description || undefined,
      costCenter: form.costCenter.trim() || undefined,
      environmentAccounts: environmentAccountsPayload(),
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

    <NxCard v-if="accepted" title="Request received">
      <p class="text-sm text-muted">
        Your domain request has been submitted for review. The platform team will assess it
        and approve or reject it — you can track its status in your domains list.
      </p>
      <template #actions>
        <NuxtLink to="/domains"><NxButton variant="secondary">Track in my domains</NxButton></NuxtLink>
      </template>
    </NxCard>

    <NxCard v-else>
      <!-- novalidate: every field's contract is checked by the app's inline
           errors below (native bubbles would bypass them for required
           fields). -->
      <form class="flex flex-col gap-4" novalidate @submit.prevent="submit">
        <NxInput
          v-model="form.domainName"
          label="Domain name"
          placeholder="my-domain"
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
        <NxInput v-model="form.description" label="Description" placeholder="Provide a description for your domain" />
        <NxInput
          v-model="form.costCenter"
          label="Cost center"
          placeholder="e.g. cc-1234"
          hint="Optional. Applied as a cost-center tag on every resource this domain provisions."
          :error="fieldErrors.costCenter"
        />
        <fieldset
          v-if="config.environments.length > 0"
          class="flex flex-col gap-3 rounded-lg border border-border p-4"
        >
          <legend class="px-1 text-sm font-medium text-muted">Per-environment accounts</legend>
          <p class="text-xs text-faint">
            Provisioning places each environment in the account you name here. Accounts are
            fixed once the domain is registered, and each account needs the trust-stack
            deployed before approval.
          </p>
          <NxInput
            v-for="environment in config.environments"
            :key="environment"
            v-model="environmentAccounts[environment]"
            :label="`${environment} account ID`"
            placeholder="111122223333"
            :error="fieldErrors[`account-${environment}`]"
            required
          />
        </fieldset>
        <div class="mt-2 flex justify-end gap-2">
          <NuxtLink to="/domains"><NxButton variant="secondary">Cancel</NxButton></NuxtLink>
          <NxButton type="submit" :loading="busy">Submit request</NxButton>
        </div>
      </form>
    </NxCard>
  </div>
</template>
