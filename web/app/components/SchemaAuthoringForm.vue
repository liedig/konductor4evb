<script setup lang="ts">
import { ApiError } from '~/composables/useApi'
import type { ContractPreviewResponse, SchemaMutationResponse } from '~/types/api'

const props = defineProps<{
  open: boolean
  domain: string
  eventName?: string
  namespace?: string
  appendSchemaName?: string
}>()

const emit = defineEmits<{
  close: []
  saved: [result: SchemaMutationResponse]
}>()

const catalog = useCatalog()
const toast = useToast()
const config = useKonductorConfig()

const eventNameInput = ref('')
const content = ref('')
const busy = ref(false)
const formError = ref('')
const contentError = ref('')
const contentTouched = ref(false)
const preview = ref<ContractPreviewResponse | null>(null)
const previewBusy = ref(false)
const previewError = ref('')

const isAppend = computed(() => props.eventName !== undefined)
const title = computed(() => isAppend.value ? `New version — ${props.eventName}` : 'Register schema')

// The preview names the registry schema exactly as the server does:
// `{source}@{detail-type}`. Appends reuse the existing schema's name; a new
// registration derives the source from the domain's namespace.
const previewSchemaName = computed(() =>
  isAppend.value
    ? (props.appendSchemaName ?? (props.namespace ? `${props.namespace}@${props.eventName}` : undefined))
    : (props.namespace ? `${props.namespace}@${eventNameInput.value.trim()}` : undefined))

const canPreview = computed(() =>
  content.value.trim() !== ''
  && previewSchemaName.value !== undefined
  && (isAppend.value || eventNameInput.value.trim() !== ''))

const prettyPreview = computed(() => {
  if (!preview.value?.composedDocument) return ''
  try {
    return JSON.stringify(JSON.parse(preview.value.composedDocument), null, 2)
  } catch {
    return preview.value.composedDocument
  }
})

// The stage segment of the API base URL is the target environment; the
// console never lets the caller choose it.
const targetEnvironment = computed(() => {
  const segment = config.apiBaseUrl.replace(/\/+$/, '').split('/').pop()
  return segment && segment !== config.apiBaseUrl ? segment : config.apiBaseUrl
})

watch(() => props.open, (open) => {
  if (open) {
    eventNameInput.value = props.eventName ?? ''
    content.value = ''
    formError.value = ''
    contentError.value = ''
    contentTouched.value = false
    preview.value = null
    previewError.value = ''
  }
})

function checkContent(): boolean {
  contentError.value = ''
  if (content.value.trim() === '') return false
  try {
    JSON.parse(content.value)
    return true
  } catch (error) {
    contentError.value = `Not valid JSON: ${error instanceof Error ? error.message : String(error)}`
    return false
  }
}

// Mid-typing noise is unhelpful before the user has left the field once.
watch(content, () => {
  if (contentTouched.value) checkContent()
})

function onContentBlur() {
  contentTouched.value = true
  if (content.value.trim() !== '') checkContent()
}

function formatContent() {
  contentTouched.value = true
  if (checkContent()) {
    content.value = JSON.stringify(JSON.parse(content.value), null, 2)
  }
}

function describeError(error: unknown): string {
  if (!(error instanceof ApiError)) return 'The request failed unexpectedly'
  const fields = error.fieldList
    .map(f => f.message ?? f.name ?? '')
    .filter(Boolean)
    .join('; ')
  if (error.status === 400) return fields ? `${error.message} (${fields})` : error.message
  if (error.status === 403) return 'A publisher binding on this domain is required to author schemas.'
  if (error.status === 409) return 'A schema with this event name already exists. Use "New version" to evolve it.'
  return error.message
}

async function submit() {
  formError.value = ''
  contentTouched.value = true
  if (!checkContent()) return
  busy.value = true
  try {
    const result = isAppend.value && props.eventName
      ? await catalog.appendSchemaVersion({ domain: props.domain, eventName: props.eventName, content: content.value })
      : await catalog.createSchema({ domain: props.domain, eventName: eventNameInput.value.trim(), schemaType: 'JSONSchemaDraft4', content: content.value })
    const name = result.schemaName ?? eventNameInput.value
    toast.success(isAppend.value ? `${name} is now at v${result.schemaVersion ?? '?'}` : `Schema ${name} created`)
    emit('saved', result)
    emit('close')
  } catch (error) {
    formError.value = describeError(error)
  } finally {
    busy.value = false
  }
}

