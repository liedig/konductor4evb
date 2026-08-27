import type { BadgeTone } from '~/components/NxBadge.vue'

export interface Toast {
  id: number
  tone: BadgeTone
  message: string
}

const toasts = ref<Toast[]>([])
let nextId = 0

/** App-wide transient notifications; rendered once by `<NxToasts />`. */
export function useToast() {
  function push(tone: BadgeTone, message: string) {
    const id = ++nextId
    toasts.value.push({ id, tone, message })
    setTimeout(() => dismiss(id), 6000)
  }

  function dismiss(id: number) {
    toasts.value = toasts.value.filter(t => t.id !== id)
  }

  return {
    toasts: readonly(toasts),
    success: (message: string) => push('success', message),
    info: (message: string) => push('info', message),
    error: (message: string) => push('danger', message),
    dismiss,
  }
}
