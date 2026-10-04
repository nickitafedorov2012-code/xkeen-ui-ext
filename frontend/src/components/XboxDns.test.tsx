import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import XboxDns from './XboxDns'
import * as api from '../api'

describe('XboxDns Component — Xbox Live & SmartDNS Management', () => {
  let container: HTMLDivElement | null = null
  let root: Root | null = null
  const notifyMock = vi.fn()

  const mockStatus = {
    active: true,
    servers: [
      {
        id: 'xbox-dns-primary',
        name: 'xbox-dns.ru (Primary)',
        provider: 'Selectel (Saint Petersburg, RU)',
        ips: ['111.88.96.54', '111.88.96.55'],
        ipv6: ['2a00:ab00:1233:26::50'],
        status: 'online',
        latency_ms: 14,
        is_recommended: true,
        supported_features: ['Xbox Live (0x80a40401 fix)', 'Game Pass', 'AI Studio'],
      },
      {
        id: 'xbox-dns-alt',
        name: 'xbox-dns.ru (Alt Pool)',
        provider: 'Selectel (Saint Petersburg, RU)',
        ips: ['111.88.96.50', '111.88.96.51'],
        ipv6: ['2a00:ab00:1233:26::50'],
        status: 'online',
        latency_ms: 18,
        is_recommended: false,
        supported_features: ['Xbox Live', 'Game Pass'],
      },
    ],
    reverse_proxy_ips: ['188.68.214.130', '188.68.214.143'],
    targets: ['auth.xboxlive.com', 'aistudio.google.com'],
    antigravity_fallback_ready: true,
  }

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
    container = null
    root = null
  })

  it('renders the XboxDns container, title and server cards', async () => {
    vi.spyOn(api, 'apiGet').mockResolvedValue(mockStatus)

    await act(async () => {
      root!.render(<XboxDns notify={notifyMock} />)
    })

    expect(container?.querySelector('[data-testid="xbox-dns-view"]')).not.toBeNull()
    expect(container?.textContent).toContain('Xbox DNS & SmartDNS Bypass')
    expect(container?.textContent).toContain('Fix 0x80a40401')
    expect(container?.textContent).toContain('111.88.96.54')
    expect(container?.textContent).toContain('111.88.96.50')
  })

  it('renders the download button for fix_xbox_dns.cmd with correct attributes', async () => {
    vi.spyOn(api, 'apiGet').mockResolvedValue(mockStatus)

    await act(async () => {
      root!.render(<XboxDns notify={notifyMock} />)
    })

    const downloadLink = container?.querySelector('[data-testid="xbox-dns-download-cmd"]') as HTMLAnchorElement
    expect(downloadLink).not.toBeNull()
    expect(downloadLink.getAttribute('href')).toBe('/api/xbox-dns/fix.cmd')
    expect(downloadLink.getAttribute('download')).toBe('fix_xbox_dns.cmd')
  })

  it('triggers check DNS action when check button is clicked', async () => {
    vi.spyOn(api, 'apiGet').mockResolvedValue(mockStatus)
    const checkSpy = vi.spyOn(api, 'apiPost').mockResolvedValue({
      timestamp: '03:15:00',
      results: [
        {
          server: 'xbox-dns.ru (Primary)',
          ip: '111.88.96.54',
          reachable: true,
          latency_ms: 12,
          resolved_ips: ['188.68.214.130'],
        },
      ],
    })

    await act(async () => {
      root!.render(<XboxDns notify={notifyMock} />)
    })

    const checkBtn = container?.querySelector('[data-testid="xbox-dns-check-btn"]') as HTMLButtonElement
    expect(checkBtn).not.toBeNull()

    await act(async () => {
      checkBtn.click()
    })

    expect(checkSpy).toHaveBeenCalledWith('xbox-dns/check', {})
    expect(notifyMock).toHaveBeenCalledWith('Диагностика DNS-серверов успешно завершена')
  })

  it('switches device setup guide tabs between Xbox, PSN, PC and Keenetic', async () => {
    vi.spyOn(api, 'apiGet').mockResolvedValue(mockStatus)

    await act(async () => {
      root!.render(<XboxDns notify={notifyMock} />)
    })

    expect(container?.textContent).toContain('Нажмите кнопку Xbox на геймпаде')

    // Switch to Windows PC guide
    const buttons = Array.from(container?.querySelectorAll('button') || [])
    const pcBtn = buttons.find((b) => b.textContent?.includes('Windows 10 / 11'))
    expect(pcBtn).toBeDefined()

    await act(async () => {
      pcBtn!.click()
    })

    expect(container?.textContent).toContain('Set-DnsClientServerAddress')

    // Switch to Keenetic guide
    const keeneticBtn = buttons.find((b) => b.textContent?.includes('Роутер Keenetic'))
    expect(keeneticBtn).toBeDefined()

    await act(async () => {
      keeneticBtn!.click()
    })

    expect(container?.textContent).toContain('ip host auth.xboxlive.com 188.68.214.130')
  })

  it('displays integration details with Google Antigravity & AI Flow', async () => {
    vi.spyOn(api, 'apiGet').mockResolvedValue(mockStatus)

    await act(async () => {
      root!.render(<XboxDns notify={notifyMock} />)
    })

    expect(container?.textContent).toContain('Интеграция с Google Antigravity & AI Flow')
    expect(container?.textContent).toContain('188.68.214.130')
    expect(container?.textContent).toContain('daily-cloudcode-pa.googleapis.com')
  })
})