// Effective-contract preview: composes the unsaved business content with the
// environment's active metadata — the producer never authors envelope fields
// (design decision 4). Registers nothing.
async function runPreview() {
  contentTouched.value = true
  if (!checkContent()) return
  previewBusy.value = true
  previewError.value = ''
  preview.value = null
  try {
    preview.value = await catalog.previewContract({
      domain: props.domain,
      schemaName: previewSchemaName.value ?? '',
      environment: targetEnvironment.value,
      content: content.value,
    })
  } catch (error) {
    previewError.value = describeError(error)
  } finally {
    previewBusy.value = false
  }
}
</script>

<template>
  <NxModal :open="open" :title="title" @close="emit('close')">
    <div class="flex flex-col gap-4">
      <p class="text-sm text-muted">
        Target environment: <span class="font-mono text-xs">{{ targetEnvironment }}</span>
        (inferred from the console's API stage)
      </p>
      <NxInput
        v-if="!isAppend"
        v-model="eventNameInput"
        label="Event name"
        placeholder="OrderNotification"
        hint="Characters A-Z, a-z, 0-9, ., _, - — used in subscription rule names."
        required
      />
      <div class="flex flex-col gap-1.5">
        <div class="flex items-center justify-between">
          <label class="text-sm font-medium text-muted" for="schema-content">
            Schema (JSON Schema draft-4)<span class="text-danger"> *</span>
          </label>
          <NxButton variant="secondary" size="sm" :disabled="content.trim() === ''" @click="formatContent">
            Format
          </NxButton>
        </div>
        <textarea
          id="schema-content"
          v-model="content"
          rows="12"
          spellcheck="false"
          placeholder='{"$schema":"http://json-schema.org/draft-04/schema#","type":"object",...}'
          :aria-invalid="!!contentError"
          class="rounded-lg border bg-surface px-3 py-2 font-mono text-xs text-foreground placeholder:text-faint"
          :class="contentError ? 'border-danger' : 'border-border-strong'"
          @blur="onContentBlur"
        />
        <p v-if="contentError" class="text-sm text-danger">{{ contentError }}</p>
        <p v-else class="text-sm text-faint">Draft-4 JSON Schema describing the event payload. Format pretty-prints valid JSON.</p>
      </div>
      <div class="flex flex-col gap-2">
        <NxButton variant="secondary" size="sm" :disabled="!canPreview || previewBusy" @click="runPreview">
          {{ previewBusy ? 'Previewing…' : 'Preview effective contract' }}
        </NxButton>
        <p class="text-xs text-faint">
          Composes this unsaved business document with the active metadata in
          {{ targetEnvironment }} — nothing is registered.
        </p>
        <template v-if="preview">
          <div class="flex flex-wrap items-center gap-2">
            <NxBadge tone="info">{{ preview.environment }} V{{ preview.schemaVersion }}</NxBadge>
            <NxBadge tone="neutral">{{ preview.environment }} M{{ preview.metadataRevision }}</NxBadge>
            <NxBadge tone="warning">Not registered</NxBadge>
          </div>
          <pre class="max-h-60 overflow-auto rounded-lg border border-border bg-surface-raised p-3 text-xs">{{ prettyPreview }}</pre>
          <p class="text-xs text-faint">
            The metadata block is platform-owned: it stays read-only here and on every revision.
          </p>
        </template>
        <p v-if="previewError" role="alert" class="text-sm text-danger">{{ previewError }}</p>
      </div>
      <p v-if="formError" class="text-sm text-danger">{{ formError }}</p>
    </div>
    <template #actions>
      <NxButton variant="secondary" :disabled="busy" @click="emit('close')">Cancel</NxButton>
      <NxButton :loading="busy" :disabled="!canPreview" @click="submit">
        {{ isAppend ? 'Append version' : 'Register' }}
      </NxButton>
    </template>
  </NxModal>
</template>
