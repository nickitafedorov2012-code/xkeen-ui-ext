/**
 * Утилита парсинга ссылок прокси-нод (VLESS, VMess, Shadowsocks, Trojan, Hysteria2, TUIC)
 * и автоматической конвертации в YAML формат ядра Mihomo (Clash.Meta).
 */

export interface ParsedNode {
  name: string
  protocol: string
  yaml: string
  raw: Record<string, any>
}

function decodeBase64Safe(str: string): string {
  try {
    const cleaned = str.replace(/-/g, '+').replace(/_/g, '/')
    const padded = cleaned.padEnd(cleaned.length + (4 - (cleaned.length % 4)) % 4, '=')
    return decodeURIComponent(
      Array.prototype.map
        .call(atob(padded), (c: string) => '%' + ('00' + c.charCodeAt(0).toString(16)).slice(-2))
        .join('')
    )
  } catch {
    try {
      return atob(str)
    } catch {
      return ''
    }
  }
}

export function parseProxyLink(rawLink: string): ParsedNode | null {
  const link = rawLink.trim()
  if (!link) return null

  try {
    // 1. VLESS
    if (link.startsWith('vless://')) {
      return parseVless(link)
    }
    // 2. VMESS
    if (link.startsWith('vmess://')) {
      return parseVmess(link)
    }
    // 3. SHADOWSOCKS
    if (link.startsWith('ss://')) {
      return parseShadowsocks(link)
    }
    // 4. TROJAN
    if (link.startsWith('trojan://')) {
      return parseTrojan(link)
    }
    // 5. HYSTERIA2 / HY2
    if (link.startsWith('hysteria2://') || link.startsWith('hy2://')) {
      return parseHysteria2(link)
    }
    // 6. TUIC
    if (link.startsWith('tuic://')) {
      return parseTuic(link)
    }
  } catch (err) {
    console.warn('Error parsing proxy link:', link, err)
  }

  return null
}

export function parseMultipleLinks(text: string): ParsedNode[] {
  const lines = text.split('\n')
  const results: ParsedNode[] = []

  for (const line of lines) {
    const trimmed = line.trim()
    if (!trimmed || trimmed.startsWith('#') || trimmed.startsWith('//')) continue
    const parsed = parseProxyLink(trimmed)
    if (parsed) {
      results.push(parsed)
    }
  }

  return results
}

/**
 * Безопасный парсинг host:port с поддержкой IPv6 [::1]:port и доменных имен.
 */
function parseHostPort(hostPort: string, defaultPort = 443): { server: string; port: number } {
  const hp = hostPort.trim()
  if (!hp) return { server: '', port: defaultPort }

  // Case 1: Bracketed IPv6, e.g. [2001:db8::1]:8443 or [2001:db8::1]
  if (hp.startsWith('[')) {
    const closeBracketIdx = hp.indexOf(']')
    if (closeBracketIdx !== -1) {
      const server = hp.slice(1, closeBracketIdx)
      const after = hp.slice(closeBracketIdx + 1)
      if (after.startsWith(':')) {
        const port = parseInt(after.slice(1), 10) || defaultPort
        return { server, port }
      }
      return { server, port: defaultPort }
    }
  }

  // Case 2: standard host:port or bare IPv6 without brackets
  const lastColon = hp.lastIndexOf(':')
  if (lastColon === -1) {
    return { server: hp, port: defaultPort }
  }

  const firstColon = hp.indexOf(':')
  if (firstColon !== lastColon) {
    // Multiple colons: likely bare IPv6 without port
    return { server: hp, port: defaultPort }
  }

  const server = hp.slice(0, lastColon)
  const portStr = hp.slice(lastColon + 1)
  const port = parseInt(portStr, 10) || defaultPort
  return { server, port }
}

