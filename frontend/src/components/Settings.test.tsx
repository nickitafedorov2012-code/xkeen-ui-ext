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

describe('Settings Component — Notifications Guidance & Validation', () => {
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

    vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path === 'settings') {
        return Promise.resolve({
          ...mockSettings,
          notifications: {
            telegram_enabled: true,
            telegram_bot_token: '8959891087:AAHWkeSw_WL6b8k3J9tONrms-7F2NgE2Yfo',
            telegram_chat_id: '123456789',
            webhook_url: '',
          },
        })
      }
      if (path === 'servers') return Promise.resolve({ servers: [] })
      if (path === 'domains') return Promise.resolve({ direct: [], force: [] })
      if (path === 'dns/mode') return Promise.resolve({ enhanced_mode: 'fake-ip' })
      if (path === 'backups') return Promise.resolve([])
      if (path === 'system/swap/status' || path === 'system/swap-status') return Promise.resolve(mockSwapInactive)
      if (path === 'system/geo-info') return Promise.resolve({ geoip: { size: 100, updated_at: '' }, geosite: { size: 100, updated_at: '' } })
      if (path === 'adblock') return Promise.resolve({ enabled: false })
      if (path === 'zapret/status') return Promise.resolve({ installed: false, running: false })
      return Promise.reject(new Error(`Unhandled path: ${path}`))
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

  it('renders help badges (?) for Bot Token, Chat ID, and Webhook URL and toggles instructions', async () => {
    await act(async () => {
      root!.render(<Settings notify={notifyMock} refresh={refreshMock} />)
    })

    const helpButtons = container?.querySelectorAll('.badge-help-btn') as NodeListOf<HTMLButtonElement>
    expect(helpButtons.length).toBe(3)

    // Initially no help-box-tip is visible
    expect(container?.querySelector('.help-box-tip')).toBeNull()

    // Click Bot Token help button
    await act(async () => {
      helpButtons[0].click()
    })

    let tip = container?.querySelector('.help-box-tip')
    expect(tip).not.toBeNull()
    expect(tip?.textContent).toContain('Как получить Bot Token')
    expect(tip?.textContent).toContain('@BotFather')

    // Click again to toggle off
    await act(async () => {
      helpButtons[0].click()
    })
    expect(container?.querySelector('.help-box-tip')).toBeNull()

    // Click Chat ID help button
    await act(async () => {
      helpButtons[1].click()
    })
    tip = container?.querySelector('.help-box-tip')
    expect(tip).not.toBeNull()
    expect(tip?.textContent).toContain('Как узнать ваш Chat ID')
    expect(tip?.textContent).toContain('@userinfobot')
  })

  it('detects when Chat ID matches bot ID and warns user', async () => {
    await act(async () => {
      root!.render(<Settings notify={notifyMock} refresh={refreshMock} />)
    })

    // Find Chat ID input
    const inputs = container?.querySelectorAll('.card input.input') as NodeListOf<HTMLInputElement>
    // Search input with current chat id
    const chatInput = Array.from(inputs).find((inp) => inp.value === '123456789')
    expect(chatInput).toBeDefined()

    // Set Chat ID to the bot ID prefix '8959891087'
    await act(async () => {
      const nativeInputValueSetter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value')?.set
      nativeInputValueSetter?.call(chatInput, '8959891087')
      chatInput!.dispatchEvent(new Event('input', { bubbles: true }))
      chatInput!.dispatchEvent(new Event('change', { bubbles: true }))
    })

    expect(container?.textContent).toContain('Вы указали ID самого бота (8959891087)! Бот не может писать сам себе')
  })

  it('detects when Bot Token is missing bot ID and warns user', async () => {
    await act(async () => {
      root!.render(<Settings notify={notifyMock} refresh={refreshMock} />)
    })

    const inputs = container?.querySelectorAll('.card input.input') as NodeListOf<HTMLInputElement>
    const tokenInput = Array.from(inputs).find((inp) => inp.value.includes('8959891087:'))
    expect(tokenInput).toBeDefined()

    // Cut off bot ID (as in user screenshot 2)
    await act(async () => {
      const nativeInputValueSetter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value')?.set
      nativeInputValueSetter?.call(tokenInput, 'AAHWkeSw_WL6b8k3J9tONrms-7F2NgE2Yfo')
      tokenInput!.dispatchEvent(new Event('input', { bubbles: true }))
      tokenInput!.dispatchEvent(new Event('change', { bubbles: true }))
    })

    expect(container?.textContent).toContain('Токен неполный! В начале обязательно должен быть числовой ID бота и двоеточие')
  })
})

describe('Settings Component — Failover Zombie Servers and Canary Last Result', () => {
  let container: HTMLDivElement | null = null
  let root: ReturnType<typeof createRoot> | null = null
  const notifyMock = vi.fn()
  const refreshMock = vi.fn().mockResolvedValue(undefined)

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

  it('renders 🧟 Зомби badge and last test banner when candidate failed L7 test', async () => {
    vi.spyOn(api, 'apiGet').mockImplementation(async (path: string) => {
      if (path === 'settings') {
        return {
          ...mockSettings,
          failover: {
            ...mockSettings.failover,
            priority_chain: ['germany_node', 'backup_node'],
          },
        }
      }
      if (path === 'servers') {
        return {
          servers: [
            { id: 'germany_node', name: 'Германия', ping_ms: 120, is_active: false },
            { id: 'backup_node', name: 'Резерв Нидерланды', ping_ms: 85, is_active: true },
          ],
        }
      }
      if (path === 'routes') return { routes: [] }
      if (path === 'domains') return { direct: [], force: [] }
      if (path === 'system/swap/status') return mockSwapInactive
      return {}
    })

    const mockStatus: any = {
      active_server: { id: 'backup_node', name: 'Резерв Нидерланды', ping_ms: 85 },
      failover: {
        enabled: true,
        priority_chain: ['germany_node', 'backup_node'],
        priority_chain_pings: { germany_node: -1, backup_node: 85 },
        zombie_servers: ['germany_node'],
        last_test: {
          timestamp: Math.floor(Date.now() / 1000),
          target_id: 'germany_node',
          target_name: 'Германия',
          success: false,
          message: 'L7 Canary test failed (HTTP probe timeout / zero bytes)',
          elapsed_secs: 5,
        },
      },
    }

    await act(async () => {
      root!.render(<Settings notify={notifyMock} status={mockStatus} refresh={refreshMock} />)
    })

    const text = container?.textContent || ''
    // Banner displays failure info
    expect(text).toContain('L7-тест провален:')
    expect(text).toContain('Германия')
    expect(text).toContain('L7 Canary test failed')

    // Zombie server has 🧟 Зомби badge
    expect(text).toContain('🧟 Зомби')

    // Tooltip includes explanation
    const zombieElements = container?.querySelectorAll('[title*="L7-сбой передачи данных"]')
    expect(zombieElements && zombieElements.length).toBeGreaterThan(0)
  })
})



