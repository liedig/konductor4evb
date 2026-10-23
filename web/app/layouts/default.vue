<script setup lang="ts">
import { useSessionStore } from '~/stores/session'

const session = useSessionStore()
const route = useRoute()

// The header shows the local part only; the full address stays on the
// tooltip. Email-alias pools put a UUID in cognito:username — with no '@'
// the split returns it whole, which is still the right display.
const displayName = computed(() => session.username?.split('@')[0] ?? '')

const navItems = computed(() => {
  const items: { to: string; label: string; icon?: boolean }[] = [
    { to: '/domains', label: 'Domains' },
    { to: '/catalog', label: 'Catalog' },
    { to: '/subscriptions', label: 'Subscriptions' },
    { to: '/system-events', label: 'System events', icon: true },
  ]
  if (session.isPlatformRole) {
    items.unshift({ to: '/admin', label: 'Approvals' })
  }
  return items
})

const { unread: systemEventsUnread, refresh: refreshSystemEvents } = useSystemEventsUnread()

// Freshness check on shell mount and on navigation; the markers come from
// the domain list, so this is one inexpensive request per route change.
onMounted(refreshSystemEvents)
watch(() => route.path, () => refreshSystemEvents())

// Approvals matches exactly — /admin/* children belong to the sections that
// own them. All domains is grouped under Domains, so /admin/domains also
// activates the Domains item.
function isActive(path: string): boolean {
  if (path === '/admin') return route.path === '/admin'
  if (path === '/domains') return route.path.startsWith('/domains') || route.path.startsWith('/admin/domains')
  return route.path === path || route.path.startsWith(`${path}/`)
}

// Section tabs replace the old top-level "All domains" entry; only platform
// roles have a second tab to switch to.
const inDomainsSection = computed(
  () => session.isPlatformRole && (route.path.startsWith('/domains') || route.path.startsWith('/admin/domains')),
)

const settingsOpen = ref(false)
const settingsRoot = ref<HTMLElement | null>(null)
const settingsTrigger = ref<HTMLButtonElement | null>(null)
const settingsActive = computed(() => route.path.startsWith('/admin/metadata'))

