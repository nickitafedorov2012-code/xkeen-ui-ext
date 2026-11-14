import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import Gaming from './Gaming'
import AdGuard from './AdGuard'
import Settings from './Settings'
import Whitelist from './Whitelist'
import * as api from '../api'
import type { GamingStatus } from '../types'

// @ts-ignore
globalThis.IS_REACT_ACT_ENVIRONMENT = true

function makeBaseGamingStatus(overrides: Partial<GamingStatus> = {}): GamingStatus {
  return {
    config: {
      enabled: false,
      mode: 'known_services',
      target_server: 'Fastest',
      devices: [
        {
          mac: '00:11:22:33:44:55',
          ip: '192.168.2.115',
          ipv6: [],
          name: 'Gaming-PC',
          enabled: true,
        },
      ],
      smart_split: true,
      fix_nat_fake_ip: true,
      platforms: {
        discord: true,
        steam: true,
        playstation: false,
        xbox: false,
        battlenet: false,
        epicgames: false,
        ea: false,
        riot: false,
        supercell: false,
        nintendo: false,
        roblox: false,
        category_games: false,
      },
      custom_domains: [],
      smart_mode: false,
      ignored_game_conns: [],
    },
    active_server: 'Finland-Fastest',
    domains_count: 25,
    is_active: false,
    tunnel_status: {
      target: 'Fastest',
      active_node: 'Finland-Fastest',
      reachable: true,
      latency_ms: 35,
    },
    tcp_interception: false,
    udp_interception: false,
    ipv6_status: {
      supported: false,
      active: false,
      addresses: [],
    },
    active_device: null,
    ...overrides,
  }
}

