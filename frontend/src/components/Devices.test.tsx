import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import Devices from './Devices'
import * as api from '../api'

// @ts-ignore
globalThis.IS_REACT_ACT_ENVIRONMENT = true

const mockDevices = [
  {
    mac: 'AA:BB:CC:DD:EE:01',
    name: 'Smart TV Samsung',
    ip: '192.168.1.100',
    policy: 'default',
    policy_name: 'Основная',
    online: true,
    interface: 'eth0',
    is_current_device: false,
    rxbytes: 1000,
    txbytes: 500,
    speed_limit_kbps: 0,
    current_server: 'default',
  },
  {
    mac: 'AA:BB:CC:DD:EE:02',
    name: 'Work Laptop',
    ip: '192.168.1.101',
    policy: 'default',
    policy_name: 'Основная',
    online: true,
    interface: 'wifi',
    is_current_device: true,
    rxbytes: 2000,
    txbytes: 1000,
    speed_limit_kbps: 0,
    current_server: 'default',
  },
]

describe('Devices Component — Per-Device Zapret DPI (Feature 3)', () => {
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

  it('renders Zapret (DPI) column header and per-device toggle buttons with glowing active state', async () => {
    vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path === 'devices') {
        return Promise.resolve({ devices: mockDevices })
      }
      if (path === 'policies') {
        return Promise.resolve({ policies: [{ id: 'default', name: 'Основная', is_default: true }] })
      }
      if (path === 'servers') {
        return Promise.resolve({ servers: [] })
      }
      if (path === 'routing') {
        return Promise.resolve({ assignments: [] })
      }
      if (path === 'device-routing') {
        return Promise.resolve({ routing: {}, device_failover_enabled: false })
      }
      if (path === 'devices/zapret') {
        // Exclude Laptop (192.168.1.101), keep Samsung TV enabled
        return Promise.resolve({ excluded_devices: ['192.168.1.101'] })
      }
      if (path === 'devices/traffic') {
        return Promise.resolve({ download_total: 0, upload_total: 0, devices: {} })
      }
      return Promise.resolve({})
    })

    const postSpy = vi.spyOn(api, 'apiPost').mockResolvedValue({ success: true })

    await act(async () => {
      root!.render(<Devices notify={notifyMock} />)
    })

    const text = container?.textContent || ''
    expect(text).toContain('Zapret (DPI)')
    expect(text).toContain('Smart TV Samsung')
    expect(text).toContain('Work Laptop')

    // 1. Verify restored Zapret DPI buttons
    const zapretButtons = Array.from(
      container?.querySelectorAll('button.device-zapret-btn') || []
    ) as HTMLButtonElement[]
    expect(zapretButtons.length).toBe(2)

    // First button (Smart TV - enabled): active class, "Zapret ON"
    const tvBtn = zapretButtons[0]
    expect(tvBtn.className).toContain('active')
    expect(tvBtn.textContent).toContain('Zapret ON')

    // Second button (Work Laptop - excluded): inactive class, "Zapret OFF"
    const laptopBtn = zapretButtons[1]
    expect(laptopBtn.className).toContain('inactive')
    expect(laptopBtn.textContent).toContain('Zapret OFF')

    // Click on Smart TV button to disable Zapret for it
    await act(async () => {
      tvBtn.click()
    })

    expect(postSpy).toHaveBeenCalledWith('devices/zapret-toggle', {
      ip: '192.168.1.100',
      mac: 'AA:BB:CC:DD:EE:01',
      enabled: false,
    })

    // Now TV button should have switched to inactive
    expect(tvBtn.className).toContain('inactive')
    expect(tvBtn.textContent).toContain('Zapret OFF')

    // Click on Laptop button to enable Zapret for it
    await act(async () => {
      laptopBtn.click()
    })

    expect(postSpy).toHaveBeenCalledWith('devices/zapret-toggle', {
      ip: '192.168.1.101',
      mac: 'AA:BB:CC:DD:EE:02',
      enabled: true,
    })

    // Now Laptop button should have switched to active
    expect(laptopBtn.className).toContain('active')
    expect(laptopBtn.textContent).toContain('Zapret ON')

    // 2. Verify new Per-Device Gaming Mode styled buttons (like Zapret)
    const gamingButtons = Array.from(
      container?.querySelectorAll('button.device-gaming-btn') || []
    ) as HTMLButtonElement[]
    expect(gamingButtons.length).toBe(2)

    // Toggle gaming on TV
    await act(async () => {
      gamingButtons[0].click()
    })

    expect(postSpy).toHaveBeenCalledWith('gaming/toggle', {
      enabled: true,
      device_mac: 'AA:BB:CC:DD:EE:01',
    })
  })

  it('correctly handles offline devices with empty IP when toggling Zapret off and on', async () => {
    const offlineDevice = {
      mac: 'CC:DD:EE:FF:00:99',
      name: 'Old Offline Console',
      ip: '',
      policy: 'default',
      policy_name: 'Основная',
      online: false,
      interface: 'eth0',
      is_current_device: false,
      rxbytes: 0,
      txbytes: 0,
      speed_limit_kbps: 0,
      current_server: 'default',
    }

    vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path === 'devices') {
        return Promise.resolve({ devices: [offlineDevice] })
      }
      if (path === 'policies') {
        return Promise.resolve({ policies: [{ id: 'default', name: 'Основная', is_default: true }] })
      }
      if (path === 'servers') {
        return Promise.resolve({ servers: [] })
      }
      if (path === 'routing') {
        return Promise.resolve({ assignments: [] })
      }
      if (path === 'device-routing') {
        return Promise.resolve({ routing: {}, device_failover_enabled: false })
      }
      if (path === 'devices/zapret') {
        return Promise.resolve({ excluded_devices: [] })
      }
      if (path === 'devices/traffic') {
        return Promise.resolve({ download_total: 0, upload_total: 0, devices: {} })
      }
      return Promise.resolve({})
    })

    const postSpy = vi.spyOn(api, 'apiPost').mockResolvedValue({ success: true })

    await act(async () => {
      root!.render(<Devices notify={notifyMock} />)
    })

    // Expand offline accordion if collapsed
    const accordion = container?.querySelector('.offline-accordion-row') as HTMLDivElement
    if (accordion) {
      await act(async () => {
        accordion.click()
      })
    }

    const zapretBtn = container?.querySelector('button.device-zapret-btn') as HTMLButtonElement
    expect(zapretBtn).toBeDefined()
    expect(zapretBtn.className).toContain('active')
    expect(zapretBtn.textContent).toContain('Zapret ON')

    // Click to disable Zapret for this device without IP (only MAC)
    await act(async () => {
      zapretBtn.click()
    })

    expect(postSpy).toHaveBeenCalledWith('devices/zapret-toggle', {
      ip: '',
      mac: 'CC:DD:EE:FF:00:99',
      enabled: false,
    })

    // Button MUST optimistically switch to inactive even without IP
    expect(zapretBtn.className).toContain('inactive')
    expect(zapretBtn.textContent).toContain('Zapret OFF')
  })

  it('renders 100% Gaming badge for compatibility mode gaming devices and synchronizes server', async () => {
    vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path === 'devices') {
        return Promise.resolve({ devices: mockDevices })
      }
      if (path === 'policies') {
        return Promise.resolve({ policies: [{ id: 'default', name: 'Основная', is_default: true }] })
      }
      if (path === 'servers') {
        return Promise.resolve({
          servers: [
            { id: 'node_de', name: 'Frankfurt-1', protocol: 'vless', host: '1.2.3.4', port: 443, ping_ms: 45, is_active: false, is_priority: false },
            { id: 'node_nl', name: 'Amsterdam-1', protocol: 'vless', host: '1.2.3.5', port: 443, ping_ms: 50, is_active: false, is_priority: false },
          ]
        })
      }
      if (path === 'routing') {
        return Promise.resolve({
          assignments: [
            { ip: '192.168.1.100', group: '🎮 Gaming', current_server: 'node_de', is_gaming: true },
          ]
        })
      }
      if (path === 'device-routing') {
        return Promise.resolve({ routing: {}, device_failover_enabled: false })
      }
      if (path === 'devices/zapret') {
        return Promise.resolve({ excluded_devices: [] })
      }
      if (path === 'gaming/status') {
        return Promise.resolve({
          is_active: true,
          enabled: true,
          config: {
            enabled: true,
            mode: 'compatibility',
            devices: [{ mac: 'AA:BB:CC:DD:EE:01', enabled: true, server: 'node_de' }]
          }
        })
      }
      if (path === 'devices/traffic') {
        return Promise.resolve({ download_total: 0, upload_total: 0, devices: {} })
      }
      return Promise.resolve({})
    })

    const postSpy = vi.spyOn(api, 'apiPost').mockResolvedValue({ applied: 1 })

    await act(async () => {
      root!.render(<Devices notify={notifyMock} />)
    })

    // Check badge 🎮 100% Gaming for 192.168.1.100
    const gamingBadge = container?.querySelector('[data-testid="badge-gaming-compat"]') as HTMLElement | null
    expect(gamingBadge).not.toBeNull()
    expect(gamingBadge?.textContent).toContain('100% Gaming')

    // Change server dropdown for the gaming device
    const serverSelects = container?.querySelectorAll('select.device-select')
    expect(serverSelects?.length).toBeGreaterThan(0)

    const select = serverSelects?.[1] as HTMLSelectElement | undefined // second select in the row (first is policy)
    if (select) {
      await act(async () => {
        select.value = 'node_nl'
        select.dispatchEvent(new Event('change', { bubbles: true }))
      })

      expect(postSpy).toHaveBeenCalledWith('routing', {
        assignments: [{ ip: '192.168.1.100', name: 'Smart TV Samsung', server: 'node_nl' }]
      })
    }
  })

  it('anti-regression: renders exactly 11 table columns with aligned buttons and actions without overflowing', async () => {
    vi.spyOn(api, 'apiGet').mockImplementation((endpoint: string) => {
      if (endpoint === 'devices') return Promise.resolve({ devices: mockDevices })
      if (endpoint === 'policies') return Promise.resolve({ policies: [{ id: 'default', name: 'Основная', is_default: true }] })
      if (endpoint === 'servers') return Promise.resolve({ servers: [] })
      if (endpoint === 'routing') return Promise.resolve({ assignments: [] })
      if (endpoint === 'device-routing') return Promise.resolve({ routing: {}, device_failover_enabled: false })
      if (endpoint === 'devices/zapret') return Promise.resolve({ excluded_devices: [] })
      if (endpoint === 'devices/traffic') return Promise.resolve({ download_total: 0, upload_total: 0, devices: {} })
      if (endpoint === 'whitelist/status') return Promise.resolve({ enabled: true, devices: [{ mac: 'AA:BB:CC:DD:EE:01', ip: '192.168.1.100', enabled: true }] })
      return Promise.resolve({})
    })

    await act(async () => {
      root!.render(<Devices notify={notifyMock} />)
    })

    const ths = container?.querySelectorAll('table.devices-table thead th')
    expect(ths?.length).toBe(11)

    const firstRowTds = container?.querySelector('table.devices-table tbody tr.device-row')?.querySelectorAll('td')
    expect(firstRowTds?.length).toBe(11)

    // Check Whitelist cell (10th column, index 9) contains device-whitelist-btn
    const wlTd = firstRowTds?.[9]
    const wlBtn = wlTd?.querySelector('button.device-whitelist-btn')
    expect(wlBtn).toBeTruthy()
    expect(wlBtn?.className).toContain('active')
    expect(wlBtn?.textContent).toContain('БС ON')

    // Check actions cell (last td, index 10) contains both gear button and schedule button
    const actionsTd = firstRowTds?.[10]
    const actionButtons = actionsTd?.querySelectorAll('button.device-gear-btn')
    expect(actionButtons?.length).toBe(2)
  })

  it('toggles whitelist mode for a device when БС button is clicked', async () => {
    vi.spyOn(api, 'apiGet').mockImplementation((endpoint: string) => {
      if (endpoint === 'devices') return Promise.resolve({ devices: mockDevices })
      if (endpoint === 'policies') return Promise.resolve({ policies: [{ id: 'default', name: 'Основная', is_default: true }] })
      if (endpoint === 'servers') return Promise.resolve({ servers: [] })
      if (endpoint === 'routing') return Promise.resolve({ assignments: [] })
      if (endpoint === 'device-routing') return Promise.resolve({ routing: {}, device_failover_enabled: false })
      if (endpoint === 'devices/zapret') return Promise.resolve({ excluded_devices: [] })
      if (endpoint === 'devices/traffic') return Promise.resolve({ download_total: 0, upload_total: 0, devices: {} })
      if (endpoint === 'whitelist/status') return Promise.resolve({ enabled: false, devices: [] })
      return Promise.resolve({})
    })

    const postSpy = vi.spyOn(api, 'apiPost').mockResolvedValue({ toggled: true })

    await act(async () => {
      root!.render(<Devices notify={notifyMock} />)
    })

    const wlButtons = container?.querySelectorAll('button.device-whitelist-btn')
    expect(wlButtons?.length).toBe(2)

    // First button is inactive
    const firstWlBtn = wlButtons![0] as HTMLButtonElement
    expect(firstWlBtn.className).toContain('inactive')
    expect(firstWlBtn.textContent).toContain('БС OFF')

    await act(async () => {
      firstWlBtn.click()
    })

    expect(postSpy).toHaveBeenCalledWith('whitelist/device/toggle', {
      ip: '192.168.1.100',
      mac: 'AA:BB:CC:DD:EE:01',
      enabled: true,
    })
  })

  it('anti-regression: cleanly toggles OFF whitelist mode for active device (is_whitelist: true) without lock', async () => {
    const devicesWithWl = [
      { ...mockDevices[0], is_whitelist: true },
      mockDevices[1],
    ]

    vi.spyOn(api, 'apiGet').mockImplementation((endpoint: string) => {
      if (endpoint === 'devices') return Promise.resolve({ devices: devicesWithWl })
      if (endpoint === 'policies') return Promise.resolve({ policies: [{ id: 'default', name: 'Основная', is_default: true }] })
      if (endpoint === 'servers') return Promise.resolve({ servers: [] })
      if (endpoint === 'routing') return Promise.resolve({ assignments: [] })
      if (endpoint === 'device-routing') return Promise.resolve({ routing: {}, device_failover_enabled: false })
      if (endpoint === 'devices/zapret') return Promise.resolve({ excluded_devices: [] })
      if (endpoint === 'devices/traffic') return Promise.resolve({ download_total: 0, upload_total: 0, devices: {} })
      if (endpoint === 'whitelist/status') return Promise.resolve({
        enabled: true,
        devices: [{ mac: 'AA:BB:CC:DD:EE:01', ip: '192.168.1.100', enabled: true }],
      })
      return Promise.resolve({})
    })

    const postSpy = vi.spyOn(api, 'apiPost').mockResolvedValue({ toggled: true })

    await act(async () => {
      root!.render(<Devices notify={notifyMock} />)
    })

    const wlButtons = container?.querySelectorAll('button.device-whitelist-btn')
    expect(wlButtons?.length).toBe(2)

    // First button starts active (БС ON) because is_whitelist: true & whitelist/status has it enabled
    const tvWlBtn = wlButtons![0] as HTMLButtonElement
    expect(tvWlBtn.className).toContain('active')
    expect(tvWlBtn.textContent).toContain('БС ON')

    // Click to toggle OFF
    await act(async () => {
      tvWlBtn.click()
    })

    expect(postSpy).toHaveBeenCalledWith('whitelist/device/toggle', {
      ip: '192.168.1.100',
      mac: 'AA:BB:CC:DD:EE:01',
      enabled: false,
    })

    // Button MUST immediately switch to inactive (БС OFF), NOT staying locked in active state
    expect(tvWlBtn.className).toContain('inactive')
    expect(tvWlBtn.textContent).toContain('БС OFF')
  })
})
