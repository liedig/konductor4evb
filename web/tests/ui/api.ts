import type { Page, Route } from '@playwright/test'

/**
 * In-test route mock for `/api/{env}/**` (the dev server's same-origin API path).
 * Every call the console makes is fulfilled here with synthetic data — no
 * deployed stack, no real credentials, deterministic state per test.
 *
 * Route paths mirror the OpenAPI surfaces exactly (useMetadata/useCatalog):
 * POST /metadata/preview, POST /metadata/readiness, POST /metadata/activations.
 */

export interface MockEnvironment {
  environment: string
  activeRevision: number
  generation: number
  activatedBy: string
  activatedAt: string
  legacy: boolean
}

export interface MockState {
  head: number
  conflictNextSave: boolean
  activationDelayMs: number
  failEnvironments: boolean
  /** Simulates an API stage that predates the contract route: API Gateway
   * answers 403 with a non-ErrorResponse body before the Lambda runs. */
  failContract: boolean
  environments: MockEnvironment[]
  audits: Record<string, unknown>[]
  activationMode: 'ok' | 'stale' | 'backward'
  /** In-flight or recorded promotions, keyed `${targetEnv}::${eventName}`. */
  promotions: Record<string, MockPromotion>
  /** auto: each status poll advances the phase until COMPLETED; hold: keep
   * the seeded phase; conflict: POST always 409s; fail-*: the next polls
   * land on FAILED with the matching reason. */
  promotionMode: 'auto' | 'hold' | 'conflict' | 'fail-diverged' | 'fail-replay' | 'fail-skipped'
  promotionTicks: Record<string, number>
}

const REVISION_DEFINITION = {
  children: [
    { kind: 'field', name: 'eventId', generator: 'uuid-v4' },
    { kind: 'object', name: 'geo', children: [{ kind: 'field', name: 'region', generator: 'aws-region' }] },
  ],
}

const METADATA_SCHEMA = {
  $schema: 'http://json-schema.org/draft-04/schema#',
  type: 'object',
  properties: {
    eventId: { type: 'string', pattern: '^[0-9a-f-]{36}$' },
    geo: { type: 'object', properties: { region: { enum: ['us-east-1', 'eu-west-1'] } } },
  },
  required: ['eventId', 'geo'],
}

export interface MockSchemaSummary {
  schemaName: string
  producerDomain: string
  source: string
  detailType: string
  latestVersions: Record<string, number>
  metadataRevisions: Record<string, number>
}

export const SCHEMAS: MockSchemaSummary[] = [
  { schemaName: 'acme-orders@OrderNotification', producerDomain: 'orders', source: 'acme-orders', detailType: 'OrderNotification', latestVersions: { prod: 3, dev: 2 }, metadataRevisions: { dev: 2 } },
  { schemaName: 'acme-billing@InvoiceIssued', producerDomain: 'billing', source: 'acme-billing', detailType: 'InvoiceIssued', latestVersions: { prod: 5 }, metadataRevisions: {} },
  { schemaName: 'acme-shipping@ParcelScanned', producerDomain: 'shipping', source: 'acme-shipping', detailType: 'ParcelScanned', latestVersions: { prod: 2 }, metadataRevisions: { prod: 4 } },
  { schemaName: 'acme-loyalty@MemberUpdated', producerDomain: 'loyalty', source: 'acme-loyalty', detailType: 'MemberUpdated', latestVersions: { prod: 1 }, metadataRevisions: {} },
]

/** Plain GET /domains returns the platform-wide set (mirrors the deployed
 * scoping: platform roles get the full listing). The My domains page must
 * not render this as an ownership list for platform roles. */
export const SUBSCRIPTIONS = [
  {
    subscriptionId: 'sub-billing-orders',
    consumerDomain: 'billing',
    producerDomain: 'orders',
    schemaName: 'acme-orders@OrderNotification',
    status: 'ACTIVE',
    environments: {
      dev: { status: 'ACTIVE', enabled: true, pinnedVersion: 2 },
      staging: { status: 'ACTIVE', enabled: true, pinnedVersion: 3 },
      prod: { status: 'ACTIVE', enabled: true, pinnedVersion: 3 },
    },
    createdAt: '2026-08-02T09:00:00.000Z',
  },
  {
    subscriptionId: 'sub-loyalty-orders',
    consumerDomain: 'loyalty',
    producerDomain: 'orders',
    schemaName: 'acme-orders@OrderNotification',
    status: 'ACTIVE',
    environments: {
      prod: { status: 'ACTIVE', enabled: false, pinnedVersion: 1 },
    },
    createdAt: '2026-08-03T09:00:00.000Z',
  },
]

