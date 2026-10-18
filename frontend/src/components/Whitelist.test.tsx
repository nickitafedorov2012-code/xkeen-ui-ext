import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import Whitelist from './Whitelist'
import * as api from '../api'

// @ts-ignore
globalThis.IS_REACT_ACT_ENVIRONMENT = true

const mockWhitelistStatus = {
  enabled: true,
  auto_by_failover: true,
  strategy: 'strict_xkeen',
  target_server: null,
  follow_failover: true,
  excluded_devices: ['11:22:33:44:55:02'],
  devices: [
    { mac: '11:22:33:44:55:01', ip: '192.168.1.10', name: 'PC Gaming', enabled: true },
    { mac: '11:22:33:44:55:02', ip: '192.168.1.20', name: 'MacBook Pro', enabled: false },
  ],
  gaming_suspended: true,
  custom_domains: ['internal-bank.corp', 'my-cloud.io'],
  domestic_catalog_count: 376,
  active_server: 'NL-Amsterdam-Fast',
  is_bs_server: false,
  whitelist_servers: ['node_nl_01'],
  whitelist_providers: ['sub_alpha'],
}

const mockDevices = [
  {
    mac: '11:22:33:44:55:01',
    name: 'PC Gaming',
    ip: '192.168.1.10',
    policy: 'default',
    policy_name: 'Основная',
    online: true,
    interface: 'eth0',
    is_current_device: true,
    rxbytes: 1000,
    txbytes: 500,
    speed_limit_kbps: 0,
    current_server: 'default',
  },
  {
    mac: '11:22:33:44:55:02',
    name: 'MacBook Pro',
    ip: '192.168.1.20',
    policy: 'default',
    policy_name: 'Основная',
    online: true,
    interface: 'wifi',
    is_current_device: false,
    rxbytes: 2000,
    txbytes: 1000,
    speed_limit_kbps: 0,
    current_server: 'default',
  },
]

const mockServers = [
  { id: 'srv_nl', name: 'Netherlands Fast', protocol: 'vless', host: 'nl.proxy.com', port: 443, is_active: true, is_priority: true, ping_ms: 45, is_whitelist: true },
  { id: 'srv_de', name: 'Germany Stable', protocol: 'shadowsocks', host: 'de.proxy.com', port: 8388, is_active: false, is_priority: false, ping_ms: 60, is_whitelist: false },
]

const mockCatalog = {
  total: 376,
  domains: ['gosuslugi.ru', 'sberbank.ru', 'tbank.ru', 'vk.com', 'ozon.ru'],
  categories: {
    'Category 1: State Services, Federal Ministries & Regional Portals (55 domains)': ['gosuslugi.ru'],
    'Category 2: Banking, Financial Services, Payment Systems & FinTech (65 domains)': ['sberbank.ru', 'tbank.ru'],
    'Category 3: Search Engines, Web Portals & Major Digital Ecosystems (45 domains)': ['vk.com'],
    'Category 4: E-commerce, Marketplaces, Retail Chains & Pharmacies (55 domains)': ['ozon.ru'],
  },
  items: [
    { domain: 'gosuslugi.ru', category: 'Category 1: State Services, Federal Ministries & Regional Portals (55 domains)', description: 'Госуслуги' },
    { domain: 'sberbank.ru', category: 'Category 2: Banking, Financial Services, Payment Systems & FinTech (65 domains)', description: 'Сбербанк' },
    { domain: 'tbank.ru', category: 'Category 2: Banking, Financial Services, Payment Systems & FinTech (65 domains)', description: 'Т-Банк' },
    { domain: 'vk.com', category: 'Category 3: Search Engines, Web Portals & Major Digital Ecosystems (45 domains)', description: 'ВКонтакте' },
    { domain: 'ozon.ru', category: 'Category 4: E-commerce, Marketplaces, Retail Chains & Pharmacies (55 domains)', description: 'ОЗОН' },
  ],
}

