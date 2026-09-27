import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import Zapret from './Zapret'
import * as api from '../api'
import type { ZapretStatus } from '../types'

// @ts-ignore
globalThis.IS_REACT_ACT_ENVIRONMENT = true

const mockZapretStatus: ZapretStatus = {
  installed: true,
  running: true,
  pid: 1234,
  autostart: true,
  iptables_active: true,
  preset: 'gamer',
  cmdline: '/opt/zapret/nfq/nfqws --daemon',
  config: 'NFQWS_ARGS="--daemon"\n',
  hosts: 'rutracker.org\n',
  features: {
    enabled: true,
    hybrid_youtube: true,
    hybrid_discord: true,
    discord_voice_udp: true,
    youtube_turbo: true,
    general_bypass: true,
    aggressive_dpi: false,
    isolated_proxy: true,
    bypass_github: true,
    bypass_torrents: true,
    bypass_adult: true,
    custom_entries: [
      {
        domain: 'mysku.club',
        enabled: true,
        cdns: ['mysku-st.ru', 'img.mysku-st.ru', 'art.mysku-st.net'],
      },
    ],
  },
}

describe('Zapret Component — Toggleable Service Blocks & Custom Site CDN Boost (/boost)', () => {
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
    vi.useRealTimers()
    if (root && container) {
      act(() => {
        root!.unmount()
      })
      container.remove()
    }
  })

  it('renders all toggleable service blocks (YouTube, Discord, GitHub, Torrents, 18+, Hostlist)', async () => {
    vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path === 'zapret/status') {
        return Promise.resolve(mockZapretStatus)
      }
      return Promise.resolve({})
    })

    await act(async () => {
      root!.render(<Zapret notify={notifyMock} />)
    })

    const text = container?.textContent || ''
    expect(text).toContain('Zapret — Обход DPI')
    expect(text).toContain('Службы и сервисы (прямой обход DPI без VPS)')
    expect(text).toContain('YouTube Direct')
    expect(text).toContain('Discord Web & Chat')
    expect(text).toContain('GitHub (Релизы & Исходники)')
    expect(text).toContain('Торренты & Трекеры')
    expect(text).toContain('18+ Контент')
    expect(text).toContain('Универсальный веб-обход (Hostlist)')

    // Check tags preview in service cards
    expect(text).toContain('github.com')
    expect(text).toContain('rutracker.org')
    expect(text).toContain('pornhub.com')
    expect(text).toContain('googlevideo.com')
  })

  it('calls toggle_feature when toggling a service block (e.g. GitHub)', async () => {
    vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path === 'zapret/status') {
        return Promise.resolve(mockZapretStatus)
      }
      return Promise.resolve({})
    })

    const postSpy = vi.spyOn(api, 'apiPost').mockImplementation((path: string, body: any) => {
      if (path === 'zapret/action' && body.action === 'toggle_feature') {
        return Promise.resolve({
          success: true,
          features: {
            ...mockZapretStatus.features!,
            bypass_github: false,
          },
        })
      }
      return Promise.resolve({ success: true })
    })

    await act(async () => {
      root!.render(<Zapret notify={notifyMock} />)
    })

    // Find button for GitHub card
    const buttons = Array.from(container?.querySelectorAll('button') || [])
    const githubBtn = buttons.find(
      (b) => b.title === 'Выключить блок' && b.closest('div')?.textContent?.includes('GitHub')
    )
    expect(githubBtn).toBeDefined()

    await act(async () => {
      githubBtn?.click()
    })

    expect(postSpy).toHaveBeenCalledWith('zapret/action', {
      action: 'toggle_feature',
      feature: 'bypass_github',
      enabled: false,
    })
  })

  it('calls toggle_feature when toggling 18+ Content card', async () => {
    vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path === 'zapret/status') {
        return Promise.resolve(mockZapretStatus)
      }
      return Promise.resolve({})
    })

    const postSpy = vi.spyOn(api, 'apiPost').mockImplementation((path: string, body: any) => {
      if (path === 'zapret/action' && body.action === 'toggle_feature') {
        return Promise.resolve({
          success: true,
          features: {
            ...mockZapretStatus.features!,
            bypass_adult: false,
          },
        })
      }
      return Promise.resolve({ success: true })
    })

    await act(async () => {
      root!.render(<Zapret notify={notifyMock} />)
    })

    // Find button for 18+ card
    const buttons = Array.from(container?.querySelectorAll('button') || [])
    const adultBtn = buttons.find(
      (b) => b.title === 'Выключить блок' && b.closest('div')?.textContent?.includes('18+ Контент')
    )
    expect(adultBtn).toBeDefined()

    await act(async () => {
      adultBtn?.click()
    })

    expect(postSpy).toHaveBeenCalledWith('zapret/action', {
      action: 'toggle_feature',
      feature: 'bypass_adult',
      enabled: false,
    })
  })

  it('renders custom site & CDN Boost (/boost) card with input and suggestion chips', async () => {
    vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path === 'zapret/status') {
        return Promise.resolve(mockZapretStatus)
      }
      return Promise.resolve({})
    })

    await act(async () => {
      root!.render(<Zapret notify={notifyMock} />)
    })

    const text = container?.textContent || ''
    expect(text).toContain('Свой сайт & CDN Boost (/boost)')
    expect(text).toContain('⚡ /boost активен')
    expect(text).toContain('Добавить & Boost')
    expect(text).toContain('Быстрый выбор:')
    expect(text).toContain('habr.com')
    expect(text).toContain('speedtest.net')
  })

  it('displays primary domain badge and distinct CDN badges with /boost tag', async () => {
    vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path === 'zapret/status') {
        return Promise.resolve(mockZapretStatus)
      }
      return Promise.resolve({})
    })

    await act(async () => {
      root!.render(<Zapret notify={notifyMock} />)
    })

    const text = container?.textContent || ''
    // Primary domain badge
    expect(text).toContain('🌐 mysku.club')
    expect(text).toContain('+3 CDN подтянуто')
    expect(text).toContain('⚡ /boost CDN:')

    // Discovered CDN badges
    expect(text).toContain('⚡ mysku-st.ru')
    expect(text).toContain('⚡ img.mysku-st.ru')
    expect(text).toContain('⚡ art.mysku-st.net')
  })

  it('adds custom domain and calls add_custom_domain action', async () => {
    vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path === 'zapret/status') {
        return Promise.resolve(mockZapretStatus)
      }
      return Promise.resolve({})
    })

    const postSpy = vi.spyOn(api, 'apiPost').mockImplementation((path: string, body: any) => {
      if (path === 'zapret/action' && body.action === 'add_custom_domain') {
        return Promise.resolve({
          success: true,
          message: 'Сайт habr.com добавлен',
          features: {
            ...mockZapretStatus.features!,
            custom_entries: [
              ...mockZapretStatus.features!.custom_entries!,
              {
                domain: 'habr.com',
                enabled: true,
                cdns: ['habrastorage.org', 'hsto.org'],
              },
            ],
          },
        })
      }
      return Promise.resolve({ success: true })
    })

    await act(async () => {
      root!.render(<Zapret notify={notifyMock} />)
    })

    const input = container?.querySelector('input[type="text"]') as HTMLInputElement
    expect(input).toBeDefined()

    await act(async () => {
      const nativeInputValueSetter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value')?.set
      nativeInputValueSetter?.call(input, 'https://habr.com/')
      input.dispatchEvent(new Event('input', { bubbles: true }))
      input.dispatchEvent(new Event('change', { bubbles: true }))
    })

    const addBtn = Array.from(container?.querySelectorAll('button') || []).find((b) =>
      b.textContent?.includes('Добавить & Boost')
    )
    expect(addBtn).toBeDefined()

    await act(async () => {
      addBtn?.click()
    })

    expect(postSpy).toHaveBeenCalledWith('zapret/action', {
      action: 'add_custom_domain',
      domain: 'habr.com',
    })
  })

  it('toggles custom domain on/off', async () => {
    vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path === 'zapret/status') {
        return Promise.resolve(mockZapretStatus)
      }
      return Promise.resolve({})
    })

    const postSpy = vi.spyOn(api, 'apiPost').mockImplementation((_path: string, _body: any) => {
      return Promise.resolve({
        success: true,
        features: mockZapretStatus.features,
      })
    })

    await act(async () => {
      root!.render(<Zapret notify={notifyMock} />)
    })

    const toggleBtn = Array.from(container?.querySelectorAll('button') || []).find(
      (b) => b.title === 'Выключить обход для этого сайта'
    )
    expect(toggleBtn).toBeDefined()

    await act(async () => {
      toggleBtn?.click()
    })

    expect(postSpy).toHaveBeenCalledWith('zapret/action', {
      action: 'toggle_custom_domain',
      domain: 'mysku.club',
      enabled: false,
    })
  })

  it('boosts custom domain to discover new CDNs', async () => {
    vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path === 'zapret/status') {
        return Promise.resolve(mockZapretStatus)
      }
      return Promise.resolve({})
    })

    const postSpy = vi.spyOn(api, 'apiPost').mockImplementation((_path: string, _body: any) => {
      return Promise.resolve({
        success: true,
        features: mockZapretStatus.features,
      })
    })

    await act(async () => {
      root!.render(<Zapret notify={notifyMock} />)
    })

    const boostBtn = Array.from(container?.querySelectorAll('button') || []).find(
      (b) => b.textContent?.includes('⚡ Boost') && b.title?.includes('Повторно')
    )
    expect(boostBtn).toBeDefined()

    await act(async () => {
      boostBtn?.click()
    })

    expect(postSpy).toHaveBeenCalledWith('zapret/action', {
      action: 'boost_custom_domain',
      domain: 'mysku.club',
    })
  })

  it('removes custom domain when delete icon is clicked', async () => {
    vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path === 'zapret/status') {
        return Promise.resolve(mockZapretStatus)
      }
      return Promise.resolve({})
    })

    const postSpy = vi.spyOn(api, 'apiPost').mockImplementation((_path: string, _body: any) => {
      return Promise.resolve({
        success: true,
        features: {
          ...mockZapretStatus.features!,
          custom_entries: [],
        },
      })
    })

    await act(async () => {
      root!.render(<Zapret notify={notifyMock} />)
    })

    const deleteBtn = Array.from(container?.querySelectorAll('button') || []).find(
      (b) => b.title === 'Удалить сайт из списка'
    )
    expect(deleteBtn).toBeDefined()

    await act(async () => {
      deleteBtn?.click()
    })

    expect(postSpy).toHaveBeenCalledWith('zapret/action', {
      action: 'remove_custom_domain',
      domain: 'mysku.club',
    })
  })

  it('normalizes exotic URL formats (ports, userinfo, paths, queries, uppercase) correctly', async () => {
    vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path === 'zapret/status') {
        return Promise.resolve(mockZapretStatus)
      }
      return Promise.resolve({})
    })

    const postSpy = vi.spyOn(api, 'apiPost').mockImplementation((_path: string, _body: any) => {
      return Promise.resolve({
        success: true,
        features: mockZapretStatus.features,
      })
    })

    await act(async () => {
      root!.render(<Zapret notify={notifyMock} />)
    })

    const input = container?.querySelector('input[type="text"]') as HTMLInputElement
    expect(input).toBeDefined()

    // Test with exotic URL containing protocol, userinfo, port, path, query and hash
    await act(async () => {
      const nativeInputValueSetter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value')?.set
      nativeInputValueSetter?.call(input, 'https://user:pass@SUB.DOMAIN.CO.UK:8080/path?query=1#hash')
      input.dispatchEvent(new Event('input', { bubbles: true }))
      input.dispatchEvent(new Event('change', { bubbles: true }))
    })

    const addBtn = Array.from(container?.querySelectorAll('button') || []).find((b) =>
      b.textContent?.includes('Добавить & Boost')
    )
    expect(addBtn).toBeDefined()

    await act(async () => {
      addBtn?.click()
    })

    expect(postSpy).toHaveBeenCalledWith('zapret/action', {
      action: 'add_custom_domain',
      domain: 'sub.domain.co.uk',
    })
  })

  it('rejects invalid inputs without domain or dots', async () => {
    vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path === 'zapret/status') {
        return Promise.resolve(mockZapretStatus)
      }
      return Promise.resolve({})
    })

    const postSpy = vi.spyOn(api, 'apiPost')

    await act(async () => {
      root!.render(<Zapret notify={notifyMock} />)
    })

    const input = container?.querySelector('input[type="text"]') as HTMLInputElement

    await act(async () => {
      const nativeInputValueSetter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value')?.set
      nativeInputValueSetter?.call(input, 'invalid_site_name')
      input.dispatchEvent(new Event('input', { bubbles: true }))
      input.dispatchEvent(new Event('change', { bubbles: true }))
    })

    const addBtn = Array.from(container?.querySelectorAll('button') || []).find((b) =>
      b.textContent?.includes('Добавить & Boost')
    )

    await act(async () => {
      addBtn?.click()
    })

    // Should NOT call API
    expect(postSpy).not.toHaveBeenCalledWith('zapret/action', expect.objectContaining({ action: 'add_custom_domain' }))
    expect(notifyMock).toHaveBeenCalledWith('Некорректный домен сайта (например, mysku.club)', true)
  })

  it('displays warning banner when nfqws is running but iptables is inactive, and restores netfilter with start-fw action', async () => {
    vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path === 'zapret/status') {
        return Promise.resolve({
          ...mockZapretStatus,
          running: true,
          iptables_active: false,
        })
      }
      return Promise.resolve({})
    })

    const postSpy = vi.spyOn(api, 'apiPost').mockImplementation((path: string, body: any) => {
      if (path === 'zapret/action' && body.action === 'start-fw') {
        return Promise.resolve({
          success: true,
          message: 'Правила Netfilter успешно восстановлены',
        })
      }
      return Promise.resolve({ success: true })
    })

    await act(async () => {
      root!.render(<Zapret notify={notifyMock} />)
    })

    const text = container?.textContent || ''
    expect(text).toContain('Служба Zapret активна')
    expect(text).toContain('но перехват Netfilter отключен')

    const buttons = Array.from(container?.querySelectorAll('button') || [])
    const enableFwBtn = buttons.find((b) => b.textContent?.includes('Включить перехват'))
    expect(enableFwBtn).toBeDefined()

    await act(async () => {
      enableFwBtn?.click()
    })

    expect(postSpy).toHaveBeenCalledWith('zapret/action', {
      action: 'start-fw',
    })
    expect(notifyMock).toHaveBeenCalledWith('Правила Netfilter успешно восстановлены')
  })

  it('locks optimistic state and displays micro-spinner with glowing pulse during in-flight toggle', async () => {
    vi.useFakeTimers()
    let resolvePost: (value: any) => void
    const pendingPostPromise = new Promise((resolve) => {
      resolvePost = resolve
    })

    vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path === 'zapret/status') {
        // Return status with bypass_github = true (stale response)
        return Promise.resolve(mockZapretStatus)
      }
      return Promise.resolve({})
    })

    vi.spyOn(api, 'apiPost').mockImplementation((path: string, body: any) => {
      if (path === 'zapret/action' && body.action === 'toggle_feature' && body.feature === 'bypass_github') {
        return pendingPostPromise as Promise<any>
      }
      return Promise.resolve({ success: true })
    })

    await act(async () => {
      root!.render(<Zapret notify={notifyMock} />)
    })

    // Find button for GitHub card
    const buttons = Array.from(container?.querySelectorAll('button') || [])
    const githubBtn = buttons.find(
      (b) => b.title === 'Выключить блок' && b.closest('div')?.textContent?.includes('GitHub')
    )
    expect(githubBtn).toBeDefined()

    // Trigger toggle (turns it off optimistically)
    await act(async () => {
      githubBtn?.click()
    })

    // While in-flight:
    // 1. Button has glowing pulse class
    expect(githubBtn?.className).toContain('zapret-glow-pulse-blue')
    // 2. Micro-spinner SVG is rendered inside knob
    const microSpinner = githubBtn?.querySelector('[data-testid="zapret-micro-spinner"]')
    expect(microSpinner).not.toBeNull()
    expect(microSpinner?.classList.contains('zapret-spin')).toBe(true)

    // 3. Stale polling arrives during in-flight request via advancing 10s timer
    await act(async () => {
      vi.advanceTimersByTime(10000)
    })

    // The button must NOT have flipped back to on (still off optimistically due to pendingKeys)
    expect(githubBtn?.title).toBe('Включить блок')

    // Now resolve the backend call
    await act(async () => {
      resolvePost!({
        success: true,
        features: {
          ...mockZapretStatus.features!,
          bypass_github: false,
        },
      })
    })

    // Once finished:
    expect(githubBtn?.className).not.toContain('zapret-glow-pulse-blue')
    expect(githubBtn?.querySelector('[data-testid="zapret-micro-spinner"]')).toBeNull()
    expect(githubBtn?.title).toBe('Включить блок')
  })

  it('renders all 6 DPI presets and handles preset application correctly', async () => {
    vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path === 'zapret/status') {
        return Promise.resolve(mockZapretStatus)
      }
      return Promise.resolve({})
    })

    const postSpy = vi.spyOn(api, 'apiPost').mockImplementation((_path: string, body: any) => {
      return Promise.resolve({
        success: true,
        action: 'set_preset',
        preset: body.preset,
        features: mockZapretStatus.features,
      })
    })

    await act(async () => {
      root!.render(<Zapret notify={notifyMock} />)
    })

    const buttons = Array.from(container?.querySelectorAll('button') || [])

    // Check all 6 presets exist
    const ytPreset = buttons.find((b) => b.textContent?.includes('Только YouTube'))
    const discordPreset = buttons.find((b) => b.textContent?.includes('Только Discord'))
    const gamerPreset = buttons.find((b) => b.textContent?.includes('Медиа и Игры'))
    const aggroPreset = buttons.find((b) => b.textContent?.includes('Агрессивный'))
    const customPreset = buttons.find((b) => b.textContent?.includes('Пользовательский (Custom)'))
    const defaultPreset = buttons.find((b) => b.textContent?.includes('Все сервисы (По умолчанию)'))

    expect(ytPreset).toBeDefined()
    expect(discordPreset).toBeDefined()
    expect(gamerPreset).toBeDefined()
    expect(aggroPreset).toBeDefined()
    expect(customPreset).toBeDefined()
    expect(defaultPreset).toBeDefined()

    // Test clicking YouTube preset
    await act(async () => {
      ytPreset?.click()
    })
    expect(postSpy).toHaveBeenCalledWith('zapret/action', {
      action: 'set_preset',
      preset: 'youtube',
    })

    // Test clicking Discord preset
    await act(async () => {
      discordPreset?.click()
    })
    expect(postSpy).toHaveBeenCalledWith('zapret/action', {
      action: 'set_preset',
      preset: 'discord',
    })

    // Test clicking Default preset
    await act(async () => {
      defaultPreset?.click()
    })
    expect(postSpy).toHaveBeenCalledWith('zapret/action', {
      action: 'set_preset',
      preset: 'default',
    })

    // Test clicking Custom preset opens config editor modal
    await act(async () => {
      customPreset?.click()
    })
    expect(container?.textContent).toContain('/opt/etc/zapret/zapret.conf')
  })

  it('locks optimistic state during custom domain deletion against background polling', async () => {
    vi.useFakeTimers()
    let resolvePost: (value: any) => void
    const pendingPostPromise = new Promise((resolve) => {
      resolvePost = resolve
    })

    vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path === 'zapret/status') {
        return Promise.resolve(mockZapretStatus)
      }
      return Promise.resolve({})
    })

    vi.spyOn(api, 'apiPost').mockImplementation((path: string, body: any) => {
      if (path === 'zapret/action' && body.action === 'remove_custom_domain') {
        return pendingPostPromise as Promise<any>
      }
      return Promise.resolve({ success: true })
    })

    await act(async () => {
      root!.render(<Zapret notify={notifyMock} />)
    })

    // Verify mysku.club entry with +3 CDN is initially displayed
    expect(container?.textContent).toContain('+3 CDN подтянуто')

    // Find delete button
    const deleteBtn = Array.from(container?.querySelectorAll('button') || []).find(
      (b) => b.title?.includes('Удалить') || b.textContent?.includes('🗑️')
    )
    expect(deleteBtn).toBeDefined()

    // Click delete
    await act(async () => {
      deleteBtn?.click()
    })

    // Optimistically removed
    expect(container?.textContent).toContain('Пользовательские сайты пока не добавлены')
    expect(container?.textContent).not.toContain('+3 CDN подтянуто')

    // Background polling fires while deletion is in flight
    await act(async () => {
      vi.advanceTimersByTime(10000)
    })

    // Still removed thanks to pendingKeys protection ('delete:mysku.club')
    expect(container?.textContent).toContain('Пользовательские сайты пока не добавлены')
    expect(container?.textContent).not.toContain('+3 CDN подтянуто')

    // Resolve deletion
    await act(async () => {
      resolvePost!({
        success: true,
        action: 'remove_custom_domain',
        features: {
          ...mockZapretStatus.features!,
          custom_entries: [],
        },
      })
    })

    expect(container?.textContent).toContain('Пользовательские сайты пока не добавлены')
    expect(container?.textContent).not.toContain('+3 CDN подтянуто')
  })
})

