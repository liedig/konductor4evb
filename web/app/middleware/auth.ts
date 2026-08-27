import { useSessionStore } from '~/stores/session'

/**
 * Auth guard: protected routes require a restored session. The guard is
 * navigation hygiene — the API remains the enforcing boundary (design.md,
 * decision 7).
 */
export default defineNuxtRouteMiddleware(async () => {
  const session = useSessionStore()
  if (session.status === 'unknown') {
    await session.restore()
  }
  if (session.status !== 'signed-in') {
    return navigateTo('/login')
  }
})
