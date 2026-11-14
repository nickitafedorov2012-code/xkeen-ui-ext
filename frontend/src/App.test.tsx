import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import App from './App'
import * as api from './api'
import { useDirtyGuard, clearAllDirty } from './utils/dirtyGuard'
import { useTabCache, clearTabCache } from './utils/tabCache'

// @ts-ignore
globalThis.IS_REACT_ACT_ENVIRONMENT = true

// Mock heavy sub-components
vi.mock('./components/Dashboard', () => ({
  default: ({ notify }: { notify?: (msg: string, err?: boolean) => void }) => (
    <div data-testid="dashboard-view" onClick={() => notify?.('Тестовое уведомление')}>
      Dashboard View
    </div>
  ),
}))
vi.mock('./components/Servers', () => ({
  default: () => <div data-testid="servers-view">Servers View</div>,
}))
vi.mock('./components/Devices', () => ({
  default: () => <div data-testid="devices-view">Devices View</div>,
}))
vi.mock('./components/ConnectionsViewer', () => ({
  default: () => <div data-testid="connections-view">Connections View</div>,
}))
vi.mock('./components/RulesViewer', () => ({
  default: () => <div data-testid="rules-view">Rules View</div>,
}))
vi.mock('./components/Diagnostics', () => ({
  default: () => <div data-testid="diagnostics-view">Diagnostics View</div>,
}))
vi.mock('./components/Antigravity', () => ({
  default: () => <div data-testid="antigravity-view">Antigravity View</div>,
}))
vi.mock('./components/Zapret', () => ({
  default: () => <div data-testid="zapret-view">Zapret View</div>,
}))
vi.mock('./components/VkTurn', () => ({
  default: () => <div data-testid="vkturn-view">VkTurn View</div>,
}))
vi.mock('./components/Gaming', () => ({
  default: () => <div data-testid="gaming-view">Gaming View</div>,
}))
vi.mock('./components/XboxDns', () => ({
  default: () => <div data-testid="xbox-dns-view">Xbox DNS View</div>,
}))
vi.mock('./components/Settings', () => ({
  default: () => <div data-testid="settings-view">Settings View</div>,
}))
vi.mock('./components/Help', () => ({
  default: () => <div data-testid="help-view">Help View</div>,
}))
vi.mock('./components/AdGuard', () => ({
  default: () => <div data-testid="adguard-view">AdGuard View</div>,
}))
vi.mock('./components/Whitelist', () => ({
  default: () => <div data-testid="whitelist-view">Whitelist View</div>,
}))
vi.mock('./components/ZapretCoreModal', () => ({
  default: ({ isOpen }: { isOpen: boolean }) => (isOpen ? <div data-testid="zapret-core-modal">Zapret Modal</div> : null),
}))

