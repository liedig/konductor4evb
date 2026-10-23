<script setup lang="ts">
definePageMeta({ middleware: 'auth' })

const route = useRoute()
const domain = computed(() => String(route.params.name))
</script>

<template>
  <DomainAccessPanel :domain="domain" variant="page" />
</template>
