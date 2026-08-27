import { fetchAuthSession, signIn, signOut } from 'aws-amplify/auth'
import { defineStore } from 'pinia'

/**
 * Session/identity store — the only Pinia state in the app (design.md,
 * decision 5). Group handling mirrors the backend principal contract
 * (konductor-common `principal.rs`): the controlled groups `platform-admin` and
 * `platform-auditor` are global roles; every other group is a team group.
 */

const GLOBAL_ROLE_GROUPS = ['platform-admin', 'platform-auditor'] as const

type SessionStatus = 'unknown' | 'signed-in' | 'signed-out'

export type Persona = 'platform' | 'owner'

export const useSessionStore = defineStore('session', () => {
  const status = ref<SessionStatus>('unknown')
  const username = ref<string | null>(null)
  const groups = ref<string[]>([])

  const globalRoles = computed(() => groups.value.filter(g => (GLOBAL_ROLE_GROUPS as readonly string[]).includes(g)))
  const teamGroups = computed(() => groups.value.filter(g => !(GLOBAL_ROLE_GROUPS as readonly string[]).includes(g)))
  const isPlatformAdmin = computed(() => globalRoles.value.includes('platform-admin'))
  const isPlatformRole = computed(() => globalRoles.value.length > 0)
  const persona = computed<Persona>(() => (isPlatformRole.value ? 'platform' : 'owner'))
  const homeRoute = computed(() => (persona.value === 'platform' ? '/admin' : '/domains'))

  function applySession(payload: Record<string, unknown>) {
    // Email-alias pools put the UUID in `cognito:username`; display the email.
    username.value =
      (payload['email'] as string | undefined)
      ?? (payload['cognito:username'] as string | undefined)
      ?? null
    groups.value = (payload['cognito:groups'] as string[] | undefined) ?? []
    status.value = 'signed-in'
  }

  async function restore(): Promise<boolean> {
    try {
      const { tokens } = await fetchAuthSession()
      const payload = tokens?.idToken?.payload
      if (!tokens?.accessToken || !payload) {
        status.value = 'signed-out'
        return false
      }
      applySession(payload)
      return true
    } catch {
      status.value = 'signed-out'
      return false
    }
  }

  async function login(user: string, password: string) {
    const outcome = await signIn({ username: user, password })
    if (!outcome.isSignedIn) {
      throw new Error(`Unsupported sign-in challenge: ${outcome.nextStep.signInStep}`)
    }
    await restore()
  }

  async function logout() {
    try {
      await signOut()
    } finally {
      status.value = 'signed-out'
      username.value = null
      groups.value = []
    }
  }

  /** Access token for API calls; Amplify renews expired tokens transparently. */
  async function accessToken(forceRefresh = false): Promise<string> {
    const { tokens } = await fetchAuthSession({ forceRefresh })
    const token = tokens?.accessToken?.toString()
    if (!token) {
      status.value = 'signed-out'
      throw new Error('No active session')
    }
    return token
  }

  return {
    status,
    username,
    groups,
    globalRoles,
    teamGroups,
    isPlatformAdmin,
    isPlatformRole,
    persona,
    homeRoute,
    restore,
    login,
    logout,
    accessToken,
  }
})
