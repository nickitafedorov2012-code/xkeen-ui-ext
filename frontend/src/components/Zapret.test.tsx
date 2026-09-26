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
    if (root && container) {
      act(() => {
        root!.unmount()
      })
      container.remove()
    }
  })

  it('renders all toggleable service blocks (YouTube, Discord, GitHub, Torrents, Hostlist)', async () => {
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
    expect(text).toContain('Универсальный веб-обход (Hostlist)')

    // Check tags preview in service cards
    expect(text).toContain('github.com')
    expect(text).toContain('rutracker.org')
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
})
