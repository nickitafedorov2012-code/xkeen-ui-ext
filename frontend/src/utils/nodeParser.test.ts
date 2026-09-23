import { describe, it, expect } from 'vitest'
import { parseProxyLink, parseMultipleLinks } from './nodeParser'

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
})
