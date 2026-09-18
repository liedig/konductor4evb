const ARN_PATTERN = /^arn:aws:([^:]*):([^:]*):(\d{12}):(.+)$/
const SQS_URL_PATTERN = /^https:\/\/sqs\.([a-z0-9-]+)\.amazonaws\.com\/(\d{12})\/[^/]+$/

/** Fields managed by the platform account; never receive console links. */
export const PLATFORM_RESOURCE_FIELDS = new Set([
  'platformEventBusArn',
  'schemaRegistryArn',
  'subscriptionDlqArn',
  'dashboardArn',
])

/** Fields that live in the domain account; link-eligible when the account matches. */
export const DOMAIN_RESOURCE_FIELDS = new Set([
  'appEventBusArn',
  'integrationDlqArn',
])

/** Extracts the 12-digit account ID from an ARN or SQS queue URL. */
export function accountFromIdentifier(value: string): string | null {
  const arnMatch = ARN_PATTERN.exec(value)
  if (arnMatch) return arnMatch[3] ?? null
  const sqsMatch = SQS_URL_PATTERN.exec(value)
  if (sqsMatch) return sqsMatch[2] ?? null
  return null
}

/** Maps a provisioned-resource identifier (ARN or SQS queue URL) to its AWS console page. */
export function consoleLinkFor(value: string, fallbackRegion?: string): string | null {
  const sqs = SQS_URL_PATTERN.exec(value)
  if (sqs) {
    const [, region = ''] = sqs
    return `https://${region}.console.aws.amazon.com/sqs/v3/home?region=${region}#/queues/${encodeURIComponent(value)}`
  }

  const arn = ARN_PATTERN.exec(value)
  if (!arn) return null
  const [, service = '', region = '', , resource = ''] = arn

  if (service === 'events' && resource.startsWith('event-bus/')) {
    const name = resource.slice('event-bus/'.length)
    return `https://${region}.console.aws.amazon.com/events/home?region=${region}#eventbus/${name}`
  }
  if (service === 'schemas' && resource.startsWith('registry/')) {
    const name = resource.slice('registry/'.length)
    return `https://${region}.console.aws.amazon.com/events/home?region=${region}#registries/${name}`
  }
  if (service === 'sqs') {
    return `https://${region}.console.aws.amazon.com/sqs/v3/home?region=${region}#/queues/${encodeURIComponent(value)}`
  }
  if (service === 'cloudwatch' && resource.startsWith('dashboard/')) {
    const name = resource.slice('dashboard/'.length)
    const dashboardRegion = region || fallbackRegion
    if (!dashboardRegion) return null
    return `https://${dashboardRegion}.console.aws.amazon.com/cloudwatch/home?region=${dashboardRegion}#dashboards:name=${name}`
  }
  return null
}

/**
 * Returns a console link for a domain-account resource field, or null when
 * the field is platform-managed, the identifier is unrecognized, or the
 * account does not match the domain owner.
 */
export function domainConsoleLink(
  field: string,
  value: string | undefined,
  ownerAccountId: string | undefined,
  fallbackRegion?: string,
): string | null {
  if (!value || !ownerAccountId) return null
  if (!DOMAIN_RESOURCE_FIELDS.has(field)) return null
  const account = accountFromIdentifier(value)
  if (!account || account !== ownerAccountId) return null
  return consoleLinkFor(value, fallbackRegion)
}

/** Region of the first ARN that carries one; dashboards have an empty region field. */
export function regionFromArns(values: (string | undefined)[]): string | undefined {
  for (const value of values) {
    const region = value ? ARN_PATTERN.exec(value)?.[2] : undefined
    if (region) return region
  }
  return undefined
}
