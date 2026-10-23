<script setup lang="ts">
definePageMeta({ middleware: 'auth' })

const route = useRoute()
const domain = computed(() => String(route.params.name))
</script>

<template>
  <DomainSubscriptionsPanel :domain="domain" variant="page" />
</template>
