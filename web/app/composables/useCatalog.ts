import type {
  AppendSchemaVersionRequest,
  CommandAccepted,
  ComposedContract,
  ContractPreviewRequest,
  ContractPreviewResponse,
  CreateSchemaRequest,
  ListSchemasResponse,
  ListSubscriptionsResponse,
  PromotionView,
  RequestPromotionRequest,
  SchemaDetail,
  SchemaEnvironmentContent,
  SchemaMutationResponse,
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
    /** Registry content of one schema version in one environment (producer
     * view); `version` defaults to the environment's latest. */
    getSchemaContent: (
      schemaName: string,
      options: { domain: string, environment: string, version?: number },
    ) =>
      api.get<SchemaEnvironmentContent>(`/schemas/${encodeURIComponent(schemaName)}/content`, {
        domain: options.domain,
        environment: options.environment,
        version: options.version,
      }),
    /** Current or historical composed contract for one exact
     * (business version, metadata revision) pair. Historical reads require
     * both `schemaVersion` and `metadataRevision` — there is no
     * current-metadata fallback for a historical business version. */
    getContract: (
      schemaName: string,
      options: { domain: string, environment: string, consumerDomain?: string, schemaVersion?: number, metadataRevision?: number },
    ) =>
      api.get<ComposedContract>(`/schemas/${encodeURIComponent(schemaName)}/contract`, {
        domain: options.domain,
        environment: options.environment,
        consumerDomain: options.consumerDomain,
        schemaVersion: options.schemaVersion,
        metadataRevision: options.metadataRevision,
      }),
    /** Publisher-authorized effective-contract preview of unsaved business
     * content against the environment's active metadata. Registers nothing. */
    previewContract: (body: ContractPreviewRequest) =>
      api.post<ContractPreviewResponse>('/schemas/contract-preview', body),
    createSchema: (body: CreateSchemaRequest) =>
      api.post<SchemaMutationResponse>('/schemas', body),
    appendSchemaVersion: (body: AppendSchemaVersionRequest) =>
      api.post<SchemaMutationResponse>('/schemas/versions', body),
    /** Start a replay-driven promotion against the target environment's
     * stage (the stage selects the target; the source is its configured
     * predecessor). 202 carries the in-flight view, 409 an already-running
     * promotion's id in the error body. */
    requestPromotion: (options: RequestPromotionRequest & { environment: string }) =>
      useApi(options.environment).post<PromotionView>('/schemas/promotions', {
        domain: options.domain,
        eventName: options.eventName,
      }),
    /** Latest promotion for one (schema, target environment), read through
     * the target stage; 404 when the schema was never promoted there. */
    getPromotion: (options: { domain: string, eventName: string, environment: string }) =>
      useApi(options.environment).get<PromotionView>('/schemas/promotions', {
        domain: options.domain,
        eventName: options.eventName,
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
