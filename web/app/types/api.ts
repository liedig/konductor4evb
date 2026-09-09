/**
 * Typed models mirroring `api/openapi-oauth.yaml` component schemas.
 * Hand-written by design (design.md, decision 5); keep aligned with the spec.
 */

export type DomainStatus =
  | 'PENDING_APPROVAL'
  | 'APPROVED'
  | 'REJECTED'
  | 'APPROVAL_NOTIFICATION_FAILED'
  | 'PROVISIONING'
  | 'ACTIVE'
  | 'PROVISIONING_FAILED'
  | 'INACTIVE'

export interface DomainSummary {
  domainName: string
  domainNamespace?: string
  description?: string
  ownerAccountId?: string
  ownerEmail?: string
  status?: DomainStatus
  environments?: string[]
  secondaryOwnerEmails?: string[]
  createdAt?: string
}

/** Names-only directory projection (`GET /domains?view=directory`). */
export type DirectoryEntry = Pick<
  DomainSummary,
  'domainName' | 'domainNamespace' | 'description' | 'status'
>

export interface PerEnvResourceArns {
  platformEventBusArn?: string
  appEventBusArn?: string
  schemaRegistryArn?: string
  subscriptionDlqArn?: string
  integrationDlqArn?: string
  dashboardArn?: string
}

export interface DomainDetail {
  domainName: string
  description?: string
  ownerAccountId?: string
  ownerEmail?: string
  secondaryOwnerEmails?: string[]
  status?: DomainStatus
  environments?: string[]
  namespace?: string
  costCenter?: string
  tags?: Record<string, string>
  resourceArns?: Record<string, PerEnvResourceArns>
  namespaceParameter?: string
  createdAt?: string
  updatedAt?: string
}

export interface ListDomainsResponse {
  domains?: DomainSummary[]
  nextToken?: string
}

export interface PendingRegistration {
  domainName?: string
  description?: string
  ownerAccountId?: string
  ownerEmail?: string
  secondaryOwnerEmails?: string[]
  owningTeam?: string
  createdAt?: string
  taskToken?: string
}

export interface PendingRegistrationsResponse {
  registrations?: PendingRegistration[]
  nextToken?: string
}

export interface CommandAccepted {
  requestId?: string
  message?: string
}

export interface RegisterDomainRequest {
  domainName: string
  description?: string
  ownerAccountId: string
  ownerEmail: string
  secondaryOwnerEmails: string[]
  costCenter?: string
  owningTeam: string
}

export interface UpdateDomainRequest {
  description?: string
  ownerEmail?: string
  secondaryOwnerEmails?: string[]
  costCenter?: string
}

export type DecideDecision = 'ACCEPT' | 'REJECT'

export interface DecideRequest {
  taskToken: string
  decision: DecideDecision
  justification?: string
}

export type DomainStateAction = 'enable' | 'disable' | 'retry-provisioning'

export interface DomainStateResponse {
  domainName?: string
  status?: 'ACTIVE' | 'INACTIVE'
  updatedAt?: string
}

export interface SchemaSummary {
  schemaName?: string
  producerDomain?: string
  source?: string
  detailType?: string
  latestVersions?: Record<string, number>
  /** Active metadata revision per environment, where the pointer was readable. */
  metadataRevisions?: Record<string, number>
}

export interface SchemaDetail extends SchemaSummary {
  content?: string
}

export interface ListSchemasResponse {
  items?: SchemaSummary[]
  nextToken?: string
}

export interface CreateSchemaRequest {
  domain: string
  eventName: string
  schemaType: 'JSONSchemaDraft4'
  content: string
}

export interface AppendSchemaVersionRequest {
  domain: string
  eventName: string
  content: string
}

export interface SchemaMutationResponse {
  schemaName?: string
  schemaVersion?: number
}

export type SubscriptionStatus = 'PENDING' | 'ACTIVE' | 'AWAITING_PROMOTION' | 'FAILED' | 'REMOVED'

export interface EnvStreamState {
  pinnedVersion?: number
  ruleArn?: string
  enabled?: boolean
  status?: SubscriptionStatus
  errorDetail?: string
}

export interface SubscriptionSummary {
  subscriptionId?: string
  consumerDomain?: string
  producerDomain?: string
  schemaName?: string
  status?: SubscriptionStatus
  errorDetail?: string
  environments?: Record<string, EnvStreamState>
  createdAt?: string
}

export interface SubscriptionDetail extends SubscriptionSummary {
  source?: string
  detailType?: string
  updatedAt?: string
  removedAt?: string
}

export interface ListSubscriptionsResponse {
  items?: SubscriptionSummary[]
}