export interface MockPromotion {
  promotionId: string
  schemaName: string
  sourceEnvironment: string
  targetEnvironment: string
  requestedVersion: number
  status: 'VALIDATING' | 'REPLAYING' | 'REPINNING' | 'COMPLETED' | 'FAILED' | 'CANCELLED'
  replayedVersions: number[]
  targetVersion?: number
  repin?: { matched: number, repinned: number, failed: number }
  reason?: 'DIVERGED' | 'REPLAY_FAILED' | 'SKIPPED_ENVIRONMENT'
  createdAt: string
  updatedAt: string
}

/** Configured environment order (mirrors the fixtures.ts config.json). */
const ENV_ORDER = ['dev', 'staging', 'prod']

export const DOMAINS = [
  { domainName: 'orders', status: 'ACTIVE', description: 'Order lifecycle events', ownerEmail: 'owner@example.com', ownerAccountId: '111122223333', createdAt: '2026-08-01T09:00:00.000Z' },
  { domainName: 'billing', status: 'ACTIVE', description: 'Invoicing and payments', ownerEmail: 'billing@example.com', ownerAccountId: '444455556666', createdAt: '2026-08-12T09:00:00.000Z' },
]

const BUSINESS_CONTENT = {
  $schema: 'http://json-schema.org/draft-04/schema#',
  type: 'object',
  properties: { orderId: { type: 'string' }, total: { type: 'number' } },
  required: ['orderId'],
}

function revisionView(revision: number, head: number) {
  return {
    revision,
    predecessor: revision - 1,
    definitionDigest: revision === head ? 'd'.repeat(64) : 'a'.repeat(64),
    compilerFormat: 1,
    createdBy: 'platform-admin@example.com',
    createdAt: '2026-09-21T09:00:00.000Z',
    definition: REVISION_DEFINITION,
    metadataSchema: METADATA_SCHEMA,
  }
}

function errorBody(code: string, message: string) {
  return { code, message }
}

export function freshState(): MockState {
  return {
    head: 2,
    conflictNextSave: false,
    activationDelayMs: 0,
    failEnvironments: false,
    failContract: false,
    environments: [
      { environment: 'dev', activeRevision: 2, generation: 3, activatedBy: 'platform-admin@example.com', activatedAt: '2026-09-20T10:05:00.000Z', legacy: false },
      { environment: 'prod', activeRevision: 0, generation: 0, activatedBy: '', activatedAt: '', legacy: true },
    ],
    audits: [],
    activationMode: 'ok',
    promotions: {},
    promotionMode: 'auto',
    promotionTicks: {},
  }
}

async function fulfillJson(route: Route, status: number, body: unknown) {
  await route.fulfill({
    status,
    contentType: 'application/json',
    body: JSON.stringify(body),
  })
}

/**
 * Installs the route mock and returns the mutable state the tests steer
 * (conflict injection, activation delays, error modes).
 */
