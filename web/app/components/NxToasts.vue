<script setup lang="ts">
const { toasts, dismiss } = useToast()

const toneClasses = {
  success: 'border-success/40 text-success',
  info: 'border-info/40 text-info',
  warning: 'border-warning/40 text-warning',
  danger: 'border-danger/40 text-danger',
  neutral: 'border-border-strong text-muted',
}
</script>

<template>
  <div class="pointer-events-none fixed right-4 bottom-4 z-50 flex w-96 max-w-full flex-col gap-2" aria-live="polite">
    <div
      v-for="toast in toasts"
      :key="toast.id"
      class="pointer-events-auto flex items-start justify-between gap-3 rounded-lg border bg-surface-raised px-4 py-3 text-sm shadow-sm"
      :class="toneClasses[toast.tone]"
    >
      <span class="text-foreground">{{ toast.message }}</span>
      <button
        type="button"
        class="shrink-0 text-faint transition-colors hover:text-foreground"
        aria-label="Dismiss"
        @click="dismiss(toast.id)"
      >
        ✕
      </button>
    </div>
  </div>
</template>
