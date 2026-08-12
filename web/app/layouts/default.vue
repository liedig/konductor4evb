<script setup lang="ts">
import { useSessionStore } from '~/stores/session'

const session = useSessionStore()
const route = useRoute()

const navItems = computed(() => {
  const items = [
    { to: '/domains', label: 'Domains' },
    { to: '/catalog', label: 'Catalog' },
    { to: '/subscriptions', label: 'Subscriptions' },
  ]
  if (session.isPlatformRole) {
    items.unshift({ to: '/admin', label: 'Approvals' }, { to: '/admin/domains', label: 'All domains' })
  }
  return items
})

async function logout() {
  await session.logout()
  await navigateTo('/login')
}
</script>

<template>
  <div class="min-h-screen">
    <header class="border-b border-border bg-surface">
      <div class="mx-auto flex max-w-6xl items-center justify-between gap-6 px-6 py-3">
        <NuxtLink to="/" class="shrink-0">
          <img src="/brand/konductor-logo-dark-bg.svg" alt="Konductor for Amazon EventBridge" class="h-7 w-auto" >
        </NuxtLink>
        <nav class="flex items-center gap-1">
          <NuxtLink
            v-for="item in navItems"
            :key="item.to"
            :to="item.to"
            class="rounded-lg px-3 py-1.5 text-sm text-muted transition-colors hover:text-foreground"
            :class="{ 'bg-surface-raised text-foreground': route.path === item.to || (item.to !== '/' && route.path.startsWith(item.to + '/')) }"
          >
            {{ item.label }}
          </NuxtLink>
        </nav>
        <div class="flex items-center gap-3">
          <span class="text-sm text-muted">{{ session.username }}</span>
          <NxBadge v-if="session.isPlatformRole" tone="info">{{ session.globalRoles[0] }}</NxBadge>
          <NxButton variant="ghost" size="sm" @click="logout">Sign out</NxButton>
        </div>
      </div>
    </header>
    <main class="mx-auto max-w-6xl px-6 py-10">
      <slot />
    </main>
  </div>
</template>
