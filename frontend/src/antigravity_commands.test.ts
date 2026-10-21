import { describe, it, expect } from 'vitest'
import { computeAntigravityParams } from './utils/antigravity_commands'

describe('Antigravity dynamic IP and command generation (Task #24)', () => {
  it('correctly uses status.router_ip when provided', () => {
    const params = computeAntigravityParams('192.168.1.1', {
      hostname: 'localhost',
      host: 'localhost:1001',
      port: '1001',
    })

    expect(params.routerIp).toBe('192.168.1.1')
    expect(params.patchHost).toBe('192.168.1.1:1001')
    expect(params.patchCommandPs).toBe('irm http://192.168.1.1:1001/patch | iex')
    expect(params.patchCommandCmd).toBe('powershell -ExecutionPolicy Bypass -Command "irm http://192.168.1.1:1001/patch | iex"')
    expect(params.proxyUrl).toBe('http://192.168.1.1:53129')
    expect(params.envCmd).toBe('set AG_LS_PROXY=http://192.168.1.1:53129')
    expect(params.envPs).toBe('$env:AG_LS_PROXY="http://192.168.1.1:53129"')
  })

  it('uses browser hostname when accessing router directly by custom IP or domain', () => {
    const params = computeAntigravityParams(null, {
      hostname: '192.168.0.1',
      host: '192.168.0.1:8080',
      port: '8080',
    })

    expect(params.routerIp).toBe('192.168.0.1')
    expect(params.patchHost).toBe('192.168.0.1:8080')
    expect(params.patchCommandPs).toBe('irm http://192.168.0.1:8080/patch | iex')
    expect(params.patchCommandCmd).toBe('powershell -ExecutionPolicy Bypass -Command "irm http://192.168.0.1:8080/patch | iex"')
  })

  it('does not have hardcoded 192.168.2.1 when running on different subnet', () => {
    const params = computeAntigravityParams('10.10.1.1', {
      hostname: 'localhost',
      host: 'localhost:5173',
      port: '5173',
    })

    expect(params.patchCommandPs).not.toContain('192.168.2.1')
    expect(params.patchCommandCmd).not.toContain('192.168.2.1')
    expect(params.proxyUrl).not.toContain('192.168.2.1')
    expect(params.proxyUrl).toBe('http://10.10.1.1:53129')
  })

  it('handles custom proxy port properly', () => {
    const params = computeAntigravityParams('192.168.5.1', undefined, 54321)
    expect(params.proxyUrl).toBe('http://192.168.5.1:54321')
    expect(params.envCmd).toBe('set AG_LS_PROXY=http://192.168.5.1:54321')
  })
})
