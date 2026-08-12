import tailwindcss from '@tailwindcss/vite'

// nuxt.config.ts executes in Node; the app's tsconfig has no node types.
declare const process: { env: Record<string, string | undefined> }

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
  nitro: {
    // Local dev: browser calls stay same-origin (`/dev/...`) and are proxied
    // to the deployed OAuth API (KONDUKTOR_DEV_API_URL, set by `make web-dev`).
    // Production serves the API same-origin through CloudFront, so no CORS
    // surface exists anywhere.
    devProxy: process.env.KONDUKTOR_DEV_API_URL
      ? {
          '/dev': {
            target: `${process.env.KONDUKTOR_DEV_API_URL}/`,
            changeOrigin: true,
          },
        }
      : {},
  },
})
