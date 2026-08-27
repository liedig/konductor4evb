<script setup lang="ts">
import { ApiError } from '~/composables/useApi'
import type { DomainDetail } from '~/types/api'

definePageMeta({ middleware: 'auth' })

const route = useRoute()
const domainsApi = useDomains()
const toast = useToast()

const name = route.params.name as string

const domain = ref<DomainDetail | null>(null)
const notFound = ref(false)
const editing = ref(false)
const busy = ref(false)

const editForm = reactive({
  description: '',
  ownerEmail: '',
  secondaryOwnerEmails: [''],
})

async function load() {
  try {
    domain.value = await domainsApi.describe(name)
  } catch (error) {
    if (error instanceof ApiError && error.status === 404) {
      notFound.value = true
    } else {
      toast.error(error instanceof ApiError ? error.message : 'Failed to load the domain')
    }
  }
}

function startEdit() {
  if (!domain.value) return
  editForm.description = domain.value.description ?? ''
  editForm.ownerEmail = domain.value.ownerEmail ?? ''
  editForm.secondaryOwnerEmails = [...(domain.value.secondaryOwnerEmails ?? []), ''].slice(0, -1)
  if (editForm.secondaryOwnerEmails.length === 0) editForm.secondaryOwnerEmails = ['']
  editing.value = true
}

async function saveEdit() {
  if (busy.value) return
  busy.value = true
  try {
    await domainsApi.update(name, {
      description: editForm.description || undefined,
      ownerEmail: editForm.ownerEmail || undefined,
      secondaryOwnerEmails: editForm.secondaryOwnerEmails.map(e => e.trim()).filter(Boolean),
    })
    toast.success('Update request accepted — changes apply asynchronously')
    editing.value = false
  } catch (error) {
    toast.error(error instanceof ApiError ? error.message : 'Update failed')
  } finally {
    busy.value = false
  }
}

const environments = computed(() => domain.value?.environments ?? [])
const openEnvs = ref<Record<string, boolean>>({})

function toggleEnv(env: string) {
  openEnvs.value = { ...openEnvs.value, [env]: !openEnvs.value[env] }
}

function arnConsoleLink(env: string, arn: string | undefined): string | null {
  if (!arn) return null
  const siblings = Object.values(domain.value?.resourceArns?.[env] ?? {})
  return consoleLinkFor(arn, regionFromArns(siblings))
}

onMounted(load)
</script>

