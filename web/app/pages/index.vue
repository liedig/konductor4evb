<script setup lang="ts">
// Persona-aware landing arrives with the auth work (section 3); until then
// the root route sends visitors to the login page.
await navigateTo('/login')
</script>

<template>
  <div />
</template>
