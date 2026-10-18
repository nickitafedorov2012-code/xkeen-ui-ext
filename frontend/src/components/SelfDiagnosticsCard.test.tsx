import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import SelfDiagnosticsCard from './SelfDiagnosticsCard'
import * as api from '../api'
import type { SystemDiagnostics } from '../types'

// @ts-ignore
globalThis.IS_REACT_ACT_ENVIRONMENT = true

const mockHealthyDiag: SystemDiagnostics = {
  status: 'healthy',
  components: {
    mihomo: {
      installed: true,
      running: true,
      version: 'v1.19.32',
      tun_active: false,
      pid: 1234,
    },
    zapret: {
      installed: true,
      running: true,
      version: 'v1.0.5.2',
      engine: 'v2',
      binaries_found: ['nfqws1', 'nfqws2'],
      missing: [],
    },
    adguard: {
      installed: true,
      running: true,
      dns_port: 5354,
      loop_detected: false,
      pid: 5678,
    },
  },
  iptables: {
    redirect_active: false,
    adguard_chain_active: true,
    zapret_chain_active: true,
  },
  storage: {
    opt_mounted: true,
    opt_rw: true,
    free_space_mb: 40960,
    total_space_mb: 61440,
  },
  listening_ports: {
    '1001': 'xkeen-route',
    '5354': 'AdGuardHome',
    '7890': 'mihomo-mixed',
  },
  system_summary: {
    cpu_total_percent: 2.5,
    ram_used_mb: 180,
    ram_total_mb: 512,
    uptime_seconds: 12000,
  },
  timestamp: '2026-10-18T00:00:00Z',
}

describe('SelfDiagnosticsCard Component', () => {
  let container: HTMLDivElement | null = null
  let root: ReturnType<typeof createRoot> | null = null

  beforeEach(() => {
    container = document.createElement('div')
    document.body.appendChild(container)
    root = createRoot(container)
    vi.restoreAllMocks()
  })

  afterEach(() => {
    if (root && container) {
      act(() => {
        root!.unmount()
      })
      container.remove()
    }
  })

  it('renders healthy status with all 4 component cards', async () => {
    vi.spyOn(api, 'apiGet').mockImplementation(async (path: string) => {
      if (path === 'system/diagnostics') return mockHealthyDiag as any
      return {} as any
    })

    await act(async () => {
      root!.render(<SelfDiagnosticsCard notify={vi.fn()} />)
    })

    expect(container!.textContent).toContain('Самодиагностика в один клик')
    expect(container!.textContent).toContain('Все системы работают штатно')
    expect(container!.textContent).toContain('Ядро Mihomo')
    expect(container!.textContent).toContain('v1.19.32')
    expect(container!.textContent).toContain('AdGuard Home')
    expect(container!.textContent).toContain(':5354')
    expect(container!.textContent).toContain('Zapret (DPI)')
    expect(container!.textContent).toContain('nfqws1, nfqws2')
    expect(container!.textContent).toContain('Накопитель /opt')
    expect(container!.textContent).toContain('40.0 ГБ')
  })

  it('renders loop warning and invokes healing on click', async () => {
    const notifyMock = vi.fn()
    const loopDiag: SystemDiagnostics = {
      ...mockHealthyDiag,
      status: 'degraded',
      components: {
        ...mockHealthyDiag.components,
        adguard: {
          ...mockHealthyDiag.components.adguard,
          loop_detected: true,
        },
      },
    }

    vi.spyOn(api, 'apiGet').mockImplementation(async (path: string) => {
      if (path === 'system/diagnostics') return loopDiag as any
      return {} as any
    })
    const postSpy = vi.spyOn(api, 'apiPost').mockResolvedValue({ success: true } as any)

    await act(async () => {
      root!.render(<SelfDiagnosticsCard notify={notifyMock} />)
    })

    expect(container!.textContent).toContain('Обнаружена циклическая петля DNS!')

    const fixBtn = container!.querySelector('button.btn-danger') as HTMLButtonElement
    expect(fixBtn).not.toBeNull()

    await act(async () => {
      fixBtn.click()
    })

    expect(postSpy).toHaveBeenCalledWith('adguard/sync-firewall')
    expect(notifyMock).toHaveBeenCalledWith('Сетевые правила DNS переприменены')
  })

  it('renders stopped Mihomo alert and allows one-click start', async () => {
    const notifyMock = vi.fn()
    const stoppedDiag: SystemDiagnostics = {
      ...mockHealthyDiag,
      status: 'degraded',
      components: {
        ...mockHealthyDiag.components,
        mihomo: {
          ...mockHealthyDiag.components.mihomo,
          running: false,
        },
      },
    }

    vi.spyOn(api, 'apiGet').mockImplementation(async (path: string) => {
      if (path === 'system/diagnostics') return stoppedDiag as any
      return {} as any
    })
    const postSpy = vi.spyOn(api, 'apiPost').mockResolvedValue({ success: true } as any)

    await act(async () => {
      root!.render(<SelfDiagnosticsCard notify={notifyMock} />)
    })

    expect(container!.textContent).toContain('Служба Mihomo остановлена')

    const startBtn = container!.querySelector('button.btn-warning') as HTMLButtonElement
    expect(startBtn).not.toBeNull()

    await act(async () => {
      startBtn.click()
    })

    expect(postSpy).toHaveBeenCalledWith('service/start')
    expect(notifyMock).toHaveBeenCalledWith('Команда запуска ядра Mihomo отправлена')
  })

  it('renders read-only storage alert when opt_rw is false', async () => {
    const roDiag: SystemDiagnostics = {
      ...mockHealthyDiag,
      status: 'error',
      storage: {
        ...mockHealthyDiag.storage,
        opt_rw: false,
      },
    }

    vi.spyOn(api, 'apiGet').mockImplementation(async (path: string) => {
      if (path === 'system/diagnostics') return roDiag as any
      return {} as any
    })

    await act(async () => {
      root!.render(<SelfDiagnosticsCard notify={vi.fn()} />)
    })

    expect(container!.textContent).toContain('Диск /opt смонтирован в режиме только для чтения')
  })
})