// -------------------------------------------------------------
// VLESS Parser
// -------------------------------------------------------------
function parseVless(link: string): ParsedNode | null {
  const hashIdx = link.indexOf('#')
  const name = hashIdx !== -1 ? decodeURIComponent(link.slice(hashIdx + 1)) : 'VLESS Node'
  const mainPart = hashIdx !== -1 ? link.slice(8, hashIdx) : link.slice(8)

  const atIdx = mainPart.indexOf('@')
  if (atIdx === -1) return null

  const uuid = mainPart.slice(0, atIdx)
  const rest = mainPart.slice(atIdx + 1)

  const qIdx = rest.indexOf('?')
  const hostPort = qIdx !== -1 ? rest.slice(0, qIdx) : rest
  const queryStr = qIdx !== -1 ? rest.slice(qIdx + 1) : ''

  const { server, port } = parseHostPort(hostPort, 443)

  const params = new URLSearchParams(queryStr)
  const security = params.get('security') || 'none'
  const type = params.get('type') || 'tcp'
  const sni = params.get('sni') || ''
  const pbk = params.get('pbk') || ''
  const sid = params.get('sid') || ''
  const fp = params.get('fp') || 'chrome'
  const flow = params.get('flow') || ''
  const serviceName = params.get('serviceName') || ''
  const path = params.get('path') || ''
  const host = params.get('host') || ''

  const node: Record<string, any> = {
    name,
    type: 'vless',
    server,
    port,
    uuid,
    cipher: 'auto',
    'udp': true,
  }

  if (flow) node['flow'] = flow
  if (security === 'tls' || security === 'reality') {
    node['tls'] = true
    if (sni) node['servername'] = sni
    if (fp) node['client-fingerprint'] = fp
  }
  if (security === 'reality') {
    node['reality-opts'] = {
      'public-key': pbk,
      'short-id': sid,
    }
  }
  if (type === 'grpc') {
    node['network'] = 'grpc'
    node['grpc-opts'] = {
      'grpc-service-name': serviceName,
    }
  } else if (type === 'ws') {
    node['network'] = 'ws'
    node['ws-opts'] = {
      path: path || '/',
      headers: host ? { Host: host } : undefined,
    }
  }

  return {
    name,
    protocol: 'VLESS',
    yaml: objectToYamlNode(node),
    raw: node,
  }
}

// -------------------------------------------------------------
// VMESS Parser
// -------------------------------------------------------------
function parseVmess(link: string): ParsedNode | null {
  const b64 = link.slice(8)
  const jsonStr = decodeBase64Safe(b64)
  if (!jsonStr) return null

  const obj = JSON.parse(jsonStr)
  const name = obj.ps || 'VMess Node'
  const server = obj.add
  const port = parseInt(obj.port, 10) || 443
  const uuid = obj.id
  const alterId = parseInt(obj.aid, 10) || 0
  const cipher = obj.scy || 'auto'
  const net = obj.net || 'tcp'
  const tls = obj.tls === 'tls'

  const node: Record<string, any> = {
    name,
    type: 'vmess',
    server,
    port,
    uuid,
    alterId,
    cipher,
    'udp': true,
  }

  if (tls) {
    node['tls'] = true
    if (obj.sni) node['servername'] = obj.sni
  }
  if (net === 'ws') {
    node['network'] = 'ws'
    node['ws-opts'] = {
      path: obj.path || '/',
      headers: obj.host ? { Host: obj.host } : undefined,
    }
  } else if (net === 'grpc') {
    node['network'] = 'grpc'
    node['grpc-opts'] = {
      'grpc-service-name': obj.path || '',
    }
  }

  return {
    name,
    protocol: 'VMess',
    yaml: objectToYamlNode(node),
    raw: node,
  }
}

// -------------------------------------------------------------
// Shadowsocks Parser
// -------------------------------------------------------------
function parseShadowsocks(link: string): ParsedNode | null {
  const hashIdx = link.indexOf('#')
  const name = hashIdx !== -1 ? decodeURIComponent(link.slice(hashIdx + 1)) : 'SS Node'
  const mainPart = hashIdx !== -1 ? link.slice(5, hashIdx) : link.slice(5)

  let cipher = ''
  let password = ''
  let server = ''
  let port = 8388

  if (mainPart.includes('@')) {
    const [userinfo, hostport] = mainPart.split('@')
    const decodedUser = decodeBase64Safe(userinfo) || userinfo
    const colonIdx = decodedUser.indexOf(':')
    cipher = decodedUser.slice(0, colonIdx)
    password = decodedUser.slice(colonIdx + 1)
    const hp = parseHostPort(hostport, 8388)
    server = hp.server
    port = hp.port
  } else {
    const decoded = decodeBase64Safe(mainPart)
    if (decoded.includes('@')) {
      const [userinfo, hostport] = decoded.split('@')
      const colonIdx = userinfo.indexOf(':')
      cipher = userinfo.slice(0, colonIdx)
      password = userinfo.slice(colonIdx + 1)
      const hp = parseHostPort(hostport, 8388)
      server = hp.server
      port = hp.port
    }
  }

  if (!server || !cipher) return null

  const node: Record<string, any> = {
    name,
    type: 'ss',
    server,
    port,
    cipher,
    password,
    'udp': true,
  }

  return {
    name,
    protocol: 'Shadowsocks',
    yaml: objectToYamlNode(node),
    raw: node,
  }
}

