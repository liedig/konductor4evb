<script setup lang="ts">
import { useSessionStore } from '~/stores/session'

definePageMeta({ layout: 'plain' })

// Post-login persona redirect: platform roles land inbox-first, everyone else
// domain-first (spec: Role-gated navigation and persona dashboards).
const session = useSessionStore()

onMounted(async () => {
  const signedIn = session.status === 'signed-in' || (await session.restore())
  await navigateTo(signedIn ? session.homeRoute : '/login')
})
</script>

<template>
  <p class="text-sm text-muted">Loading…</p>
</template>
