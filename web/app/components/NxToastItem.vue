<script setup lang="ts">
import { ToastClose, ToastDescription, ToastRoot } from 'reka-ui'
import type { Toast } from '~/composables/useToast'

const props = defineProps<{ toast: Toast, paused: boolean }>()
const emit = defineEmits<{ dismiss: [] }>()

const toneClasses = {
  success: 'border-success/40',
  info: 'border-info/40',
  warning: 'border-warning/40',
  danger: 'border-danger/40',
  neutral: 'border-border-strong',
}

useToastLifetime(() => props.paused, () => emit('dismiss'))
</script>

<template>
  <ToastRoot
    type="background"
    class="flex items-start justify-between gap-3 rounded-xl border bg-surface-raised p-4 text-sm shadow-sm"
    :class="toneClasses[toast.tone]"
    @update:open="open => { if (!open) emit('dismiss') }"
  >
    <ToastDescription class="min-w-0 flex-1 break-words leading-relaxed text-foreground">{{ toast.message }}</ToastDescription>
    <ToastClose as-child>
      <NxButton variant="ghost" size="sm" aria-label="Dismiss notification">
        <span aria-hidden="true">✕</span>
      </NxButton>
    </ToastClose>
  </ToastRoot>
</template>
