import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { createRoot } from 'react-dom/client'
import { act } from 'react'
import AdGuard from './AdGuard'
import * as api from '../api'

vi.mock('../api', () => ({
  apiGet: vi.fn(),
  apiPost: vi.fn(),
}))

const mockStatus = {
  running: true,
  version: '0.107.56',
  protection_enabled: true,
  dns_addresses: ['127.0.0.1', '192.168.1.1'],
  dns_port: 53,
  http_port: 3000,
  language: 'ru',
  start_time: 1720000000,
}

const mockHealth = {
  process_alive: true,
  api_ok: true,
  api_latency_ms: 5,
  dns_listener_ok: true,
  loop_risk: false,
  version: '0.107.56',
  protection_enabled: true,
}

const mockOverview = {
  running: true,
  protection_enabled: true,
  num_dns_queries: 12450,
  num_blocked_filtering: 3210,
  num_replaced_safebrowsing: 12,
  num_replaced_parental: 0,
  num_replaced_safesearch: 45,
  avg_processing_time_ms: 4.2,
  block_percentage: 25.8,
  active_rules_count: 85400,
  top_queried_domains: [
    { domain: 'google.com', count: 1200 },
    { domain: 'apple.com', count: 850 },
  ],
  top_blocked_domains: [
    { domain: 'adservice.google.com', count: 450 },
    { domain: 'telemetry.microsoft.com', count: 320 },
  ],
  top_clients: [
    { ip: '192.168.1.100', name: 'Work PC', count: 4200 },
    { ip: '192.168.1.105', name: 'Phone', count: 2100 },
  ],
}

const mockConfig = {
  enabled: true,
  host: '127.0.0.1',
  http_port: 3000,
  dns_port: 53,
  username: '',
  password: '',
  integration_mode: 'managed' as const,
  upstream_dns: ['tls://1.1.1.1', 'tls://8.8.8.8'],
  failsafe_rollback: true,
  config_path: '/opt/etc/AdGuardHome.yaml',
}

const mockDiagnostics = {
  port_53_status: 'active',
  port_3000_status: 'active',
  dnsmasq_redirect_active: true,
  iptables_redirect_active: true,
  detected_service_path: '/opt/etc/init.d/S99adguardhome',
  detected_config_path: '/opt/etc/AdGuardHome.yaml',
  loop_risk: false,
  recommendations: ['Система работает в штатном режиме.'],
}

