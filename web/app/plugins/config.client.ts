import { Amplify } from 'aws-amplify'
import { cognitoUserPoolsTokenProvider } from 'aws-amplify/auth/cognito'
import { sessionStorage } from 'aws-amplify/utils'

import { parseNexusConfig } from '~/types/config'

// Boot-time runtime config: the served `config.json` (written by the deploy
// target from stack outputs) is the single source for API and Cognito wiring,
// so stack changes never require a rebuild (design.md, risks). Tokens persist
// per browser tab via sessionStorage (design.md, decision 2).
export default defineNuxtPlugin(async () => {
  const raw = await $fetch<unknown>('/config.json').catch(() => {
    throw new Error('Could not load /config.json — is the console deployed?')
  })
  const config = parseNexusConfig(raw)

  cognitoUserPoolsTokenProvider.setKeyValueStorage(sessionStorage)
  Amplify.configure({
    Auth: {
      Cognito: {
        userPoolId: config.userPoolId,
        userPoolClientId: config.userPoolClientId,
      },
    },
  })

  return { provide: { nexusConfig: config } }
})