describe('Whitelist Component — Whitelist Tab (Overhaul & Anti-Regression)', () => {
  let container: HTMLDivElement | null = null
  let root: ReturnType<typeof createRoot> | null = null
  const notifyMock = vi.fn()
  const refreshMock = vi.fn()

  beforeEach(() => {
    container = document.createElement('div')
    document.body.appendChild(container)
    root = createRoot(container)
    notifyMock.mockClear()
    refreshMock.mockClear()
    vi.restoreAllMocks()

    vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path === 'whitelist') return Promise.resolve(mockWhitelistStatus)
      if (path === 'devices') return Promise.resolve({ devices: mockDevices })
      if (path === 'servers') return Promise.resolve({ servers: mockServers })
      if (path === 'whitelist/catalog') return Promise.resolve(mockCatalog)
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

  it('renders Master Switch hero card with active status, gaming suspended badge, and KPIs', async () => {
    await act(async () => {
      root!.render(<Whitelist notify={notifyMock} refresh={refreshMock} />)
    })

    const badge = container?.querySelector('[data-testid="whitelist-status-badge"]')
    expect(badge?.textContent).toContain('🟢 АКТИВЕН')

    const gamingBadge = container?.querySelector('[data-testid="gaming-suspended-badge"]')
    expect(gamingBadge).toBeTruthy()
    expect(gamingBadge?.textContent).toContain('🎮 Игровой режим на паузе')

    const masterBtn = container?.querySelector('[data-testid="whitelist-master-toggle"]') as HTMLButtonElement
    expect(masterBtn).toBeTruthy()
    expect(masterBtn.textContent).toContain('Выключить режим белых списков')

    const activeDevicesKpi = container?.querySelector('[data-testid="kpi-active-devices"]')
    expect(activeDevicesKpi?.textContent).toContain('искл: 1')

    const strategyKpi = container?.querySelector('[data-testid="kpi-strategy"]')
    expect(strategyKpi?.textContent).toContain('🛡️ XKeen Route')

    const catalogKpi = container?.querySelector('[data-testid="kpi-catalog-count"]')
    expect(catalogKpi?.textContent).toBe('376')

    const customKpi = container?.querySelector('[data-testid="kpi-custom-count"]')
    expect(customKpi?.textContent).toBe('2')
  })

  it('toggles master switch when clicked', async () => {
    const postSpy = vi.spyOn(api, 'apiPost').mockResolvedValue({ saved: true })

    await act(async () => {
      root!.render(<Whitelist notify={notifyMock} refresh={refreshMock} />)
    })

    const masterBtn = container?.querySelector('[data-testid="whitelist-master-toggle"]') as HTMLButtonElement
    await act(async () => {
      masterBtn.click()
    })

    expect(postSpy).toHaveBeenCalledWith('whitelist', { enabled: false })
    expect(notifyMock).toHaveBeenCalledWith('⚪ Режим белых списков выключен')
  })

  it('switches Whitelist Strategy between StrictXKeen and Standard', async () => {
    const postSpy = vi.spyOn(api, 'apiPost').mockResolvedValue({ strategy: 'standard' })

    await act(async () => {
      root!.render(<Whitelist notify={notifyMock} refresh={refreshMock} />)
    })

    const standardRadio = container?.querySelector('[data-testid="radio-strategy-standard"]') as HTMLInputElement
    expect(standardRadio).toBeTruthy()

    await act(async () => {
      standardRadio.click()
    })

    expect(postSpy).toHaveBeenCalledWith('whitelist/strategy', {
      strategy: 'standard',
    })

    // Switch back to StrictXKeen
    const strictRadio = container?.querySelector('[data-testid="radio-strategy-strict"]') as HTMLInputElement
    await act(async () => {
      strictRadio.click()
    })

    expect(postSpy).toHaveBeenCalledWith('whitelist/strategy', {
      strategy: 'strict_xkeen',
    })
  })

  it('toggles Failover Auto-Activation switch', async () => {
    const postSpy = vi.spyOn(api, 'apiPost').mockResolvedValue({ saved: true })

    await act(async () => {
      root!.render(<Whitelist notify={notifyMock} refresh={refreshMock} />)
    })

    const autoToggle = container?.querySelector('[data-testid="auto-failover-toggle"]') as HTMLInputElement
    expect(autoToggle).toBeTruthy()
    expect(autoToggle.checked).toBe(true)

    await act(async () => {
      autoToggle.click()
    })

    expect(postSpy).toHaveBeenCalledWith('whitelist', {
      auto_by_failover: false,
      follow_failover: false,
    })
  })

  it('displays Zero-Touch device list and allows excluding/including devices', async () => {
    const postSpy = vi.spyOn(api, 'apiPost').mockResolvedValue({ success: true })

    await act(async () => {
      root!.render(<Whitelist notify={notifyMock} refresh={refreshMock} />)
    })

    // Device 1 (PC Gaming) is NOT excluded -> should have exclude button
    const excludePcBtn = container?.querySelector('[data-testid="btn-exclude-11:22:33:44:55:01"]') as HTMLButtonElement
    expect(excludePcBtn).toBeTruthy()
    expect(excludePcBtn.textContent).toContain('В БС (Защищено)')

    await act(async () => {
      excludePcBtn.click()
    })

    expect(postSpy).toHaveBeenCalledWith('whitelist/exclude', {
      id: '11:22:33:44:55:01',
      excluded: true,
    })

    // Device 2 (MacBook Pro) IS in excluded_devices -> should have include button
    const includeMacBtn = container?.querySelector('[data-testid="btn-include-11:22:33:44:55:02"]') as HTMLButtonElement
    expect(includeMacBtn).toBeTruthy()
    expect(includeMacBtn.textContent).toContain('Исключено (DIRECT)')

    await act(async () => {
      includeMacBtn.click()
    })

    expect(postSpy).toHaveBeenCalledWith('whitelist/exclude', {
      id: '11:22:33:44:55:02',
      excluded: false,
    })
  })

  function setInputValue(input: HTMLInputElement, val: string) {
    const nativeSetter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value')?.set
    nativeSetter?.call(input, val)
    input.dispatchEvent(new Event('input', { bubbles: true }))
    input.dispatchEvent(new Event('change', { bubbles: true }))
  }

  it('filters domestic catalog by search query and category pill', async () => {
    await act(async () => {
      root!.render(<Whitelist notify={notifyMock} refresh={refreshMock} />)
    })

    // Search input
    const searchInput = container?.querySelector('[data-testid="catalog-search-input"]') as HTMLInputElement
    expect(searchInput).toBeTruthy()

    await act(async () => {
      setInputValue(searchInput, 'sber')
    })

    // Only sberbank card should be present
    expect(container?.querySelector('[data-testid="catalog-card-sberbank.ru"]')).toBeTruthy()
    expect(container?.querySelector('[data-testid="catalog-card-vk.com"]')).toBeFalsy()

    // Clear search and test category pill
    await act(async () => {
      setInputValue(searchInput, '')
    })

    const bankPill = container?.querySelector('[data-testid="category-pill-bank"]') as HTMLButtonElement
    await act(async () => {
      bankPill.click()
    })

    expect(container?.querySelector('[data-testid="catalog-card-sberbank.ru"]')).toBeTruthy()
    expect(container?.querySelector('[data-testid="catalog-card-tbank.ru"]')).toBeTruthy()
    expect(container?.querySelector('[data-testid="catalog-card-gosuslugi.ru"]')).toBeFalsy()
  })

  it('adds and normalizes custom domain, and allows removing custom domain', async () => {
    const postSpy = vi.spyOn(api, 'apiPost').mockResolvedValue({ saved: true })

    await act(async () => {
      root!.render(<Whitelist notify={notifyMock} refresh={refreshMock} />)
    })

    // Switch to Custom Domains tab
    const customTabBtn = container?.querySelector('[data-testid="tab-custom"]') as HTMLButtonElement
    await act(async () => {
      customTabBtn.click()
    })

    expect(container?.querySelector('[data-testid="custom-domains-manager"]')).toBeTruthy()
    expect(container?.querySelector('[data-testid="custom-chip-internal-bank.corp"]')).toBeTruthy()

    // Add new domain with URL normalization: https://NEW-PORTAL.RU/test -> new-portal.ru
    const input = container?.querySelector('[data-testid="custom-domain-input"]') as HTMLInputElement
    const form = input.closest('form')!

    await act(async () => {
      setInputValue(input, '  HTTPS://NEW-PORTAL.RU/test/path  ')
    })

    await act(async () => {
      form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }))
    })

    expect(postSpy).toHaveBeenCalledWith('whitelist', {
      custom_domains: ['internal-bank.corp', 'my-cloud.io', 'new-portal.ru'],
    })

    // Delete custom domain
    const removeBtn = container?.querySelector('[data-testid="btn-remove-custom-internal-bank.corp"]') as HTMLButtonElement
    await act(async () => {
      removeBtn.click()
    })

    expect(postSpy).toHaveBeenCalledWith('whitelist', {
      custom_domains: ['my-cloud.io', 'new-portal.ru'],
    })
  })

  it('anti-regression: searches catalog by Cyrillic service description (госуслуги, сбербанк)', async () => {
    await act(async () => {
      root!.render(<Whitelist notify={notifyMock} refresh={refreshMock} />)
    })

    const searchInput = container?.querySelector('[data-testid="catalog-search-input"]') as HTMLInputElement
    expect(searchInput).toBeTruthy()

    // Search for Cyrillic name "госуслуги"
    await act(async () => {
      setInputValue(searchInput, 'госуслуги')
    })

    const gosuslugiCard = container?.querySelector('[data-testid="catalog-card-gosuslugi.ru"]')
    expect(gosuslugiCard).toBeTruthy()
    expect(gosuslugiCard?.textContent).toContain('Госуслуги')
    expect(container?.querySelector('[data-testid="catalog-card-sberbank.ru"]')).toBeFalsy()

    // Search for Cyrillic name "сбербанк"
    await act(async () => {
      setInputValue(searchInput, 'сбербанк')
    })

    const sberCard = container?.querySelector('[data-testid="catalog-card-sberbank.ru"]')
    expect(sberCard).toBeTruthy()
    expect(sberCard?.textContent).toContain('Сбербанк')
    expect(container?.querySelector('[data-testid="catalog-card-gosuslugi.ru"]')).toBeFalsy()
  })

  it('anti-regression: renders empty state message when search returns 0 matching services', async () => {
    await act(async () => {
      root!.render(<Whitelist notify={notifyMock} refresh={refreshMock} />)
    })

    const searchInput = container?.querySelector('[data-testid="catalog-search-input"]') as HTMLInputElement
    await act(async () => {
      setInputValue(searchInput, 'несуществующий')
    })

    const grid = container?.querySelector('[data-testid="catalog-grid"]')
    expect(grid?.textContent).toContain('Сервисы по запросу «несуществующий» не найдены')
  })

  it('anti-regression: rejects custom domain containing embedded whitespace', async () => {
    const postSpy = vi.spyOn(api, 'apiPost').mockResolvedValue({ saved: true })

    await act(async () => {
      root!.render(<Whitelist notify={notifyMock} refresh={refreshMock} />)
    })

    const customTabBtn = container?.querySelector('[data-testid="tab-custom"]') as HTMLButtonElement
    await act(async () => {
      customTabBtn.click()
    })

    const input = container?.querySelector('[data-testid="custom-domain-input"]') as HTMLInputElement
    const form = input.closest('form')!

    await act(async () => {
      setInputValue(input, 'invalid domain .ru')
    })

    await act(async () => {
      form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }))
    })

    expect(notifyMock).toHaveBeenCalledWith('Доменное имя не должно содержать пробелов', true)
    expect(postSpy).not.toHaveBeenCalledWith('whitelist', expect.anything())
  })
})
