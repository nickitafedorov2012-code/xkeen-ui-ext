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

describe('Signal quality bars indicator (Apple-style cellular bars)', () => {
  const getSignalBars = (ping: number) => {
    if (ping <= 0 || ping >= 9999) return { active: 0, status: 'timeout' }
    if (ping < 80) return { active: 4, status: 'excellent', color: '#22c55e' }
    if (ping < 160) return { active: 3, status: 'good', color: '#38bdf8' }
    if (ping < 300) return { active: 2, status: 'medium', color: '#f59e0b' }
    return { active: 1, status: 'slow', color: '#ef4444' }
  }

  it('reports timeout when ping is 0 or >= 9999', () => {
    expect(getSignalBars(0)).toEqual({ active: 0, status: 'timeout' })
    expect(getSignalBars(9999)).toEqual({ active: 0, status: 'timeout' })
    expect(getSignalBars(-1)).toEqual({ active: 0, status: 'timeout' })
  })

  it('allocates 4 green bars for ping < 80ms', () => {
    expect(getSignalBars(45)).toEqual({ active: 4, status: 'excellent', color: '#22c55e' })
  })

  it('allocates 3 cyan bars for ping 80..160ms', () => {
    expect(getSignalBars(120)).toEqual({ active: 3, status: 'good', color: '#38bdf8' })
  })

  it('allocates 2 amber bars for ping 160..300ms', () => {
    expect(getSignalBars(220)).toEqual({ active: 2, status: 'medium', color: '#f59e0b' })
  })

  it('allocates 1 red bar for slow ping >= 300ms', () => {
    expect(getSignalBars(450)).toEqual({ active: 1, status: 'slow', color: '#ef4444' })
  })
})

describe('Whitelist button label display per column mode', () => {
  const getWhitelistButtonLabels = (columns: number, isWhitelisted: boolean) => {
    const isMultiCol = columns === 0 || columns >= 2
    return {
      full: isWhitelisted ? 'В белом списке' : 'Белый список',
      short: isWhitelisted ? 'БС ON' : 'БС',
      effectiveDisplay: isMultiCol ? (isWhitelisted ? 'БС ON' : 'БС') : (isWhitelisted ? 'В белом списке' : 'Белый список'),
    }
  }

  it('shows short label БС in multi-column modes (0, 2, 3, 4) to prevent vanishing labels', () => {
    for (const col of [0, 2, 3, 4]) {
      const active = getWhitelistButtonLabels(col, true)
      expect(active.effectiveDisplay).toBe('БС ON')
      const inactive = getWhitelistButtonLabels(col, false)
      expect(inactive.effectiveDisplay).toBe('БС')
    }
  })

  it('shows full label in 1 column mode', () => {
    const active = getWhitelistButtonLabels(1, true)
    expect(active.effectiveDisplay).toBe('В белом списке')
    const inactive = getWhitelistButtonLabels(1, false)
    expect(inactive.effectiveDisplay).toBe('Белый список')
  })
})