const mockDevicesList = [
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

const mockWhitelistStatus = {
  enabled: true,
  auto_by_failover: true,
  strategy: 'strict_xkeen',
  target_server: null,
  follow_failover: true,
  excluded_devices: [] as string[],
  devices: [
    { mac: '11:22:33:44:55:01', ip: '192.168.1.10', name: 'PC Gaming', enabled: true },
    { mac: '11:22:33:44:55:02', ip: '192.168.1.20', name: 'MacBook Pro', enabled: true },
  ],
  gaming_suspended: false,
  custom_domains: [],
  domestic_catalog_count: 10,
  active_server: 'NL-Amsterdam-Fast',
  is_bs_server: true,
  whitelist_servers: ['srv_nl'],
  whitelist_providers: [],
}

describe('Bug 50 — UI conflict pairs integration', () => {
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
    container = null
    root = null
  })

  it('bug 50: gaming toggle when whitelist is active shows error and keeps toggle off', async () => {
    const baseStatus = makeBaseGamingStatus()
    vi.spyOn(api, 'apiGet').mockImplementation(async (path: string) => {
      if (path === 'gaming/status') return baseStatus
      if (path === 'devices') return mockDevicesList
      if (path === 'servers') return { servers: [{ id: 'srv_nl', name: 'NL Fast', is_active: true }] }
      if (path === 'gaming/smart-connections') return { connections: [], tracked_count: 0 }
      return {}
    })
    const postSpy = vi.spyOn(api, 'apiPost').mockImplementation(async (path: string) => {
      if (path === 'gaming/toggle') {
        throw new Error('Нельзя включить игровой режим при активных белых списках (БС)')
      }
      return { ok: true }
    })

    await act(async () => {
      root!.render(<Gaming notify={notifyMock} />)
    })

    const toggleBtn = Array.from(container!.querySelectorAll('button')).find((b) =>
      b.textContent?.includes('Включить игровой режим')
    )
    expect(toggleBtn).toBeDefined()

    await act(async () => {
      toggleBtn!.click()
    })

    expect(postSpy).toHaveBeenCalledWith('gaming/toggle', expect.objectContaining({ enabled: true }))
    expect(notifyMock).toHaveBeenCalledWith(
      expect.stringContaining('белых списках'),
      true
    )
    const afterBtn = Array.from(container!.querySelectorAll('button')).find((b) =>
      b.textContent?.includes('Включить игровой режим')
    )
    expect(afterBtn).toBeDefined()
    expect(container!.textContent).not.toContain('⏹ Выключить')
  })

  it('bug 50: AdGuard and AdBlock toggles are independent', async () => {
    const getSpy = vi.spyOn(api, 'apiGet').mockImplementation(async (rawPath: string) => {
      const path = rawPath.replace(/^\//, '')
      if (path === 'adguard/status') {
        return {
          running: true,
          version: '0.107.56',
          protection_enabled: true,
          dns_addresses: ['127.0.0.1'],
          dns_port: 5353,
          http_port: 3000,
          language: 'ru',
          start_time: 1720000000,
        }
      }
      if (path === 'adguard/health') {
        return {
          process_alive: true,
          api_ok: true,
          api_latency_ms: 4,
          dns_listener_ok: true,
          loop_risk: false,
          version: '0.107.56',
          protection_enabled: true,
        }
      }
      if (path === 'adguard/stats/overview') {
        return {
          running: true,
          protection_enabled: true,
          num_dns_queries: 100,
          num_blocked_filtering: 10,
          num_replaced_safebrowsing: 0,
          num_replaced_parental: 0,
          num_replaced_safesearch: 0,
          avg_processing_time_ms: 2.0,
          block_percentage: 10.0,
          active_rules_count: 1000,
          top_queried_domains: [],
          top_blocked_domains: [],
          top_clients: [],
        }
      }
      if (path === 'adguard/config') {
        return { enabled: true, host: '127.0.0.1', port: 3000, dns_port: 5353, username: 'admin', has_password: false }
      }
      if (path === 'adguard/backups') return []
      if (path === 'settings') {
        return {
          rci: { host: '127.0.0.1', port: 80, login: 'admin', password: '123', use_https: false, token: '' },
          mihomo: { host: '127.0.0.1', port: 9090, secret: '', config_path: '/opt/etc/mihomo/config.yaml', device_providers: [] },
          failover: {
            enabled: true,
            ping_threshold_ms: 300,
            priority_server: 'srv_nl',
            priority_chain: ['srv_nl'],
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
      }
      if (path === 'adblock') return { enabled: false }
      if (path === 'swap') {
        return {
          swap_supported: false,
          swap_active: false,
          swap_total_mb: 0,
          swap_used_mb: 0,
          swap_free_mb: 0,
          swap_file_exists: false,
          swap_file_path: '/opt/swapfile',
          swappiness: 10,
          opt_disk_free_mb: 500,
          opt_disk_total_mb: 1000,
          ram_total_mb: 256,
          ram_free_mb: 64,
          recommend_swap: false,
        }
      }
      if (path === 'servers') return { servers: [{ id: 'srv_nl', name: 'NL Fast', is_active: true }] }
      if (path === 'domains') return { force_domains: [], direct_domains: [], block_domains: [] }
      if (path === 'notifications') return { telegram: { enabled: false, bot_token: '', chat_id: '' }, webhook: { enabled: false, url: '' } }
      if (path === 'whitelist/detector') return { enabled: false, auto_switch: false, threshold: 3, whitelist_active: false }
      if (path === 'geo/info') return { geoip: null, geosite: null }
      if (path === 'status') return { xkeen_running: true, mihomo_running: true, active_server: 'srv_nl' }
      return {}
    })

    const postSpy = vi.spyOn(api, 'apiPost').mockImplementation(async (path: string, body?: any) => {
      if (path === 'adguard/protection') return { protection_enabled: Boolean(body?.enabled) }
      if (path === 'adblock/toggle') return { enabled: Boolean(body?.enabled) }
      return { ok: true }
    })

    // Part 1: toggling AdGuard protection must not call adblock/toggle
    await act(async () => {
      root!.render(<AdGuard notify={notifyMock} />)
    })

    const aghBtn = container!.querySelector('[data-testid="adguard-protection-toggle"]') as HTMLButtonElement | null
    expect(aghBtn).toBeTruthy()

    await act(async () => {
      aghBtn!.click()
    })

    expect(postSpy).toHaveBeenCalledWith('/adguard/protection', { enabled: false })
    expect(postSpy).not.toHaveBeenCalledWith('adblock/toggle', expect.anything())

    // Part 2: toggling AdBlock in Settings must not call adguard/*
    postSpy.mockClear()
    getSpy.mockClear()

    await act(async () => {
      root!.render(<Settings notify={notifyMock} refresh={vi.fn().mockResolvedValue(undefined)} />)
    })

    const adblockSpan = Array.from(container!.querySelectorAll('span')).find((s) =>
      s.textContent?.includes('Включить AdBlock для всех устройств')
    )
    const adblockCheckbox = (adblockSpan?.closest('label') ?? adblockSpan?.parentElement)?.querySelector(
      'input[type="checkbox"]'
    ) as HTMLInputElement | null
    expect(adblockCheckbox).toBeTruthy()

    await act(async () => {
      adblockCheckbox!.click()
    })

    expect(postSpy).toHaveBeenCalledWith('adblock/toggle', { enabled: true })
    const adguardCalls = postSpy.mock.calls.filter(([p]) => String(p).startsWith('adguard/'))
    expect(adguardCalls).toHaveLength(0)
  })

  it('bug 50: excluding device from whitelist during gaming mode is rejected with clear message', async () => {
    vi.spyOn(api, 'apiGet').mockImplementation(async (path: string) => {
      if (path === 'whitelist') return { ...mockWhitelistStatus, gaming_suspended: true }
      if (path === 'devices') return { devices: mockDevicesList }
      if (path === 'servers') return { servers: [{ id: 'srv_nl', name: 'Netherlands Fast', is_active: true, is_whitelist: true }] }
      if (path === 'whitelist/catalog') return { total: 0, domains: [], categories: {}, items: [] }
      return {}
    })

    const postSpy = vi.spyOn(api, 'apiPost').mockImplementation(async (path: string) => {
      if (path === 'whitelist/exclude') {
        throw new Error('Исключение устройства из БС запрещено, пока активен игровой режим')
      }
      return { ok: true }
    })

    await act(async () => {
      root!.render(<Whitelist notify={notifyMock} />)
    })

    const excludeBtn = container!.querySelector('[data-testid="btn-exclude-11:22:33:44:55:01"]') as HTMLButtonElement | null
    expect(excludeBtn).toBeTruthy()

    await act(async () => {
      excludeBtn!.click()
    })

    expect(postSpy).toHaveBeenCalledWith('whitelist/exclude', {
      id: '11:22:33:44:55:01',
      excluded: true,
    })
    expect(notifyMock).toHaveBeenCalledWith(
      expect.stringContaining('игровой режим'),
      true
    )
    expect(container!.querySelector('[data-testid="btn-exclude-11:22:33:44:55:01"]')).toBeTruthy()
  })

  it('bug 50: smart mode toggle and "Не реагировать" are not overwritten by subsequent save', async () => {
    const baseStatus: any = {
      ...makeBaseGamingStatus(),
      recent_gaming_conns: [
        {
          host: 'game.example.com',
          destination: '9.9.9.9:7777',
          network: 'UDP',
          last_seen: 'только что',
          ignored: false,
        },
      ],
    }
    vi.spyOn(api, 'apiGet').mockImplementation(async (path: string) => {
      if (path === 'gaming/status') return baseStatus
      if (path === 'devices') return { devices: mockDevicesList }
      if (path === 'servers') return { proxies: [] }
      return {}
    })

    const postSpy = vi.spyOn(api, 'apiPost').mockImplementation(async (path: string) => {
      if (path === 'gaming/save') {
        return {
          saved: true,
          config: {
            ...baseStatus.config,
            smart_mode: true,
            ignored_game_conns: ['game.example.com'],
          },
        }
      }
      return { saved: true }
    })

    await act(async () => {
      root!.render(<Gaming notify={notifyMock} />)
    })

    const switchInput = container!.querySelector('label.switch input[type="checkbox"]') as HTMLInputElement | null
    expect(switchInput).toBeTruthy()
    await act(async () => {
      switchInput!.click()
    })
    expect(postSpy).toHaveBeenCalledWith('gaming/save', { smart_mode: true })

    postSpy.mockClear()
    const ignoreBtn = Array.from(container!.querySelectorAll('button')).find((b) =>
      b.textContent?.includes('Не реагировать')
    )
    expect(ignoreBtn).toBeDefined()
    await act(async () => {
      ignoreBtn!.click()
    })
    expect(postSpy).toHaveBeenCalledWith('gaming/ignore-conn', {
      target: 'game.example.com',
      remove: false,
    })

    postSpy.mockClear()
    const saveBtn = Array.from(container!.querySelectorAll('button')).find((b) =>
      b.textContent?.includes('Сохранить и применить')
    )
    expect(saveBtn).toBeDefined()
    await act(async () => {
      saveBtn!.click()
    })

    expect(postSpy).toHaveBeenCalledTimes(1)
    const [savePath, saveBody] = postSpy.mock.calls[0]
    expect(savePath).toBe('gaming/save')
    expect(saveBody).not.toHaveProperty('ignored_game_conns')
    expect(saveBody).not.toHaveProperty('gaming_suspended')
  })

  it('bug 50: pinning whitelist node sends single device_patch without touching other devices IPs', async () => {
    vi.spyOn(api, 'apiGet').mockImplementation(async (path: string) => {
      if (path === 'whitelist') return mockWhitelistStatus
      if (path === 'devices') return { devices: mockDevicesList }
      if (path === 'servers') {
        return {
          servers: [
            { id: 'srv_nl', name: 'Netherlands Fast', protocol: 'vless', host: 'nl.proxy.com', port: 443, is_active: true, is_whitelist: true },
            { id: 'srv_de', name: 'Germany Stable', protocol: 'vless', host: 'de.proxy.com', port: 443, is_active: false, is_whitelist: true },
          ],
        }
      }
      if (path === 'whitelist/catalog') return { total: 0, domains: [], categories: {}, items: [] }
      return {}
    })

    const postSpy = vi.spyOn(api, 'apiPost').mockResolvedValue({ ok: true })

    await act(async () => {
      root!.render(<Whitelist notify={notifyMock} />)
    })

    const selectEl = container!.querySelector('[data-testid="pin-server-11:22:33:44:55:01"]') as HTMLSelectElement | null
    expect(selectEl).toBeTruthy()

    await act(async () => {
      selectEl!.value = 'srv_nl'
      selectEl!.dispatchEvent(new Event('change', { bubbles: true }))
    })

    expect(postSpy).toHaveBeenCalledTimes(1)
    const [path, body] = postSpy.mock.calls[0]
    expect(path).toBe('whitelist')
    expect(body).toEqual({
      device_patch: {
        mac: '11:22:33:44:55:01',
        ip: '192.168.1.10',
        enabled: true,
        name: 'PC Gaming',
        server: 'srv_nl',
      },
    })
    expect(body).not.toHaveProperty('devices')
  })
})
