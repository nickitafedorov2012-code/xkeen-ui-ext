import { describe, it, expect } from 'vitest'
import { parseProxyLink, parseMultipleLinks, exportServerToLink } from './nodeParser'

describe('nodeParser', () => {
  it('parses standard VLESS link with domain and reality parameters', () => {
    const link = 'vless://11111111-2222-3333-4444-555555555555@example.com:443?security=reality&sni=example.com&fp=chrome&pbk=pubkey123&sid=1234#MyVlessNode'
    const parsed = parseProxyLink(link)

    expect(parsed).not.toBeNull()
    expect(parsed?.name).toBe('MyVlessNode')
    expect(parsed?.protocol).toBe('VLESS')
    expect(parsed?.raw.server).toBe('example.com')
    expect(parsed?.raw.port).toBe(443)
    expect(parsed?.raw.uuid).toBe('11111111-2222-3333-4444-555555555555')
    expect(parsed?.raw['reality-opts']['public-key']).toBe('pubkey123')
    expect(parsed?.yaml).toContain('type: "vless"')
  })

  it('parses VLESS with IPv6 in brackets [2001:db8::1]:8443', () => {
    const link = 'vless://uuid-v6-test@[2001:db8::1]:8443?type=tcp#IPv6Node'
    const parsed = parseProxyLink(link)

    expect(parsed).not.toBeNull()
    expect(parsed?.name).toBe('IPv6Node')
    expect(parsed?.raw.server).toBe('2001:db8::1')
    expect(parsed?.raw.port).toBe(8443)
  })

  it('parses Trojan with IPv6 [::1]:443', () => {
    const link = 'trojan://topsecret@[::1]:443?sni=trojan.local#TrojanV6'
    const parsed = parseProxyLink(link)

    expect(parsed).not.toBeNull()
    expect(parsed?.name).toBe('TrojanV6')
    expect(parsed?.protocol).toBe('Trojan')
    expect(parsed?.raw.server).toBe('::1')
    expect(parsed?.raw.port).toBe(443)
    expect(parsed?.raw.password).toBe('topsecret')
  })

  it('handles Trojan password percent-decoding and round-trip correctly', () => {
    const link = 'trojan://p%40ss@example.com:443?sni=example.com#TrojanTest'
    const parsed = parseProxyLink(link)
    expect(parsed).not.toBeNull()
    expect(parsed?.raw.password).toBe('p@ss')

    const exported = exportServerToLink({
      name: parsed!.name,
      protocol: parsed!.protocol,
      host: parsed!.raw.server,
      port: parsed!.raw.port,
      raw: parsed!.raw,
    })
    expect(exported).toContain('trojan://p%40ss@example.com:443')

    const reparsed = parseProxyLink(exported)
    expect(reparsed?.raw.password).toBe('p@ss')
  })

  it('exports and roundtrips VLESS WS path and host', () => {
    const link = 'vless://uuid-ws@example.com:443?security=tls&type=ws&path=%2Fcustom&host=cdn.example.com#VlessWS'
    const parsed = parseProxyLink(link)
    expect(parsed).not.toBeNull()
    expect(parsed?.raw['ws-opts'].path).toBe('/custom')
    expect(parsed?.raw['ws-opts'].headers.Host).toBe('cdn.example.com')

    const exported = exportServerToLink({
      name: parsed!.name,
      protocol: parsed!.protocol,
      host: parsed!.raw.server,
      port: parsed!.raw.port,
      raw: parsed!.raw,
    })
    expect(exported).toContain('path=%2Fcustom')
    expect(exported).toContain('host=cdn.example.com')

    const reparsed = parseProxyLink(exported)
    expect(reparsed?.raw['ws-opts'].path).toBe('/custom')
    expect(reparsed?.raw['ws-opts'].headers.Host).toBe('cdn.example.com')
  })

  it('exports and roundtrips VLESS gRPC serviceName', () => {
    const link = 'vless://uuid-grpc@example.com:443?security=tls&type=grpc&serviceName=my-grpc-service#VlessGrpc'
    const parsed = parseProxyLink(link)
    expect(parsed).not.toBeNull()
    expect(parsed?.raw['grpc-opts']['grpc-service-name']).toBe('my-grpc-service')

    const exported = exportServerToLink({
      name: parsed!.name,
      protocol: parsed!.protocol,
      host: parsed!.raw.server,
      port: parsed!.raw.port,
      raw: parsed!.raw,
    })
    expect(exported).toContain('serviceName=my-grpc-service')

    const reparsed = parseProxyLink(exported)
    expect(reparsed?.raw['grpc-opts']['grpc-service-name']).toBe('my-grpc-service')
  })

  it('parses and exports TUIC link', () => {
    const link = 'tuic://my-uuid:my%40pass@tuic.example.com:8443?congestion_control=bbr&sni=tuic.example.com#TuicNode'
    const parsed = parseProxyLink(link)
    expect(parsed).not.toBeNull()
    expect(parsed?.name).toBe('TuicNode')
    expect(parsed?.protocol).toBe('TUIC')
    expect(parsed?.raw.uuid).toBe('my-uuid')
    expect(parsed?.raw.password).toBe('my@pass')
    expect(parsed?.raw['congestion-controller']).toBe('bbr')

    const exported = exportServerToLink({
      name: parsed!.name,
      protocol: parsed!.protocol,
      host: parsed!.raw.server,
      port: parsed!.raw.port,
      raw: parsed!.raw,
    })
    expect(exported).toContain('tuic://my-uuid:my%40pass@tuic.example.com:8443')

    const reparsed = parseProxyLink(exported)
    expect(reparsed?.raw.password).toBe('my@pass')
  })

  it('parses Shadowsocks with legacy base64 format', () => {
    const link = 'ss://YWVzLTEyOC1nY206cGFzc3dvcmQ=@192.168.1.1:8388#SSNode'
    const parsed = parseProxyLink(link)

    expect(parsed).not.toBeNull()
    expect(parsed?.name).toBe('SSNode')
    expect(parsed?.protocol).toBe('Shadowsocks')
    expect(parsed?.raw.server).toBe('192.168.1.1')
    expect(parsed?.raw.port).toBe(8388)
    expect(parsed?.raw.cipher).toBe('aes-128-gcm')
    expect(parsed?.raw.password).toBe('password')
  })

  it('parses Hysteria2 link', () => {
    const link = 'hysteria2://myauth@hy2.example.com:443?sni=hy2.example.com#Hy2Node'
    const parsed = parseProxyLink(link)

    expect(parsed).not.toBeNull()
    expect(parsed?.name).toBe('Hy2Node')
    expect(parsed?.protocol).toBe('Hysteria2')
    expect(parsed?.raw.server).toBe('hy2.example.com')
    expect(parsed?.raw.port).toBe(443)
    expect(parsed?.raw.password).toBe('myauth')
  })

  it('returns null for empty or invalid proxy schemes', () => {
    expect(parseProxyLink('')).toBeNull()
    expect(parseProxyLink('   ')).toBeNull()
    expect(parseProxyLink('http://example.com')).toBeNull()
    expect(parseProxyLink('not-a-link')).toBeNull()
  })

  it('parseMultipleLinks parses valid nodes and ignores comments/empty lines', () => {
    const text = `
# Comment line
// Another comment
vless://uuid1@node1.com:443#Node1

trojan://pass2@node2.com:443#Node2
`
    const nodes = parseMultipleLinks(text)
    expect(nodes.length).toBe(2)
    expect(nodes[0].name).toBe('Node1')
    expect(nodes[1].name).toBe('Node2')
  })

  // ==================== EXPLICIT REGRESSION TESTS ====================

  it('regression: Trojan password percent-encoding round-trip preserves %40 -> @ -> %40 without %2540', () => {
    const originalLink = 'trojan://user%40domain.com%40extra@vpn.example.com:443#UserAtDomain'
    const parsed = parseProxyLink(originalLink)

    expect(parsed).not.toBeNull()
    // Percent decoding during parse
    expect(parsed?.raw.password).toBe('user@domain.com@extra')
    expect(parsed?.yaml).toContain('password: "user@domain.com@extra"')

    // Exporting back to URI
    const exportedLink = exportServerToLink({
      name: parsed!.name,
      protocol: parsed!.protocol,
      host: parsed!.raw.server,
      port: parsed!.raw.port,
      raw: parsed!.raw,
    })

    // Must be encoded as %40, NOT double-encoded as %2540
    expect(exportedLink).toContain('trojan://user%40domain.com%40extra@vpn.example.com:443')
    expect(exportedLink).not.toContain('%2540')

    // Re-parsing exported link
    const reparsed = parseProxyLink(exportedLink)
    expect(reparsed?.raw.password).toBe('user@domain.com@extra')
  })

  it('regression: VLESS WS, VLESS gRPC, and TUIC full round-trip preservation of transport options', () => {
    // 1. VLESS WS
    const vlessWsLink = 'vless://a1b2c3d4-e5f6-7890-abcd-ef1234567890@ws.example.com:443?security=tls&type=ws&path=%2Fcustom%2Fws&host=cdn.myhost.com&fp=firefox#VlessWsTest'
    const parsedWs = parseProxyLink(vlessWsLink)
    expect(parsedWs).not.toBeNull()
    expect(parsedWs?.raw.network).toBe('ws')
    expect(parsedWs?.raw['ws-opts'].path).toBe('/custom/ws')
    expect(parsedWs?.raw['ws-opts'].headers.Host).toBe('cdn.myhost.com')

    const exportedWs = exportServerToLink({
      name: parsedWs!.name,
      protocol: parsedWs!.protocol,
      host: parsedWs!.raw.server,
      port: parsedWs!.raw.port,
      raw: parsedWs!.raw,
    })
    expect(exportedWs).toContain('type=ws')
    expect(exportedWs).toContain('path=%2Fcustom%2Fws')
    expect(exportedWs).toContain('host=cdn.myhost.com')
    const reparsedWs = parseProxyLink(exportedWs)
    expect(reparsedWs?.raw['ws-opts'].path).toBe('/custom/ws')
    expect(reparsedWs?.raw['ws-opts'].headers.Host).toBe('cdn.myhost.com')

    // 2. VLESS gRPC
    const vlessGrpcLink = 'vless://a1b2c3d4-e5f6-7890-abcd-ef1234567890@grpc.example.com:443?security=tls&type=grpc&serviceName=my-grpc-service&fp=chrome#VlessGrpcTest'
    const parsedGrpc = parseProxyLink(vlessGrpcLink)
    expect(parsedGrpc).not.toBeNull()
    expect(parsedGrpc?.raw.network).toBe('grpc')
    expect(parsedGrpc?.raw['grpc-opts']['grpc-service-name']).toBe('my-grpc-service')

    const exportedGrpc = exportServerToLink({
      name: parsedGrpc!.name,
      protocol: parsedGrpc!.protocol,
      host: parsedGrpc!.raw.server,
      port: parsedGrpc!.raw.port,
      raw: parsedGrpc!.raw,
    })
    expect(exportedGrpc).toContain('type=grpc')
    expect(exportedGrpc).toContain('serviceName=my-grpc-service')
    const reparsedGrpc = parseProxyLink(exportedGrpc)
    expect(reparsedGrpc?.raw['grpc-opts']['grpc-service-name']).toBe('my-grpc-service')

    // 3. TUIC
    const tuicLink = 'tuic://uuid-user-1234:secret%40pass%23extra@tuic.example.com:8443?alpn=h3&congestion_control=bbr&sni=sni.tuic.com#TuicTest'
    const parsedTuic = parseProxyLink(tuicLink)
    expect(parsedTuic).not.toBeNull()
    expect(parsedTuic?.protocol).toBe('TUIC')
    expect(parsedTuic?.raw.uuid).toBe('uuid-user-1234')
    expect(parsedTuic?.raw.password).toBe('secret@pass#extra')
    expect(parsedTuic?.raw.alpn).toEqual(['h3'])
    expect(parsedTuic?.raw['congestion-controller']).toBe('bbr')

    const exportedTuic = exportServerToLink({
      name: parsedTuic!.name,
      protocol: parsedTuic!.protocol,
      host: parsedTuic!.raw.server,
      port: parsedTuic!.raw.port,
      raw: parsedTuic!.raw,
    })
    expect(exportedTuic).toContain('tuic://uuid-user-1234:secret%40pass%23extra@tuic.example.com:8443')
    expect(exportedTuic).toContain('congestion_control=bbr')
    expect(exportedTuic).toContain('alpn=h3')
    expect(exportedTuic).toContain('sni=sni.tuic.com')

    const reparsedTuic = parseProxyLink(exportedTuic)
    expect(reparsedTuic?.raw.uuid).toBe('uuid-user-1234')
    expect(reparsedTuic?.raw.password).toBe('secret@pass#extra')
    expect(reparsedTuic?.raw['congestion-controller']).toBe('bbr')
  })
})
