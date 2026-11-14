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

  it('renders Whitelist Detector widget with honest cards, 1-click toggle, manual check and classifier modal', async () => {
    const detectorStatus: any = {
      ...baseStatus,
      failover: {
        ...baseStatus.failover!,
        whitelist_detector: {
          enabled: true,
          monitored_provider: 'Geodema',
          target_bs_provider: 'BS-Special',
          threshold_mode: 'percent',
          trigger_offline_percent: 80,
          trigger_offline_count: 75,
          max_acceptable_ping_ms: 450,
          exit_offline_percent: 45,
          exit_offline_count: 48,
          check_interval_mins: 10,
          auto_restore_vps: true,
        },
        whitelist_detector_status: {
          state: 'Normal',
          monitored_provider: 'Geodema',
          total_nodes: 50,
          offline_nodes: 5,
          online_nodes: 45,
          offline_percent: 10,
          last_checked: '2026-11-01 23:00',
          last_event: null,
          check_interval_mins: 10,
        },
      },
    }

    const checkSpy = vi.spyOn(api, 'runWhitelistDetectorCheck').mockResolvedValue({
      message: 'Детектор БС: статус Normal, оффлайн 10% (5/50 узлов)',
    })

    await act(async () => {
      root!.render(<Dashboard status={detectorStatus} notify={notifyMock} refresh={refreshMock} />)
    })

    // 1. Widget is rendered
    const widget = container?.querySelector('[data-testid="dashboard-whitelist-detector-widget"]')
    expect(widget).toBeTruthy()

    // 2. Toggle button in header
    const toggleBtn = container?.querySelector('[data-testid="detector-toggle-btn"]')
    expect(toggleBtn?.textContent).toContain('Авто: ВКЛ 🟢')

    // 3. Honest interval card shows minutes (10 мин), not 15s
    const intervalCard = container?.querySelector('[data-testid="card-detector-interval"]')
    expect(intervalCard?.textContent).toContain('10 мин')
    expect(intervalCard?.textContent).not.toContain('15 сек')

    // 4. Foreign health card shows nodes and percent
    const healthCard = container?.querySelector('[data-testid="card-foreign-servers-health"]')
    expect(healthCard?.textContent).toContain('45 из 50 онлайн (10% оффлайн')

    // 4b. When monitored_total_nodes > total_nodes, verifies slice format
    const detectorWithSlice: any = {
      ...detectorStatus,
      failover: {
        ...detectorStatus.failover,
        whitelist_detector_status: {
          ...detectorStatus.failover.whitelist_detector_status,
          total_nodes: 24,
          monitored_total_nodes: 95,
          online_nodes: 22,
          offline_percent: 8,
        },
      },
    }
    await act(async () => {
      root!.render(<Dashboard status={detectorWithSlice} notify={notifyMock} refresh={refreshMock} />)
    })
    const healthCardSlice = container?.querySelector('[data-testid="card-foreign-servers-health"]')
    expect(healthCardSlice?.textContent).toContain('22 из 24 онлайн (срез из 95 узлов, 8% оффлайн)')

    // 5. Check now button triggers runWhitelistDetectorCheck
    const checkBtn = container?.querySelector('[data-testid="detector-check-now-btn"]') as HTMLButtonElement
    expect(checkBtn).toBeTruthy()
    await act(async () => {
      checkBtn.click()
    })
    expect(checkSpy).toHaveBeenCalled()

    // 6. Open Classifier modal
    const classifyBtn = container?.querySelector('[data-testid="detector-classify-modal-btn"]') as HTMLButtonElement
    expect(classifyBtn).toBeTruthy()
    await act(async () => {
      classifyBtn.click()
    })
    const modal = container?.querySelector('[data-testid="detector-classifier-modal"]')
    expect(modal).toBeTruthy()
    expect(modal?.textContent).toContain('Автоопределение серверов и подписок БС')
  })

  it('does not flip AdGuard toggle state and notifies error when /adguard/protection rejects', async () => {
    vi.spyOn(api, 'apiGet').mockImplementation(async (path: string) => {
      if (path === 'failover/events') return { events: [] }
      if (path === 'devices') return { devices: [] }
      if (path === '/adguard/status') return { running: true, protection_enabled: false }
      return {}
    })
    const postSpy = vi.spyOn(api, 'apiPost').mockRejectedValue(new Error('iptables failed'))

    await act(async () => {
      root!.render(
        <Dashboard status={{ ...baseStatus, adblock: false }} notify={notifyMock} refresh={refreshMock} />,
      )
    })

    const aghBtn = container?.querySelector('[data-testid="dashboard-adguard-btn"]') as HTMLButtonElement
    expect(aghBtn).toBeTruthy()
    expect(aghBtn.textContent).toContain('ВЫКЛ')

    await act(async () => {
      aghBtn.click()
    })

    expect(postSpy).toHaveBeenCalledWith('/adguard/protection', { enabled: true })
    expect(notifyMock).toHaveBeenCalledWith('Ошибка переключения AdGuard: iptables failed', true)
    expect(aghBtn.textContent).toContain('ВЫКЛ')
    expect(aghBtn.textContent).not.toContain('ВКЛ 🟢')
  })
})
