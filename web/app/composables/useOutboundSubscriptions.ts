import { ApiError } from '~/composables/useApi'
import type { SubscriptionSummary } from '~/types/api'

/**
 * Loads one domain's outbound subscriptions — who consumes this domain's
 * schemas. Shared by the domain page's summary panel and the dedicated
 * subscriptions view; the API returns the full set in one flat response,
 * so both surfaces see everything and differ only in how much they show.
 */
export function useOutboundSubscriptions(domain: () => string) {
  const catalog = useCatalog()
  const toast = useToast()

  const subscriptions = ref<SubscriptionSummary[]>([])
  const forbidden = ref(false)
  const loadFailed = ref(false)
  const pending = ref(true)

  async function load() {
    pending.value = true
    try {
      const response = await catalog.listSubscriptions({ domain: domain(), direction: 'outbound' })
      subscriptions.value = (response.items ?? []).filter(subscription => subscription.status !== 'REMOVED')
    } catch (error) {
      if (error instanceof ApiError && error.status === 403) {
        forbidden.value = true
      } else {
        loadFailed.value = true
        toast.error(error instanceof ApiError ? error.message : 'Failed to load subscriptions')
      }
    } finally {
      pending.value = false
    }
  }

  return { subscriptions, forbidden, loadFailed, pending, load }
}
