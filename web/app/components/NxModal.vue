<script setup lang="ts">
import { DialogContent, DialogOverlay, DialogPortal, DialogRoot, DialogTitle } from 'reka-ui'

const props = defineProps<{
  open: boolean
  title: string
}>()

const emit = defineEmits<{ close: [] }>()

const { restoreFocus, focusContent } = useDialogFocus(() => props.open)
</script>

<template>
  <DialogRoot :open="open" @update:open="value => { if (!value) emit('close') }">
    <DialogPortal>
      <DialogOverlay class="nx-overlay" @click.self="emit('close')" />
      <DialogContent
        class="nx-dialog"
        aria-modal="true"
        :aria-describedby="undefined"
        @open-auto-focus="focusContent"
        @close-auto-focus="restoreFocus"
      >
        <header class="mb-4 flex items-start justify-between gap-4">
          <DialogTitle class="text-lg font-semibold">{{ title }}</DialogTitle>
          <NxButton variant="ghost" size="sm" aria-label="Close" @click="emit('close')">
            <span aria-hidden="true">✕</span>
          </NxButton>
        </header>
        <slot />
        <footer v-if="$slots.actions" class="mt-6 flex flex-wrap justify-end gap-2 border-t border-border pt-4">
          <slot name="actions" />
        </footer>
      </DialogContent>
    </DialogPortal>
  </DialogRoot>
</template>
