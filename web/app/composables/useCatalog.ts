import type {
  CommandAccepted,
  ListSchemasResponse,
  ListSubscriptionsResponse,
  SchemaDetail,
  SubscribeRequest,
  SubscriptionDetail,
  UpdateSubscriptionStateRequest,
} from '~/types/api'

/** Catalog (schema discovery) and subscription endpoints (OAuth surface). */
export function useCatalog() {
  const api = useApi()

  return {
    /** Entitlement-scoped consumer catalog, or a producer's own catalog when
     * `producerDomain` is given without a consumer domain. */
    listSchemas: (options: { consumerDomain?: string, producerDomain?: string, environment?: string, nextToken?: string } = {}) =>
      api.get<ListSchemasResponse>('/schemas', {
        consumerDomain: options.consumerDomain,
        domain: options.producerDomain,
        environment: options.environment,
        nextToken: options.nextToken,
      }),
    describeSchema: (schemaName: string, options: { consumerDomain?: string, producerDomain?: string } = {}) =>
      api.get<SchemaDetail>(`/schemas/${encodeURIComponent(schemaName)}`, {
        consumerDomain: options.consumerDomain,
        domain: options.producerDomain,
      }),
    subscribe: (body: SubscribeRequest) => api.post<CommandAccepted>('/subscriptions', body),
    listSubscriptions: (options: { domain: string, direction: 'inbound' | 'outbound', schema?: string, status?: string }) =>
      api.get<ListSubscriptionsResponse>('/subscriptions', {
        domain: options.domain,
        direction: options.direction,
        schema: options.schema,
        status: options.status,
      }),
    describeSubscription: (subscriptionId: string) =>
      api.get<SubscriptionDetail>(`/subscriptions/${encodeURIComponent(subscriptionId)}`),
    unsubscribe: (subscriptionId: string) =>
      api.del<CommandAccepted>(`/subscriptions/${encodeURIComponent(subscriptionId)}`),
    updateSubscriptionState: (body: UpdateSubscriptionStateRequest) =>
      api.post<CommandAccepted>('/subscriptions/state', body),
  }
}
