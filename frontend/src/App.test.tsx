import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import App from './App'
import * as api from './api'

// @ts-ignore
globalThis.IS_REACT_ACT_ENVIRONMENT = true

// Mock heavy sub-components
vi.mock('./components/Dashboard', () => ({
  default: () => <div data-testid="dashboard-view">Dashboard View</div>,
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

  it('anti-regression (STARTUP-02): TabLoadingFallback renders with 340px minimum height and zero CLS', async () => {
    const { TabLoadingFallback } = await import('./App')
    await act(async () => {
      root!.render(<TabLoadingFallback />)
    })
    const card = container?.querySelector('section.card') as HTMLElement | null
    expect(card).not.toBeNull()
    expect(card?.textContent).toContain('Загрузка раздела…')
    expect(card?.style.minHeight).toBe('340px')
  })
})

