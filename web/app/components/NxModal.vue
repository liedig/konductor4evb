<script setup lang="ts">
const props = withDefaults(defineProps<{
  open: boolean
  title: string
}>(), {})

const emit = defineEmits<{ close: [] }>()

watch(() => props.open, (open) => {
  if (!open) return
  const onKey = (event: KeyboardEvent) => {
    if (event.key === 'Escape') emit('close')
  }
  window.addEventListener('keydown', onKey)
  onWatcherCleanup(() => window.removeEventListener('keydown', onKey))
})
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open"
      class="fixed inset-0 z-50 flex items-center justify-center p-4"
      role="dialog"
      aria-modal="true"
      :aria-label="title"
    >
      <div class="absolute inset-0 bg-background/80" @click="emit('close')" />
      <div class="relative w-full max-w-lg rounded-xl border border-border bg-surface p-6 shadow-sm">
        <header class="mb-4 flex items-start justify-between gap-4">
          <h2 class="text-base font-semibold">{{ title }}</h2>
          <button
            type="button"
            class="text-faint transition-colors hover:text-foreground"
            aria-label="Close"
            @click="emit('close')"
          >
            ✕
          </button>
        </header>
        <slot />
        <footer v-if="$slots.actions" class="mt-6 flex justify-end gap-2">
          <slot name="actions" />
        </footer>
      </div>
    </div>
  </Teleport>
</template>
