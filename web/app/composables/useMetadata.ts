import type {
  ActivateMetadataRequest,
  CreateMetadataRevisionRequest,
  ListMetadataRevisionsResponse,
  MetadataActivationList,
  MetadataActivationResponse,
  MetadataEnvironmentList,
  MetadataPreviewRequest,
  MetadataPreviewResponse,
  MetadataReadinessReport,
  MetadataReadinessRequest,
  MetadataRevision,
} from '~/types/api'

/**
 * Thin typed client for the platform metadata administration routes
 * (OAuth surface; the API remains the enforcing authorization boundary).
 * Reads serve platform roles; mutations require `platform-admin`.
 */
export function useMetadata() {
  const api = useApi()

  return {
    listRevisions: (nextToken?: string) =>
      api.get<ListMetadataRevisionsResponse>('/metadata/revisions', { nextToken }),
    getRevision: (revision: number) =>
      api.get<MetadataRevision>(`/metadata/revisions/${revision}`),
    createRevision: (body: CreateMetadataRevisionRequest) =>
      api.post<MetadataRevision>('/metadata/revisions', body),
    preview: (body: MetadataPreviewRequest) =>
      api.post<MetadataPreviewResponse>('/metadata/preview', body),
    listEnvironments: () => api.get<MetadataEnvironmentList>('/metadata/environments'),
    readiness: (body: MetadataReadinessRequest) =>
      api.post<MetadataReadinessReport>('/metadata/readiness', body),
    listActivations: (nextToken?: string) =>
      api.get<MetadataActivationList>('/metadata/activations', { nextToken }),
    activate: (body: ActivateMetadataRequest) =>
      api.post<MetadataActivationResponse>('/metadata/activations', body),
  }
}
