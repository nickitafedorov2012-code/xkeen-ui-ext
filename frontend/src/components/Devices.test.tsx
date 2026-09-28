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

    // Find all zapret switches
    const zapretSwitches = Array.from(
      container?.querySelectorAll('input.device-zapret-toggle') || []
    ) as HTMLInputElement[]
    expect(zapretSwitches.length).toBe(2)

    // First switch (Smart TV - enabled): checked
    const tvSwitch = zapretSwitches[0]
    expect(tvSwitch.checked).toBe(true)

    // Second switch (Work Laptop - excluded): unchecked
    const laptopSwitch = zapretSwitches[1]
    expect(laptopSwitch.checked).toBe(false)

    // Click on Smart TV switch to disable Zapret for it
    await act(async () => {
      tvSwitch.click()
    })

    expect(postSpy).toHaveBeenCalledWith('devices/zapret-toggle', {
      ip: '192.168.1.100',
      mac: 'AA:BB:CC:DD:EE:01',
      enabled: false,
    })

    // Now TV switch should have switched to unchecked
    expect(tvSwitch.checked).toBe(false)

    // Click on Laptop switch to enable Zapret for it
    await act(async () => {
      laptopSwitch.click()
    })

    expect(postSpy).toHaveBeenCalledWith('devices/zapret-toggle', {
      ip: '192.168.1.101',
      mac: 'AA:BB:CC:DD:EE:02',
      enabled: true,
    })

    // Now Laptop switch should have switched to checked
    expect(laptopSwitch.checked).toBe(true)
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

    const zapretToggle = container?.querySelector('input.device-zapret-toggle') as HTMLInputElement
    expect(zapretToggle).toBeDefined()
    expect(zapretToggle.checked).toBe(true)

    // Click to disable Zapret for this device without IP (only MAC)
    await act(async () => {
      zapretToggle.click()
    })

    expect(postSpy).toHaveBeenCalledWith('devices/zapret-toggle', {
      ip: '',
      mac: 'CC:DD:EE:FF:00:99',
      enabled: false,
    })

    // Switch MUST optimistically switch to unchecked even without IP
    expect(zapretToggle.checked).toBe(false)
  })
})