describe('Zapret Component — Features 1, 2, 4, 5 (Mini-Blockcheck, DPI Analytics, Smart TV, Community Hostlists)', () => {
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
    vi.useRealTimers()
    if (root && container) {
      act(() => {
        root!.unmount()
      })
      container.remove()
    }
  })

  it('renders DPI Analytics live-widget with intercepted bytes, VPS saved bytes and handles reset', async () => {
    vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path === 'zapret/status') {
        return Promise.resolve(mockZapretStatus)
      }
      if (path === 'zapret/analytics') {
        return Promise.resolve({
          bytes_intercepted: 52428800, // 50 MB
          packets_intercepted: 45000,
          tcp_packets: 40000,
          udp_packets: 5000,
          vps_saved_bytes: 52428800,
          uptime_seconds: 3660, // 1h 1m
          nfqws_mem_bytes: 4194304, // 4 MB
          is_active: true,
        })
      }
      return Promise.resolve({})
    })

    const postSpy = vi.spyOn(api, 'apiPost').mockResolvedValue({ success: true })

    await act(async () => {
      root!.render(<Zapret notify={notifyMock} />)
    })

    const text = container?.textContent || ''
    expect(text).toContain('Live-монитор и счётчик спасённого трафика (DPI Analytics)')
    expect(text).toContain('50.0 МБ')
    expect(text).toMatch(/45[,\s\u00A0]?000 пакетов/)
    expect(text).toContain('Сэкономлено трафика на VPS')
    expect(text).toContain('1 ч 1 мин')
    expect(text).toContain('4.0 МБ')

    // Find reset button
    const resetBtn = Array.from(container?.querySelectorAll('button') || []).find((b) =>
      b.textContent?.includes('Сбросить счётчики')
    )
    expect(resetBtn).toBeDefined()

    await act(async () => {
      resetBtn?.click()
    })

    expect(postSpy).toHaveBeenCalledWith('zapret/action', {
      action: 'reset_analytics',
    })
    expect(notifyMock).toHaveBeenCalledWith('Счётчики перехваченного трафика DPI сброшены')
  })

  it('runs Mini-Blockcheck and applies best strategy in 1 click', async () => {
    vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path === 'zapret/status') {
        return Promise.resolve(mockZapretStatus)
      }
      return Promise.resolve({})
    })

    const mockBlockcheck = {
      best_strategy_id: 'multisplit',
      strategies: [
        {
          id: 'multisplit',
          name: 'Multisplit TLS + Fake',
          description: 'SLD split with fake SNI',
          args: '--lua-desync=fake:blob=fake_default_tls --lua-desync=multisplit:pos=1,midsld',
          youtube_ok: true,
          youtube_time_ms: 82,
          discord_ok: true,
          discord_time_ms: 105,
          score: 95,
          is_best: true,
        },
        {
          id: 'split2_pos1',
          name: 'Split2 Pos 1',
          description: 'Classic split',
          args: '--lua-desync=split2:pos=1',
          youtube_ok: true,
          youtube_time_ms: 140,
          discord_ok: false,
          discord_time_ms: 0,
          score: 65,
          is_best: false,
        },
      ],
    }

    const postSpy = vi.spyOn(api, 'apiPost').mockImplementation((path: string, body: any) => {
      if (path === 'zapret/blockcheck') {
        return Promise.resolve(mockBlockcheck)
      }
      if (path === 'zapret/action' && body?.action === 'apply_strategy') {
        return Promise.resolve({ success: true, message: 'Стратегия применена' })
      }
      return Promise.resolve({ success: true })
    })

    await act(async () => {
      root!.render(<Zapret notify={notifyMock} />)
    })

    // Find and click start blockcheck button
    const startBtn = Array.from(container?.querySelectorAll('button') || []).find((b) =>
      b.textContent?.includes('Запустить автоподбор стратегий')
    )
    expect(startBtn).toBeDefined()

    await act(async () => {
      startBtn?.click()
    })

    expect(postSpy).toHaveBeenCalledWith('zapret/blockcheck')

    const text = container?.textContent || ''
    expect(text).toContain('Лучшая рекомендуемая стратегия: Multisplit TLS + Fake')
    expect(text).toContain('Score: 95/100')
    expect(text).toContain('82 мс')
    expect(text).toContain('105 мс')

    // Find and click apply 1-click button
    const applyBestBtn = Array.from(container?.querySelectorAll('button') || []).find((b) =>
      b.textContent?.includes('Применить лучшую стратегию в 1 клик')
    )
    expect(applyBestBtn).toBeDefined()

    await act(async () => {
      applyBestBtn?.click()
    })

    expect(postSpy).toHaveBeenCalledWith('zapret/action', {
      action: 'apply_strategy',
      strategy_id: 'multisplit',
      custom_args: '--lua-desync=fake:blob=fake_default_tls --lua-desync=multisplit:pos=1,midsld',
    })
    expect(notifyMock).toHaveBeenCalledWith('Стратегия применена')
  })

  it('renders Smart TV / Кинотеатр profile card and toggles it', async () => {
    vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path === 'zapret/status') {
        return Promise.resolve(mockZapretStatus)
      }
      return Promise.resolve({})
    })

    const postSpy = vi.spyOn(api, 'apiPost').mockImplementation((path: string, body: any) => {
      if (path === 'zapret/action' && body?.action === 'toggle_feature') {
        return Promise.resolve({
          success: true,
          features: {
            ...mockZapretStatus.features!,
            smart_tv_mode: true,
          },
        })
      }
      return Promise.resolve({ success: true })
    })

    await act(async () => {
      root!.render(<Zapret notify={notifyMock} />)
    })

    const text = container?.textContent || ''
    expect(text).toContain('Профиль «Smart TV / Кинотеатр»')
    expect(text).toContain('4K HDR READY')
    expect(text).toContain('UDP 443 DROP (QUIC)')
    expect(text).toContain('redirector.googlevideo.com')

    const smartTvBtn = Array.from(container?.querySelectorAll('button') || []).find(
      (b) => b.title === 'Включить профиль Smart TV'
    )
    expect(smartTvBtn).toBeDefined()

    await act(async () => {
      smartTvBtn?.click()
    })

    expect(postSpy).toHaveBeenCalledWith('zapret/action', {
      action: 'toggle_feature',
      feature: 'smart_tv_mode',
      enabled: true,
      url: undefined,
    })
  })

  it('renders Community Hostlists card, toggles auto-update and triggers manual sync', async () => {
    vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path === 'zapret/status') {
        return Promise.resolve(mockZapretStatus)
      }
      return Promise.resolve({})
    })

    const postSpy = vi.spyOn(api, 'apiPost').mockImplementation((path: string, body: any) => {
      if (path === 'zapret/community-hostlist/sync') {
        return Promise.resolve({
          success: true,
          count: 14200,
          message: 'Синхронизировано 14200 доменов',
          last_updated: '2026-09-27 19:30:00',
        })
      }
      if (path === 'zapret/action' && body?.action === 'toggle_feature') {
        return Promise.resolve({
          success: true,
          features: {
            ...mockZapretStatus.features!,
            community_hostlist_auto_update: true,
          },
        })
      }
      return Promise.resolve({ success: true })
    })

    await act(async () => {
      root!.render(<Zapret notify={notifyMock} />)
    })

    const text = container?.textContent || ''
    expect(text).toContain('Автообновление списков (Community Hostlists)')
    expect(text).toContain('Автоматическое обновление списка каждые 24 часа')

    // Find and click sync button
    const syncBtn = Array.from(container?.querySelectorAll('button') || []).find((b) =>
      b.textContent?.includes('Синхронизировать сейчас')
    )
    expect(syncBtn).toBeDefined()

    await act(async () => {
      syncBtn?.click()
    })

    expect(postSpy).toHaveBeenCalledWith('zapret/community-hostlist/sync', {
      url: 'https://raw.githubusercontent.com/zapret-info/z-block/master/hosts.txt',
    })
    expect(notifyMock).toHaveBeenCalledWith('Синхронизировано 14200 доменов')

    // Find auto-update checkbox
    const checkboxes = Array.from(container?.querySelectorAll('input[type="checkbox"]') || []) as HTMLInputElement[]
    const autoUpdateCb = checkboxes.find(
      (cb) => cb.closest('label')?.textContent?.includes('Автоматическое обновление списка каждые 24 часа')
    )
    expect(autoUpdateCb).toBeDefined()

    await act(async () => {
      autoUpdateCb?.click()
    })

    expect(postSpy).toHaveBeenCalledWith('zapret/action', {
      action: 'toggle_feature',
      feature: 'community_hostlist_auto_update',
      enabled: true,
      url: undefined,
    })
  })

  it('displays dynamic /boost badge when zapret is stopped or all sites are disabled', async () => {
    // 1. When Zapret service is stopped
    const stoppedStatus: ZapretStatus = {
      ...mockZapretStatus,
      running: false,
    }
    vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path === 'zapret/status') {
        return Promise.resolve(stoppedStatus)
      }
      return Promise.resolve({})
    })

    await act(async () => {
      root!.render(<Zapret notify={notifyMock} />)
    })

    let text = container?.textContent || ''
    expect(text).toContain('⚪ /boost выключен (Zapret остановлен)')

    // 2. When Zapret is running but all custom sites are disabled
    const disabledSitesStatus: ZapretStatus = {
      ...mockZapretStatus,
      running: true,
      features: {
        ...mockZapretStatus.features!,
        custom_entries: [
          {
            domain: 'mysku.club',
            enabled: false,
            cdns: ['mysku-st.ru'],
          },
        ],
      },
    }
    vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path === 'zapret/status') {
        return Promise.resolve(disabledSitesStatus)
      }
      return Promise.resolve({})
    })

    await act(async () => {
      root!.render(<Zapret notify={notifyMock} />)
    })

    text = container?.textContent || ''
    expect(text).toContain('⚪ /boost выключен')
  })

  it('toggles all custom sites via master switch in /boost card header', async () => {
    vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path === 'zapret/status') {
        return Promise.resolve(mockZapretStatus)
      }
      return Promise.resolve({})
    })

    const postSpy = vi.spyOn(api, 'apiPost').mockImplementation((_path: string, _body: any) => {
      return Promise.resolve({
        success: true,
        features: {
          ...mockZapretStatus.features!,
          custom_entries: mockZapretStatus.features!.custom_entries!.map((e) => ({ ...e, enabled: false })),
        },
      })
    })

    await act(async () => {
      root!.render(<Zapret notify={notifyMock} />)
    })

    const masterSwitch = Array.from(container?.querySelectorAll('button') || []).find(
      (b) => b.title === 'Выключить все сайты /boost'
    )
    expect(masterSwitch).toBeDefined()

    await act(async () => {
      masterSwitch?.click()
    })

    expect(postSpy).toHaveBeenCalledWith('zapret/action', {
      action: 'toggle_custom_domain',
      domain: 'all',
      enabled: false,
    })
    expect(notifyMock).toHaveBeenCalledWith('⚪ Все сайты /boost выключены')
  })

  it('toggles main Zapret switch to stop service without reverting back to active', async () => {
    let callCount = 0
    vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path === 'zapret/status') {
        callCount++
        if (callCount === 1) {
          return Promise.resolve({
            ...mockZapretStatus,
            running: true,
            features: {
              ...mockZapretStatus.features!,
              enabled: true,
            },
          })
        }
        return Promise.resolve({
          ...mockZapretStatus,
          running: false,
          features: {
            ...mockZapretStatus.features!,
            enabled: false,
          },
        })
      }
      return Promise.resolve({})
    })

    const postSpy = vi.spyOn(api, 'apiPost').mockImplementation((_path: string, _body: any) => {
      return Promise.resolve({
        success: true,
        action: 'stop',
      })
    })

    await act(async () => {
      root!.render(<Zapret notify={notifyMock} />)
    })

    const mainSwitch = Array.from(container?.querySelectorAll('button') || []).find(
      (b) => b.title === 'Выключить Zapret'
    )
    expect(mainSwitch).toBeDefined()

    await act(async () => {
      mainSwitch?.click()
    })

    expect(postSpy).toHaveBeenCalledWith('zapret/action', {
      action: 'stop',
      enabled: false,
    })
    expect(notifyMock).toHaveBeenCalledWith('⚪ Служба Zapret остановлена')
  })

  it('treats zapret as inactive when features.enabled is false even if status.running is true', async () => {
    // Simulates case where orphan pid was detected by pidof, but user turned off Zapret (features.enabled = false)
    const ghostRunningStatus: ZapretStatus = {
      ...mockZapretStatus,
      running: true,
      features: {
        ...mockZapretStatus.features!,
        enabled: false,
      },
    }
    vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path === 'zapret/status') {
        return Promise.resolve(ghostRunningStatus)
      }
      return Promise.resolve({})
    })

    await act(async () => {
      root!.render(<Zapret notify={notifyMock} />)
    })

    const text = container?.textContent || ''
    // Main switch should show service is stopped
    expect(text).toContain('СЛУЖБА ВЫКЛЮЧЕНА')
    expect(text).not.toContain('СЛУЖБА АКТИВНА')

    // /boost badge should indicate Zapret is stopped
    expect(text).toContain('⚪ /boost выключен (Zapret остановлен)')

    // Custom domain badge should show Zapret остановлен
    expect(text).toContain('⚪ DIRECT (Zapret остановлен)')
  })

  it('displays stopped status badges on custom entries and strategy cards when zapret is not running', async () => {
    const stoppedStatus: ZapretStatus = {
      ...mockZapretStatus,
      running: false,
      features: {
        ...mockZapretStatus.features!,
        enabled: false,
        smart_tv_mode: true,
        community_hostlist_enabled: true,
      },
    }
    vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path === 'zapret/status') {
        return Promise.resolve(stoppedStatus)
      }
      return Promise.resolve({})
    })

    await act(async () => {
      root!.render(<Zapret notify={notifyMock} />)
    })

    const text = container?.textContent || ''
    expect(text).toContain('⚪ DIRECT (Zapret остановлен)')
    expect(text).toContain('⚪ SMART TV (Zapret остановлен)')
    expect(text).toContain('⚪ ВЫКЛЮЧЕН (Zapret остановлен)')
    expect(text).toContain('⚪ Демон остановлен')
    expect(text).toContain('Остановлена со службой')
  })
})

