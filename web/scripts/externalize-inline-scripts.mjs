/**
 * Post-build step: moves inline <script> blocks (Nuxt's import map and
 * `window.__NUXT__` config) into hashed files under `_nuxt/` so the strict
 * CloudFront CSP (`script-src 'self'`, design.md risks) can stand without
 * 'unsafe-inline'. JSON payload scripts are inert and stay inline.
 */
import { createHash } from 'node:crypto'
import { mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const publicDir = join(dirname(fileURLToPath(import.meta.url)), '..', '.output', 'public')
const nuxtDir = join(publicDir, '_nuxt')

const INLINE_SCRIPT = /<script(?![^>]*\bsrc=)([^>]*)>([\s\S]*?)<\/script>/g

function* htmlFiles(dir) {
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry)
    if (statSync(path).isDirectory()) {
      if (entry !== '_nuxt') yield* htmlFiles(path)
    } else if (entry.endsWith('.html')) {
      yield path
    }
  }
}

let extracted = 0
for (const file of htmlFiles(publicDir)) {
  const html = readFileSync(file, 'utf8')
  const rewritten = html.replace(INLINE_SCRIPT, (match, attrs, body) => {
    if (attrs.includes('application/json') || body.trim() === '') return match
    const hash = createHash('sha256').update(body).digest('hex').slice(0, 16)
    const name = `inline.${hash}.js`
    mkdirSync(nuxtDir, { recursive: true })
    writeFileSync(join(nuxtDir, name), body)
    extracted += 1
    return `<script${attrs} src="/_nuxt/${name}" crossorigin></script>`
  })
  if (rewritten !== html) writeFileSync(file, rewritten)
}
console.log(`externalized ${extracted} inline script(s)`)
