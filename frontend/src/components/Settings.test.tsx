import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import Settings, { SwapManager } from './Settings'
import { SwapRecommendationBanner } from './Dashboard'
import * as api from '../api'
import type { AppSettings, SwapStatus } from '../types'

// @ts-ignore
globalThis.IS_REACT_ACT_ENVIRONMENT = true

const mockSettings: AppSettings = {
  rci: { host: '127.0.0.1', port: 80, login: 'admin', password: '123', use_https: false, token: '' },
  mihomo: { host: '127.0.0.1', port: 9090, secret: '', config_path: '/opt/etc/mihomo/config.yaml', device_providers: [] },
  failover: {
    enabled: true,
    ping_threshold_ms: 300,
    priority_server: 'node_de',
    priority_chain: ['node_de', 'node_nl'],
    auto_restore_priority: true,
    interval_secs: 60,
    device_failover_enabled: false,
    best_ping_enabled: false,
    best_ping_pool_size: 15,
    best_ping_hysteresis_ms: 40,
  },
  refresh_interval_sec: 10,
  system: { xkeen_init: '/opt/etc/init.d/S99xkeen', backup_dir: '/opt/var/backups' },
  logs: { level: 'info', remote_syslog: '', log_requests: false },
}

const mockSwapInactive: SwapStatus = {
  swap_supported: true,
  swap_active: false,
  swap_total_mb: 0,
  swap_used_mb: 0,
  swap_free_mb: 0,
  swap_file_exists: false,
  swap_file_path: '/opt/swapfile',
  swappiness: 10,
  opt_disk_free_mb: 1200,
  opt_disk_total_mb: 2000,
  ram_total_mb: 256,
  ram_free_mb: 24,
  recommend_swap: true,
}

const mockSwapActive: SwapStatus = {
  swap_supported: true,
  swap_active: true,
  swap_total_mb: 512,
  swap_used_mb: 64,
  swap_free_mb: 448,
  swap_file_exists: true,
  swap_file_path: '/opt/swapfile',
  swappiness: 10,
  opt_disk_free_mb: 688,
  opt_disk_total_mb: 2000,
  ram_total_mb: 256,
  ram_free_mb: 48,
  recommend_swap: false,
}

describe('Settings Component — Best-Ping Failover Configuration', () => {
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
  })

  afterEach(() => {
    if (root && container) {
      act(() => {
        root!.unmount()
      })
      container.remove()
    }
  })

  it('renders Best-Ping toggle switch and updates state when toggled', async () => {
    vi.spyOn(api, 'apiGet').mockImplementation(async (path: string) => {
      if (path === 'settings') return mockSettings
      if (path === 'servers') return { servers: [] }
      if (path === 'routes') return { routes: [] }
      if (path === 'domains') return { direct: [], force: [] }
      if (path === 'system/swap/status') return mockSwapInactive
      return {}
    })
    const putSpy = vi.spyOn(api, 'apiPut').mockResolvedValue({ success: true })

    await act(async () => {
      root!.render(<Settings notify={notifyMock} status={null} refresh={refreshMock} />)
    })

    const toggle = container?.querySelector('[data-testid="settings-best-ping-toggle"]') as HTMLInputElement | null
    expect(toggle).not.toBeNull()
    expect(toggle?.checked).toBe(false)

    // Toggle Best-Ping to true
    await act(async () => {
      toggle?.click()
    })

    expect(toggle?.checked).toBe(true)

    // Trigger auto-save debounce (400ms)
    await act(async () => {
      await new Promise((r) => setTimeout(r, 500))
    })

    expect(putSpy).toHaveBeenCalled()
    const lastCall = putSpy.mock.calls[putSpy.mock.calls.length - 1]
    expect(lastCall[0]).toBe('settings')
    expect((lastCall[1] as any).failover.best_ping_enabled).toBe(true)
  })
})