// Menu hygiene: Escape closes and returns focus to the trigger, an outside
// pointer closes, and navigation closes.
watch(settingsOpen, (open) => {
  if (!open) return
  const onKey = (event: KeyboardEvent) => {
    if (event.key === 'Escape') {
      settingsOpen.value = false
      settingsTrigger.value?.focus()
    }
  }
  const onPointer = (event: PointerEvent) => {
    if (event.target instanceof Node && settingsRoot.value?.contains(event.target)) return
    settingsOpen.value = false
  }
  window.addEventListener('keydown', onKey)
  document.addEventListener('pointerdown', onPointer)
  onWatcherCleanup(() => {
    window.removeEventListener('keydown', onKey)
    document.removeEventListener('pointerdown', onPointer)
  })
})
watch(() => route.path, () => {
  settingsOpen.value = false
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
            class="flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-sm text-muted transition-colors hover:text-foreground"
            :class="{ 'bg-surface-raised text-foreground': isActive(item.to) }"
            :aria-label="item.icon ? item.label : undefined"
          >
            <span v-if="item.icon" class="relative">
              <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor" class="h-4 w-4" aria-hidden="true">
                <path stroke-linecap="round" stroke-linejoin="round" d="M14.857 17.082a23.848 23.848 0 005.454-1.31A8.967 8.967 0 0118 9.75v-.7V9A6 6 0 006 9v.75a8.967 8.967 0 01-2.312 6.022c1.733.64 3.56 1.085 5.455 1.31m5.714 0a24.255 24.255 0 01-5.714 0m5.714 0a3 3 0 11-5.714 0" />
              </svg>
              <span
                v-if="systemEventsUnread"
                data-testid="system-events-unread-dot"
                class="absolute -right-0.5 -top-0.5 h-2 w-2 rounded-full bg-warning"
                aria-label="New system events"
                role="status"
              />
            </span>
            <!-- Icon items render no visible label; their aria-label above
                 keeps the accessible name. -->
            <span v-if="!item.icon">{{ item.label }}</span>
          </NuxtLink>

        </nav>
        <div class="flex items-center gap-3">
          <div v-if="session.isPlatformRole" ref="settingsRoot" class="relative">
            <button
              ref="settingsTrigger"
              type="button"
              aria-label="Settings"
              aria-haspopup="menu"
              :aria-expanded="settingsOpen"
              class="rounded-lg p-1.5 transition-colors"
              :class="settingsActive || settingsOpen
                ? 'bg-surface-raised text-foreground'
                : 'text-muted hover:text-foreground'"
              @click="settingsOpen = !settingsOpen"
            >
              <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor" class="h-5 w-5" aria-hidden="true">
                <path stroke-linecap="round" stroke-linejoin="round" d="M9.594 3.94c.09-.542.56-.94 1.11-.94h2.593c.55 0 1.02.398 1.11.94l.213 1.281c.063.374.313.686.645.87.074.04.147.083.22.127.324.196.72.257 1.075.124l1.217-.456a1.125 1.125 0 011.37.49l1.296 2.247a1.125 1.125 0 01-.26 1.431l-1.003.827c-.293.24-.438.613-.431.992a6.759 6.759 0 010 .255c-.007.378.138.75.43.99l1.005.828c.424.35.534.954.26 1.43l-1.298 2.247a1.125 1.125 0 01-1.369.491l-1.217-.456c-.355-.133-.75-.072-1.076.124a6.57 6.57 0 01-.22.128c-.331.183-.581.495-.644.869l-.213 1.28c-.09.543-.56.941-1.11.941h-2.594c-.55 0-1.02-.398-1.11-.94l-.213-1.281c-.062-.374-.312-.686-.644-.87a6.52 6.52 0 01-.22-.127c-.325-.196-.72-.257-1.076-.124l-1.217.456a1.125 1.125 0 01-1.369-.49l-1.297-2.247a1.125 1.125 0 01.26-1.431l1.004-.827c.292-.24.437-.613.43-.992a6.932 6.932 0 010-.255c.007-.378-.138-.75-.43-.99l-1.004-.828a1.125 1.125 0 01-.26-1.43l1.297-2.247a1.125 1.125 0 011.37-.491l1.216.456c.356.133.751.072 1.076-.124.072-.044.146-.087.22-.128.332-.183.582-.495.644-.869l.214-1.28z" />
                <path stroke-linecap="round" stroke-linejoin="round" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
              </svg>
            </button>
            <div
              v-if="settingsOpen"
              role="menu"
              class="absolute right-0 z-40 mt-1 w-40 rounded-lg border border-border bg-surface p-1 shadow-sm"
            >
              <NuxtLink
                to="/admin/metadata"
                role="menuitem"
                class="block rounded-md px-3 py-1.5 text-sm transition-colors"
                :class="settingsActive ? 'bg-surface-raised text-foreground' : 'text-muted hover:text-foreground'"
              >
                Metadata
              </NuxtLink>
            </div>
          </div>
          <span class="text-sm text-muted" :title="session.username ?? undefined">{{ displayName }}</span>
          <NxBadge v-if="session.isPlatformRole" tone="info">{{ session.globalRoles[0] }}</NxBadge>
          <NxButton variant="ghost" size="sm" @click="logout">Sign out</NxButton>
        </div>
      </div>
      <div v-if="inDomainsSection" class="border-t border-border">
        <nav class="mx-auto flex max-w-6xl items-center gap-1 px-6 py-1.5" aria-label="Domains sections">
          <NuxtLink
            to="/domains"
            class="rounded-lg px-3 py-1 text-sm text-muted transition-colors hover:text-foreground"
            :class="{ 'bg-surface-raised text-foreground': route.path.startsWith('/domains') }"
          >
            My domains
          </NuxtLink>
          <NuxtLink
            to="/admin/domains"
            class="rounded-lg px-3 py-1 text-sm text-muted transition-colors hover:text-foreground"
            :class="{ 'bg-surface-raised text-foreground': route.path.startsWith('/admin/domains') }"
          >
            All domains
          </NuxtLink>
        </nav>
      </div>
    </header>
    <main class="mx-auto max-w-6xl px-6 py-10">
      <slot />
    </main>
  </div>
</template>
