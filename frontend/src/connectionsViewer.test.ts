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

import { registrableDomain, parseHostPort } from './components/ConnectionsViewer'

describe('bug 137: registrable domain, host/port, and IP detection', () => {
  it('bug 137: registrable domain handles public suffixes, ips and ipv6', () => {
    // 137.3: parseHostPort — корректное отрезание порта для host:443, [2a00::1]:443, [2a00::1]
    expect(parseHostPort('host:443')).toEqual({ host: 'host', port: '443', isIp: false, ipVersion: null })
    expect(parseHostPort('[2a00::1]:443')).toEqual({ host: '2a00::1', port: '443', isIp: true, ipVersion: 6 })
    expect(parseHostPort('[2a00::1]')).toEqual({ host: '2a00::1', port: null, isIp: true, ipVersion: 6 })
    expect(parseHostPort('1.2.3.4:443')).toEqual({ host: '1.2.3.4', port: '443', isIp: true, ipVersion: 4 })
    expect(parseHostPort('2a00:1450::1')).toEqual({ host: '2a00:1450::1', port: null, isIp: true, ipVersion: 6 })

    // 137.1: registrableDomain — расширенные составные суффиксы и общие хостинги
    expect(registrableDomain('a.b.example.co.uk')).toBe('example.co.uk')
    expect(registrableDomain('sub.example.com.au:443')).toBe('example.com.au')
    expect(registrableDomain('sub.example.co.kr')).toBe('example.co.kr')
    expect(registrableDomain('sub.example.com.cn')).toBe('example.com.cn')
    expect(registrableDomain('sub.example.co.jp')).toBe('example.co.jp')
    expect(registrableDomain('sub.example.com.br')).toBe('example.com.br')

    // Общие хостинги: возвращается полное имя хоста (поддомен), а не общий суффикс
    expect(registrableDomain('foo.github.io')).toBe('foo.github.io')
    expect(registrableDomain('d111.cloudfront.net')).toBe('d111.cloudfront.net')
    expect(registrableDomain('s3.amazonaws.com')).toBe('s3.amazonaws.com')
    expect(registrableDomain('my-app.appspot.com')).toBe('my-app.appspot.com')
    expect(registrableDomain('my-worker.sub.workers.dev')).toBe('my-worker.sub.workers.dev')
    expect(registrableDomain('my-site.vercel.app')).toBe('my-site.vercel.app')
    expect(registrableDomain('cdn.akamaihd.net')).toBe('cdn.akamaihd.net')

    // 137.2: IP-адреса не должны трактоваться как домены
    expect(registrableDomain('1.2.3.4:443')).toBeNull()
    expect(registrableDomain('[2a00:1450::1]:443')).toBeNull()
    expect(registrableDomain('[2a00::1]')).toBeNull()
    expect(registrableDomain('198.51.100.25')).toBeNull()
  })
})
