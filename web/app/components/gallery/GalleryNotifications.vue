<script setup lang="ts">
const toast = useToast()
</script>

<template>
  <NxCard title="Toast">
    <p class="mb-4 text-sm text-muted">Notifications last six seconds. Hover or focus to pause. Press F8 to reach the notification region.</p>
    <div class="flex flex-wrap gap-3">
      <NxButton variant="secondary" @click="toast.success('Registration request accepted')">Success</NxButton>
      <NxButton variant="secondary" @click="toast.info('Subscription is enabling')">Information</NxButton>
      <NxButton variant="secondary" @click="toast.error('Access denied: not an owner of this domain')">Error</NxButton>
      <NxButton variant="ghost" @click="toast.info('This longer notification demonstrates readable wrapping on a narrow screen, with room to reach its dismiss control using the keyboard.')">Long notification</NxButton>
    </div>
  </NxCard>
</template>
