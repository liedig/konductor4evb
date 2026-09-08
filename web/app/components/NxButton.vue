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
  primary: 'border border-accent bg-accent text-on-accent enabled:hover:border-accent-strong enabled:hover:bg-accent-strong',
  secondary: 'border border-border-strong bg-surface-raised text-foreground enabled:hover:border-muted',
  danger: 'border border-danger/35 bg-danger/10 text-danger enabled:hover:bg-danger/20',
  ghost: 'border border-transparent text-muted enabled:hover:bg-surface-raised enabled:hover:text-foreground',
}
const sizes = {
  sm: 'px-3 py-1.5 text-sm',
  md: 'px-4 py-2 text-sm',
}
</script>

<template>
  <button
    :type="type"
    class="inline-flex shrink-0 items-center justify-center gap-2 rounded-lg font-medium leading-5 transition-colors duration-150 disabled:cursor-not-allowed disabled:opacity-50"
    :class="[variants[variant], sizes[size]]"
    :disabled="disabled || loading"
    :aria-busy="loading"
  >
    <span v-if="loading" class="inline-block size-3.5 animate-spin rounded-full border-2 border-current border-t-transparent" aria-hidden="true" />
    <slot />
  </button>
</template>
