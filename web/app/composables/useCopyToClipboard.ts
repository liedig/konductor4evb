/** Copies text to the clipboard and flips `copied` for a short
 * confirmation window so the trigger can acknowledge inline. */
export function useCopyToClipboard(resetAfterMs = 1600) {
  const copied = ref(false)
  let timer: ReturnType<typeof setTimeout> | undefined

  async function copy(text: string) {
    await navigator.clipboard.writeText(text)
    copied.value = true
    clearTimeout(timer)
    timer = setTimeout(() => {
      copied.value = false
    }, resetAfterMs)
  }

  return { copied: readonly(copied), copy }
}
