<script setup lang="ts">
import { AlertDialogContent, AlertDialogDescription, AlertDialogOverlay, AlertDialogPortal, AlertDialogRoot, AlertDialogTitle } from 'reka-ui'

const props = withDefaults(defineProps<{
  open: boolean
  title: string
  confirmLabel?: string
  danger?: boolean
  busy?: boolean
}>(), {
  confirmLabel: 'Confirm',
  danger: false,
  busy: false,
})

const emit = defineEmits<{ confirm: [], cancel: [] }>()
const { restoreFocus, focusContent } = useDialogFocus(() => props.open)

function cancel() {
  if (!props.busy) emit('cancel')
}
</script>

<template>
  <AlertDialogRoot :open="open" @update:open="value => { if (!value) cancel() }">
    <AlertDialogPortal>
      <AlertDialogOverlay class="nx-overlay" />
      <AlertDialogContent
        class="nx-dialog"
        aria-modal="true"
        :aria-busy="busy"
        @open-auto-focus="event => focusContent(event, '[data-cancel]')"
        @close-auto-focus="restoreFocus"
        @escape-key-down="event => { if (busy) event.preventDefault() }"
      >
        <AlertDialogTitle class="mb-3 text-lg font-semibold">{{ title }}</AlertDialogTitle>
        <AlertDialogDescription as="div" class="text-sm leading-relaxed text-muted">
          <slot />
        </AlertDialogDescription>
        <footer class="mt-6 flex flex-wrap justify-end gap-2 border-t border-border pt-4">
          <NxButton data-cancel variant="secondary" :disabled="busy" @click="cancel">
            Cancel
          </NxButton>
          <NxButton :variant="danger ? 'danger' : 'primary'" :loading="busy" @click="!busy && emit('confirm')">
            {{ confirmLabel }}
          </NxButton>
        </footer>
      </AlertDialogContent>
    </AlertDialogPortal>
  </AlertDialogRoot>
</template>