export async function installApi(page: Page, state: MockState = freshState()): Promise<MockState> {
  await page.route('**/api/**', async (route) => {
    const request = route.request()
    const url = new URL(request.url())
    // Strip the /api prefix and the environment segment; handlers match the
    // bare OpenAPI paths.
    const stage = decodeURIComponent(url.pathname).match(/^\/api\/([^/]+)/)?.[1] ?? 'dev'
    const path = decodeURIComponent(url.pathname).replace(/^\/api\/[^/]+/, '')
    const query = url.searchParams

    if (request.method() === 'GET') {
      if (path === '/metadata/environments') {
        if (state.failEnvironments) {
          return fulfillJson(route, 500, errorBody('InternalError', 'metadata table unavailable'))
        }
        return fulfillJson(route, 200, {
          environments: state.environments.map(environment => ({
            environment: environment.environment,
            activeRevision: environment.activeRevision,
            generation: environment.generation,
            legacy: environment.legacy,
            activatedBy: environment.activatedBy || null,
            activatedAt: environment.activatedAt || null,
          })),
        })
      }
      if (path === '/metadata/revisions') {
        if (query.get('nextToken')) return fulfillJson(route, 200, { revisions: [] })
        const items = Array.from({ length: state.head }, (_, index) => revisionView(state.head - index, state.head))
        return fulfillJson(route, 200, { revisions: items })
      }
      if (path === '/metadata/activations') {
        const page_ = query.get('nextToken') ? state.audits.slice(2) : state.audits.slice(0, 2)
        return fulfillJson(route, 200, { activations: page_ })
      }
      if (path === '/domains') {
        // The directory projection stays empty: DomainAccessPanel tests predate
        // this fixture and assert against an empty picker.
        if (query.get('view') === 'directory') {
          return fulfillJson(route, 200, {})
        }
        return fulfillJson(route, 200, { domains: DOMAINS })
      }
      if (path === '/schemas') {
        if (query.get('nextToken')) {
          return fulfillJson(route, 200, { items: SCHEMAS.slice(2) })
        }
        return fulfillJson(route, 200, { items: SCHEMAS.slice(0, 2), nextToken: 'page-2' })
      }
      if (path === '/subscriptions') {
        const schema = query.get('schema')
        const items = schema ? SUBSCRIPTIONS.filter(item => item.schemaName === schema) : SUBSCRIPTIONS
        return fulfillJson(route, 200, { items })
      }
      if (path === '/schemas/promotions') {
        const eventName = query.get('eventName') ?? ''
        const promotion = state.promotions[promotionKey(stage, eventName)]
        if (!promotion) return fulfillJson(route, 404, errorBody('NotFound', 'No promotion recorded for this schema in this environment'))
        return fulfillJson(route, 200, advancePromotion(state, stage, eventName))
      }
      if (path.endsWith('/contract')) {
        return contractRoute(route, path, query, state)
      }
      if (path.startsWith('/schemas/')) {
        const schema = SCHEMAS.find(item => item.schemaName === path.slice('/schemas/'.length))
        if (!schema) return fulfillJson(route, 404, errorBody('NotFound', 'Schema not found'))
        return fulfillJson(route, 200, {
          schemaName: schema.schemaName,
          source: schema.source,
          detailType: schema.detailType,
          latestVersions: schema.latestVersions,
          content: JSON.stringify(BUSINESS_CONTENT),
        })
      }
      return fulfillJson(route, 200, {})
    }

    const payload = (() => {
      try {
        return JSON.parse(request.postData() ?? '{}') as Record<string, unknown>
      } catch {
        return {}
      }
    })()

    if (request.method() === 'POST') {
      if (path === '/metadata/preview') {
        return fulfillJson(route, 200, {
          candidateRevision: state.head + 1,
          metadataSchema: METADATA_SCHEMA,
          bounds: { extensionNodesUsed: 3, extensionNodesMax: 64, objectLevelsMax: 2, definitionBytesUsed: 120, definitionBytesMax: 16384 },
          generators: [{ path: 'eventId', generator: 'uuid-v4', constraint: 'RFC 4122 v4', source: 'platform' }],
          example: { metadata: { eventId: 'synthetic-uuid', geo: { region: 'us-east-1' } }, data: { orderId: 'o-1' } },
        })
      }
      if (path === '/metadata/revisions') {
        if (state.conflictNextSave) {
          state.conflictNextSave = false
          state.head += 1
          return fulfillJson(route, 409, errorBody('RevisionConflict', 'Another administrator advanced the revision history first'))
        }
        state.head += 1
        return fulfillJson(route, 200, revisionView(state.head, state.head))
      }
      if (path === '/metadata/readiness') {
        return fulfillJson(route, 200, {
          revision: payload.revision,
          environment: payload.environment,
          definitionDigest: 'd'.repeat(64),
          compilerFormat: 1,
          checks: { definitionRecompilesExactly: true, generatedValuesSatisfyContract: true, exampleMetadataBytes: 96, sizeBudgetBytes: 1024 },
        })
      }
      if (path === '/metadata/activations') {
        const environment = state.environments.find(item => item.environment === payload.environment)
        if (!environment) return fulfillJson(route, 404, errorBody('NotFound', 'Unknown environment'))
        if (state.activationMode === 'stale' || (payload.expectedActiveRevision as number) !== environment.activeRevision) {
          return fulfillJson(route, 409, errorBody('ActivationConflict', 'The environment moved since this screen was opened'))
        }
        if (state.activationDelayMs > 0) {
          await new Promise(resolve => setTimeout(resolve, state.activationDelayMs))
        }
        const before = environment.activeRevision
        environment.activeRevision = payload.revision as number
        environment.generation += 1
        environment.legacy = false
        environment.activatedBy = 'platform-admin@example.com'
        environment.activatedAt = '2026-09-22T08:30:00.000Z'
        state.audits.unshift({
          operation: 'environment-activated',
          actor: 'platform-admin@example.com',
          beforeRevision: before,
          afterRevision: payload.revision,
          operationId: payload.operationId,
          occurredAt: '2026-09-22T08:30:00.000Z',
        })
        // Mirrors the API contract: a read-only no-op omits generation; a
        // committed activation reports the bumped value.
        const alreadyActive = before === payload.revision
        return fulfillJson(route, 200, {
          environment: environment.environment,
          activeRevision: environment.activeRevision,
          ...(alreadyActive ? {} : { generation: environment.generation }),
          activatedBy: environment.activatedBy,
          activatedAt: environment.activatedAt,
          alreadyActive,
        })
      }
      if (path === '/schemas/promotions') {
        const eventName = (payload.eventName as string) ?? ''
        const key = promotionKey(stage, eventName)
        const active = state.promotions[key]
        if (state.promotionMode === 'conflict' || (active && ['VALIDATING', 'REPLAYING', 'REPINNING'].includes(active.status))) {
          const id = active?.promotionId ?? 'promo-existing'
          return fulfillJson(route, 409, {
            code: 'PromotionInProgress',
            message: `a promotion is already active for this schema in this environment: ${id}`,
            promotionId: id,
          })
        }
        const sourceIndex = ENV_ORDER.indexOf(stage) - 1
        const source = ENV_ORDER[Math.max(0, sourceIndex)] ?? 'dev'
        const schema = SCHEMAS.find(item => item.detailType === eventName)
        const requested = schema?.latestVersions[source] ?? 1
        state.promotions[key] = {
          promotionId: `promo-${stage}-${eventName}`,
          schemaName: eventName,
          sourceEnvironment: source,
          targetEnvironment: stage,
          requestedVersion: requested,
          status: 'VALIDATING',
          replayedVersions: [],
          createdAt: '2026-10-01T09:00:00.000Z',
          updatedAt: '2026-10-01T09:00:00.000Z',
        }
        return fulfillJson(route, 202, state.promotions[key])
      }
      if (path === '/schemas/contract-preview') {
        const environment = state.environments.find(item => item.environment === payload.environment)
        const revision = environment?.legacy ? 0 : environment?.activeRevision ?? 0
        let business: Record<string, unknown>
        try {
          business = JSON.parse(payload.content as string)
        } catch {
          return fulfillJson(route, 400, errorBody('InvalidRequest', 'content is not valid JSON'))
        }
        const composed = revision === 0
          ? business
          : {
              ...business,
              properties: {
                ...(business.properties as object),
                metadata: { properties: { eventId: { type: 'string' } }, required: ['eventId'] },
              },
            }
        return fulfillJson(route, 200, {
          schemaName: payload.schemaName,
          environment: payload.environment,
          schemaVersion: 2,
          metadataRevision: revision,
          metadataSchema: revision === 0 ? { type: 'object' } : METADATA_SCHEMA,
          generators: revision === 0 ? [] : [{ name: 'eventId', generator: 'uuid-v4', source: 'platform', constraint: 'RFC 4122 v4' }],
          composerFormat: 2,
          composedDocument: JSON.stringify(composed),
          registered: false,
          note: 'Preview only — nothing was registered',
        })
      }
      return fulfillJson(route, 200, {})
    }

    return fulfillJson(route, 200, {})
  })
  return state
}

