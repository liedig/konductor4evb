<script setup lang="ts">
import { ApiError } from '~/composables/useApi'
import type { SchemaMutationResponse } from '~/types/api'

const props = defineProps<{
  open: boolean
  domain: string
  eventName?: string
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

const isAppend = computed(() => props.eventName !== undefined)
const title = computed(() => isAppend.value ? `New version — ${props.eventName}` : 'Register schema')

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

const canSubmit = computed(() => content.value.trim() !== '' && (isAppend.value || eventNameInput.value.trim() !== ''))
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
      <p v-if="formError" class="text-sm text-danger">{{ formError }}</p>
    </div>
    <template #actions>
      <NxButton variant="secondary" :disabled="busy" @click="emit('close')">Cancel</NxButton>
      <NxButton :loading="busy" :disabled="!canSubmit" @click="submit">
        {{ isAppend ? 'Append version' : 'Register' }}
      </NxButton>
    </template>
  </NxModal>
</template>
