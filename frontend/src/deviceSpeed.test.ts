import { describe, it, expect, beforeEach } from 'vitest'

describe('Device speed calculation and unit conversions', () => {
  it('converts kbps to mbps for display', () => {
    const toMbps = (kbps?: number) => {
      if (!kbps || kbps <= 0) return ''
      const mbps = kbps / 1024
      return kbps % 1024 === 0 ? String(mbps) : mbps.toFixed(1)
    }

    expect(toMbps(undefined)).toBe('')
    expect(toMbps(0)).toBe('')
    expect(toMbps(1024)).toBe('1')
    expect(toMbps(10240)).toBe('10')
    expect(toMbps(30720)).toBe('30')
    expect(toMbps(102400)).toBe('100')
    expect(toMbps(512)).toBe('0.5')
    expect(toMbps(25600)).toBe('25')
  })

  it('converts user entered mbps string to kbps for API', () => {
    const toKbps = (val: string) => {
      const trimmed = val.trim()
      if (!trimmed || trimmed === '0') return 0
      const num = parseFloat(trimmed.replace(',', '.'))
      if (!isNaN(num) && num > 0) {
        return Math.round(num * 1024)
      }
      return 0
    }

    expect(toKbps('')).toBe(0)
    expect(toKbps('0')).toBe(0)
    expect(toKbps('10')).toBe(10240)
    expect(toKbps('35')).toBe(35840)
    expect(toKbps('0.5')).toBe(512)
    expect(toKbps('1,5')).toBe(1536)
    expect(toKbps('100')).toBe(102400)
    expect(toKbps('invalid')).toBe(0)
  })
})

describe('Flow server localStorage persistence', () => {
  beforeEach(() => {
    localStorage.clear()
  })

  it('persists and restores selected flow server', () => {
    expect(localStorage.getItem('xr_flow_server')).toBeNull()
    localStorage.setItem('xr_flow_server', 'node-nl-vless')
    expect(localStorage.getItem('xr_flow_server')).toBe('node-nl-vless')
  })

  it('persists and restores google geo check cache', () => {
    const geo = {
      is_clean: true,
      google_country: 'NL',
      google_domain: 'google.com',
      server_name: 'Нидерланды',
    }
    localStorage.setItem('xr_google_geo', JSON.stringify(geo))
    const parsed = JSON.parse(localStorage.getItem('xr_google_geo') || '{}')
    expect(parsed.is_clean).toBe(true)
    expect(parsed.google_country).toBe('NL')
  })
})
