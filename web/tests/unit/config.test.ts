import assert from 'node:assert/strict'
import test from 'node:test'

import { parseKonductorConfig } from '../../app/types/config.ts'

const base = {
  apiBaseUrl: '/api',
  userPoolId: 'pool-1',
  userPoolClientId: 'client-1',
  region: 'ap-southeast-2',
  environments: ['dev', 'staging', 'prod'],
}

test('accepts the /api root', () => {
  assert.equal(parseKonductorConfig(base).apiBaseUrl, '/api')
})

test('accepts an absolute API root', () => {
  const config = parseKonductorConfig({ ...base, apiBaseUrl: 'https://d123.cloudfront.net/api' })
  assert.equal(config.apiBaseUrl, 'https://d123.cloudfront.net/api')
})

test('rejects an apiBaseUrl that ends in a stage segment', () => {
  assert.throws(
    () => parseKonductorConfig({ ...base, apiBaseUrl: 'https://api.example.com/dev' }),
    /stage segment/,
  )
  assert.throws(() => parseKonductorConfig({ ...base, apiBaseUrl: '/api/staging' }), /"staging"/)
})

test('trailing slashes do not hide a stage segment', () => {
  assert.throws(() => parseKonductorConfig({ ...base, apiBaseUrl: '/api/prod/' }), /"prod"/)
})

test('a segment that is not a configured environment is allowed', () => {
  assert.equal(parseKonductorConfig({ ...base, apiBaseUrl: '/api/gateway' }).apiBaseUrl, '/api/gateway')
})

test('missing required keys still throw', () => {
  assert.throws(() => parseKonductorConfig({ ...base, region: '' }), /"region"/)
  assert.throws(() => parseKonductorConfig(null), /not an object/)
})
