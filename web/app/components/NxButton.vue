<script setup lang="ts">
withDefaults(defineProps<{
  variant?: 'primary' | 'secondary' | 'danger' | 'ghost'
  size?: 'sm' | 'md'
  type?: 'button' | 'submit'
  disabled?: boolean
  loading?: boolean
}>(), {
  variant: 'primary',
  size: 'md',
  type: 'button',
  disabled: false,
  loading: false,
})

const variants = {
  primary: 'bg-accent text-on-accent hover:bg-accent-strong',
  secondary: 'border border-border-strong bg-surface-raised text-foreground hover:border-faint',
  danger: 'bg-danger/15 text-danger hover:bg-danger/25',
  ghost: 'text-muted hover:bg-surface-raised hover:text-foreground',
}
const sizes = {
  sm: 'px-3 py-1.5 text-sm',
  md: 'px-4 py-2 text-sm',
}
</script>

<template>
  <button
    :type="type"
    class="inline-flex items-center justify-center gap-2 rounded-lg font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-50"
    :class="[variants[variant], sizes[size]]"
    :disabled="disabled || loading"
  >
    <span v-if="loading" class="inline-block size-3.5 animate-spin rounded-full border-2 border-current border-t-transparent" aria-hidden="true" />
    <slot />
  </button>
</template>