describe('App Component — Top Header Navigation & Settings Tab Relocation', () => {
  let container: HTMLDivElement | null = null
  let root: ReturnType<typeof createRoot> | null = null

  beforeEach(() => {
    container = document.createElement('div')
    document.body.appendChild(container)
    root = createRoot(container)
    vi.restoreAllMocks()
    localStorage.clear()
    window.location.hash = ''

    // Default API mocks
    vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path === 'status') {
        return Promise.resolve({
          version: 'v1.5.19',
          config_path: '/opt/etc/xkeen.json',
          router: { model: 'KN-1811' },
          system: { cpu_percent: 5, memory_used_mb: 120, memory_total_mb: 512 },
          mihomo_version: 'v1.19.0',
          zapret: {
            engine: 'v2',
            version: '1.0.5.2',
            label: 'Zapret 2 v1.0.5.2',
            installed: true,
            running: true,
            update_available: false,
          },
          active_server: null,
          refresh_interval_sec: 60,
        })
      }
      if (path === 'auth/status') {
        return Promise.resolve({ enabled: false, authenticated: true })
      }
      if (path === 'update/check') {
        return Promise.resolve({ current: 'v1.5.19', latest: 'v1.5.19', update_available: false })
      }
      if (path === 'mihomo/releases') {
        return Promise.resolve({ current: 'v1.19.0', latest: 'v1.19.0', update_available: false })
      }
      if (path === 'zapret/update/check') {
        return Promise.resolve({
          current_engine: 'v2',
          current_version: '1.0.5.2',
          latest_version: '1.0.5.2',
          update_available: false,
        })
      }
      return Promise.resolve({})
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

  it('removes Settings from bottom navigation tabs bar', async () => {
    await act(async () => {
      root!.render(<App />)
    })

    const tabsNav = container?.querySelector('nav.tabs')
    expect(tabsNav).not.toBeNull()
    expect(tabsNav?.textContent).not.toContain('Настройки')
    expect(tabsNav?.textContent).not.toContain('⚙️')
  })

  it('keeps VK calls inside the whitelist section and supports the existing link', async () => {
    window.location.hash = '#vkturn'
    await act(async () => {
      root!.render(<App />)
    })

    const mainNav = container!.querySelector('nav.tabs')!
    const whitelistButton = Array.from(mainNav.querySelectorAll('button')).find((button) =>
      button.textContent?.includes('Белые списки')
    )!
    expect(whitelistButton.classList.contains('active')).toBe(true)
    expect(mainNav.textContent).not.toContain('VK-туннель')
    expect(container!.querySelector('[data-testid="vkturn-view"]')).not.toBeNull()

    const sectionNav = container!.querySelector('nav[aria-label="Настройки белых списков"]')!
    const modeButton = Array.from(sectionNav.querySelectorAll('button')).find((button) =>
      button.textContent?.includes('Режим и устройства')
    )!
    const callsButton = Array.from(sectionNav.querySelectorAll('button')).find((button) =>
      button.textContent?.includes('Звонки ВКонтакте')
    )!
    expect(callsButton.getAttribute('aria-current')).toBe('page')

    await act(async () => { modeButton.click() })
    expect(window.location.hash).toBe('#whitelist')
    expect(container!.querySelector('[data-testid="whitelist-view"]')).not.toBeNull()
    expect(container!.querySelector('nav[aria-label="Настройки белых списков"] button[aria-current="page"]')?.textContent).toContain('Режим и устройства')

    await act(async () => {
      window.location.hash = '#vkturn'
      window.dispatchEvent(new HashChangeEvent('hashchange'))
    })
    expect(container!.querySelector('[data-testid="vkturn-view"]')).not.toBeNull()
    expect(whitelistButton.classList.contains('active')).toBe(true)
  })

  it('switches to Settings view when header settings button is clicked', async () => {
    await act(async () => {
      root!.render(<App />)
    })

    // Initially on dashboard
    expect(container?.querySelector('[data-testid="dashboard-view"]')).not.toBeNull()
    expect(container?.querySelector('[data-testid="settings-view"]')).toBeNull()

    const settingsBtn = container?.querySelector('[data-testid="header-settings-btn"]') as HTMLButtonElement | null
    expect(settingsBtn).not.toBeNull()
    expect(settingsBtn?.classList.contains('active')).toBe(false)

    // Click settings button in header
    await act(async () => {
      settingsBtn?.click()
    })

    // Active tab is now settings
    expect(settingsBtn?.classList.contains('active')).toBe(true)
    expect(container?.querySelector('[data-testid="settings-view"]')).not.toBeNull()
    expect(container?.querySelector('[data-testid="dashboard-view"]')).toBeNull()
  })

  it('initializes on Settings tab when URL hash is #settings', async () => {
    window.location.hash = '#settings'

    await act(async () => {
      root!.render(<App />)
    })

    expect(container?.querySelector('[data-testid="settings-view"]')).not.toBeNull()
    const settingsBtn = container?.querySelector('[data-testid="header-settings-btn"]') as HTMLButtonElement | null
    expect(settingsBtn?.classList.contains('active')).toBe(true)
  })

  it('switches to Zapret view when header Zapret pill is clicked', async () => {
    await act(async () => {
      root!.render(<App />)
    })

    const zapretPill = container?.querySelector('[data-testid="header-zapret-pill"]') as HTMLButtonElement | null
    expect(zapretPill).not.toBeNull()

    await act(async () => {
      zapretPill?.click()
    })

    expect(container?.querySelector('[data-testid="zapret-view"]')).not.toBeNull()
  })

  it('switches to settings on xr:switch-tab custom event and on Ctrl+, hotkey', async () => {
    await act(async () => {
      root!.render(<App />)
    })

    expect(container?.querySelector('[data-testid="settings-view"]')).toBeNull()

    // Dispatch xr:switch-tab custom event
    await act(async () => {
      window.dispatchEvent(new CustomEvent('xr:switch-tab', { detail: 'settings' }))
    })

    expect(container?.querySelector('[data-testid="settings-view"]')).not.toBeNull()

    // Switch away to servers
    const serversTabBtn = Array.from(container?.querySelectorAll('nav.tabs button') || []).find((b) =>
      b.textContent?.includes('Серверы')
    ) as HTMLButtonElement | undefined
    await act(async () => {
      serversTabBtn?.click()
    })
    expect(container?.querySelector('[data-testid="settings-view"]')).toBeNull()

    // Dispatch Ctrl+, keyboard shortcut
    await act(async () => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: ',', ctrlKey: true }))
    })

    expect(container?.querySelector('[data-testid="settings-view"]')).not.toBeNull()
  })

  it('renders AdGuard tab button and switches to AdGuard view when clicked', async () => {
    await act(async () => {
      root!.render(<App />)
    })

    const adguardTabBtn = Array.from(container?.querySelectorAll('nav.tabs button') || []).find((b) =>
      b.textContent?.includes('AdGuard')
    ) as HTMLButtonElement | undefined
    expect(adguardTabBtn).toBeDefined()

    const adguardPill = container?.querySelector('[data-testid="header-adguard-pill"]') as HTMLElement | null
    expect(adguardPill).toBeNull()

    await act(async () => {
      adguardTabBtn?.click()
    })

    expect(container?.querySelector('[data-testid="adguard-view"]')).not.toBeNull()
  })

  it('opens VK calls from the whitelist section', async () => {
    await act(async () => {
      root!.render(<App />)
    })

    const buttons = Array.from(container?.querySelectorAll<HTMLButtonElement>('nav.tabs button') || [])
    const whitelistTabBtn = buttons.find((b) => b.textContent?.includes('Белые списки'))
    expect(whitelistTabBtn).toBeDefined()
    await act(async () => {
      whitelistTabBtn!.click()
    })

    const callsButton = Array.from(container!.querySelectorAll('nav[aria-label="Настройки белых списков"] button')).find((button) =>
      button.textContent?.includes('Звонки ВКонтакте')
    ) as HTMLButtonElement
    expect(callsButton).toBeDefined()
    await act(async () => { callsButton.click() })
    expect(container?.querySelector('[data-testid="vkturn-view"]')).not.toBeNull()
  })

  it('renders #help hash as help view without duplicating adguard', async () => {
    window.location.hash = '#help'
    await act(async () => {
      root!.render(<App />)
    })
    expect(container?.querySelector('[data-testid="help-view"]')).not.toBeNull()
    expect(container?.querySelector('[data-testid="adguard-view"]')).toBeNull()
    window.location.hash = ''
  })

  it('renders #xbox-dns hash as gaming view with integrated xbox-dns', async () => {
    window.location.hash = '#xbox-dns'
    await act(async () => {
      root!.render(<App />)
    })
    expect(container?.querySelector('[data-testid="gaming-view"]')).not.toBeNull()
    window.location.hash = ''
  })

  it('renders #adguard hash as adguard view on direct initial navigation and hashchange', async () => {
    window.location.hash = '#adguard'
    await act(async () => {
      root!.render(<App />)
    })
    expect(container?.querySelector('[data-testid="adguard-view"]')).not.toBeNull()

    // Test hashchange event
    window.location.hash = '#settings'
    await act(async () => {
      window.dispatchEvent(new HashChangeEvent('hashchange'))
    })
    expect(container?.querySelector('[data-testid="settings-view"]')).not.toBeNull()

    window.location.hash = '#adguard'
    await act(async () => {
      window.dispatchEvent(new HashChangeEvent('hashchange'))
    })
    expect(container?.querySelector('[data-testid="adguard-view"]')).not.toBeNull()
    window.location.hash = ''
  })

  it('anti-regression (DIAG-01): renders #diagnostics hash directly without redirecting to connections', async () => {
    window.location.hash = '#diagnostics'
    await act(async () => {
      root!.render(<App />)
    })
    expect(container?.querySelector('[data-testid="diagnostics-view"]')).not.toBeNull()
    expect(container?.querySelector('[data-testid="connections-view"]')).toBeNull()
    window.location.hash = ''
  })

  it('anti-regression (DIAG-01): renders Diagnostics tab button in bottom navigation', async () => {
    await act(async () => {
      root!.render(<App />)
    })
    const diagTab = Array.from(container?.querySelectorAll('nav.tabs button') || []).find((b) =>
      b.textContent?.includes('Диагностика')
    )
    expect(diagTab).toBeDefined()
  })

  it('anti-regression (POLL-01): clamps refresh interval to minimum 3 seconds when refresh_interval_sec is 0 or negative', async () => {
    const setIntervalSpy = vi.spyOn(globalThis, 'setInterval')
    vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path === 'status') {
        return Promise.resolve({
          version: '1.9.4',
          service_running: true,
          refresh_interval_sec: 0,
        })
      }
      return Promise.resolve({})
    })

    await act(async () => {
      root!.render(<App />)
    })

    const intervals = setIntervalSpy.mock.calls.map((c) => c[1])
    expect(intervals.some((ms) => typeof ms === 'number' && ms >= 3000)).toBe(true)
    expect(intervals.some((ms) => typeof ms === 'number' && ms > 0 && ms < 3000)).toBe(false)
  })

  it('anti-regression (STARTUP-01): lazy-loads secondary tabs and heavy modals within Suspense boundary without crash', async () => {
    await act(async () => {
      root!.render(<App />)
    })

    // 1. Initial render displays static Dashboard view
    expect(container?.querySelector('[data-testid="dashboard-view"]')).not.toBeNull()

    // 2. Switch to a lazy-loaded tab: Servers
    const serversTabBtn = Array.from(container?.querySelectorAll('nav.tabs button') || []).find((b) =>
      b.textContent?.includes('Серверы')
    ) as HTMLButtonElement | undefined
    expect(serversTabBtn).toBeDefined()

    await act(async () => {
      serversTabBtn?.click()
    })

    // Lazy component must be rendered within Suspense boundary
    expect(container?.querySelector('[data-testid="servers-view"]')).not.toBeNull()
    expect(container?.querySelector('[data-testid="dashboard-view"]')).toBeNull()

    // 3. Switch to another lazy-loaded tab: Whitelist
    const whitelistTabBtn = Array.from(container?.querySelectorAll('nav.tabs button') || []).find((b) =>
      b.textContent?.includes('Белые списки')
    ) as HTMLButtonElement | undefined
    expect(whitelistTabBtn).toBeDefined()

    await act(async () => {
      whitelistTabBtn?.click()
    })

    expect(container?.querySelector('[data-testid="whitelist-view"]')).not.toBeNull()

    // 4. Verify custom event modal trigger loads heavy modal in Suspense
    await act(async () => {
      window.dispatchEvent(new CustomEvent('xr:open-zapret-modal'))
    })
    expect(container?.querySelector('[data-testid="zapret-core-modal"]')).not.toBeNull()
  })

  it('anti-regression (STARTUP-02): TabLoadingFallback renders skeleton with zero CLS', async () => {
    const { TabLoadingFallback } = await import('./App')
    await act(async () => {
      root!.render(<TabLoadingFallback />)
    })
    const skeleton = container?.querySelector('.tab-skeleton[role="status"]') as HTMLElement | null
    expect(skeleton).not.toBeNull()
    expect(skeleton?.getAttribute('aria-label')).toContain('Загрузка раздела')
    expect(skeleton?.querySelectorAll('.tab-skeleton-line').length).toBe(3)
  })

  it.each([
    ['#bs', 'whitelist'],
    ['#whitelists', 'whitelist'],
    ['#whitelist', 'whitelist'],
    ['#antigravity', 'google-ai'],
    ['#xbox-dns', 'gaming'],
    ['#diagnostics', 'diagnostics'],
    ['#unknown-hash-garbage', null],
    ['', null],
  ] as const)('bug 47: tabFromHash(%s) -> %s', async (hash, expected) => {
    // @ts-ignore - tabFromHash does not exist yet before implementation
    const { tabFromHash } = await import('./utils/tabHash')
    expect(tabFromHash(hash)).toBe(expected)
  })

  it('bug 47: hash aliases work for popstate and initial load', async () => {
    // @ts-ignore - tabFromHash does not exist yet before implementation
    const { tabFromHash } = await import('./utils/tabHash')
    expect(tabFromHash('#bs')).toBe('whitelist')
    expect(tabFromHash('#whitelists')).toBe('whitelist')
    expect(tabFromHash('#whitelist')).toBe('whitelist')
    expect(tabFromHash('#antigravity')).toBe('google-ai')
    expect(tabFromHash('#xbox-dns')).toBe('gaming')
    expect(tabFromHash('#diagnostics')).toBe('diagnostics')
    expect(tabFromHash('#unknown-hash-garbage')).toBeNull()

    // Initial load with #bs
    window.location.hash = '#bs'
    await act(async () => {
      root!.render(<App />)
    })
    expect(container?.querySelector('[data-testid=\"whitelist-view\"]')).not.toBeNull()

    // Switch to #dashboard first
    await act(async () => {
      window.location.hash = '#dashboard'
      window.dispatchEvent(new HashChangeEvent('hashchange'))
    })
    expect(container?.querySelector('[data-testid=\"dashboard-view\"]')).not.toBeNull()

    // Test #bs alias via hashchange
    await act(async () => {
      window.location.hash = '#bs'
      window.dispatchEvent(new HashChangeEvent('hashchange'))
    })
    expect(container?.querySelector('[data-testid=\"whitelist-view\"]')).not.toBeNull()

    // Switch back to #dashboard
    await act(async () => {
      window.location.hash = '#dashboard'
      window.dispatchEvent(new PopStateEvent('popstate'))
    })
    expect(container?.querySelector('[data-testid=\"dashboard-view\"]')).not.toBeNull()

    // Test #whitelists alias via popstate
    await act(async () => {
      window.location.hash = '#whitelists'
      window.dispatchEvent(new PopStateEvent('popstate'))
    })
    expect(container?.querySelector('[data-testid=\"whitelist-view\"]')).not.toBeNull()

    // Test #diagnostics alias via popstate
    await act(async () => {
      window.location.hash = '#diagnostics'
      window.dispatchEvent(new PopStateEvent('popstate'))
    })
    expect(container?.querySelector('[data-testid=\"diagnostics-view\"]')).not.toBeNull()
  })

  it('bug 139: refresh requested during in-flight poll runs afterwards', async () => {
    vi.useFakeTimers()
    let statusCallCount = 0
    let resolveFirstStatus: ((val: unknown) => void) | null = null
    let resolveSecondStatus: ((val: unknown) => void) | null = null

    try {
      vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
        if (path === 'status') {
          statusCallCount++
          if (statusCallCount === 1) {
            return new Promise((resolve) => {
              resolveFirstStatus = resolve
            })
          }
          if (statusCallCount === 2) {
            return new Promise((resolve) => {
              resolveSecondStatus = resolve
            })
          }
          return Promise.resolve({
            version: 'v1.5.19',
            refresh_interval_sec: 60,
          })
        }
        if (path === 'auth/status') {
          return Promise.resolve({ enabled: false, authenticated: true })
        }
        if (path === 'update/check') {
          return Promise.resolve({ current: 'v1.5.19', latest: 'v1.5.19', update_available: false })
        }
        if (path === 'mihomo/releases') {
          return Promise.resolve({ current: 'v1.19.0', latest: 'v1.19.0', update_available: false })
        }
        if (path === 'zapret/update/check') {
          return Promise.resolve({
            current_engine: 'v2',
            current_version: '1.0.5.2',
            latest_version: '1.0.5.2',
            update_available: false,
          })
        }
        return Promise.resolve({})
      })

      await act(async () => {
        root!.render(<App />)
      })

      // Initial poll is in-flight
      expect(statusCallCount).toBe(1)

      // Dispatch xr:refresh-status while the first poll is still in-flight
      await act(async () => {
        window.dispatchEvent(new CustomEvent('xr:refresh-status'))
      })
      // Should not start a second concurrent request while first is in-flight
      expect(statusCallCount).toBe(1)

      // Complete the first in-flight request
      await act(async () => {
        resolveFirstStatus!({
          version: 'v1.5.19',
          refresh_interval_sec: 60,
          active_server: 'old-server',
        })
      })

      // Exactly one follow-up poll must have started immediately after the first finished
      expect(statusCallCount).toBe(2)

      // Complete the follow-up request and verify no extra third request runs without another event
      await act(async () => {
        resolveSecondStatus!({
          version: 'v1.5.19',
          refresh_interval_sec: 60,
          active_server: 'new-server',
        })
      })
      expect(statusCallCount).toBe(2)
    } finally {
      vi.useRealTimers()
    }
  })

  it('bug 139: xr:refresh-status runs even when document.hidden is true while timer tick is skipped', async () => {
    vi.useFakeTimers()
    let statusCallCount = 0
    let isHidden = false
    Object.defineProperty(document, 'hidden', {
      configurable: true,
      get: () => isHidden,
    })

    try {
      isHidden = false
      vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
        if (path === 'status') {
          statusCallCount++
          return Promise.resolve({
            version: 'v1.5.19',
            refresh_interval_sec: 60,
          })
        }
        if (path === 'auth/status') {
          return Promise.resolve({ enabled: false, authenticated: true })
        }
        return Promise.resolve({})
      })

      await act(async () => {
        root!.render(<App />)
      })
      // Initial render calls refresh() (plus effect re-run when status.refresh_interval_sec updates from undefined->60).
      // Reset counter after initial mount settles, then test hidden timer tick vs explicit xr:refresh-status.
      const countBeforeHide = statusCallCount
      expect(countBeforeHide).toBeGreaterThanOrEqual(1)

      // Hide tab: timer tick must be skipped
      isHidden = true
      await act(async () => {
        vi.advanceTimersByTime(60000)
      })
      expect(statusCallCount).toBe(countBeforeHide)

      // Explicit xr:refresh-status event must NOT be skipped even when document.hidden is true
      await act(async () => {
        window.dispatchEvent(new CustomEvent('xr:refresh-status'))
      })
      expect(statusCallCount).toBe(countBeforeHide + 1)
    } finally {
      delete (document as any).hidden
      vi.useRealTimers()
    }
  })

  it('bug 48: tab switch asks before dropping unsaved changes', async () => {
    clearAllDirty()
    function DirtyForm({ dirty }: { dirty: boolean }) {
      useDirtyGuard('test-form', dirty)
      return <div data-testid="dirty-probe" />
    }

    const confirmSpy = vi.spyOn(window, 'confirm')
    try {
      await act(async () => {
        root!.render(<App />)
      })

      // Without dirty forms, switching tabs does not call confirm
      confirmSpy.mockReturnValue(true)
      const serversBtn = container!.querySelector('[data-testid="nav-servers"]') as HTMLButtonElement | null
      if (serversBtn) {
        await act(async () => {
          serversBtn.click()
        })
      }
      expect(confirmSpy).not.toHaveBeenCalled()

      // Mount a dirty guard alongside App
      const probeRootEl = document.createElement('div')
      document.body.appendChild(probeRootEl)
      const probeRoot = createRoot(probeRootEl)
      await act(async () => {
        probeRoot.render(<DirtyForm dirty={true} />)
      })

      // Reject confirm -> tab stays on current view
      confirmSpy.mockReturnValue(false)
      const hashBefore = window.location.hash
      await act(async () => {
        window.location.hash = '#devices'
        window.dispatchEvent(new HashChangeEvent('hashchange'))
      })
      expect(confirmSpy).toHaveBeenCalledTimes(1)
      expect(container!.querySelector('[data-testid="devices-view"]')).toBeNull()
      expect(window.location.hash).toBe(hashBefore || '#dashboard')

      // Accept confirm -> tab switches to devices
      confirmSpy.mockReturnValue(true)
      await act(async () => {
        window.location.hash = '#devices'
        window.dispatchEvent(new HashChangeEvent('hashchange'))
      })
      expect(confirmSpy).toHaveBeenCalledTimes(2)
      expect(container!.querySelector('[data-testid="devices-view"]')).not.toBeNull()

      await act(async () => {
        probeRoot.unmount()
      })
      probeRootEl.remove()
    } finally {
      confirmSpy.mockRestore()
      clearAllDirty()
    }
  })

  it('bug 49: toast leaves with exit class before removal', async () => {
    clearTabCache()
    vi.useFakeTimers()
    try {
      await act(async () => {
        root!.render(<App />)
      })

      const dash = container!.querySelector('[data-testid="dashboard-view"]') as HTMLElement
      await act(async () => {
        dash.click()
      })
      const toastBefore = container!.querySelector('.toast')
      expect(toastBefore).not.toBeNull()
      expect(toastBefore?.classList.contains('toast-exit')).toBe(false)

      // Just before removal (~4800ms): toast enters exit phase with .toast-exit
      await act(async () => {
        vi.advanceTimersByTime(4800)
      })
      const toastExiting = container!.querySelector('.toast')
      expect(toastExiting).not.toBeNull()
      expect(toastExiting?.classList.contains('toast-exit')).toBe(true)

      // After exit animation (~250ms): toast is removed from DOM
      await act(async () => {
        vi.advanceTimersByTime(250)
      })
      expect(container!.querySelector('.toast')).toBeNull()

      // Test useTabCache stale-while-revalidate:
      let resolveSecond!: (v: string) => void
      const fetcher = vi
        .fn<() => Promise<string>>()
        .mockResolvedValueOnce('cached-v1')
        .mockImplementationOnce(
          () =>
            new Promise<string>((res) => {
              resolveSecond = res
            })
        )

      function CacheProbe({ label }: { label: string }) {
        const { data, loading } = useTabCache('probe-key', fetcher)
        return (
          <div data-testid="cache-probe" data-loading={String(loading)}>
            {label}:{data ?? 'none'}
          </div>
        )
      }

      const probeEl = document.createElement('div')
      document.body.appendChild(probeEl)
      const probeRoot = createRoot(probeEl)

      await act(async () => {
        probeRoot.render(<CacheProbe label="first" />)
      })
      await act(async () => {
        await Promise.resolve()
      })
      expect(probeEl.textContent).toBe('first:cached-v1')

      // Unmount and remount: stale data must render immediately BEFORE second fetch resolves
      await act(async () => {
        probeRoot.unmount()
      })
      const probeRoot2 = createRoot(probeEl)
      await act(async () => {
        probeRoot2.render(<CacheProbe label="second" />)
      })
      expect(probeEl.textContent).toBe('second:cached-v1')

      await act(async () => {
        resolveSecond('fresh-v2')
        await Promise.resolve()
      })
      expect(probeEl.textContent).toBe('second:fresh-v2')

      await act(async () => {
        probeRoot2.unmount()
      })
      probeEl.remove()
    } finally {
      vi.useRealTimers()
      clearTabCache()
    }
  })
})

