import type {
  DecideRequest,
  PendingRegistrationsResponse,
} from '~/types/api'

/** Pending-registrations inbox + decide (platform-admin only, OAuth surface). */
export function useApprovals() {
  const api = useApi()

  return {
    list: (nextToken?: string) =>
      api.get<PendingRegistrationsResponse>('/domains/registrations/pending', { nextToken }),
    decide: (body: DecideRequest) =>
      api.post<{ message?: string }>('/domains/registrations/decide', body),
  }
}
