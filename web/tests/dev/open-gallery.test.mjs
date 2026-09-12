import assert from 'node:assert/strict'
import { test } from 'node:test'

import { createGalleryOpener } from '../../scripts/open-gallery.mjs'

const server = { address: () => ({ port: 3210 }) }

for (const [os, command, prefix] of [
  ['darwin', 'open', []],
  ['linux', 'xdg-open', []],
  ['win32', 'rundll32', ['url.dll,FileProtocolHandler']],
]) {
  test(`opens the actual gallery port once on ${os}`, () => {
    const calls = []
    const listen = createGalleryOpener(true, {
      os,
      run: (command, args) => calls.push({ command, args }),
    })
    listen(server)
    listen(server)
    assert.deepEqual(calls, [{ command, args: [...prefix, 'http://localhost:3210/design-system'] }])
  })
}

test('plain dev mode does not open the browser', () => {
  const listen = createGalleryOpener(false, { run: () => assert.fail('unexpected browser launch') })
  listen(server)
})

test('waits for a TCP address', () => {
  const calls = []
  const listen = createGalleryOpener(true, { run: (_command, args) => calls.push(args) })
  listen({ address: () => null })
  listen({ address: () => '/tmp/socket' })
  assert.equal(calls.length, 0)
  listen(server)
  assert.equal(calls.length, 1)
})

test('browser launch failure leaves the server usable and prints the URL', () => {
  const warnings = []
  const listen = createGalleryOpener(true, {
    run: (_command, _args, callback) => callback(new Error('No desktop opener')),
    warn: message => warnings.push(message),
  })
  assert.doesNotThrow(() => listen(server))
  assert.deepEqual(warnings, ['Could not open the browser. Open http://localhost:3210/design-system manually.'])
})
