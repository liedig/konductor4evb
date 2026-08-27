import { useSessionStore } from '~/stores/session'

/**
 * Persona guard for `/admin/*`: platform roles only. Wrong-persona navigation
 * redirects to the caller's own dashboard; the API still denies server-side.
 */
export default defineNuxtRouteMiddleware(async () => {
  const session = useSessionStore()
  if (session.status === 'unknown') {
    await session.restore()
  }
  if (session.status !== 'signed-in') {
    return navigateTo('/login')
  }
  if (!session.isPlatformRole) {
    return navigateTo(session.homeRoute)
  }
})
