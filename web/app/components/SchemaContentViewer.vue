<script setup lang="ts">
import { ApiError } from '~/composables/useApi'

const props = defineProps<{
  open: boolean
  schemaName: string
  domain: string
  environment: string
  version?: number
}>()

const emit = defineEmits<{ close: [] }>()

const catalog = useCatalog()
const toast = useToast()

const content = ref('')
const resolvedVersion = ref<number | null>(null)
const pending = ref(false)
const failed = ref(false)

const prettyContent = computed(() => {
  if (!content.value) return ''
  try {
    return JSON.stringify(JSON.parse(content.value), null, 2)
  }
  catch {
    return content.value
  }
})

const title = computed(() => {
  const version = resolvedVersion.value ?? props.version
  return `${props.environment} · ${version ? `v${version}` : 'latest'}`
})

// Guards capture both selectors: close-then-reopen on the same environment
// with a different version must not let the slower first request win.
async function load() {
  if (!props.open) return
  const environment = props.environment
  const version = props.version
  pending.value = true
  failed.value = false
  content.value = ''
  resolvedVersion.value = null
  const stale = () => !props.open || props.environment !== environment || props.version !== version
  try {
    const result = await catalog.getSchemaContent(props.schemaName, {
      domain: props.domain,
      environment,
      version,
    })
    if (stale()) return
    content.value = result.content ?? ''
    resolvedVersion.value = result.version ?? null
  }
  catch (error) {
    if (stale()) return
    failed.value = true
    toast.error(error instanceof ApiError ? error.message : 'Failed to load the schema content')
  }
  finally {
    if (!stale()) pending.value = false
  }
}

watch(() => [props.open, props.environment, props.version], load, { immediate: true })

const { copied, copy } = useCopyToClipboard()
</script>

<template>
  <NxModal :open="open" :title="title" @close="emit('close')">
    <p v-if="pending" class="text-sm text-muted">
      Loading…
    </p>
    <template v-else-if="prettyContent">
      <div class="mb-3 flex justify-end">
        <NxButton
          variant="ghost"
          size="sm"
          aria-label="Copy schema content JSON"
          @click="copy(prettyContent)"
        >
          {{ copied ? 'Copied' : 'Copy' }}
        </NxButton>
      </div>
      <pre class="max-h-[36rem] overflow-auto rounded-lg border border-border bg-background p-4 font-mono text-xs">{{ prettyContent }}</pre>
    </template>
    <p v-else-if="failed" role="alert" class="text-sm text-danger">
      The content could not be loaded.
    </p>
    <p v-else class="text-sm text-muted">
      No content to display.
    </p>
    <template #actions>
      <NxButton variant="ghost" @click="emit('close')">
        Close
      </NxButton>
    </template>
  </NxModal>
</template>
