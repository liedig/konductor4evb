<script setup lang="ts">
import { useSessionStore } from '~/stores/session'

definePageMeta({ layout: 'plain' })

const session = useSessionStore()

const username = ref('')
const password = ref('')
const error = ref('')
const busy = ref(false)

// Safe failure surface: never disclose which factor failed or whether the
// user exists (spec: Custom Cognito login and session).
const GENERIC_ERROR = 'Invalid username or password.'

async function submit() {
  if (busy.value) return
  error.value = ''
  busy.value = true
  try {
    await session.login(username.value.trim(), password.value)
    await navigateTo(session.homeRoute)
  } catch {
    error.value = GENERIC_ERROR
    password.value = ''
  } finally {
    busy.value = false
  }
}

onMounted(async () => {
  if (await session.restore()) {
    await navigateTo(session.homeRoute)
  }
})
</script>

<template>
  <div class="w-full max-w-sm">
    <div class="mb-8 text-center">
      <p class="text-lg font-semibold tracking-wide">Konductor <span class="text-accent">for Amazon EventBridge</span></p>
      <p class="mt-1 text-sm text-muted">Sign in to the control-plane console</p>
    </div>
    <form class="rounded-xl border border-border bg-surface p-6 shadow-sm" @submit.prevent="submit">
      <div class="flex flex-col gap-4">
        <NxInput v-model="username" label="Username" autocomplete="username" required />
        <NxInput v-model="password" label="Password" type="password" autocomplete="current-password" required />
        <p v-if="error" role="alert" class="text-sm text-danger">{{ error }}</p>
        <NxButton type="submit" :loading="busy" class="w-full">Sign in</NxButton>
      </div>
    </form>
    <p class="mt-4 text-center text-xs text-faint">
      Accounts are provisioned by the platform team. Sessions persist per tab.
    </p>
  </div>
</template>
