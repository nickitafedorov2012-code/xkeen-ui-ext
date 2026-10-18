import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import DeviceRow from './DeviceRow'
import { type DeviceInfo } from '../types'

// @ts-ignore
globalThis.IS_REACT_ACT_ENVIRONMENT = true

const mockDevice: DeviceInfo = {
  mac: '11:22:33:44:55:66',
  name: 'Living Room Apple TV',
  ip: '192.168.1.55',
  policy: 'default',
  policy_name: 'Основная',
  online: true,
  interface: 'eth0',
  is_current_device: false,
  rxbytes: 500000,
  txbytes: 250000,
  speed_limit_kbps: 0,
  current_server: 'default',
}

describe('DeviceRow Component — Per-Device Whitelist Toggle (Milestone 3)', () => {
  let container: HTMLDivElement | null = null
  let root: ReturnType<typeof createRoot> | null = null

  beforeEach(() => {
    container = document.createElement('div')
    document.body.appendChild(container)
    root = createRoot(container)
  })

  afterEach(() => {
    if (root && container) {
      act(() => {
        root!.unmount()
      })
      container.remove()
    }
  })

  it('renders inactive whitelist button (БС OFF) by default', async () => {
    const onToggleWhitelistMock = vi.fn()

    await act(async () => {
      root!.render(
        <table>
          <tbody>
            <DeviceRow
              d={mockDevice}
              policies={[]}
              servers={[]}
              devFailover={false}
              busy={false}
              selected={false}
              onToggleSelect={vi.fn()}
              applyPolicy={vi.fn()}
              applySpeed={vi.fn()}
              applyServer={vi.fn()}
              serverLabel={(id) => id}
              openDrModal={vi.fn()}
              whitelistActive={false}
              onToggleWhitelist={onToggleWhitelistMock}
            />
          </tbody>
        </table>
      )
    })

    const btn = container?.querySelector('button.device-whitelist-btn') as HTMLButtonElement
    expect(btn).toBeTruthy()
    expect(btn.className).toContain('inactive')
    expect(btn.textContent).toContain('БС OFF')
    expect(btn.getAttribute('data-testid')).toBe(`device-whitelist-btn-${mockDevice.mac}`)

    // Click triggers toggle with enabled=true
    await act(async () => {
      btn.click()
    })

    expect(onToggleWhitelistMock).toHaveBeenCalledTimes(1)
    expect(onToggleWhitelistMock).toHaveBeenCalledWith('192.168.1.55', '11:22:33:44:55:66', true)
  })

  it('renders active whitelist button (БС ON) when whitelistActive is true', async () => {
    const onToggleWhitelistMock = vi.fn()

    await act(async () => {
      root!.render(
        <table>
          <tbody>
            <DeviceRow
              d={mockDevice}
              policies={[]}
              servers={[]}
              devFailover={false}
              busy={false}
              selected={false}
              onToggleSelect={vi.fn()}
              applyPolicy={vi.fn()}
              applySpeed={vi.fn()}
              applyServer={vi.fn()}
              serverLabel={(id) => id}
              openDrModal={vi.fn()}
              whitelistActive={true}
              onToggleWhitelist={onToggleWhitelistMock}
            />
          </tbody>
        </table>
      )
    })

    const btn = container?.querySelector('button.device-whitelist-btn') as HTMLButtonElement
    expect(btn).toBeTruthy()
    expect(btn.className).toContain('active')
    expect(btn.textContent).toContain('БС ON')

    // Click triggers toggle with enabled=false
    await act(async () => {
      btn.click()
    })

    expect(onToggleWhitelistMock).toHaveBeenCalledTimes(1)
    expect(onToggleWhitelistMock).toHaveBeenCalledWith('192.168.1.55', '11:22:33:44:55:66', false)
  })

  it('disables whitelist button when busy is true', async () => {
    const onToggleWhitelistMock = vi.fn()

    await act(async () => {
      root!.render(
        <table>
          <tbody>
            <DeviceRow
              d={mockDevice}
              policies={[]}
              servers={[]}
              devFailover={false}
              busy={true}
              selected={false}
              onToggleSelect={vi.fn()}
              applyPolicy={vi.fn()}
              applySpeed={vi.fn()}
              applyServer={vi.fn()}
              serverLabel={(id) => id}
              openDrModal={vi.fn()}
              whitelistActive={false}
              onToggleWhitelist={onToggleWhitelistMock}
            />
          </tbody>
        </table>
      )
    })

    const btn = container?.querySelector('button.device-whitelist-btn') as HTMLButtonElement
    expect(btn).toBeTruthy()
    expect(btn.disabled).toBe(true)

    await act(async () => {
      btn.click()
    })

    expect(onToggleWhitelistMock).not.toHaveBeenCalled()
  })
})
