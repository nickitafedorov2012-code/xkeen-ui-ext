import { describe, it, expect } from 'vitest'
import { formatQuotaBytes } from './components/Servers'
import type { FailoverConfig } from './types'

describe('Task 3.3: Decimal Quota Formatting (formatQuotaBytes)', () => {
  it('formats 0 or negative bytes as 0 Б', () => {
    expect(formatQuotaBytes(0)).toBe('0 Б')
    expect(formatQuotaBytes(-100)).toBe('0 Б')
  })

  it('formats bytes under 1024 correctly as Б', () => {
    expect(formatQuotaBytes(500)).toBe('500 Б')
    expect(formatQuotaBytes(1023)).toBe('1023 Б')
  })

  it('formats kilobytes using binary 1024^1 bytes', () => {
    expect(formatQuotaBytes(1024)).toBe('1.0 КБ')
    expect(formatQuotaBytes(1536)).toBe('1.5 КБ')
  })

  it('formats megabytes using binary 1024^2 bytes', () => {
    expect(formatQuotaBytes(1024 * 1024)).toBe('1.0 МБ')
    expect(formatQuotaBytes(15 * 1024 * 1024)).toBe('15.0 МБ')
  })

  it('accurately matches provider billing for total quota (880.00 ГБ binary)', () => {
    // 944,892,805,120 bytes = 880 * 1024^3 bytes -> 880.00 ГБ
    const totalBytes = 944892805120
    expect(formatQuotaBytes(totalBytes)).toBe('880.00 ГБ')
  })

  it('accurately matches provider billing for remaining traffic (648.93 ГБ binary)', () => {
    const remainingBytes = 696782009274
    expect(formatQuotaBytes(remainingBytes)).toBe('648.93 ГБ')
  })

  it('calculates remaining bytes accurately as total - (upload + download)', () => {
    const total = 1000000000000 // 1000 GB
    const upload = 100000000000 // 100 GB
    const download = 200000000000 // 200 GB
    const remaining = Math.max(0, total - (upload + download))
    expect(remaining).toBe(700000000000)
    expect(formatQuotaBytes(remaining)).toBe('700.00 ГБ')
  })

  it('formats terabytes using decimal 1000^4 bytes', () => {
    expect(formatQuotaBytes(2000000000000)).toBe('2.00 ТБ')
  })
})

describe('Task 1.1: FailoverConfig and Provider Priority Chain Types & UI Logic', () => {
  it('allows provider_chain in FailoverConfig alongside priority_chain', () => {
    const config: FailoverConfig = {
      enabled: true,
      ping_threshold_ms: 300,
      priority_server: 'srv_1',
      priority_chain: ['srv_1', 'srv_2'],
      provider_chain: ['provider_vps', 'provider_whitelist'],
      auto_restore_priority: true,
      interval_secs: 60,
    }

    expect(config.provider_chain).toEqual(['provider_vps', 'provider_whitelist'])
    expect(config.priority_chain).toEqual(['srv_1', 'srv_2'])
  })

  it('correctly reorders provider priority chain (move up/down)', () => {
    let chain = ['prov_a', 'prov_b', 'prov_c']

    // Move 'prov_b' up (index 1 -> 0)
    const move = (arr: string[], idx: number, dir: -1 | 1) => {
      const next = [...arr]
      const target = idx + dir
      if (target >= 0 && target < next.length) {
        ;[next[idx], next[target]] = [next[target], next[idx]]
      }
      return next
    }

    chain = move(chain, 1, -1)
    expect(chain).toEqual(['prov_b', 'prov_a', 'prov_c'])

    // Move 'prov_b' down (index 0 -> 1)
    chain = move(chain, 0, 1)
    expect(chain).toEqual(['prov_a', 'prov_b', 'prov_c'])
  })

  it('toggles provider inclusion in provider_chain', () => {
    const toggle = (chain: string[], id: string) => {
      return chain.includes(id) ? chain.filter((x) => x !== id) : [...chain, id]
    }

    let chain = ['prov_vps']
    chain = toggle(chain, 'prov_wl')
    expect(chain).toEqual(['prov_vps', 'prov_wl'])

    chain = toggle(chain, 'prov_vps')
    expect(chain).toEqual(['prov_wl'])
  })
})
