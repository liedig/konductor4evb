import type { Page, Route } from '@playwright/test'

/**
 * In-test route mock for `/dev/**` (the dev server's same-origin API path).
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
  environments: MockEnvironment[]
  audits: Record<string, unknown>[]
  activationMode: 'ok' | 'stale' | 'backward'
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
  { schemaName: 'acme-shipping@ParcelScanned', producerDomain: 'shipping', source: 'acme-shipping', detailType: 'ParcelScanned', latestVersions: { prod: 2 }, metadataRevisions: {} },
  { schemaName: 'acme-loyalty@MemberUpdated', producerDomain: 'loyalty', source: 'acme-loyalty', detailType: 'MemberUpdated', latestVersions: { prod: 1 }, metadataRevisions: {} },
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
    environments: [
      { environment: 'dev', activeRevision: 2, generation: 3, activatedBy: 'platform-admin@example.com', activatedAt: '2026-09-20T10:05:00.000Z', legacy: false },
      { environment: 'prod', activeRevision: 0, generation: 0, activatedBy: '', activatedAt: '', legacy: true },
    ],
    audits: [],
    activationMode: 'ok',
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
  await page.route('**/dev/**', async (route) => {
    const request = route.request()
    const url = new URL(request.url())
    const path = decodeURIComponent(url.pathname).replace(/^\/dev/, '')
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
      if (path === '/schemas') {
        if (query.get('nextToken')) {
          return fulfillJson(route, 200, { items: SCHEMAS.slice(2) })
        }
        return fulfillJson(route, 200, { items: SCHEMAS.slice(0, 2), nextToken: 'page-2' })
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
        return fulfillJson(route, 200, {
          environment: environment.environment,
          activeRevision: environment.activeRevision,
          generation: environment.generation,
          activatedBy: environment.activatedBy,
          activatedAt: environment.activatedAt,
          alreadyActive: before === payload.revision,
        })
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

function contractRoute(
  route: Route,
  path: string,
  query: URLSearchParams,
  state: MockState,
) {
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
