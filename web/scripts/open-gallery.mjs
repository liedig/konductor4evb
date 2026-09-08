import { execFile } from 'node:child_process'
import { platform } from 'node:os'

export function createGalleryOpener(enabled, { run = execFile, os = platform(), warn = console.warn } = {}) {
  let opened = false
  return (server) => {
    if (!enabled || opened) return
    const address = server.address()
    if (!address || typeof address === 'string') return
    opened = true
    const url = `http://localhost:${address.port}/design-system`
    const command = os === 'darwin' ? 'open' : os === 'win32' ? 'rundll32' : 'xdg-open'
    const args = os === 'win32' ? ['url.dll,FileProtocolHandler', url] : [url]
    run(command, args, (error) => {
      if (error) warn(`Could not open the browser. Open ${url} manually.`)
    })
  }
}