describe('AdGuard Component — Dashboard, Query Log, Filtering & Rewrites', () => {
  let container: HTMLDivElement | null = null
  let root: ReturnType<typeof createRoot> | null = null
  const notifyMock = vi.fn()

  beforeEach(() => {
    container = document.createElement('div')
    document.body.appendChild(container)
    root = createRoot(container)
    vi.clearAllMocks()

    vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path.includes('/adguard/status')) return Promise.resolve(mockStatus)
      if (path.includes('/adguard/health')) return Promise.resolve(mockHealth)
      if (path.includes('/adguard/overview')) return Promise.resolve(mockOverview)
      if (path.includes('/adguard/config')) return Promise.resolve(mockConfig)
      if (path.includes('/adguard/diagnostics')) return Promise.resolve(mockDiagnostics)
      if (path.includes('/adguard/query-log')) {
        return Promise.resolve({
          data: [
            {
              timestamp: '2026-09-29T20:00:00Z',
              client_ip: '192.168.1.100',
              client_name: 'Work PC',
              question_name: 'adservice.google.com',
              question_type: 'A',
              status: 'Filtered',
              elapsed_ms: 1.5,
              reason: 'FilteredBlockList',
              rule: '||google.com^',
              answer: ['0.0.0.0'],
            },
          ],
        })
      }
      if (path.includes('/adguard/filtering')) {
        return Promise.resolve({
          filters: [
            {
              id: 1,
              name: 'AdGuard Base',
              url: 'https://adguard.com/filters.txt',
              rules_count: 50000,
              enabled: true,
            },
          ],
          user_rules: ['||badsite.com^', '@@||goodsite.com^'],
        })
      }
      if (path.includes('/adguard/rewrites')) {
        return Promise.resolve([
          { domain: 'router.lan', answer: '192.168.1.1' },
          { domain: '*.local', answer: '192.168.1.200' },
        ])
      }
      return Promise.resolve({})
    })

    vi.spyOn(api, 'apiPost').mockResolvedValue({ success: true })
  })

  afterEach(() => {
    act(() => {
      root?.unmount()
    })
    container?.remove()
    container = null
    root = null
  })

  it('renders AdGuard Home banner, version badge, and overview statistics', async () => {
    await act(async () => {
      root!.render(<AdGuard notify={notifyMock} />)
    })

    expect(container?.querySelector('[data-testid="adguard-view"]')).not.toBeNull()
    expect(container?.textContent).toContain('AdGuard Home')
    expect(container?.textContent).toContain('v0.107.56')
    expect(container?.textContent).toContain('Защита включена')
    expect(container?.textContent).toMatch(/12[\s\u00A0,]450/) // DNS queries
    expect(container?.textContent).toMatch(/3[\s\u00A0,]210/) // Blocked queries
    expect(container?.textContent).toContain('25.8%') // Percentage
    expect(container?.textContent).toContain('4.2 мс') // Processing time
  })

  it('toggles protection when protection switch is clicked', async () => {
    await act(async () => {
      root!.render(<AdGuard notify={notifyMock} />)
    })

    const toggleBtn = container?.querySelector('[data-testid="adguard-protection-toggle"]') as HTMLButtonElement
    expect(toggleBtn).not.toBeNull()

    await act(async () => {
      toggleBtn.click()
    })

    expect(api.apiPost).toHaveBeenCalledWith('/adguard/protection', { enabled: false })
  })

  it('switches to Query Log tab and displays DNS query items', async () => {
    await act(async () => {
      root!.render(<AdGuard notify={notifyMock} />)
    })

    const queryLogTabBtn = Array.from(container?.querySelectorAll('button') || []).find((b) =>
      b.textContent?.includes('Журнал запросов')
    ) as HTMLButtonElement
    expect(queryLogTabBtn).toBeDefined()

    await act(async () => {
      queryLogTabBtn.click()
    })

    expect(container?.textContent).toContain('adservice.google.com')
    expect(container?.textContent).toContain('192.168.1.100')
    expect(container?.textContent).toContain('Блокировка')
  })

  it('switches to Rewrites tab and displays DNS rewrites table', async () => {
    await act(async () => {
      root!.render(<AdGuard notify={notifyMock} />)
    })

    const rewritesTabBtn = Array.from(container?.querySelectorAll('button') || []).find((b) =>
      b.textContent?.includes('DNS Переопределения')
    ) as HTMLButtonElement
    expect(rewritesTabBtn).toBeDefined()

    await act(async () => {
      rewritesTabBtn.click()
    })

    expect(container?.textContent).toContain('router.lan')
    expect(container?.textContent).toContain('192.168.1.1')
    expect(container?.textContent).toContain('*.local')
  })

  it('switches to Diagnostics tab and displays port statuses', async () => {
    await act(async () => {
      root!.render(<AdGuard notify={notifyMock} />)
    })

    const diagTabBtn = Array.from(container?.querySelectorAll('button') || []).find((b) =>
      b.textContent?.includes('Диагностика и сеть')
    ) as HTMLButtonElement
    expect(diagTabBtn).toBeDefined()

    await act(async () => {
      diagTabBtn.click()
    })

    expect(container?.textContent).toContain('Порт 53 (DNS UDP Listener)')
    expect(container?.textContent).toContain('Открыт и отвечает')
    expect(container?.textContent).toContain('Порт 3000 (Веб-интерфейс)')
  })
})