function promotionKey(stage: string, eventName: string) {
  return `${stage}::${eventName}`
}

/** Deterministic phase machine for promotion polls: auto advances one
 * phase per read, fail-* land on FAILED with the matching reason, hold
 * returns the seeded view untouched. */
function advancePromotion(state: MockState, stage: string, eventName: string): MockPromotion {
  const key = promotionKey(stage, eventName)
  const promotion = state.promotions[key]!
  if (state.promotionMode === 'hold' || state.promotionMode === 'conflict' || !['VALIDATING', 'REPLAYING', 'REPINNING'].includes(promotion.status)) {
    return promotion
  }
  const tick = (state.promotionTicks[key] ?? 0) + 1
  state.promotionTicks[key] = tick
  const schema = SCHEMAS.find(item => item.detailType === eventName)
  const from = schema?.latestVersions[stage] ?? 0
  promotion.updatedAt = '2026-10-01T09:00:0' + String(Math.min(tick, 9)) + '.000Z'
  if (state.promotionMode === 'fail-diverged') {
    promotion.status = 'FAILED'
    promotion.reason = 'DIVERGED'
    return promotion
  }
  if (state.promotionMode === 'fail-skipped') {
    promotion.status = 'FAILED'
    promotion.reason = 'SKIPPED_ENVIRONMENT'
    return promotion
  }
  if (state.promotionMode === 'fail-replay') {
    if (tick >= 2) {
      promotion.status = 'FAILED'
      promotion.reason = 'REPLAY_FAILED'
    } else {
      promotion.status = 'REPLAYING'
      promotion.replayedVersions = from + 1 <= promotion.requestedVersion ? [from + 1] : []
    }
    return promotion
  }
  if (promotion.status === 'VALIDATING') {
    promotion.status = 'REPLAYING'
    promotion.replayedVersions = from + 1 <= promotion.requestedVersion ? [from + 1] : []
    return promotion
  }
  if (promotion.status === 'REPLAYING') {
    const last = promotion.replayedVersions[promotion.replayedVersions.length - 1] ?? from
    if (last < promotion.requestedVersion) {
      promotion.replayedVersions = [...promotion.replayedVersions, last + 1]
      return promotion
    }
    promotion.status = 'REPINNING'
    return promotion
  }
  promotion.status = 'COMPLETED'
  promotion.targetVersion = promotion.requestedVersion
  promotion.repin = { matched: 2, repinned: 2, failed: 0 }
  return promotion
}

