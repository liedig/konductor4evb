import { ApiError } from '~/composables/useApi'
import type { DirectoryEntry, Entitlement, SchemaSummary } from '~/types/api'

/**
 * Loads and manages one domain's access grants — which consumer domains
 * may discover and subscribe to this domain's schemas. Shared by the
 * domain page's summary panel and the dedicated grants view, so the grant
 * and revoke flows work identically on both surfaces.
 */
export function useAccessGrants(domain: () => string) {
  const api = useApi()
  const domainsApi = useDomains()
  const catalog = useCatalog()
  const toast = useToast()

  const entitlements = ref<Entitlement[]>([])
  const forbidden = ref(false)
  const loadFailed = ref(false)
  const pending = ref(true)

  const grantOpen = ref(false)
  const grantBusy = ref(false)
  const directoryEntries = ref<DirectoryEntry[]>([])
  const producerSchemas = ref<SchemaSummary[]>([])
  const grantForm = reactive({ consumerDomain: '', scopePattern: '' })

  const revokeTarget = ref<Entitlement | null>(null)
  const revokeBusy = ref(false)

  async function load() {
    pending.value = true
    try {
      const response = await api.get<{ entitlements?: Entitlement[] }>('/entitlements', { producerDomain: domain() })
      entitlements.value = response.entitlements ?? []
    } catch (error) {
      if (error instanceof ApiError && error.status === 403) {
        forbidden.value = true
      } else {
        loadFailed.value = true
        toast.error(error instanceof ApiError ? error.message : 'Failed to load access grants')
      }
    } finally {
      pending.value = false
    }
  }

  async function openGrant() {
    grantForm.consumerDomain = ''
    grantForm.scopePattern = ''
    grantOpen.value = true
    const [directory, schemas] = await Promise.all([
      domainsApi.directory().catch(() => [] as DirectoryEntry[]),
      catalog.listSchemas({ producerDomain: domain() }).catch(() => ({ items: [] as SchemaSummary[] })),
    ])
    directoryEntries.value = directory.filter(entry => entry.domainName !== domain())
    producerSchemas.value = schemas.items ?? []
  }

  const scopeOptions = computed(() => [
    { value: '*', label: '* — entire domain, including future schemas' },
    ...producerSchemas.value.map((schema) => {
      const detailType = schema.schemaName?.split('@')[1] ?? schema.schemaName ?? ''
      return { value: detailType, label: `${detailType} — exact schema` }
    }),
  ])

  async function submitGrant() {
    if (!grantForm.consumerDomain || !grantForm.scopePattern) return
    grantBusy.value = true
    try {
      await api.post('/entitlements', {
        consumerDomain: grantForm.consumerDomain,
        producerDomain: domain(),
        scopePattern: grantForm.scopePattern,
      })
      toast.success(`Access granted to ${grantForm.consumerDomain}`)
      grantOpen.value = false
      await load()
    } catch (error) {
      toast.error(error instanceof ApiError ? error.message : 'Grant failed')
    } finally {
      grantBusy.value = false
    }
  }

  async function confirmRevoke() {
    const target = revokeTarget.value
    if (!target?.entitlementId) return
    revokeBusy.value = true
    try {
      await api.del(`/entitlements/${encodeURIComponent(target.entitlementId)}`)
      toast.success(`Access revoked for ${target.consumerDomain}`)
      revokeTarget.value = null
      await load()
    } catch (error) {
      toast.error(error instanceof ApiError ? error.message : 'Revoke failed')
    } finally {
      revokeBusy.value = false
    }
  }

  return {
    entitlements,
    forbidden,
    loadFailed,
    pending,
    load,
    grantOpen,
    grantBusy,
    directoryEntries,
    scopeOptions,
    grantForm,
    openGrant,
    submitGrant,
    revokeTarget,
    revokeBusy,
    confirmRevoke,
  }
}
