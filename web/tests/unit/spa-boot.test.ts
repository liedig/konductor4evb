import assert from 'node:assert/strict'
import test from 'node:test'

import { processHtml, rewriteEntrySpecifiers } from '../../scripts/fix-spa-output.mjs'

const inlineMapHtml = `<!DOCTYPE html><html><head><script type="importmap">{"imports":{"#entry":"/_nuxt/BjS66dRF.js"}}</script><script>window.__NUXT__={config:{}}</script></head><body><div id="__nuxt"></div><script type="application/json" id="__NUXT_DATA__">[{"prerenderedAt":1}]</script></body></html>`

test('an inline import map is removed, not externalized', () => {
  const result = processHtml(inlineMapHtml)
  assert.equal(result.entryFile, 'BjS66dRF.js')
  assert.doesNotMatch(result.html, /importmap/)
  assert.doesNotMatch(result.html, /#entry/)
  assert.deepEqual(result.droppedFiles, [])
})

test('the config shim is still externalized and the JSON payload stays inline', () => {
  const result = processHtml(inlineMapHtml)
  assert.equal(result.extractedScripts.length, 1)
  assert.match(result.extractedScripts[0].name, /^inline\.[0-9a-f]{16}\.js$/)
  assert.equal(result.extractedScripts[0].body, 'window.__NUXT__={config:{}}')
  assert.match(result.html, /<script[^>]*src="\/_nuxt\/inline\.[0-9a-f]{16}\.js"/)
  assert.match(result.html, /type="application\/json"/)
})

test('an already-externalized import map is removed along with its file', () => {
  const html = inlineMapHtml.replace(
    /<script type="importmap">[\s\S]*?<\/script>/,
    '<script type="importmap" src="/_nuxt/inline.26f817d63dea4325.js" crossorigin></script>',
  )
  const result = processHtml(html)
  assert.doesNotMatch(result.html, /importmap/)
  assert.deepEqual(result.droppedFiles, ['/_nuxt/inline.26f817d63dea4325.js'])
  assert.equal(result.entryFile, undefined)
})

test('html without an import map passes through untouched by the map handling', () => {
  const html = '<html><body><script>window.__NUXT__={}</script></body></html>'
  const result = processHtml(html)
  assert.equal(result.entryFile, undefined)
  assert.match(result.html, /src="\/_nuxt\/inline\./)
})

test('import maps with unknown entries fail the build instead of shipping broken', () => {
  const extra = `{"imports":{"#entry":"/_nuxt/BjS66dRF.js","#other":"/_nuxt/x.js"}}`
  assert.throws(() => processHtml(inlineMapHtml.replace('{"imports":{"#entry":"/_nuxt/BjS66dRF.js"}}', extra)), /#other/)
})

test('import maps with scopes fail the build', () => {
  const scoped = `{"imports":{"#entry":"/_nuxt/BjS66dRF.js"},"scopes":{}}`
  assert.throws(() => processHtml(inlineMapHtml.replace('{"imports":{"#entry":"/_nuxt/BjS66dRF.js"}}', scoped)), /scopes/)
})

test('a missing #entry mapping fails the build', () => {
  assert.throws(
    () => processHtml(inlineMapHtml.replace('{"imports":{"#entry":"/_nuxt/BjS66dRF.js"}}', '{"imports":{}}')),
    /#entry/,
  )
})

test('an entry outside the _nuxt asset dir fails the build', () => {
  assert.throws(
    () => processHtml(inlineMapHtml.replace('/_nuxt/BjS66dRF.js', 'https://cdn.example.com/entry.js')),
    /_nuxt/,
  )
})

test('chunk importers of #entry are rewritten to the sibling entry file', () => {
  const chunk = 'import{z as x}from"#entry";const y=await import("#entry");'
  const result = rewriteEntrySpecifiers(chunk, 'BjS66dRF.js')
  assert.equal(result.source, 'import{z as x}from"./BjS66dRF.js";const y=await import("./BjS66dRF.js");')
  assert.equal(result.count, 2)
})

test('a bare side-effect import of #entry is rewritten too', () => {
  const result = rewriteEntrySpecifiers('import "#entry"', 'BjS66dRF.js')
  assert.equal(result.source, 'import "./BjS66dRF.js"')
  assert.equal(result.count, 1)
})

test('string literals that merely contain #entry are left alone', () => {
  const source = 'const anchor="#entry";console.log(anchor)'
  const result = rewriteEntrySpecifiers(source, 'BjS66dRF.js')
  assert.equal(result.source, source)
  assert.equal(result.count, 0)
})

test('export-from of #entry is rewritten', () => {
  const result = rewriteEntrySpecifiers('export{a as b} from "#entry";', 'BjS66dRF.js')
  assert.equal(result.source, 'export{a as b} from "./BjS66dRF.js";')
})