// -------------------------------------------------------------
// Trojan Parser
// -------------------------------------------------------------
function parseTrojan(link: string): ParsedNode | null {
  const hashIdx = link.indexOf('#')
  const name = hashIdx !== -1 ? decodeURIComponent(link.slice(hashIdx + 1)) : 'Trojan Node'
  const mainPart = hashIdx !== -1 ? link.slice(9, hashIdx) : link.slice(9)

  const atIdx = mainPart.indexOf('@')
  if (atIdx === -1) return null

  const password = mainPart.slice(0, atIdx)
  const rest = mainPart.slice(atIdx + 1)
  const qIdx = rest.indexOf('?')
  const hostPort = qIdx !== -1 ? rest.slice(0, qIdx) : rest
  const queryStr = qIdx !== -1 ? rest.slice(qIdx + 1) : ''

  const { server, port } = parseHostPort(hostPort, 443)
  const params = new URLSearchParams(queryStr)
  const sni = params.get('sni') || params.get('peer') || ''

  const node: Record<string, any> = {
    name,
    type: 'trojan',
    server,
    port,
    password,
    'udp': true,
  }
  if (sni) node['sni'] = sni

  return {
    name,
    protocol: 'Trojan',
    yaml: objectToYamlNode(node),
    raw: node,
  }
}

// -------------------------------------------------------------
// Hysteria2 Parser
// -------------------------------------------------------------
function parseHysteria2(link: string): ParsedNode | null {
  const prefix = link.startsWith('hy2://') ? 'hy2://' : 'hysteria2://'
  const hashIdx = link.indexOf('#')
  const name = hashIdx !== -1 ? decodeURIComponent(link.slice(hashIdx + 1)) : 'Hysteria2 Node'
  const mainPart = hashIdx !== -1 ? link.slice(prefix.length, hashIdx) : link.slice(prefix.length)

  const atIdx = mainPart.indexOf('@')
  if (atIdx === -1) return null

  const auth = decodeURIComponent(mainPart.slice(0, atIdx))
  const rest = mainPart.slice(atIdx + 1)
  const qIdx = rest.indexOf('?')
  const hostPort = qIdx !== -1 ? rest.slice(0, qIdx) : rest
  const queryStr = qIdx !== -1 ? rest.slice(qIdx + 1) : ''

  const { server, port } = parseHostPort(hostPort, 443)
  const params = new URLSearchParams(queryStr)
  const sni = params.get('sni') || ''
  const obfs = params.get('obfs') || ''
  const obfsPassword = params.get('obfs-password') || ''

  const node: Record<string, any> = {
    name,
    type: 'hysteria2',
    server,
    port,
    password: auth,
  }
  if (sni) node['sni'] = sni
  if (obfs) {
    node['obfs'] = obfs
    if (obfsPassword) node['obfs-password'] = obfsPassword
  }

  return {
    name,
    protocol: 'Hysteria2',
    yaml: objectToYamlNode(node),
    raw: node,
  }
}

// -------------------------------------------------------------
// TUIC Parser
// -------------------------------------------------------------
function parseTuic(link: string): ParsedNode | null {
  const hashIdx = link.indexOf('#')
  const name = hashIdx !== -1 ? decodeURIComponent(link.slice(hashIdx + 1)) : 'TUIC Node'
  const mainPart = hashIdx !== -1 ? link.slice(7, hashIdx) : link.slice(7)

  const atIdx = mainPart.indexOf('@')
  if (atIdx === -1) return null

  const userpass = mainPart.slice(0, atIdx)
  const rest = mainPart.slice(atIdx + 1)
  const qIdx = rest.indexOf('?')
  const hostPort = qIdx !== -1 ? rest.slice(0, qIdx) : rest
  const queryStr = qIdx !== -1 ? rest.slice(qIdx + 1) : ''

  const colonIdx = userpass.indexOf(':')
  const uuid = colonIdx !== -1 ? userpass.slice(0, colonIdx) : userpass
  const password = colonIdx !== -1 ? userpass.slice(colonIdx + 1) : ''
  const { server, port } = parseHostPort(hostPort, 443)
  const params = new URLSearchParams(queryStr)
  const sni = params.get('sni') || ''
  const congestion = params.get('congestion_control') || 'bbr'

  const node: Record<string, any> = {
    name,
    type: 'tuic',
    server,
    port,
    uuid,
    password: password || '',
    'congestion-controller': congestion,
  }
  if (sni) node['sni'] = sni

  return {
    name,
    protocol: 'TUIC',
    yaml: objectToYamlNode(node),
    raw: node,
  }
}

