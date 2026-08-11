declare module '#app' {
  interface NuxtApp {
    $nexusConfig: NexusConfig
  }
}

/** Runtime configuration served from `/config.json`, written by `make deploy-web`. */
export interface NexusConfig {
  apiBaseUrl: string
  userPoolId: string
  userPoolClientId: string
  region: string
}

export function parseNexusConfig(raw: unknown): NexusConfig {
  if (typeof raw !== 'object' || raw === null) {
    throw new Error('config.json is not an object')
  }
  const candidate = raw as Record<string, unknown>
  for (const key of ['apiBaseUrl', 'userPoolId', 'userPoolClientId', 'region'] as const) {
    if (typeof candidate[key] !== 'string' || candidate[key] === '') {
      throw new Error(`config.json is missing a non-empty "${key}"`)
    }
  }
  return {
    apiBaseUrl: candidate.apiBaseUrl as string,
    userPoolId: candidate.userPoolId as string,
    userPoolClientId: candidate.userPoolClientId as string,
    region: candidate.region as string,
  }
}
