import type { KonductorConfig } from '~/types/config'

export function useKonductorConfig(): KonductorConfig {
  const config = useNuxtApp().$konductorConfig
  if (!config) {
    throw new Error('Nexus runtime config is not loaded yet')
  }
  return config
}
