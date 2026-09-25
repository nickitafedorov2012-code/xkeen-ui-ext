import { describe, it, expect } from 'vitest'
import { extractCleanDomain } from './components/ConnectionsViewer'

describe('extractCleanDomain utility', () => {
  it('extracts root domain from standard FQDN with port', () => {
    const res = extractCleanDomain('gateway-us-east1-d.discord.gg:443')
    expect(res.host).toBe('gateway-us-east1-d.discord.gg')
    expect(res.rootDomain).toBe('discord.gg')
    expect(res.isIp).toBe(false)
  })

  it('extracts root domain for video CDN servers', () => {
    const res = extractCleanDomain('rr1---sn-5hne6nzk.googlevideo.com:443')
    expect(res.host).toBe('rr1---sn-5hne6nzk.googlevideo.com')
    expect(res.rootDomain).toBe('googlevideo.com')
    expect(res.isIp).toBe(false)
  })

  it('handles two-part TLDs properly', () => {
    const res = extractCleanDomain('sub.example.co.uk:8080')
    expect(res.host).toBe('sub.example.co.uk')
    expect(res.rootDomain).toBe('example.co.uk')
    expect(res.isIp).toBe(false)
  })

  it('identifies IPv4 destinations correctly without domain splitting', () => {
    const res = extractCleanDomain('198.51.100.25:443')
    expect(res.host).toBe('198.51.100.25')
    expect(res.rootDomain).toBe('198.51.100.25')
    expect(res.isIp).toBe(true)
  })

  it('handles plain domains without subdomains', () => {
    const res = extractCleanDomain('youtube.com')
    expect(res.host).toBe('youtube.com')
    expect(res.rootDomain).toBe('youtube.com')
    expect(res.isIp).toBe(false)
  })

  it('handles empty input gracefully', () => {
    const res = extractCleanDomain('')
    expect(res.host).toBe('')
    expect(res.rootDomain).toBe('')
    expect(res.isIp).toBe(false)
  })
})
