export interface CommandLocationEnv {
  hostname?: string
  host?: string
  port?: string
}

export function computeAntigravityParams(
  statusRouterIp?: string | null,
  loc?: CommandLocationEnv,
  proxyPort: number = 53129
) {
  const rawHostname = loc?.hostname || (typeof window !== 'undefined' ? window.location.hostname : '')
  const rawHost = loc?.host || (typeof window !== 'undefined' ? window.location.host : '')
  const rawPort = loc?.port || (typeof window !== 'undefined' ? window.location.port : '')

  const routerIp =
    statusRouterIp ||
    (rawHostname !== 'localhost' && rawHostname !== '127.0.0.1' && rawHostname !== ''
      ? rawHostname
      : (statusRouterIp || rawHostname || '192.168.1.1'))

  const patchHost =
    rawHost.includes('localhost') && statusRouterIp
      ? `${statusRouterIp}:${rawPort || '1001'}`
      : (rawHost || `${routerIp}:1001`)

  const patchCommandPs = `irm http://${patchHost}/patch | iex`
  const patchCommandCmd = `powershell -ExecutionPolicy Bypass -Command "irm http://${patchHost}/patch | iex"`
  const proxyUrl = `http://${routerIp}:${proxyPort}`

  return {
    routerIp,
    patchHost,
    patchCommandPs,
    patchCommandCmd,
    proxyUrl,
    envCmd: `set AG_LS_PROXY=${proxyUrl}`,
    envPs: `$env:AG_LS_PROXY="${proxyUrl}"`,
    envSh: `export AG_LS_PROXY="${proxyUrl}"`,
  }
}
