<script setup lang="ts">
withDefaults(defineProps<{
  label?: string
  hint?: string
  error?: string
  options: { value: string, label: string }[]
  placeholder?: string
  required?: boolean
  disabled?: boolean
  id?: string
}>(), {
  required: false,
  disabled: false,
  label: undefined,
  hint: undefined,
  error: undefined,
  placeholder: undefined,
  id: undefined,
})

const model = defineModel<string>({ default: '' })
const uid = useId()
</script>

<template>
  <div class="flex flex-col gap-1.5">
    <label v-if="label" :for="id ?? uid" class="text-sm font-medium text-muted">
      {{ label }}<span v-if="required" class="text-danger"> *</span>
    </label>
    <select
      :id="id ?? uid"
      v-model="model"
      :required="required"
      :disabled="disabled"
      :aria-invalid="!!error"
      :aria-describedby="error || hint ? `${id ?? uid}-description` : undefined"
      class="min-w-0 rounded-lg border bg-background px-3 py-2.5 text-sm text-foreground transition-colors disabled:cursor-not-allowed disabled:opacity-50"
      :class="error ? 'border-danger' : 'border-border-strong enabled:hover:border-muted focus:border-focus'"
    >
      <option v-if="placeholder" value="" disabled>{{ placeholder }}</option>
      <option v-for="option in options" :key="option.value" :value="option.value">
        {{ option.label }}
      </option>
    </select>
    <p v-if="error" :id="`${id ?? uid}-description`" class="text-sm text-danger">{{ error }}</p>
    <p v-else-if="hint" :id="`${id ?? uid}-description`" class="text-sm text-muted">{{ hint }}</p>
  </div>
</template>