describe('SwapManager Component — Swap in 1 Click on USB', () => {
  let container: HTMLDivElement | null = null
  let root: ReturnType<typeof createRoot> | null = null
  const notifyMock = vi.fn()

  beforeEach(() => {
    container = document.createElement('div')
    document.body.appendChild(container)
    root = createRoot(container)
    vi.restoreAllMocks()
    notifyMock.mockClear()
  })

  afterEach(() => {
    if (root && container) {
      act(() => {
        root!.unmount()
      })
      container.remove()
    }
  })

  it('renders inactive status and allows creating swap with selected preset', async () => {
    vi.spyOn(api, 'apiGet').mockImplementation(async (path: string) => {
      if (path === 'system/swap/status') return mockSwapInactive
      return {}
    })
    const postSpy = vi.spyOn(api, 'apiPost').mockResolvedValue(mockSwapActive)

    await act(async () => {
      root!.render(<SwapManager notify={notifyMock} />)
    })

    const badge = container?.querySelector('[data-testid="swap-status-badge"]')
    expect(badge?.textContent).toContain('Swap выключен')

    const createBtn = container?.querySelector('[data-testid="swap-create-button"]') as HTMLButtonElement | null
    expect(createBtn).not.toBeNull()
    expect(createBtn?.textContent).toContain('512 МБ')

    // Click 1024 MB preset
    const preset1024 = container?.querySelector('[data-testid="swap-preset-1024"]') as HTMLButtonElement | null
    expect(preset1024).not.toBeNull()
    await act(async () => {
      preset1024?.click()
    })

    expect(createBtn?.textContent).toContain('1024 МБ')

    // Click create button
    await act(async () => {
      createBtn?.click()
    })

    expect(postSpy).toHaveBeenCalledWith('system/swap/create', { size_mb: 1024 })
    expect(notifyMock).toHaveBeenCalled()
  })

  it('renders active status and allows removing swap', async () => {
    vi.spyOn(api, 'apiGet').mockImplementation(async (path: string) => {
      if (path === 'system/swap/status') return mockSwapActive
      return {}
    })
    const postSpy = vi.spyOn(api, 'apiPost').mockResolvedValue(mockSwapInactive)
    vi.spyOn(window, 'confirm').mockReturnValue(true)

    await act(async () => {
      root!.render(<SwapManager notify={notifyMock} />)
    })

    const badge = container?.querySelector('[data-testid="swap-status-badge"]')
    expect(badge?.textContent).toContain('Swap активен')

    const removeBtn = container?.querySelector('[data-testid="swap-remove-button"]') as HTMLButtonElement | null
    expect(removeBtn).not.toBeNull()

    await act(async () => {
      removeBtn?.click()
    })

    expect(postSpy).toHaveBeenCalledWith('system/swap/remove', {})
    expect(notifyMock).toHaveBeenCalledWith('Файл подкачки отключен и удален')
  })
})

describe('SwapRecommendationBanner Component', () => {
  let container: HTMLDivElement | null = null
  let root: ReturnType<typeof createRoot> | null = null
  const switchTabMock = vi.fn()

  beforeEach(() => {
    container = document.createElement('div')
    document.body.appendChild(container)
    root = createRoot(container)
    switchTabMock.mockClear()
  })

  afterEach(() => {
    if (root && container) {
      act(() => {
        root!.unmount()
      })
      container.remove()
    }
  })

  it('renders recommendation banner when recommend_swap is true and swap is inactive', async () => {
    await act(async () => {
      root!.render(<SwapRecommendationBanner status={mockSwapInactive} onSwitchTab={switchTabMock} />)
    })

    const banner = container?.querySelector('[data-testid="swap-recommendation-banner"]')
    expect(banner).not.toBeNull()
    expect(banner?.textContent).toContain('Мало оперативной памяти (24 МБ свободно)')

    const btn = container?.querySelector('[data-testid="swap-recommendation-btn"]') as HTMLButtonElement | null
    expect(btn).not.toBeNull()

    await act(async () => {
      btn?.click()
    })

    expect(switchTabMock).toHaveBeenCalledWith('settings')
  })

  it('does not render banner when swap is already active or not recommended', async () => {
    await act(async () => {
      root!.render(<SwapRecommendationBanner status={mockSwapActive} onSwitchTab={switchTabMock} />)
    })

    const banner = container?.querySelector('[data-testid="swap-recommendation-banner"]')
    expect(banner).toBeNull()
  })
})

