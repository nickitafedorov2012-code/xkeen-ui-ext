import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import Dashboard from './Dashboard'
import * as api from '../api'

// @ts-ignore
globalThis.IS_REACT_ACT_ENVIRONMENT = true

const baseStatus: any = {
  version: '1.2.0',
  uptime_seconds: 3600,
  mihomo_running: true,
  refresh_interval_sec: 10,
  active_server: {
    id: 'srv_main_vps',
    name: 'Main VPS Node',
    protocol: 'vless',
    host: '1.2.3.4',
    port: 443,
    ping_ms: 45,
  },
  router: {
    model: 'Keenetic Hero 4G+',
    version: '4.2.1',
    memory_free_mb: 120,
    memory_total_mb: 256,
  },
  failover: {
    enabled: true,
    ping_threshold_ms: 300,
    priority_server: 'srv_main_vps',
    priority_chain: ['srv_main_vps', 'srv_backup_bs'],
    auto_restore_priority: true,
    interval_secs: 15,
    active_tier: 1,
    smart_failover_enabled: true,
    whitelist_episode_bytes: 0,
    zombie_servers: [],
  },
}

describe('Dashboard Component — Failover Card 10-Point Audit & Tier Modernization', () => {
  let container: HTMLDivElement | null = null
  let root: ReturnType<typeof createRoot> | null = null
  const notifyMock = vi.fn()
  const refreshMock = vi.fn().mockResolvedValue(undefined)

  beforeEach(() => {
    container = document.createElement('div')
    document.body.appendChild(container)
    root = createRoot(container)
    vi.restoreAllMocks()
    notifyMock.mockClear()
    refreshMock.mockClear()

    vi.spyOn(api, 'apiGet').mockImplementation(async (path: string) => {
      if (path === 'failover/events') return { events: [] }
      if (path === 'devices') return { devices: [] }
      if (path === '/adguard/status') return { running: false, protection_enabled: false }
      return {}
    })
  })

  afterEach(() => {
    if (root && container) {
      act(() => {
        root!.unmount()
      })
      container.remove()
    }
  })

  it('renders Active Server Tier 1 (VPS) badge and eradicates duplicate status text', async () => {
    await act(async () => {
      root!.render(<Dashboard status={baseStatus} notify={notifyMock} refresh={refreshMock} />)
    })

    const badge = container?.querySelector('[data-testid="failover-tier-badge"]')
    expect(badge).toBeTruthy()
    expect(badge?.textContent).toContain('Tier 1 (VPS)')

    // Duplicate text badge "🟢 включён" / "⚪ выключен" must NOT be in the card header
    const cardHeader = container?.querySelector('.card h2')?.parentElement
    expect(cardHeader?.textContent).not.toContain('🟢 включён')
    expect(cardHeader?.textContent).not.toContain('⚪ выключен')
  })

  it('renders Active Server Tier 2 (БС) badge with warning styling when on backup node', async () => {
    const tier2Status: any = {
      ...baseStatus,
      failover: {
        ...baseStatus.failover!,
        active_tier: 2,
        whitelist_episode_bytes: 14500000,
      },
    }

    await act(async () => {
      root!.render(<Dashboard status={tier2Status} notify={notifyMock} refresh={refreshMock} />)
    })

    const badge = container?.querySelector('[data-testid="failover-tier-badge"]')
    expect(badge).toBeTruthy()
    expect(badge?.textContent).toContain('Tier 2 (БС)')

    // Check honest Whitelist episode traffic row
    const wlTraffic = container?.querySelector('[data-testid="dashboard-whitelist-traffic"]')
    expect(wlTraffic).toBeTruthy()
    expect(wlTraffic?.textContent).toContain('13.8 МБ за сессию')
  })

  it('toggles Smart Failover switch and dispatches apiPut to settings', async () => {
    const putSpy = vi.spyOn(api, 'apiPut').mockResolvedValue({ success: true })

    await act(async () => {
      root!.render(<Dashboard status={baseStatus} notify={notifyMock} refresh={refreshMock} />)
    })

    const toggle = container?.querySelector('[data-testid="dashboard-smart-failover-toggle"]') as HTMLInputElement
    expect(toggle).toBeTruthy()
    expect(toggle.checked).toBe(true)

    await act(async () => {
      toggle.click()
    })

    expect(putSpy).toHaveBeenCalledWith('settings', {
      failover: { smart_failover_enabled: false },
    })
  })

  it('eradicates generic reserves counter and fake (10 мин) zombie counter', async () => {
    const zombieStatus: any = {
      ...baseStatus,
      failover: {
        ...baseStatus.failover!,
        zombie_servers: ['srv_zombie_1', 'srv_zombie_2'],
      },
    }

    await act(async () => {
      root!.render(<Dashboard status={zombieStatus} notify={notifyMock} refresh={refreshMock} />)
    })

    const text = container?.textContent || ''

    // 1. Generic counter row "Резервы: X сервер(ов)" must be absent
    expect(text).not.toContain('сервер(ов)')

    // 2. Fake "(10 мин)" must be completely absent from zombie counter
    expect(text).not.toContain('(10 мин)')

    // 3. Clean zombie badge present
    const zombieCount = container?.querySelector('[data-testid="dashboard-zombie-count"]')
    expect(zombieCount).toBeTruthy()
    expect(zombieCount?.textContent).toContain('🧟 2 в карантине')
  })

  it('does not render Whitelist episode traffic row when on Tier 1 with 0 bytes', async () => {
    await act(async () => {
      root!.render(<Dashboard status={baseStatus} notify={notifyMock} refresh={refreshMock} />)
    })

    const wlTraffic = container?.querySelector('[data-testid="dashboard-whitelist-traffic"]')
    expect(wlTraffic).toBeNull()
  })
})
