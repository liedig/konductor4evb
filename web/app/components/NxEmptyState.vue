<script setup lang="ts">
defineProps<{
  title: string
  body?: string
}>()
</script>

<template>
  <div class="flex flex-col items-center gap-3 rounded-xl border border-dashed border-border-strong px-6 py-12 text-center">
    <div class="text-3xl text-faint" aria-hidden="true">
      <slot name="icon">◌</slot>
    </div>
    <h3 class="text-base font-semibold">{{ title }}</h3>
    <p v-if="body" class="max-w-md text-sm text-muted">{{ body }}</p>
    <div v-if="$slots.actions" class="mt-2 flex items-center gap-2">
      <slot name="actions" />
    </div>
  </div>
</template>
