<script setup lang="ts">
withDefaults(defineProps<{
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
</script>

<template>
  <NxModal :open="open" :title="title" @close="emit('cancel')">
    <div class="text-sm text-muted">
      <slot />
    </div>
    <template #actions>
      <NxButton variant="secondary" :disabled="busy" @click="emit('cancel')">
        Cancel
      </NxButton>
      <NxButton :variant="danger ? 'danger' : 'primary'" :loading="busy" @click="emit('confirm')">
        {{ confirmLabel }}
      </NxButton>
    </template>
  </NxModal>
</template>
