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
    mode: 'compatibility',
    target_server: 'Fastest',
    devices: [
      {
        mac: '00:11:22:33:44:55',
        ip: '192.168.2.115',
        ipv6: ['2001:db8::10'],
        name: 'Gaming-PC',
        enabled: true,
      },
    ],
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
  is_active: true,
  tunnel_status: {
    target: 'Fastest',
    active_node: 'Finland-Fastest',
    reachable: true,
    latency_ms: 38,
  },
  tcp_interception: true,
  udp_interception: true,
  ipv6_status: {
    supported: true,
    active: true,
    addresses: ['2001:db8::10'],
  },
  active_device: {
    mac: '00:11:22:33:44:55',
    ip: '192.168.2.115',
    ipv6: ['2001:db8::10'],
    name: 'Gaming-PC',
    enabled: true,
  },
  real_connections: [
    {
      id: 'conn-1',
      host: 'valve102.steamserver.net',
      destination: '162.254.192.1:27015',
      network: 'UDP',
      chains: ['🎮 Gaming', 'Finland-Fastest'],
      rule: 'SRC-IP-CIDR',
      download: 1048576,
      upload: 524288,
    },
    {
      id: 'conn-2',
      host: 'discord.media',
      destination: '66.22.244.1:50001',
      network: 'UDP',
      chains: ['🎮 Gaming'],
      rule: 'SRC-IP-CIDR',
      download: 2097152,
      upload: 1048576,
    },
  ],
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
      if (path === 'devices') {
        return Promise.resolve({
          devices: [
            {
              mac: '00:11:22:33:44:55',
              name: 'Gaming-PC',
              ip: '192.168.2.115',
              ipv6: ['2001:db8::10'],
              policy: 'default',
              policy_name: 'Политика по умолчанию',
              online: true,
              interface: 'Bridge0',
              is_current_device: true,
              rxbytes: 0,
              txbytes: 0,
              speed_limit_kbps: 0,
              current_server: 'Finland-Fastest',
            },
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
      if (path === 'devices') {
        return Promise.resolve({ devices: [] })
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
      if (path === 'devices') {
        return Promise.resolve({ devices: [] })
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

  it('renders "Включить совместимость" button when disabled and passes device_mac to toggle', async () => {
    const disabledStatus: GamingStatus = {
      ...mockStatus,
      config: {
        ...mockStatus.config,
        enabled: false,
        mode: 'compatibility',
        devices: [],
      },
      active_device: null,
      is_active: false,
    }

    vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path === 'gaming/status') return Promise.resolve(disabledStatus)
      if (path === 'servers') return Promise.resolve({ proxies: [] })
      if (path === 'devices') {
        return Promise.resolve({
          devices: [
            {
              mac: 'aa:bb:cc:dd:ee:ff',
              name: 'My-Console',
              ip: '192.168.2.200',
              policy: 'default',
              policy_name: '',
              online: true,
              interface: 'br0',
              is_current_device: true,
              rxbytes: 0,
              txbytes: 0,
              speed_limit_kbps: 0,
              current_server: '',
            },
          ],
        })
      }
      return Promise.reject(new Error('not mocked'))
    })

    const postSpy = vi.spyOn(api, 'apiPost').mockResolvedValue({ enabled: true })
    const notify = vi.fn()

    await act(async () => {
      root!.render(<Gaming notify={notify} />)
    })

    const enableCompatBtn = Array.from(container!.querySelectorAll('button')).find((b) =>
      b.textContent?.includes('Включить совместимость')
    )
    expect(enableCompatBtn).toBeDefined()

    await act(async () => {
      enableCompatBtn?.click()
    })

    expect(postSpy).toHaveBeenCalledWith('gaming/toggle', {
      enabled: true,
      mode: 'compatibility',
      device_mac: 'aa:bb:cc:dd:ee:ff',
      target_server: 'Fastest',
    })
  })

  it('displays warning that known services mode does not cover unknown games', async () => {
    vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path === 'gaming/status') return Promise.resolve(mockStatus)
      if (path === 'servers') return Promise.resolve({ proxies: [] })
      if (path === 'devices') return Promise.resolve({ devices: [] })
      return Promise.reject(new Error('not mocked'))
    })

    await act(async () => {
      root!.render(<Gaming notify={vi.fn()} />)
    })

    expect(container?.innerHTML).toContain('не охватывает неизвестные игры')
    expect(container?.innerHTML).toContain('category-games')
  })

  it('displays verified TCP/UDP interception, IPv6, and real connections through Mihomo', async () => {
    vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path === 'gaming/status') return Promise.resolve(mockStatus)
      if (path === 'servers') return Promise.resolve({ proxies: [] })
      if (path === 'devices') return Promise.resolve({ devices: [] })
      return Promise.reject(new Error('not mocked'))
    })

    await act(async () => {
      root!.render(<Gaming notify={vi.fn()} />)
    })

    expect(container?.innerHTML).toContain('TCP: ✓ OK')
    expect(container?.innerHTML).toContain('UDP: ✓ OK')
    expect(container?.innerHTML).toContain('IPv6 маршрут')
    expect(container?.innerHTML).toContain('valve102.steamserver.net')
    expect(container?.innerHTML).toContain('discord.media')
  })

  it('does NOT consider mode active just because button was pressed if unverified', async () => {
    const unverifiedStatus: GamingStatus = {
      ...mockStatus,
      config: {
        ...mockStatus.config,
        enabled: true,
      },
      is_active: false,
      verification_error: 'Игровой туннель недоступен или не отвечает на запросы',
    }

    vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path === 'gaming/status') return Promise.resolve(unverifiedStatus)
      if (path === 'servers') return Promise.resolve({ proxies: [] })
      if (path === 'devices') return Promise.resolve({ devices: [] })
      return Promise.reject(new Error('not mocked'))
    })

    await act(async () => {
      root!.render(<Gaming notify={vi.fn()} />)
    })

    expect(container?.innerHTML).toContain('МАРШРУТ НЕ ПОДТВЕРЖДЕН')
    expect(container?.innerHTML).toContain('Игровой туннель недоступен или не отвечает на запросы')
    expect(container?.innerHTML).not.toContain('🟢 АКТИВЕН')
  })

  it('activates known services mode from additional settings button', async () => {
    const disabledStatus: GamingStatus = {
      ...mockStatus,
      config: {
        ...mockStatus.config,
        enabled: false,
        mode: 'compatibility',
        devices: [],
      },
      active_device: null,
      is_active: false,
    }

    vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path === 'gaming/status') return Promise.resolve(disabledStatus)
      if (path === 'servers') return Promise.resolve({ proxies: [] })
      if (path === 'devices') {
        return Promise.resolve({
          devices: [
            {
              mac: 'aa:bb:cc:dd:ee:ff',
              name: 'My-Console',
              ip: '192.168.2.200',
              policy: 'default',
              policy_name: '',
              online: true,
              interface: 'br0',
              is_current_device: true,
              rxbytes: 0,
              txbytes: 0,
              speed_limit_kbps: 0,
              current_server: '',
            },
          ],
        })
      }
      return Promise.reject(new Error('not mocked'))
    })

    const postSpy = vi.spyOn(api, 'apiPost').mockResolvedValue({ enabled: true })

    await act(async () => {
      root!.render(<Gaming notify={vi.fn()} />)
    })

    const enableKnownBtn = Array.from(container!.querySelectorAll('button')).find((b) =>
      b.textContent?.includes('Включить режим известных сервисов')
    )
    expect(enableKnownBtn).toBeDefined()

    await act(async () => {
      enableKnownBtn?.click()
    })

    expect(postSpy).toHaveBeenCalledWith('gaming/toggle', {
      enabled: true,
      mode: 'known_services',
      device_mac: 'aa:bb:cc:dd:ee:ff',
      target_server: 'Fastest',
    })
  })

  it('always enforces compatibility mode when main button is clicked', async () => {
    const knownStatus: GamingStatus = {
      ...mockStatus,
      config: {
        ...mockStatus.config,
        enabled: false,
        mode: 'known_services',
        devices: [],
      },
      active_device: null,
      is_active: false,
    }

    vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path === 'gaming/status') return Promise.resolve(knownStatus)
      if (path === 'servers') return Promise.resolve({ proxies: [] })
      if (path === 'devices') {
        return Promise.resolve({
          devices: [
            {
              mac: '11:22:33:44:55:66',
              name: 'My-Console',
              ip: '192.168.2.205',
              policy: 'default',
              policy_name: '',
              online: true,
              interface: 'br0',
              is_current_device: true,
              rxbytes: 0,
              txbytes: 0,
              speed_limit_kbps: 0,
              current_server: '',
            },
          ],
        })
      }
      return Promise.reject(new Error('not mocked'))
    })

    const postSpy = vi.spyOn(api, 'apiPost').mockResolvedValue({ enabled: true })

    await act(async () => {
      root!.render(<Gaming notify={vi.fn()} />)
    })

    // The primary button should still be "Включить совместимость"
    const mainBtn = Array.from(container!.querySelectorAll('button')).find((b) =>
      b.textContent?.includes('Включить совместимость')
    )
    expect(mainBtn).toBeDefined()

    await act(async () => {
      mainBtn?.click()
    })

    expect(postSpy).toHaveBeenCalledWith('gaming/toggle', {
      enabled: true,
      mode: 'compatibility',
      device_mac: '11:22:33:44:55:66',
      target_server: 'Fastest',
    })
  })
})
