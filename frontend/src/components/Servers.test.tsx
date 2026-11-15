import { describe, it, expect } from 'vitest'
import { mergePolledServerInfo } from './Servers'
import type { ServerInfo } from '../types'

function makeServer(overrides: Partial<ServerInfo> = {}): ServerInfo {
  return {
    id: 'node-1',
    name: 'Node 1',
    protocol: 'VLESS',
    host: '1.2.3.4',
    port: 443,
    is_active: false,
    is_priority: false,
    ping_ms: 120,
    ...overrides,
  }
}

describe('Servers ping state synchronization', () => {
  it('replaces stale positive ping_ms with -1 when polled server becomes offline', () => {
    const prev = makeServer({ id: 'de-1', ping_ms: 120 })
    const fresh = makeServer({ id: 'de-1', ping_ms: -1 })

    const merged = mergePolledServerInfo(prev, fresh)
    expect(merged.ping_ms).toBe(-1)
  })

  it('updates ping_ms when server recovers from -1 to positive latency', () => {
    const prev = makeServer({ id: 'de-1', ping_ms: -1 })
    const fresh = makeServer({ id: 'de-1', ping_ms: 68 })

    const merged = mergePolledServerInfo(prev, fresh)
    expect(merged.ping_ms).toBe(68)
  })
})
