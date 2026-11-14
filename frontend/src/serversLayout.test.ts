import { describe, it, expect, beforeEach } from 'vitest'
import {
  SERVERS_COLUMNS_KEY,
  SERVERS_PAGE_SIZE,
  loadServersColumns,
  saveServersColumns,
  isWhitelistToggleVisible,
  isDeviceActionDisabled,
  getSignalBarsInfo,
  getWhitelistButtonLabels,
} from './utils/serversLayout'

describe('Servers grid column layout logic', () => {
  beforeEach(() => {
    localStorage.clear()
  })

  it('defaults to 0 (auto-fill) when no preference is saved', () => {
    expect(loadServersColumns()).toBe(0)
  })

  it('restores valid columns from localStorage (0, 1, 2, 3, 4)', () => {
    for (const col of [0, 1, 2, 3, 4]) {
      saveServersColumns(col)
      expect(localStorage.getItem(SERVERS_COLUMNS_KEY)).toBe(String(col))
      expect(loadServersColumns()).toBe(col)
    }
  })

  it('falls back to 0 if localStorage has an invalid value', () => {
    localStorage.setItem(SERVERS_COLUMNS_KEY, '99')
    expect(loadServersColumns()).toBe(0)
    localStorage.setItem(SERVERS_COLUMNS_KEY, '-1')
    expect(loadServersColumns()).toBe(0)
  })

  it('page size 24 is divisible evenly by 1, 2, 3, and 4 columns', () => {
    for (const col of [1, 2, 3, 4]) {
      expect(SERVERS_PAGE_SIZE % col).toBe(0)
    }
  })
})

describe('DeviceRoutingModal zero-clutter rule for whitelist toggle', () => {
  it('hides whitelist toggle when no servers and no providers are whitelisted', () => {
    const servers = [{ id: 's1', is_whitelist: false }, { id: 's2' }]
    expect(isWhitelistToggleVisible(servers, [])).toBe(false)
    expect(isWhitelistToggleVisible(servers, undefined)).toBe(false)
  })

  it('shows whitelist toggle when at least one server is whitelisted', () => {
    const servers = [{ id: 's1', is_whitelist: true }, { id: 's2', is_whitelist: false }]
    expect(isWhitelistToggleVisible(servers, [])).toBe(true)
  })

  it('shows whitelist toggle when at least one subscription provider is whitelisted', () => {
    const servers = [{ id: 's1', is_whitelist: false }]
    expect(isWhitelistToggleVisible(servers, ['provider_1'])).toBe(true)
  })
})

describe('Devices table offline dimming and actions lock', () => {
  it('disables actions when device is offline or has no IP', () => {
    expect(isDeviceActionDisabled({ ip: '', online: false })).toBe(true)
    expect(isDeviceActionDisabled({ ip: '192.168.1.100', online: false })).toBe(true)
    expect(isDeviceActionDisabled({ ip: '', online: true })).toBe(true)
  })

  it('enables actions when device has valid IP and is online', () => {
    expect(isDeviceActionDisabled({ ip: '192.168.1.100', online: true })).toBe(false)
  })
})

describe('Signal quality bars indicator (Apple-style cellular bars)', () => {
  it('reports timeout when ping is 0 or >= 9999', () => {
    expect(getSignalBarsInfo(0)).toEqual({ active: 0, status: 'timeout' })
    expect(getSignalBarsInfo(9999)).toEqual({ active: 0, status: 'timeout' })
    expect(getSignalBarsInfo(-1)).toEqual({ active: 0, status: 'timeout' })
  })

  it('allocates 4 green bars for ping < 80ms', () => {
    expect(getSignalBarsInfo(45)).toMatchObject({ active: 4, status: 'excellent', color: '#22c55e' })
  })

  it('allocates 3 cyan bars for ping 80..160ms', () => {
    expect(getSignalBarsInfo(120)).toMatchObject({ active: 3, status: 'good', color: '#38bdf8' })
  })

  it('allocates 2 amber bars for ping 160..300ms', () => {
    expect(getSignalBarsInfo(220)).toMatchObject({ active: 2, status: 'medium', color: '#f59e0b' })
  })

  it('allocates 1 red bar for slow ping >= 300ms', () => {
    expect(getSignalBarsInfo(450)).toMatchObject({ active: 1, status: 'slow', color: '#ef4444' })
  })
})

describe('Whitelist button label display per column mode', () => {
  it('shows short label БС in multi-column modes (0, 2, 3, 4) to prevent vanishing labels', () => {
    for (const col of [0, 2, 3, 4]) {
      expect(getWhitelistButtonLabels(col, true).effectiveDisplay).toBe('БС ON')
      expect(getWhitelistButtonLabels(col, false).effectiveDisplay).toBe('БС')
    }
  })

  it('shows full label in 1 column mode', () => {
    expect(getWhitelistButtonLabels(1, true).effectiveDisplay).toBe('В белом списке')
    expect(getWhitelistButtonLabels(1, false).effectiveDisplay).toBe('Белый список')
  })
})
