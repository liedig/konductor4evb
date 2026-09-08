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
