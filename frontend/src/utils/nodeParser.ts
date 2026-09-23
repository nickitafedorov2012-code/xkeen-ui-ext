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

  const [server, portStr] = hostPort.split(':')
  const port = parseInt(portStr, 10) || 443

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
    const [s, p] = hostport.split(':')
    server = s
    port = parseInt(p, 10) || 8388
  } else {
    const decoded = decodeBase64Safe(mainPart)
    if (decoded.includes('@')) {
      const [userinfo, hostport] = decoded.split('@')
      const colonIdx = userinfo.indexOf(':')
      cipher = userinfo.slice(0, colonIdx)
      password = userinfo.slice(colonIdx + 1)
      const [s, p] = hostport.split(':')
      server = s
      port = parseInt(p, 10) || 8388
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

  const [server, portStr] = hostPort.split(':')
  const port = parseInt(portStr, 10) || 443
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

  const auth = mainPart.slice(0, atIdx)
  const rest = mainPart.slice(atIdx + 1)
  const qIdx = rest.indexOf('?')
  const hostPort = qIdx !== -1 ? rest.slice(0, qIdx) : rest
  const queryStr = qIdx !== -1 ? rest.slice(qIdx + 1) : ''

  const [server, portStr] = hostPort.split(':')
  const port = parseInt(portStr, 10) || 443
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

  const [uuid, password] = userpass.split(':')
  const [server, portStr] = hostPort.split(':')
  const port = parseInt(portStr, 10) || 443
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

  lines.push(`  - name: "${obj.name}"`)
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
              lines.push(`        ${h}: "${subval[h]}"`)
            }
          } else {
            lines.push(`      ${subk}: "${subval}"`)
          }
        }
      }
    } else if (typeof val === 'boolean' || typeof val === 'number') {
      lines.push(`    ${k}: ${val}`)
    } else {
      lines.push(`    ${k}: "${val}"`)
    }
  }

  return lines.join('\n')
}
