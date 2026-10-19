declare module '#app' {
  interface NuxtApp {
    $konductorConfig: KonductorConfig
  }
}

/** Runtime configuration served from `/config.json`, written by `make deploy-web`. */
export interface KonductorConfig {
  apiBaseUrl: string
  userPoolId: string
  userPoolClientId: string
  region: string
  /** Platform environments a domain provisions into; drives the request form's per-environment account override fields. Empty when the serving stack predates the KonductorEnvironments output. */
  environments: string[]
}

export function parseKonductorConfig(raw: unknown): KonductorConfig {
  if (typeof raw !== 'object' || raw === null) {
    throw new Error('config.json is not an object')
  }
  const candidate = raw as Record<string, unknown>
  for (const key of ['apiBaseUrl', 'userPoolId', 'userPoolClientId', 'region'] as const) {
    if (typeof candidate[key] !== 'string' || candidate[key] === '') {
      throw new Error(`config.json is missing a non-empty "${key}"`)
    }
  }
  const environments = Array.isArray(candidate.environments)
    ? candidate.environments.filter((e): e is string => typeof e === 'string' && e !== '')
    : []
  const apiBaseUrl = (candidate.apiBaseUrl as string).replace(/\/+$/, '')
  const lastSegment = apiBaseUrl.split('/').pop() ?? ''
  if (environments.includes(lastSegment)) {
    throw new Error(
      `config.json "apiBaseUrl" must be the API root (e.g. "/api"); it ends with the "${lastSegment}" stage segment`,
    )
  }
  return {
    apiBaseUrl,
    userPoolId: candidate.userPoolId as string,
    userPoolClientId: candidate.userPoolClientId as string,
    region: candidate.region as string,
    environments,
  }
}
