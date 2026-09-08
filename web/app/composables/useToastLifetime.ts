export function useToastLifetime(paused: () => boolean, dismiss: () => void) {
  let remaining = 6000

  watch(paused, (isPaused, _previous, onCleanup) => {
    if (isPaused) return
    const started = Date.now()
    const timer = setTimeout(dismiss, remaining)
    onCleanup(() => {
      clearTimeout(timer)
      remaining = Math.max(0, remaining - (Date.now() - started))
    })
  }, { immediate: true, flush: 'sync' })
}
