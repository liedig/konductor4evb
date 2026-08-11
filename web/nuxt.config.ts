import tailwindcss from '@tailwindcss/vite'

// The console is an auth-gated SPA: no SSR, statically generated and served
// from S3 + CloudFront (see design.md, decision 1).
export default defineNuxtConfig({
  compatibilityDate: '2026-08-01',
  ssr: false,
  devtools: { enabled: false },
  modules: ['@pinia/nuxt', '@nuxt/eslint'],
  css: ['~/assets/css/main.css'],
  vite: {
    plugins: [tailwindcss()],
  },
  app: {
    head: {
      title: 'EventBridge Nexus',
      meta: [
        { name: 'description', content: 'EventBridge Nexus control-plane console' },
      ],
    },
  },
  typescript: {
    typeCheck: false,
  },
})