// -------------------------------------------------------------
// YAML Formatter
// -------------------------------------------------------------
function objectToYamlNode(obj: Record<string, any>): string {
  const lines: string[] = []
  const keys = Object.keys(obj)

  lines.push(`  - name: ${JSON.stringify(String(obj.name))}`)
  for (const k of keys) {
    if (k === 'name') continue
    const val = obj[k]
    if (val === undefined || val === null) continue

    if (typeof val === 'object' && !Array.isArray(val)) {
      lines.push(`    ${k}:`)
      for (const subk of Object.keys(val)) {
        const subval = val[subk]
        if (subval !== undefined && subval !== null) {
          if (typeof subval === 'object') {
            lines.push(`      ${subk}:`)
            for (const h of Object.keys(subval)) {
              lines.push(`        ${h}: ${JSON.stringify(String(subval[h]))}`)
            }
          } else {
            lines.push(`      ${subk}: ${JSON.stringify(String(subval))}`)
          }
        }
      }
    } else if (typeof val === 'boolean' || typeof val === 'number') {
      lines.push(`    ${k}: ${val}`)
    } else {
      lines.push(`    ${k}: ${JSON.stringify(String(val))}`)
    }
  }

  return lines.join('\n')
}

/**
 * Экспорт параметров сервера в стандартную URI-ссылку подключения (VLESS, VMess, SS, Trojan, Hysteria2)
 */
export function exportServerToLink(server: {
  name: string
  protocol: string
  host: string
  port: number
  raw?: Record<string, any>
}): string {
  const name = encodeURIComponent(server.name)
  const rawHost = server.host || ''
  const host = rawHost.includes(':') && !rawHost.startsWith('[') ? `[${rawHost}]` : rawHost
  const port = server.port || 443
  const proto = (server.protocol || '').toLowerCase()
  const raw = server.raw || {}

  if (proto === 'vless' || raw.type?.toLowerCase() === 'vless') {
    const uuid = raw.uuid || '00000000-0000-0000-0000-000000000000'
    const security = raw.tls ? (raw['reality-opts'] ? 'reality' : 'tls') : (raw.security || 'none')
    const sni = raw.servername || raw.sni || ''
    const flow = raw.flow || ''
    const network = raw.network || 'tcp'
    const pbk = raw['reality-opts']?.['public-key'] || ''
    const sid = raw['reality-opts']?.['short-id'] || ''
    const fp = raw['client-fingerprint'] || 'chrome'

    let query = `security=${security}&type=${network}`
    if (sni) query += `&sni=${encodeURIComponent(sni)}`
    if (flow) query += `&flow=${encodeURIComponent(flow)}`
    if (pbk) query += `&pbk=${encodeURIComponent(pbk)}`
    if (sid) query += `&sid=${encodeURIComponent(sid)}`
    if (fp) query += `&fp=${encodeURIComponent(fp)}`

    return `vless://${uuid}@${host}:${port}?${query}#${name}`
  }

  if (proto === 'vmess' || raw.type?.toLowerCase() === 'vmess') {
    const vmessObj = {
      v: '2',
      ps: server.name,
      add: host,
      port: port,
      id: raw.uuid || '00000000-0000-0000-0000-000000000000',
      aid: raw.alterId || '0',
      net: raw.network || 'tcp',
      type: 'none',
      host: raw.servername || raw.sni || '',
      path: raw['ws-opts']?.path || '',
      tls: raw.tls ? 'tls' : '',
    }
    return `vmess://${btoa(unescape(encodeURIComponent(JSON.stringify(vmessObj))))}`
  }

  if (proto === 'shadowsocks' || proto === 'ss' || raw.type?.toLowerCase() === 'ss') {
    const cipher = raw.cipher || 'chacha20-ietf-poly1305'
    const password = raw.password || 'password'
    const userinfo = btoa(`${cipher}:${password}`)
    return `ss://${userinfo}@${host}:${port}#${name}`
  }

  if (proto === 'trojan' || raw.type?.toLowerCase() === 'trojan') {
    const password = raw.password || 'password'
    const sni = raw.servername || raw.sni || ''
    return `trojan://${encodeURIComponent(String(password))}@${host}:${port}?security=tls&sni=${encodeURIComponent(sni)}#${name}`
  }

  if (proto === 'hysteria2' || proto === 'hy2' || raw.type?.toLowerCase() === 'hysteria2') {
    const auth = raw.password || raw.auth || ''
    const sni = raw.servername || raw.sni || ''
    const params = new URLSearchParams()
    if (sni) params.set('sni', sni)
    if (raw.obfs) params.set('obfs', String(raw.obfs))
    if (raw['obfs-password']) params.set('obfs-password', String(raw['obfs-password']))
    return `hysteria2://${encodeURIComponent(String(auth))}@${host}:${port}?${params.toString()}#${name}`
  }

  return `${proto}://${host}:${port}#${name}`
}

