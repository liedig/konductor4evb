<script setup lang="ts">
export type BadgeTone = 'success' | 'info' | 'warning' | 'danger' | 'neutral'

withDefaults(defineProps<{ tone?: BadgeTone }>(), { tone: 'neutral' })

const tones: Record<BadgeTone, string> = {
  success: 'bg-success/15 text-success',
  info: 'bg-info/15 text-info',
  warning: 'bg-warning/15 text-warning',
  danger: 'bg-danger/15 text-danger',
  neutral: 'bg-neutral/15 text-neutral',
}
</script>

<template>
  <span
    class="inline-flex items-center gap-1.5 rounded-lg px-2 py-0.5 text-xs font-medium"
    :class="tones[tone]"
  >
    <slot />
  </span>
</template>
