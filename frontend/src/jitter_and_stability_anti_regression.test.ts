import { describe, it, expect, beforeEach } from 'vitest'
import {
  calcJitter,
  getJitterColor,
  loadServerPingHistory,
  saveServerPingHistory,
  loadDashPingHistory,
  saveDashPingHistory,
} from './utils/jitter'

describe('Jitter & Ping Stability Anti-Regression Tests', () => {
  beforeEach(() => {
    sessionStorage.clear()
  })

  it('calculates jitter correctly for stable and unstable connections', () => {
    expect(calcJitter([45, 46, 44, 45, 47])).toBe(2)
    expect(calcJitter([50, 65, 45, 60])).toBe(17)
    expect(calcJitter([30, 90, 25, 110])).toBe(70)
  })

  it('returns 0 when less than 2 measurements are available', () => {
    expect(calcJitter([])).toBe(0)
    expect(calcJitter([42])).toBe(0)
  })

  it('strictly isolates ping history per server in sessionStorage', () => {
    const serverA = 'vless_de_frankfurt'
    const serverB = 'vless_ru_moscow'

    saveServerPingHistory(serverA, [138, 142, 140, 139])
    saveServerPingHistory(serverB, [19, 21, 20])

    const loadedA = loadServerPingHistory(serverA)
    const loadedB = loadServerPingHistory(serverB)

    expect(calcJitter(loadedA)).toBe(2)
    expect(calcJitter(loadedB)).toBe(2)
    expect(loadedA).not.toContain(20)
    expect(loadedB).not.toContain(140)
  })

  it('prevents cross-contamination spike when switching servers', () => {
    const prevServerHistory = [140, 142, 141]
    const newServerPing = 20

    expect(calcJitter([...prevServerHistory, newServerPing])).toBe(41)
    expect(calcJitter([newServerPing])).toBe(0)
  })

  it('assigns correct visual status colors based on thresholds', () => {
    expect(getJitterColor(0, 1)).toBe('var(--muted)')
    expect(getJitterColor(3, 5)).toBe('#22c55e')
    expect(getJitterColor(10, 5)).toBe('#22c55e')
    expect(getJitterColor(11, 5)).toBe('#eab308')
    expect(getJitterColor(25, 5)).toBe('#eab308')
    expect(getJitterColor(26, 5)).toBe('#ef4444')
  })

  it('strictly isolates dashboard ping history per active server', () => {
    saveDashPingHistory('server_1', [{ ms: 30, ok: true }, { ms: 32, ok: true }])
    saveDashPingHistory('server_2', [{ ms: 120, ok: true }])

    const pingsS1 = loadDashPingHistory('server_1')
    const pingsS2 = loadDashPingHistory('server_2')

    expect(pingsS1.length).toBe(2)
    expect(pingsS2.length).toBe(1)
    expect(pingsS1[0].ms).toBe(30)
    expect(pingsS2[0].ms).toBe(120)
  })
})
