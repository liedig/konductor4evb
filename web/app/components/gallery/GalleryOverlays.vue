<script setup lang="ts">
const modalOpen = ref(false)
const confirmOpen = ref(false)
const longOpen = ref(false)
const fallbackOpen = ref(false)
const showOpener = ref(true)
const value = ref('')
const busy = ref(false)
const attempts = ref(0)
const failed = ref(false)
let pending: ReturnType<typeof setTimeout> | undefined

function confirm() {
  if (busy.value) return
  busy.value = true
  failed.value = false
  attempts.value++
  pending = setTimeout(() => {
    busy.value = false
    failed.value = true
  }, 1500)
}

onUnmounted(() => clearTimeout(pending))
</script>

<template>
  <NxCard title="Modal and confirm">
    <p class="mb-4 text-sm text-muted">Focus stays inside dialogs and returns to the opener. Confirmations cannot be dismissed while busy.</p>
    <div class="flex flex-wrap gap-3">
      <NxButton variant="secondary" @click="modalOpen = true">Open modal</NxButton>
      <NxButton variant="danger" @click="confirmOpen = true">Open confirm</NxButton>
      <NxButton variant="secondary" @click="longOpen = true">Long modal</NxButton>
      <NxButton v-if="showOpener" variant="ghost" @click="fallbackOpen = true">Removed opener demo</NxButton>
    </div>
    <p class="mt-3 text-xs text-faint">Demo attempts: {{ attempts }}. No service calls are made.</p>
    <NxModal :open="modalOpen" title="Modal" @close="modalOpen = false">
      <NxInput v-model="value" label="Demo field" hint="Tab through the dialog, then press Escape." />
      <template #actions>
        <NxButton variant="secondary" @click="modalOpen = false">Done</NxButton>
      </template>
    </NxModal>
    <NxModal :open="longOpen" title="Long modal" @close="longOpen = false">
      <p v-for="index in 20" :key="index" class="mb-4 text-sm text-muted">Section {{ index }}. This long example demonstrates scrolling inside a dialog without moving the background page.</p>
      <template #actions>
        <NxButton @click="longOpen = false">Finish reading</NxButton>
      </template>
    </NxModal>
    <NxModal :open="fallbackOpen" title="Removed opener" @close="fallbackOpen = false">
      <NxButton variant="secondary" @click="showOpener = false">Remove opening control</NxButton>
    </NxModal>
    <NxConfirm
      :open="confirmOpen"
      title="Disable domain"
      confirm-label="Disable"
      :busy="busy"
      danger
      @confirm="confirm"
      @cancel="confirmOpen = false"
    >
      <p>Disabling rejects new publish and subscription activity. Existing subscriptions are retained.</p>
      <p v-if="failed" role="status" class="mt-3 text-danger">Demo failed. Try again or cancel.</p>
    </NxConfirm>
  </NxCard>
</template>
