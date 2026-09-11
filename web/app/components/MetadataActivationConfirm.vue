<script setup lang="ts">
/**
 * Explicit activation confirmation: names the environment, current revision,
 * and candidate revision; requires the consumer-readiness acknowledgement;
 * blocks dismissal while the activation is in flight; and on a stale-state
 * conflict reloads authoritative state instead of optimistically succeeding.
 */
const props = withDefaults(defineProps<{
  open: boolean
  environment: string
  currentRevision: number
  candidateRevision: number
  busy?: boolean
  alreadyAcknowledged?: boolean
}>(), {
  busy: false,
  alreadyAcknowledged: false,
})

const emit = defineEmits<{
  confirm: []
  close: []
}>()

const acknowledged = ref(false)

watch(() => props.open, (open) => {
  if (open) acknowledged.value = props.alreadyAcknowledged
})

/** Busy-safe dismissal: clicks and Escape during flight are refused. */
function requestClose(): void {
  if (props.busy) return
  emit('close')
}
</script>

<template>
  <NxModal :open="open" :title="`Activate M${candidateRevision} for ${environment}`" @close="requestClose">
    <div class="flex flex-col gap-4">
      <dl class="grid grid-cols-2 gap-2 text-sm">
        <dt class="text-muted">Environment</dt>
        <dd class="font-medium">{{ environment }}</dd>
        <dt class="text-muted">Current active revision</dt>
        <dd>M{{ currentRevision }}</dd>
        <dt class="text-muted">Candidate revision</dt>
        <dd class="font-medium">M{{ candidateRevision }}</dd>
      </dl>

      <p class="rounded-lg border border-warning bg-warning/10 px-4 py-3 text-sm text-warning">
        Consumers must tolerate metadata additions. Consumers that validate the exact
        metadata block can reject events carrying the new fields even when business data
        is unchanged. Readiness and inventory results are timestamped observations, not
        proof of external consumer compatibility.
      </p>

      <label class="flex items-start gap-2 text-sm">
        <input
          v-model="acknowledged"
          type="checkbox"
          class="mt-0.5"
        >
        <span>
          I acknowledge consumers of {{ environment }} events must tolerate the fields added by
          M{{ candidateRevision }}, and activation moves {{ environment }} forward only.
        </span>
      </label>
    </div>

    <template #actions>
      <NxButton variant="secondary" size="sm" :disabled="busy" @click="requestClose">Cancel</NxButton>
      <NxButton
        variant="primary"
        size="sm"
        :disabled="!acknowledged || busy"
        @click="emit('confirm')"
      >
        {{ busy ? 'Activating…' : `Activate M${candidateRevision} for ${environment}` }}
      </NxButton>
    </template>
  </NxModal>
</template>
