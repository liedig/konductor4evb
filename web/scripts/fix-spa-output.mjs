/**
 * Post-build step for the statically hosted SPA.
 *
 * 1. Nuxt resolves its entry through an import map (`#entry`). Import maps
 *    are inline-only in HTML: browsers ignore the `src` attribute on
 *    `<script type="importmap">`, and the deployed CSP (`script-src 'self'`)
 *    blocks the inline form. The map is therefore removed and every
 *    `#entry` specifier in the emitted chunks is rewritten to a plain
 *    relative import of the sibling entry file.
 * 2. Inline `<script>` blocks (`window.__NUXT__` config) move into hashed
 *    files under `_nuxt/` so the strict CSP can stand without
 *    'unsafe-inline'. JSON payload scripts are inert and stay inline.
 *
 * Anything unexpected (unknown map entries, scopes, entry outside `_nuxt/`,
 * surviving references) fails the build rather than shipping a blank page.
 */
import { createHash } from 'node:crypto'
import { readdirSync, readFileSync, realpathSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const scriptDir = dirname(fileURLToPath(import.meta.url))
const publicDir = join(scriptDir, '..', '.output', 'public')
const nuxtDir = join(publicDir, '_nuxt')

const SCRIPT_TAG = /<script\b([^>]*)>([\s\S]*?)<\/script>/g
const INLINE_SCRIPT = /<script(?![^>]*\bsrc=)([^>]*)>([\s\S]*?)<\/script>/g
const ENTRY_SPECIFIERS = [
  { pattern: /\bfrom(\s*)"#entry"/g, buildReplacement: (target, [ws]) => `from${ws}${target}` },
  { pattern: /\bimport(\s*)"#entry"/g, buildReplacement: (target, [ws]) => `import${ws}${target}` },
  {
    pattern: /\bimport(\s*\(\s*)"#entry"(\s*\))/g,
    buildReplacement: (target, [open, close]) => `import${open}${target}${close}`,
  },
]

function entryFileName(raw, where) {
  const file = /^\/_nuxt\/([^/]+)$/.exec(raw)?.[1] ?? /^\.{1,2}\/_nuxt\/([^/]+)$/.exec(raw)?.[1]
  if (!file) {
    throw new Error(
      `import map entry must point into the flat _nuxt asset dir, got "${raw}" (${where}); ` +
        'extend scripts/fix-spa-output.mjs for the new entry shape',
    )
  }
  return file
}

function parseImportMap(body, where) {
  const map = JSON.parse(body)
  const keys = Object.keys(map)
  if (keys.length !== 1 || keys[0] !== 'imports') {
    throw new Error(`unsupported import map keys [${keys}] (${where}): only a flat "imports" object is handled`)
  }
  const entries = Object.keys(map.imports)
  if (entries.length !== 1 || entries[0] !== '#entry') {
    throw new Error(
      `unsupported import map entries [${entries}] (${where}): only "#entry" is handled; ` +
        'rewriting other specifiers needs new handling in scripts/fix-spa-output.mjs',
    )
  }
  return entryFileName(map.imports['#entry'], where)
}

export function processHtml(html) {
  let entryFile
  const droppedFiles = []
  const extractedScripts = []
  for (const [tag, attrs, body] of html.matchAll(SCRIPT_TAG)) {
    if (!attrs.includes('type="importmap"')) continue
    const src = /\bsrc="([^"]+)"/.exec(attrs)?.[1]
    if (src) {
      droppedFiles.push(src)
    } else {
      const file = parseImportMap(body, tag)
      if (entryFile !== undefined && entryFile !== file) {
        throw new Error(`conflicting import map entries "${entryFile}" and "${file}"`)
      }
      entryFile = file
    }
    html = html.replace(tag, '')
  }
  html = html.replace(INLINE_SCRIPT, (match, attrs, body) => {
    if (attrs.includes('application/json') || body.trim() === '') return match
    const hash = createHash('sha256').update(body).digest('hex').slice(0, 16)
    const name = `inline.${hash}.js`
    extractedScripts.push({ name, body })
    return `<script${attrs} src="/_nuxt/${name}" crossorigin></script>`
  })
  return { html, entryFile, droppedFiles, extractedScripts }
}

export function rewriteEntrySpecifiers(source, entryFile) {
  let count = 0
  let rewritten = source
  const target = `"./${entryFile}"`
  for (const { pattern, buildReplacement } of ENTRY_SPECIFIERS) {
    rewritten = rewritten.replace(pattern, (...args) => {
      count += 1
      return buildReplacement(target, args.slice(1, -2))
    })
  }
  return { source: rewritten, count }
}

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

function* chunkFiles(dir) {
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry)
    if (statSync(path).isDirectory()) {
      yield* chunkFiles(path)
    } else if (entry.endsWith('.js')) {
      yield path
    }
  }
}

function verifyNoSurvivors() {
  for (const file of htmlFiles(publicDir)) {
    const html = readFileSync(file, 'utf8')
    if (/importmap/.test(html)) throw new Error(`an import map survived post-build processing in ${file}`)
    if (html.includes('#entry')) {
      throw new Error(`the "#entry" specifier survived post-build processing in ${file}`)
    }
  }
  for (const file of chunkFiles(nuxtDir)) {
    const { count } = rewriteEntrySpecifiers(readFileSync(file, 'utf8'), 'never-reached.js')
    if (count > 0) throw new Error(`the "#entry" specifier survived post-build processing in ${file}`)
  }
}

function main() {
  let entryFile
  let externalized = 0
  const droppedFiles = []
  for (const file of htmlFiles(publicDir)) {
    const html = readFileSync(file, 'utf8')
    const result = processHtml(html)
    if (result.entryFile !== undefined) {
      if (entryFile !== undefined && entryFile !== result.entryFile) {
        throw new Error(`conflicting import map entries across pages: "${entryFile}" and "${result.entryFile}"`)
      }
      entryFile = result.entryFile
    }
    droppedFiles.push(...result.droppedFiles)
    for (const { name, body } of result.extractedScripts) writeFileSync(join(nuxtDir, name), body)
    if (result.html !== html) writeFileSync(file, result.html)
    externalized += result.extractedScripts.length
  }
  for (const src of droppedFiles) rmSync(join(publicDir, src), { force: true })
  if (droppedFiles.length > 0) console.log(`removed ${droppedFiles.length} externalized import map file(s)`)

  let references = 0
  if (entryFile !== undefined) {
    let touchedChunks = 0
    for (const file of chunkFiles(nuxtDir)) {
      const { source, count } = rewriteEntrySpecifiers(readFileSync(file, 'utf8'), entryFile)
      if (count > 0) {
        writeFileSync(file, source)
        references += count
        touchedChunks += 1
      }
    }
    if (references === 0) {
      console.warn('warning: an import map was removed but no chunk referenced "#entry"')
    } else {
      console.log(`rewrote ${references} "#entry" reference(s) across ${touchedChunks} chunk file(s) to ./${entryFile}`)
    }
  }
  verifyNoSurvivors()
  console.log(`externalized ${externalized} inline script(s)`)
}

if (process.argv[1] && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main()
}
