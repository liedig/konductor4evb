<script setup lang="ts">
import type { BadgeTone } from './NxBadge.vue'

/**
 * Status pill — the single place where domain/subscription lifecycle states
 * map to status semantics (green = ready, blue = active, red = urgent).
 */
const props = defineProps<{ status: string }>()

const tones: Record<string, BadgeTone> = {
  ACTIVE: 'success',
  ENABLED: 'success',
  APPROVED: 'info',
  PROVISIONING: 'info',
  PENDING: 'warning',
  PENDING_APPROVAL: 'warning',
  AWAITING_PROMOTION: 'warning',
  APPROVAL_NOTIFICATION_FAILED: 'danger',
  PROVISIONING_FAILED: 'danger',
  FAILED: 'danger',
  REJECTED: 'danger',
  INACTIVE: 'neutral',
  REMOVED: 'neutral',
  VALIDATING: 'info',
  REPLAYING: 'info',
  REPINNING: 'info',
  COMPLETED: 'success',
  CANCELLED: 'neutral',
}

const tone = computed<BadgeTone>(() => tones[props.status] ?? 'neutral')
const label = computed(() => props.status.replaceAll('_', ' '))
</script>

<template>
  <NxBadge :tone="tone">{{ label }}</NxBadge>
</template>
