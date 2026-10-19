/**
 * Console-wide environment ordering: the platform's configured order first
 * (`config.environments`), with any server-reported extras appended in their
 * own order so nothing renders twice or disappears.
 */
export function orderEnvironments(reported: Iterable<string>, configured: string[]): string[] {
  const seen = [...reported]
  const ordered = configured.filter(env => seen.includes(env))
  for (const env of seen) {
    if (!configured.includes(env)) ordered.push(env)
  }
  return ordered
}
