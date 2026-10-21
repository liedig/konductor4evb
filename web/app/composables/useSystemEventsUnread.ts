const STORAGE_KEY = 'konductor:system-events:last-seen'

/**
 * Browser-local unread state for the System events page. The platform is
 * push-only (SNS), so freshness comes from each domain's latest-event marker
 * on the domain list, compared against the newest marker the user has seen
 * on this browser. No event feed and no server-side read state exist.
 */
export function useSystemEventsUnread() {
  const { list } = useDomains()
  const unread = useState<boolean>('system-events-unread', () => false)
  const latestMarker = useState<string | null>('system-events-latest-marker', () => null)

  /** Recompute the indicator from the domain list (markers are list-level). */
  async function refresh() {
    try {
      const response = await list()
      const markers = (response.domains ?? [])
        .map((d) => d.systemEventsLastEventAt)
        .filter((m): m is string => Boolean(m))
        .sort()
      latestMarker.value = markers.at(-1) ?? null
      const seen = localStorage.getItem(STORAGE_KEY)
      unread.value = latestMarker.value !== null && latestMarker.value !== seen
    } catch {
      // The indicator is best-effort; a failed list must not break the shell.
      unread.value = false
    }
  }

  /** Record a marker as seen on this browser and clear the dot. Callers pass
   * the newest marker they rendered; the nav falls back to its own refresh. */
  function markSeen(marker: string | null = latestMarker.value) {
    if (marker !== null) {
      localStorage.setItem(STORAGE_KEY, marker)
    }
    unread.value = false
  }

  return { unread, latestMarker, refresh, markSeen }
}