<template>
  <div class="flex flex-col gap-6">
    <NxEmptyState
      v-if="notFound"
      title="Domain not found"
      body="This domain does not exist or is not visible to you."
    >
      <template #actions>
        <NuxtLink to="/domains"><NxButton variant="secondary">Back to my domains</NxButton></NuxtLink>
      </template>
    </NxEmptyState>

    <template v-else-if="domain">
      <header class="flex items-start justify-between gap-4">
        <div>
          <div class="flex items-center gap-3">
            <h1 class="font-mono text-2xl font-semibold">{{ domain.domainName }}</h1>
            <NxStatusPill :status="domain.status ?? ''" />
          </div>
          <p class="mt-1 text-sm text-muted">{{ domain.description || 'No description' }}</p>
        </div>
        <NxButton variant="secondary" @click="startEdit">Edit</NxButton>
      </header>

      <NxCard title="Overview">
        <dl class="grid grid-cols-1 gap-4 text-sm sm:grid-cols-2">
          <div>
            <dt class="text-muted">Owner account</dt>
            <dd class="mt-0.5 font-mono">{{ domain.ownerAccountId }}</dd>
          </div>
          <div>
            <dt class="text-muted">Owner email</dt>
            <dd class="mt-0.5">{{ domain.ownerEmail }}</dd>
          </div>
          <div>
            <dt class="text-muted">Secondary owners</dt>
            <dd class="mt-0.5">{{ domain.secondaryOwnerEmails?.join(', ') || '—' }}</dd>
          </div>
          <div>
            <dt class="text-muted">Namespace</dt>
            <dd class="mt-0.5 font-mono text-xs">{{ domain.namespace || '—' }}</dd>
          </div>
          <div>
            <dt class="text-muted">Namespace parameter</dt>
            <dd class="mt-0.5 font-mono text-xs">{{ domain.namespaceParameter || '—' }}</dd>
          </div>
          <div>
            <dt class="text-muted">Tags</dt>
            <dd class="mt-0.5">
              <span v-if="!domain.tags || Object.keys(domain.tags).length === 0">—</span>
              <template v-for="(value, key) in domain.tags" :key="key">
                <NxBadge v-if="String(key).startsWith('konductor:')" tone="neutral" class="mr-1">
                  {{ key }}={{ value }}
                </NxBadge>
                <NxBadge v-else tone="info" class="mr-1" :title="'Custom tag'">
                  {{ key }}={{ value }}
                </NxBadge>
              </template>
              <p class="mt-1 text-xs text-faint">Standard platform tags plus your cost center, applied to every provisioned resource.</p>
            </dd>
          </div>
          <div>
            <dt class="text-muted">Created</dt>
            <dd class="mt-0.5">{{ domain.createdAt ? new Date(domain.createdAt).toLocaleString() : '—' }}</dd>
          </div>
        </dl>
      </NxCard>

      <NxCard title="Environments">
        <div class="flex flex-wrap gap-2">
          <NxBadge v-for="env in environments" :key="env" tone="info">{{ env }}</NxBadge>
          <span v-if="environments.length === 0" class="text-sm text-muted">Not provisioned yet</span>
        </div>
        <div v-if="environments.length > 0" class="mt-4 flex flex-col gap-3">
          <div v-for="env in environments" :key="env" class="rounded-lg border border-border">
            <button
              type="button"
              class="flex w-full items-center justify-between gap-3 px-4 py-3 text-left"
              :aria-expanded="!!openEnvs[env]"
              :aria-controls="`env-panel-${env}`"
              @click="toggleEnv(env)"
            >
              <span class="text-sm font-medium">{{ env }}</span>
              <svg
                class="h-4 w-4 shrink-0 text-muted transition-transform"
                :class="{ 'rotate-180': openEnvs[env] }"
                viewBox="0 0 16 16"
                fill="none"
                aria-hidden="true"
              >
                <path d="M4 6l4 4 4-4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" />
              </svg>
            </button>
            <dl
              v-show="openEnvs[env]"
              :id="`env-panel-${env}`"
              class="grid gap-1 border-t border-border p-4 font-mono text-xs break-all text-muted"
            >
              <div v-for="(arn, label) in domain?.resourceArns?.[env] ?? {}" :key="label" class="grid grid-cols-[180px_1fr] gap-2">
                <dt>{{ label }}</dt>
                <dd class="text-foreground">
                  <a
                    v-if="arnConsoleLink(env, arn)"
                    :href="arnConsoleLink(env, arn)!"
                    target="_blank"
                    rel="noopener"
                    class="underline decoration-dotted underline-offset-2 hover:text-accent"
                  >{{ arn }}</a>
                  <template v-else>{{ arn }}</template>
                </dd>
              </div>
            </dl>
          </div>
        </div>
      </NxCard>

      <DomainSchemasPanel :domain="name" />
      <DomainSubscriptionsPanel :domain="name" />
      <DomainAccessPanel :domain="name" />

      <NxModal :open="editing" title="Edit domain" @close="editing = false">
        <div class="flex flex-col gap-4">
          <NxInput v-model="editForm.description" label="Description" />
          <NxInput v-model="editForm.ownerEmail" label="Owner email" type="email" />
          <div class="flex flex-col gap-2">
            <div v-for="(_, index) in editForm.secondaryOwnerEmails" :key="index" class="flex items-end gap-2">
              <NxInput
                v-model="editForm.secondaryOwnerEmails[index]"
                :label="index === 0 ? 'Secondary owner emails' : undefined"
                type="email"
                class="flex-1"
              />
              <NxButton
                v-if="editForm.secondaryOwnerEmails.length > 1"
                variant="ghost"
                size="sm"
                @click="editForm.secondaryOwnerEmails.splice(index, 1)"
              >
                Remove
              </NxButton>
            </div>
            <div>
              <NxButton variant="ghost" size="sm" @click="editForm.secondaryOwnerEmails.push('')">
                Add secondary owner
              </NxButton>
            </div>
          </div>
          <p class="text-sm text-faint">Name, account, environments, and cost center are not editable.</p>
        </div>
        <template #actions>
          <NxButton variant="secondary" :disabled="busy" @click="editing = false">Cancel</NxButton>
          <NxButton :loading="busy" @click="saveEdit">Save changes</NxButton>
        </template>
      </NxModal>
    </template>

    <p v-else class="text-sm text-muted">Loading…</p>
  </div>
</template>
