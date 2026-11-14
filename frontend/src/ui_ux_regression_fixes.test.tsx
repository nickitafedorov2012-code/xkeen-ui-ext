import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { act, type ReactElement } from 'react'
import { createRoot } from 'react-dom/client'
import * as api from './api'
import PoliciesMap from './components/PoliciesMap'
import XboxDns from './components/XboxDns'
import RulesViewer from './components/RulesViewer'
import Diagnostics from './components/Diagnostics'
import { ZapretMainCard } from './components/zapret/ZapretMainCard'
import TaskManager from './components/TaskManager'
import ConnectionsViewer from './components/ConnectionsViewer'
import PresetCatalogModal from './components/PresetCatalogModal'
import ShareNodeModal from './components/ShareNodeModal'
import Dashboard from './components/Dashboard'
import Help from './components/Help'
import Devices from './components/Devices'

// @ts-ignore
globalThis.IS_REACT_ACT_ENVIRONMENT = true

async function flushMicrotasks() {
  await act(async () => {
    await new Promise((r) => setTimeout(r, 0))
  })
}

describe('UI/UX & P0 Crash Anti-Regression Tests', () => {
  let container: HTMLDivElement
  let root: ReturnType<typeof createRoot>

  beforeEach(() => {
    container = document.createElement('div')
    document.body.appendChild(container)
    root = createRoot(container)
  })

  afterEach(async () => {
    await act(async () => {
      root.unmount()
    })
    container.remove()
    vi.restoreAllMocks()
  })

  async function mount(el: ReactElement) {
    await act(async () => {
      root.render(el)
    })
    await flushMicrotasks()
  }

  it('PoliciesMap does not crash on incomplete API response', async () => {
    vi.spyOn(api, 'apiGet').mockResolvedValueOnce({} as any)
    const notify = vi.fn()
    await mount(<PoliciesMap notify={notify} />)
    expect(container.textContent).toContain('Визуальная карта маршрутизации Keenetic')
  })

  it('XboxDns does not crash on check DNS click when results are missing', async () => {
    vi.spyOn(api, 'apiGet').mockResolvedValueOnce({ servers: [] } as any)
    vi.spyOn(api, 'apiPost').mockResolvedValueOnce({ timestamp: '12:00:00' } as any)
    const notify = vi.fn()
    await mount(<XboxDns notify={notify} />)
    const btn = container.querySelector('[data-testid="xbox-dns-check-btn"]') as HTMLButtonElement
    expect(btn).toBeTruthy()
    await act(async () => {
      btn.click()
    })
    await flushMicrotasks()
    expect(notify).toHaveBeenCalledWith('Диагностика DNS-серверов успешно завершена')
  })

  it('RulesViewer does not crash on YouTube chip click when matched_rule is missing', async () => {
    vi.spyOn(api, 'apiGet').mockResolvedValueOnce({ rules: [] } as any)
    vi.spyOn(api, 'apiPost').mockResolvedValueOnce({ target_group: 'PROXY' } as any)
    const notify = vi.fn()
    await mount(<RulesViewer notify={notify} />)
    const chip = Array.from(container.querySelectorAll('button')).find((b) => b.textContent?.includes('YouTube'))!
    expect(chip).toBeTruthy()
    await act(async () => {
      chip.click()
    })
    await flushMicrotasks()
    expect(container.textContent).toContain('PROXY')
  })

  it('Diagnostics does not crash when dnsResult.resolved_ips is missing', async () => {
    vi.spyOn(api, 'apiGet').mockResolvedValueOnce({ status: 'ok', checks: [] } as any)
    vi.spyOn(api, 'apiPost').mockResolvedValueOnce({ domain: 'example.com', is_poisoned: false, http_direct_ok: true } as any)
    const notify = vi.fn()
    await mount(<Diagnostics notify={notify} />)
    const btn = Array.from(container.querySelectorAll('button')).find((b) => b.textContent?.includes('Проверить домен'))!
    expect(btn).toBeTruthy()
    await act(async () => {
      btn.click()
    })
    await flushMicrotasks()
    expect(container.textContent).toContain('Не удалось получить IP')
  })

  it('ZapretMainCard does not crash when testResult youtube/discord are missing and hides empty PID', async () => {
    const handleTestDpi = vi.fn()
    await mount(
      <ZapretMainCard
        status={{ installed: true, running: true, fw_active: false, engine: 'v2' } as any}
        isRunning={true}
        isInstalled={true}
        activeEngine="v2"
        v2Installed={true}
        canRollbackV1={false}
        hardwareHint=""
        targetArchLabel="arm64"
        busy={false}
        pendingKeys={new Set()}
        switchingEngine={false}
        upgradingEngine={false}
        stepLog={null}
        setStepLog={vi.fn()}
        testingDpi={false}
        testResult={{ youtube: undefined, discord: undefined } as any}
        showHostsEditor={false}
        setShowHostsEditor={vi.fn()}
        hostsDraft=""
        setHostsDraft={vi.fn()}
        hostsDirtyRef={{ current: false }}
        savingHosts={false}
        showConfigEditor={false}
        setShowConfigEditor={vi.fn()}
        configDraft=""
        setConfigDraft={vi.fn()}
        configDirtyRef={{ current: false }}
        savingConfig={false}
        handleToggle={vi.fn()}
        handleAction={vi.fn()}
        handleSwitchEngine={vi.fn()}
        handleUpgradeZapret2={vi.fn()}
        handleTestDpi={handleTestDpi}
        handleSaveHosts={vi.fn()}
        handleSaveConfig={vi.fn()}
      />
    )
    expect(container.textContent).not.toMatch(/PID:\s*\)/)
    expect(container.textContent).not.toContain('PID: undefined')
    const testBtn = Array.from(container.querySelectorAll('button')).find((b) => b.textContent?.includes('Тест YouTube & Discord'))!
    expect(testBtn).toBeTruthy()
    await act(async () => {
      testBtn.click()
    })
    expect(handleTestDpi).toHaveBeenCalledTimes(1)
  })

  it('TaskManager renders — instead of undefined КБ when mem_rss_kb is missing', async () => {
    vi.spyOn(api, 'apiGet').mockResolvedValueOnce({
      total_count: 1,
      cpu_percent: 10,
      mem_total_kb: 102400,
      mem_used_kb: 51200,
      uptime_secs: 600,
      processes: [{ pid: 42, ppid: 1, user: 'root', state: 'S', cpu_percent: 1.2, mem_percent: 0.5, threads: 1, name: 'mihomo', command: 'mihomo' }],
    } as any)
    await mount(<TaskManager notify={vi.fn()} />)
    expect(container.textContent).toContain('mihomo')
    expect(container.textContent).not.toMatch(/undefined\s*КБ/i)
  })

  it('ConnectionsViewer does not crash on Активные filter click with incomplete connection data', async () => {
    vi.spyOn(api, 'apiGet').mockImplementation(async (url) => {
      if (url === 'connections') {
        return { connections: [{ id: 'c1', download: 10, upload: 5, start: new Date().toISOString() }] } as any
      }
      return {} as any
    })
    await mount(<ConnectionsViewer notify={vi.fn()} />)
    const activeBtn = Array.from(container.querySelectorAll('button')).find((b) => b.textContent?.includes('Активные'))!
    expect(activeBtn).toBeTruthy()
    await act(async () => {
      activeBtn.click()
    })
    expect(activeBtn.className).toContain('primary')
  })

  it('PresetCatalogModal closes on Escape key', async () => {
    const onClose = vi.fn()
    await mount(<PresetCatalogModal isOpen={true} onClose={onClose} notify={vi.fn()} />)
    await act(async () => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))
    })
    expect(onClose).toHaveBeenCalledTimes(1)
  })

  it('ShareNodeModal closes on Escape key', async () => {
    const onClose = vi.fn()
    await mount(
      <ShareNodeModal
        isOpen={true}
        onClose={onClose}
        notify={vi.fn()}
        server={{ id: 's1', name: 'DE Frankfurt', protocol: 'vless', host: '1.2.3.4', port: 443, ping_ms: 50, is_active: true } as any}
      />
    )
    await act(async () => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))
    })
    expect(onClose).toHaveBeenCalledTimes(1)
  })

  it('Dashboard formats ISO last_test.timestamp into readable local time', async () => {
    vi.spyOn(api, 'apiGet').mockResolvedValue({ servers: [], traffic: {}, providers: [] } as any)
    await mount(
      <Dashboard
        status={{
          xkeen_running: true,
          active_server: 's1',
          failover: {
            enabled: true,
            last_test: { success: true, target_name: 'NL-1', timestamp: '2026-11-14T14:37:14.914Z' },
          },
        } as any}
        notify={vi.fn()}
        refresh={vi.fn()}
      />
    )
    expect(container.textContent).toContain('Последний тест')
    expect(container.textContent).not.toContain('2026-11-14T14:37:14.914Z')
  })

  it('Help section 1 title does not duplicate rocket emoji', async () => {
    await mount(<Help status={null} />)
    expect(container.textContent).not.toMatch(/Раздел 1:\s*🚀/)
    expect(container.textContent).toContain('Раздел 1: Быстрый старт')
  })

  it('Devices does not crash with TypeError when /api/routing or /api/devices return partial objects', async () => {
    vi.spyOn(api, 'apiGet').mockImplementation(async () => ({}) as any)
    await mount(<Devices notify={vi.fn()} />)
    expect(container.textContent).toContain('Нет устройств в сети')
  })
})
