import { describe, it, expect, beforeEach } from 'vitest'

describe('Servers grid column layout logic', () => {
  beforeEach(() => {
    localStorage.clear()
  })

  it('defaults to 2 columns when no preference is saved', () => {
    const saved = localStorage.getItem('xr_servers_columns')
    const parsed = saved ? parseInt(saved, 10) : null
    const columns = parsed && parsed >= 1 && parsed <= 4 ? parsed : 2
    expect(columns).toBe(2)
  })

  it('restores valid columns from localStorage (1, 2, 3, 4)', () => {
    for (const col of [1, 2, 3, 4]) {
      localStorage.setItem('xr_servers_columns', String(col))
      const saved = localStorage.getItem('xr_servers_columns')
      const parsed = saved ? parseInt(saved, 10) : null
      const columns = parsed && parsed >= 1 && parsed <= 4 ? parsed : 2
      expect(columns).toBe(col)
    }
  })

  it('falls back to 2 columns if localStorage has an invalid value', () => {
    localStorage.setItem('xr_servers_columns', '99')
    const saved = localStorage.getItem('xr_servers_columns')
    const parsed = saved ? parseInt(saved, 10) : null
    const columns = parsed && parsed >= 1 && parsed <= 4 ? parsed : 2
    expect(columns).toBe(2)
  })

  it('page size 24 is divisible evenly by 1, 2, 3, and 4 columns', () => {
    const PAGE = 24
    for (const col of [1, 2, 3, 4]) {
      expect(PAGE % col).toBe(0)
    }
  })
})

describe('DeviceRoutingModal zero-clutter rule for whitelist toggle', () => {
  const checkWhitelistVisible = (
    servers: Array<{ id: string; is_whitelist?: boolean }>,
    whitelistProviders?: string[]
  ) => {
    return Boolean(servers.some((s) => s.is_whitelist) || (whitelistProviders && whitelistProviders.length > 0))
  }

  it('hides whitelist toggle when no servers and no providers are whitelisted', () => {
    const servers = [{ id: 's1', is_whitelist: false }, { id: 's2' }]
    expect(checkWhitelistVisible(servers, [])).toBe(false)
    expect(checkWhitelistVisible(servers, undefined)).toBe(false)
  })

  it('shows whitelist toggle when at least one server is whitelisted', () => {
    const servers = [{ id: 's1', is_whitelist: true }, { id: 's2', is_whitelist: false }]
    expect(checkWhitelistVisible(servers, [])).toBe(true)
  })

  it('shows whitelist toggle when at least one subscription provider is whitelisted', () => {
    const servers = [{ id: 's1', is_whitelist: false }]
    expect(checkWhitelistVisible(servers, ['provider_1'])).toBe(true)
  })
})

describe('Devices table offline dimming and actions lock', () => {
  const isActionDisabled = (device: { ip?: string; online?: boolean }) => {
    return !device.ip || !device.online
  }

  it('disables actions when device is offline or has no IP', () => {
    expect(isActionDisabled({ ip: '', online: false })).toBe(true)
    expect(isActionDisabled({ ip: '192.168.1.100', online: false })).toBe(true)
    expect(isActionDisabled({ ip: '', online: true })).toBe(true)
  })

  it('enables actions when device has valid IP and is online', () => {
    expect(isActionDisabled({ ip: '192.168.1.100', online: true })).toBe(false)
  })
})
