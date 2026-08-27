import type {
  CommandAccepted,
  DirectoryEntry,
  DomainDetail,
  DomainStateAction,
  DomainStateResponse,
  ListDomainsResponse,
  RegisterDomainRequest,
  UpdateDomainRequest,
} from '~/types/api'

/** Domain API endpoints (OAuth surface). */
export function useDomains() {
  const api = useApi()

  /** Server scopes visibility by caller (platform roles see all). */
  async function list(nextToken?: string): Promise<ListDomainsResponse> {
    return api.get<ListDomainsResponse>('/domains', { nextToken })
  }

  /** Names-only projection for pickers (any authenticated caller). */
  async function directory(): Promise<DirectoryEntry[]> {
    const response = await api.get<ListDomainsResponse>('/domains', { view: 'directory' })
    return (response.domains ?? []) as DirectoryEntry[]
  }

  return {
    list,
    directory,
    describe: (name: string) => api.get<DomainDetail>(`/domains/${encodeURIComponent(name)}`),
    register: (body: RegisterDomainRequest) => api.post<CommandAccepted>('/domains', body),
    update: (name: string, body: UpdateDomainRequest) =>
      api.put<CommandAccepted>(`/domains/${encodeURIComponent(name)}`, body),
    setState: (name: string, action: DomainStateAction) =>
      api.post<DomainStateResponse>(`/domains/${encodeURIComponent(name)}/state`, { action }),
  }
}
