import { ApiError } from '~/composables/useApi'
import type { SchemaSummary } from '~/types/api'

/**
 * Loads one domain's schemas through the producer-scoped catalog, one page
 * at a time. Shared by the domain page's summary panel and the dedicated
 * schemas view; both render the same rows, the panel just caps them.
 *
 * Producer scope: publishers see their own domain's schemas without any
 * consumer entitlement (`GET /schemas?domain=…` with no `consumerDomain`).
 */
export function useDomainSchemas(domain: () => string) {
  const catalog = useCatalog()
  const toast = useToast()

  const schemas = ref<SchemaSummary[]>([])
  const nextToken = ref<string | undefined>()
  const forbidden = ref(false)
  const loadFailed = ref(false)
  const pending = ref(true)

  // Requests can overlap (a page request while a filter change is in
  // flight); only the latest request may write into the list.
  let requestSeq = 0

  async function load(reset = true) {
    const seq = ++requestSeq
    pending.value = true
    if (reset) {
      schemas.value = []
      nextToken.value = undefined
      forbidden.value = false
      loadFailed.value = false
    }
    try {
      const response = await catalog.listSchemas({
        producerDomain: domain(),
        nextToken: reset ? undefined : nextToken.value,
      })
      if (seq !== requestSeq) return
      if (reset) {
        schemas.value = response.items ?? []
      } else {
        schemas.value.push(...(response.items ?? []))
      }
      // The catalog ends its pages with an explicit `"nextToken": null`;
      // normalize it so "no further pages" is always undefined.
      nextToken.value = response.nextToken ?? undefined
    } catch (error) {
      if (seq !== requestSeq) return
      if (error instanceof ApiError && error.status === 403) {
        forbidden.value = true
      } else {
        loadFailed.value = true
        toast.error(error instanceof ApiError ? error.message : 'Failed to load schemas')
      }
    } finally {
      if (seq === requestSeq) pending.value = false
    }
  }

  return { schemas, nextToken, forbidden, loadFailed, pending, load }
}