export interface SubscribeRequest {
  consumerDomain: string
  producerDomain: string
  schemaName: string
  environments?: Record<string, { enabled: boolean }>
}

export interface UpdateSubscriptionStateRequest {
  subscriptionId: string
  consumerDomain: string
  environments: Record<string, { enabled: boolean }>
}

export interface Entitlement {
  entitlementId?: string
  consumerDomain?: string
  producerDomain?: string
  scopePattern?: string
  createdAt?: string
  createdBy?: string
}

export interface ListEntitlementsResponse {
  entitlements?: Entitlement[]
}

export interface CreateEntitlementRequest {
  consumerDomain: string
  producerDomain: string
  scopePattern: string
}

/** API error body (`ErrorResponse` in the OpenAPI spec). */
export interface ApiErrorBody {
  code: string
  message: string
  requestId?: string
  reason?: 'unknownOperation' | 'cannotParse' | 'fieldValidationFailed' | 'other'
  fieldList?: { name?: string, message?: string }[]
}

// ---- Platform metadata administration (`/metadata/*`, OAuth surface) ----

/** The supported generator catalogue (design.md, decision 2). */
export type MetadataGenerator =
  | 'uuid-v4'
  | 'utc-timestamp'
  | 'utc-date'
  | 'aws-region'
  | 'organisation'
  | 'environment'

/** A generated leaf field in a metadata definition tree. */
export interface MetadataFieldNode {
  kind: 'field'
  name: string
  generator: MetadataGenerator
}

/** A nested object holding further definition nodes. */
export interface MetadataObjectNode {
  kind: 'object'
  name: string
  children: MetadataNode[]
}

export type MetadataNode = MetadataFieldNode | MetadataObjectNode

/** Extension fields/objects only; built-ins are implicit in every revision. */
export interface MetadataDefinition {
  children?: MetadataNode[]
}

export interface CreateMetadataRevisionRequest {
  expectedHead: number
  operationId: string
  definition: MetadataDefinition
}

/** Immutable revision summary/detail (`RevisionView`, camelCase on the wire). */
export interface MetadataRevision {
  revision: number
  predecessor: number
  definitionDigest: string
  compilerFormat: number
  createdBy: string
  createdAt: string
  definition?: MetadataDefinition
  metadataSchema?: Record<string, unknown>
}

export interface ListMetadataRevisionsResponse {
  revisions?: MetadataRevision[]
  nextToken?: string
}

export interface MetadataPreviewRequest {
  definition: MetadataDefinition
  environment?: string
}

export interface MetadataGeneratorDescription {
  path?: string
  generator?: string
  constraint?: string
  source?: string
}

/** Preview bounds mirror the server-side additive limits. */
export interface MetadataPreviewBounds {
  extensionNodesUsed?: number
  extensionNodesMax?: number
  objectLevelsMax?: number
  definitionBytesUsed?: number
  definitionBytesMax?: number
}

export interface MetadataPreviewResponse {
  candidateRevision?: number
  metadataSchema?: Record<string, unknown>
  bounds?: MetadataPreviewBounds
  generators?: MetadataGeneratorDescription[]
  example?: Record<string, unknown>
}

export interface MetadataReadinessRequest {
  revision: number
  environment: string
}

export interface MetadataReadinessReport {
  revision?: number
  environment?: string
  definitionDigest?: string
  compilerFormat?: number
  checks?: {
    definitionRecompilesExactly?: boolean
    generatedValuesSatisfyContract?: boolean
    exampleMetadataBytes?: number
    sizeBudgetBytes?: number
  }
}

/** Per-environment active pointer state; `activeRevision` 0 is the legacy baseline. */
export interface MetadataEnvironmentState {
  environment: string
  activeRevision: number
  generation?: number
  legacy?: boolean
  definitionDigest?: string
  activatedBy?: string
  activatedAt?: string
}

export interface MetadataEnvironmentList {
  environments?: MetadataEnvironmentState[]
}

export interface ActivateMetadataRequest {
  revision: number
  environment: string
  expectedActiveRevision: number
  operationId: string
}

export interface MetadataActivationResponse {
  environment: string
  activeRevision: number
  generation?: number
  activatedBy: string
  activatedAt: string
  definitionDigest?: string
  alreadyActive?: boolean
}

export interface MetadataActivationAudit {
  operation?: string
  actor?: string
  beforeRevision?: number
  afterRevision?: number
  operationId?: string
  occurredAt?: string
}

export interface MetadataActivationList {
  activations?: MetadataActivationAudit[]
  nextToken?: string
}
