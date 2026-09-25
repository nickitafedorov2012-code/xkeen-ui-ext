import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import Gaming from './Gaming'
import * as api from '../api'
import type { GamingStatus } from '../types'

// @ts-ignore
globalThis.IS_REACT_ACT_ENVIRONMENT = true

const mockStatus: GamingStatus = {
  config: {
    enabled: true,
    target_server: 'Fastest',
    smart_split: true,
    fix_nat_fake_ip: true,
    platforms: {
      discord: true,
      steam: true,
      playstation: true,
      xbox: true,
      battlenet: true,
      epicgames: true,
      ea: true,
      riot: true,
      supercell: true,
      nintendo: true,
      roblox: true,
      category_games: false,
    },
    custom_domains: ['mygame.net'],
  },
  active_server: 'Finland-Fastest',
  domains_count: 35,
}

describe('Gaming Component', () => {
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

  it('renders Gaming component with active status, platforms, and server selector', async () => {
    vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path === 'gaming/status') {
        return Promise.resolve(mockStatus)
      }
      if (path === 'servers') {
        return Promise.resolve({
          proxies: [
            { id: 'Finland-Fastest', name: 'Finland VLESS', ping_ms: 38 },
            { id: 'Germany-Node', name: 'Germany Shadowsocks', ping_ms: 55 },
          ],
        })
      }
      return Promise.reject(new Error('not mocked'))
    })

    const notify = vi.fn()

    await act(async () => {
      root!.render(<Gaming notify={notify} />)
    })

    expect(container?.innerHTML).toContain('Игровой режим (Gaming Mode)')
    expect(container?.innerHTML).toContain('АКТИВЕН')
    expect(container?.innerHTML).toContain('Discord')
    expect(container?.innerHTML).toContain('Steam')
    expect(container?.innerHTML).toContain('Xbox Live &amp; Game Pass')
    expect(container?.innerHTML).toContain('Fix 0x80a40401')
    expect(container?.innerHTML).toContain('Supercell / Brawl Stars')
    expect(container?.innerHTML).toContain('PlayStation Network (PSN)')
    expect(container?.innerHTML).toContain('Battle.net / Blizzard')
    expect(container?.innerHTML).toContain('EA App / Origin')
    expect(container?.innerHTML).toContain('Finland-Fastest')
  })

  it('calls apiPost when toggling gaming mode', async () => {
    vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path === 'gaming/status') {
        return Promise.resolve(mockStatus)
      }
      if (path === 'servers') {
        return Promise.resolve({ proxies: [] })
      }
      return Promise.reject(new Error('not mocked'))
    })

    const postSpy = vi.spyOn(api, 'apiPost').mockResolvedValue({ enabled: false })
    const notify = vi.fn()

    await act(async () => {
      root!.render(<Gaming notify={notify} />)
    })

    const toggleBtn = Array.from(container!.querySelectorAll('button')).find((b) =>
      b.textContent?.includes('Выключить')
    )
    expect(toggleBtn).toBeDefined()

    await act(async () => {
      toggleBtn?.click()
    })

    expect(postSpy).toHaveBeenCalledWith('gaming/toggle', { enabled: false })
    expect(notify).toHaveBeenCalledWith('⚪ Игровой режим выключен')
  })

  it('runs ping test when ping button is clicked', async () => {
    vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path === 'gaming/status') {
        return Promise.resolve(mockStatus)
      }
      if (path === 'servers') {
        return Promise.resolve({ proxies: [] })
      }
      return Promise.reject(new Error('not mocked'))
    })

    const pingSpy = vi.spyOn(api, 'apiPost').mockImplementation((path: string) => {
      if (path === 'gaming/ping') {
        return Promise.resolve({
          results: [
            { name: 'Steam', host: 'steamcommunity.com', ping_ms: 32, available: true },
            { name: 'Discord', host: 'discord.com', ping_ms: 28, available: true },
          ],
        })
      }
      return Promise.resolve({})
    })

    const notify = vi.fn()

    await act(async () => {
      root!.render(<Gaming notify={notify} />)
    })

    const pingBtn = Array.from(container!.querySelectorAll('button')).find((b) =>
      b.textContent?.includes('Замерить пинг')
    )
    expect(pingBtn).toBeDefined()

    await act(async () => {
      pingBtn?.click()
    })

    expect(pingSpy).toHaveBeenCalledWith('gaming/ping', {})
    expect(container?.innerHTML).toContain('steamcommunity.com')
    expect(container?.innerHTML).toContain('32 ms')
  })
})