function contractRoute(
  route: Route,
  path: string,
  query: URLSearchParams,
  state: MockState,
) {
  if (state.failContract) {
    return fulfillJson(route, 403, { message: 'Missing Authentication Token' })
  }
  const schemaName = path.slice('/schemas/'.length, -'/contract'.length)
  const schema = SCHEMAS.find(item => item.schemaName === schemaName)
  const environmentName = query.get('environment')
  if (!schema || !query.get('domain') || !environmentName) {
    return fulfillJson(route, 400, errorBody('InvalidRequest', 'schemaName, domain and environment are required'))
  }
  const schemaVersion = query.get('schemaVersion')
  const metadataRevision = query.get('metadataRevision')
  if ((schemaVersion === null) !== (metadataRevision === null)) {
    return fulfillJson(route, 400, errorBody('InvalidRequest', 'Historical inspection requires both schemaVersion and metadataRevision'))
  }
  const environment = state.environments.find(item => item.environment === environmentName)
  const version = schemaVersion ? Number(schemaVersion) : schema.latestVersions[environmentName] ?? 1
  const revision = metadataRevision ? Number(metadataRevision) : environment?.activeRevision ?? 0
  if (schemaVersion && version !== 2) {
    return fulfillJson(route, 404, errorBody('NotFound', `No historical contract for v${version} M${revision} in ${environmentName}`))
  }
  return fulfillJson(route, 200, {
    schemaName: schema.schemaName,
    source: schema.source,
    detailType: schema.detailType,
    environment: environmentName,
    registry: `konductor-schemas-${environmentName}`,
    schemaVersion: version,
    metadataRevision: revision,
    generation: revision === 0 ? null : environment?.generation ?? null,
    definitionDigest: revision === 0 ? null : 'd'.repeat(64),
    businessContent: JSON.stringify(BUSINESS_CONTENT),
    metadataSchema: JSON.stringify(revision === 0 ? { type: 'object' } : METADATA_SCHEMA),
    generators: revision === 0 ? [] : [{ name: 'eventId', generator: 'uuid-v4', source: 'platform', constraint: 'RFC 4122 v4' }],
    composerFormat: 2,
    composedDocument: JSON.stringify(revision === 0
      ? BUSINESS_CONTENT
      : { ...BUSINESS_CONTENT, properties: { ...BUSINESS_CONTENT.properties, metadata: { properties: { eventId: { type: 'string' } }, required: ['eventId'] } } }),
  })
}

/**
 * Dev-only session seam (`konductorTestSession` in sessionStorage): mirrors a
 * Cognito ID-token payload so the harness can exercise every role without
 * real credentials. `platform-admin` and `platform-auditor` are the global
 * roles; any other group is a team (owner-persona) group.
 */
export async function signIn(page: Page, groups: string[], email = 'platform-admin@example.com') {
  await page.addInitScript(
    ({ groups, email }) => {
      sessionStorage.setItem('konductorTestSession', JSON.stringify({
        email,
        'cognito:groups': groups,
        'cognito:username': email.split('@')[0],
      }))
    },
    { groups, email },
  )
}
