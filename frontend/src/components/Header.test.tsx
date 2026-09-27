import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import Header, { is5AmCheckDue } from './Header'
import * as api from '../api'
import type { StatusInfo } from '../types'

// @ts-ignore
globalThis.IS_REACT_ACT_ENVIRONMENT = true

const mockStatus: StatusInfo = {
  version: 'v0.9.5',
  config_path: '/opt/etc/xkeen.json',
  router: { model: 'Titan KN-1811', version: '4.2', uptime: '10d' },
  system: { cpu_percent: 12, memory_used_mb: 180, memory_total_mb: 512 },
  mihomo_version: 'v1.19.0',
  zapret: {
    engine: 'v1',
    version: '72.13',
    label: 'Запрет 1 v72.13',
    installed: true,
    running: true,
    update_available: false,
  },
  active_server: null,
  mihomo: { host: '127.0.0.1', port: 9090 },
  rci: { host: '127.0.0.1', port: 80 },
  failover: {
    enabled: true,
    ping_threshold_ms: 500,
    priority_server: 'srv1',
    auto_restore_priority: true,
    interval_secs: 30,
  },
  refresh_interval_sec: 10,
}

describe('Header Component — Settings Navigation, Zapret 1/2 Pill & 5 AM Update Check', () => {
  let container: HTMLDivElement | null = null
  let root: ReturnType<typeof createRoot> | null = null
  const notifyMock = vi.fn()
  const refreshMock = vi.fn().mockResolvedValue(undefined)
  const onSwitchTabMock = vi.fn()

  beforeEach(() => {
    container = document.createElement('div')
    document.body.appendChild(container)
    root = createRoot(container)
    vi.restoreAllMocks()
    notifyMock.mockClear()
    refreshMock.mockClear()
    onSwitchTabMock.mockClear()
    localStorage.clear()
  })

  afterEach(() => {
    vi.useRealTimers()
    if (root && container) {
      act(() => {
        root!.unmount()
      })
      container.remove()
    }
  })

  it('renders Settings button in top header navigation and switches to settings tab on click', async () => {
    vi.spyOn(api, 'apiGet').mockResolvedValue({})

    await act(async () => {
      root!.render(
        <Header
          status={mockStatus}
          notify={notifyMock}
          refresh={refreshMock}
          onSwitchTab={onSwitchTabMock}
          activeTab="dashboard"
        />
      )
    })

    const settingsBtn = container?.querySelector('[data-testid="header-settings-btn"]') as HTMLButtonElement | null
    expect(settingsBtn).not.toBeNull()
    expect(settingsBtn?.textContent).toContain('⚙️')
    expect(settingsBtn?.classList.contains('active')).toBe(false)

    // Click settings button
    await act(async () => {
      settingsBtn?.click()
    })
    expect(onSwitchTabMock).toHaveBeenCalledWith('settings')
  })

  it('applies active class to settings button when activeTab is settings', async () => {
    vi.spyOn(api, 'apiGet').mockResolvedValue({})

    await act(async () => {
      root!.render(
        <Header
          status={mockStatus}
          notify={notifyMock}
          refresh={refreshMock}
          onSwitchTab={onSwitchTabMock}
          activeTab="settings"
        />
      )
    })

    const settingsBtn = container?.querySelector('[data-testid="header-settings-btn"]') as HTMLButtonElement | null
    expect(settingsBtn?.classList.contains('active')).toBe(true)
  })

  it('renders Zapret 1 chip with version in top header navigation and switches to zapret tab', async () => {
    vi.spyOn(api, 'apiGet').mockResolvedValue({})

    await act(async () => {
      root!.render(
        <Header
          status={mockStatus}
          notify={notifyMock}
          refresh={refreshMock}
          onSwitchTab={onSwitchTabMock}
          activeTab="dashboard"
        />
      )
    })

    const zapretPill = container?.querySelector('[data-testid="header-zapret-pill"]') as HTMLButtonElement | null
    expect(zapretPill).not.toBeNull()
    expect(zapretPill?.textContent).toContain('Запрет 1')
    expect(zapretPill?.textContent).toContain('v72.13')
    expect(zapretPill?.classList.contains('header-pill-update-green')).toBe(false)

    // Click Zapret pill
    await act(async () => {
      zapretPill?.click()
    })
    expect(onSwitchTabMock).toHaveBeenCalledWith('zapret')
  })

  it('renders Zapret 2 chip when engine is v2', async () => {
    vi.spyOn(api, 'apiGet').mockResolvedValue({})

    const statusV2: StatusInfo = {
      ...mockStatus,
      zapret: {
        engine: 'v2',
        version: '1.0.5.2',
        label: 'Запрет 2 v1.0.5.2',
        installed: true,
        running: true,
        update_available: false,
      },
    }

    await act(async () => {
      root!.render(
        <Header
          status={statusV2}
          notify={notifyMock}
          refresh={refreshMock}
          onSwitchTab={onSwitchTabMock}
        />
      )
    })

    const zapretPill = container?.querySelector('[data-testid="header-zapret-pill"]') as HTMLButtonElement | null
    expect(zapretPill?.textContent).toContain('Запрет 2')
    expect(zapretPill?.textContent).toContain('v1.0.5.2')
  })

  it('shows green pulsating animation and badge when Zapret update is available', async () => {
    vi.spyOn(api, 'apiGet').mockResolvedValue({})

    const statusUpdate: StatusInfo = {
      ...mockStatus,
      zapret: {
        engine: 'v1',
        version: '72.13',
        label: 'Запрет 1 v72.13',
        installed: true,
        running: true,
        update_available: true,
        latest_version: 'v72.14',
      },
    }

    await act(async () => {
      root!.render(
        <Header
          status={statusUpdate}
          notify={notifyMock}
          refresh={refreshMock}
          onSwitchTab={onSwitchTabMock}
        />
      )
    })

    const zapretPill = container?.querySelector('[data-testid="header-zapret-pill"]') as HTMLButtonElement | null
    expect(zapretPill?.classList.contains('header-pill-update-green')).toBe(true)

    const badge = container?.querySelector('[data-testid="zapret-update-badge"]')
    expect(badge).not.toBeNull()
    expect(badge?.classList.contains('update-pill-badge-green')).toBe(true)
    expect(badge?.textContent).toContain('72.14')
  })

  it('shows green update badge for Mihomo core and blue update badge for Panel', async () => {
    vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path === 'update/check') {
        return Promise.resolve({
          current: 'v1.5.20',
          latest: 'v1.5.21',
          update_available: true,
        })
      }
      if (path === 'mihomo/releases') {
        return Promise.resolve({
          current_version: 'v1.19.0',
          latest_version: 'v1.19.1',
        })
      }
      return Promise.resolve({})
    })

    await act(async () => {
      root!.render(
        <Header
          status={mockStatus}
          notify={notifyMock}
          refresh={refreshMock}
          onSwitchTab={onSwitchTabMock}
        />
      )
    })

    await act(async () => {
      await new Promise((r) => setTimeout(r, 50))
    })

    const mihomoPill = container?.querySelector('[data-testid="header-mihomo-pill"]') as HTMLButtonElement | null
    expect(mihomoPill).not.toBeNull()
    expect(mihomoPill?.classList.contains('header-pill-update-green')).toBe(true)
    const mihomoBadge = container?.querySelector('[data-testid="mihomo-update-badge"]')
    expect(mihomoBadge?.classList.contains('update-pill-badge-green')).toBe(true)

    const appPill = container?.querySelector('[data-testid="header-app-pill"]') as HTMLButtonElement | null
    expect(appPill).not.toBeNull()
    expect(appPill?.classList.contains('header-pill-update-blue')).toBe(true)
    const appBadge = container?.querySelector('[data-testid="app-update-badge"]')
    expect(appBadge?.classList.contains('update-pill-badge-blue')).toBe(true)
  })

  it('correctly calculates is5AmCheckDue boundary logic', () => {
    // 1. Never checked (0) -> due
    expect(is5AmCheckDue(0)).toBe(true)

    // Pre-NTP router boot clock (year 1970) -> NOT due until NTP clock synchronizes
    const ntpUnsyncedNow = new Date(1970, 0, 1, 0, 0, 0).getTime()
    expect(is5AmCheckDue(0, ntpUnsyncedNow)).toBe(false)

    // Reference time: 2026-09-28 10:00:00 (past 5 AM today)
    const now10am = new Date(2026, 8, 28, 10, 0, 0).getTime()
    const today501am = new Date(2026, 8, 28, 5, 1, 0).getTime()
    const yesterday6pm = new Date(2026, 8, 27, 18, 0, 0).getTime()

    // Checked after 5 AM today -> NOT due
    expect(is5AmCheckDue(today501am, now10am)).toBe(false)
    // Checked before 5 AM today -> DUE
    expect(is5AmCheckDue(yesterday6pm, now10am)).toBe(true)

    // Reference time: 2026-09-28 04:00:00 (before 5 AM today, most recent 5 AM was yesterday 5 AM)
    const now4am = new Date(2026, 8, 28, 4, 0, 0).getTime()
    const yesterday6am = new Date(2026, 8, 27, 6, 0, 0).getTime()
    const dayBeforeYesterday = new Date(2026, 8, 26, 20, 0, 0).getTime()

    // Checked yesterday at 6 AM (after yesterday's 5 AM) -> NOT due
    expect(is5AmCheckDue(yesterday6am, now4am)).toBe(false)
    // Checked before yesterday's 5 AM -> DUE
    expect(is5AmCheckDue(dayBeforeYesterday, now4am)).toBe(true)
  })

  it('triggers 5 AM zapret update check API call when due and updates pill state', async () => {
    // Mock API
    vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path === 'zapret/update/check') {
        return Promise.resolve({
          current_engine: 'v2',
          current_version: '1.0.5.2',
          label: 'Запрет 2 v1.0.5.2',
          latest_version: 'v1.0.5.3',
          update_available: true,
        })
      }
      return Promise.resolve({})
    })

    // Set last check to yesterday
    localStorage.setItem('xr_zapret_last_check', '1000')

    await act(async () => {
      root!.render(
        <Header
          status={mockStatus}
          notify={notifyMock}
          refresh={refreshMock}
          onSwitchTab={onSwitchTabMock}
        />
      )
    })

    // Wait for async effect
    await act(async () => {
      await new Promise((r) => setTimeout(r, 50))
    })

    const zapretPill = container?.querySelector('[data-testid="header-zapret-pill"]') as HTMLButtonElement | null
    expect(zapretPill?.classList.contains('header-pill-update-green')).toBe(true)
    const badge = container?.querySelector('[data-testid="zapret-update-badge"]')
    expect(badge).not.toBeNull()
    expect(badge?.classList.contains('update-pill-badge-green')).toBe(true)
    expect(badge?.textContent).toContain('1.0.5.3')
    expect(localStorage.getItem('xr_zapret_last_check')).not.toBe('1000')
  })

  it('resets green update badge immediately when xr:zapret-updated event is dispatched with update_available false', async () => {
    vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path === 'zapret/update/check') {
        return Promise.resolve({
          current_engine: 'v2',
          current_version: '1.0.5.2',
          label: 'Запрет 2 v1.0.5.2',
          latest_version: 'v1.0.5.3',
          update_available: true,
        })
      }
      return Promise.resolve({})
    })

    localStorage.setItem('xr_zapret_last_check', '1000')

    await act(async () => {
      root!.render(
        <Header
          status={mockStatus}
          notify={notifyMock}
          refresh={refreshMock}
          onSwitchTab={onSwitchTabMock}
        />
      )
    })

    await act(async () => {
      await new Promise((r) => setTimeout(r, 50))
    })

    const zapretPill = container?.querySelector('[data-testid="header-zapret-pill"]') as HTMLButtonElement | null
    expect(zapretPill?.classList.contains('header-pill-update-green')).toBe(true)

    // Dispatch xr:zapret-updated indicating upgrade/update finished
    await act(async () => {
      window.dispatchEvent(
        new CustomEvent('xr:zapret-updated', {
          detail: {
            update_available: false,
            engine: 'v2',
            version: 'v1.0.5.2',
            latest_version: 'v1.0.5.2',
          },
        })
      )
    })

    expect(zapretPill?.classList.contains('header-pill-update-green')).toBe(false)
    expect(container?.querySelector('[data-testid="zapret-update-badge"]')).toBeNull()
  })
})
