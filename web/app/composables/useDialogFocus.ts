export function useDialogFocus(open: () => boolean) {
  let opener: HTMLElement | null = null

  watch(open, (value) => {
    if (value) opener = document.activeElement instanceof HTMLElement ? document.activeElement : null
  }, { immediate: true, flush: 'sync' })

  function restoreFocus(event: Event) {
    event.preventDefault()
    if (opener?.isConnected && !opener.matches(':disabled') && opener.getClientRects().length) {
      opener.focus({ preventScroll: true })
      if (document.activeElement === opener) return
    }
    const fallback = document.querySelector<HTMLElement>('main h1') ?? document.querySelector<HTMLElement>('main')
    if (!fallback) return
    const hadTabIndex = fallback.hasAttribute('tabindex')
    if (!hadTabIndex) fallback.setAttribute('tabindex', '-1')
    fallback.focus({ preventScroll: true })
    if (!hadTabIndex) fallback.addEventListener('blur', () => fallback.removeAttribute('tabindex'), { once: true })
  }

  function focusContent(event: Event, preferred = 'input:not(:disabled), select:not(:disabled), textarea:not(:disabled)') {
    if (!(event.target instanceof HTMLElement)) return
    event.preventDefault()
    const content = event.target
    const target = content.querySelector<HTMLElement>(preferred)
      ?? content.querySelector<HTMLElement>('button:not(:disabled), a[href], [tabindex="0"]')
      ?? content
    target.focus({ preventScroll: true })
  }

  return { restoreFocus, focusContent }
}
