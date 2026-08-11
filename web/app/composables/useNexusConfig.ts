import type { NexusConfig } from '~/types/config'

export function useNexusConfig(): NexusConfig {
  const config = useNuxtApp().$nexusConfig
  if (!config) {
    throw new Error('Nexus runtime config is not loaded yet')
  }
  return config
}
