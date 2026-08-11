<script setup lang="ts">
defineProps<{ title?: string }>()
</script>

<template>
  <section class="rounded-xl border border-border bg-surface p-6 shadow-sm">
    <header v-if="title || $slots.actions" class="mb-4 flex items-center justify-between gap-4">
      <h2 v-if="title" class="text-base font-semibold">{{ title }}</h2>
      <div v-if="$slots.actions" class="flex items-center gap-2">
        <slot name="actions" />
      </div>
    </header>
    <slot />
  </section>
</template>
