<script setup lang="ts">
import { ToastProvider, ToastViewport } from 'reka-ui'

const { toasts, dismiss } = useToast()

const host = useTemplateRef<HTMLElement>('host')
const hovered = ref(false)
const focused = ref(false)
const inactive = ref(false)
const paused = computed(() => hovered.value || focused.value || inactive.value)

function focusOut(event: FocusEvent) {
  focused.value = event.relatedTarget instanceof Node && !!host.value?.contains(event.relatedTarget)
}

function windowBlur() { inactive.value = true }
function windowFocus() { inactive.value = false }

onMounted(() => {
  inactive.value = !document.hasFocus()
  window.addEventListener('blur', windowBlur)
  window.addEventListener('focus', windowFocus)
})

onUnmounted(() => {
  window.removeEventListener('blur', windowBlur)
  window.removeEventListener('focus', windowFocus)
})

watch(() => toasts.value.length, async () => {
  await nextTick()
  hovered.value = !!toasts.value.length && !!host.value?.matches(':hover')
  focused.value = !!host.value?.contains(document.activeElement)
})
</script>

<template>
  <ToastProvider :duration="Infinity">
    <NxToastItem
      v-for="toast in toasts"
      :key="toast.id"
      :toast="toast"
      :paused="paused"
      @dismiss="dismiss(toast.id)"
    />
    <div
      ref="host"
      @pointerenter="hovered = true"
      @pointerleave="hovered = false"
      @focusin="focused = true"
      @focusout="focusOut"
    >
      <ToastViewport class="fixed right-4 bottom-4 z-[60] flex max-h-[calc(100dvh-2rem)] w-[calc(100%-2rem)] max-w-sm list-none flex-col gap-3 overflow-y-auto" />
    </div>
  </ToastProvider>
</template>
