const ARN_PATTERN = /^arn:aws:([^:]*):([^:]*):(\d{12}):(.+)$/

/** Maps a provisioned-resource identifier (ARN or SQS queue URL) to its AWS console page. */
export function consoleLinkFor(value: string, fallbackRegion?: string): string | null {
  const sqs = /^https:\/\/sqs\.([a-z0-9-]+)\.amazonaws\.com\/\d{12}\/[^/]+$/.exec(value)
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
  if (service === 'cloudwatch' && resource.startsWith('dashboard/')) {
    const name = resource.slice('dashboard/'.length)
    const dashboardRegion = region || fallbackRegion
    if (!dashboardRegion) return null
    return `https://${dashboardRegion}.console.aws.amazon.com/cloudwatch/home?region=${dashboardRegion}#dashboards:name=${name}`
  }
  return null
}

/** Region of the first ARN that carries one; dashboards have an empty region field. */
export function regionFromArns(values: (string | undefined)[]): string | undefined {
  for (const value of values) {
    const region = value ? ARN_PATTERN.exec(value)?.[2] : undefined
    if (region) return region
  }
  return undefined
}
