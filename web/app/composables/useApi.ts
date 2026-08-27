import { FetchError } from 'ofetch'

import { useSessionStore } from '~/stores/session'
import type { ApiErrorBody } from '~/types/api'

/** Typed API error carrying the API's `ErrorResponse` body when present. */
export class ApiError extends Error {
  constructor(
    public readonly status: number,
    public readonly body: ApiErrorBody | null,
  ) {
    super(body?.message ?? `Request failed with status ${status}`)
    this.name = 'ApiError'
  }

  get code(): string | undefined {
    return this.body?.code
  }

  get fieldList(): { name?: string, message?: string }[] {
    return this.body?.fieldList ?? []
  }
}

type Query = Record<string, string | number | boolean | undefined>

interface RequestOptions {
  query?: Query
  body?: unknown
}

function isErrorBody(data: unknown): data is ApiErrorBody {
  return (
    typeof data === 'object'
    && data !== null
    && typeof (data as ApiErrorBody).code === 'string'
    && typeof (data as ApiErrorBody).message === 'string'
  )
}

/**
 * One typed API client layer (design.md, decision 5). Attaches the session
 * access token; on 401 refreshes once and retries once, then clears the
 * session and hands off to the auth guard's `/login` redirect.
 */
export function useApi() {
  const config = useKonductorConfig()
  const session = useSessionStore()

  async function request<T>(method: string, path: string, options: RequestOptions = {}, retried = false): Promise<T> {
    const token = await session.accessToken(retried)
    try {
      return await $fetch<T>(path, {
        baseURL: config.apiBaseUrl,
        method: method as 'GET',
        query: options.query,
        body: options.body as Record<string, unknown> | undefined,
        headers: { Authorization: `Bearer ${token}` },
        // A request that never settles must not hold a page's busy state hostage.
        signal: AbortSignal.timeout(30_000),
      })
    } catch (error) {
      if (error instanceof DOMException && error.name === 'TimeoutError') {
        throw new ApiError(0, { code: 'RequestTimeout', message: 'The request timed out. Check your connection and try again.' })
      }
      const status = error instanceof FetchError ? (error.status ?? 0) : 0
      if (status === 401 && !retried) {
        return request<T>(method, path, options, true)
      }
      if (status === 401) {
        await session.logout()
        await navigateTo('/login')
      }
      const data = error instanceof FetchError ? error.data : null
      throw new ApiError(status, isErrorBody(data) ? data : null)
    }
  }

  return {
    get: <T>(path: string, query?: Query) => request<T>('GET', path, { query }),
    post: <T>(path: string, body?: unknown, query?: Query) => request<T>('POST', path, { body, query }),
    put: <T>(path: string, body?: unknown) => request<T>('PUT', path, { body }),
    del: <T>(path: string) => request<T>('DELETE', path),
  }
}
