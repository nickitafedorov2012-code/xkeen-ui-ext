import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import DeviceScheduleModal from './DeviceScheduleModal'
import * as api from '../api'

// @ts-ignore
globalThis.IS_REACT_ACT_ENVIRONMENT = true

describe('DeviceScheduleModal Component — [FE-01] Node ID with Emoji/Flags', () => {
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

  it('preserves raw node id with emoji in target_server while displaying clean name in UI', async () => {
    const rawServerId = '🇫🇮 Finland [⚡ High Speed]'
    const cleanServerName = 'Finland [High Speed]'

    const availableServers = [
      { id: rawServerId, name: cleanServerName },
      { id: '🇩🇪 Germany-01', name: 'Germany-01' },
    ]

    vi.spyOn(api, 'apiGet').mockImplementation(async (endpoint: string) => {
      if (endpoint === 'schedules') {
        return {
          schedules: [
            {
              id: 'sched-1',
              ip: '192.168.1.100',
              enabled: true,
              time_start: '23:00',
              time_end: '07:00',
              days: [1, 2, 3, 4, 5, 6, 7],
              action: 'proxy',
              target_server: rawServerId,
            },
          ],
          revision: 'rev-1',
        }
      }
      return {}
    })

    const apiPostSpy = vi.spyOn(api, 'apiPost').mockResolvedValue({ saved: true, revision: 'rev-2' })

    await act(async () => {
      root!.render(
        <DeviceScheduleModal
          isOpen={true}
          onClose={vi.fn()}
          deviceIp="192.168.1.100"
          deviceName="Samsung Smart TV"
          notify={notifyMock}
          availableServers={availableServers}
        />
      )
    })

    // 1. Check existing schedule item displays the clean friendly name
    const badge = container!.querySelector('.schedule-action-badge.proxy')
    expect(badge).not.toBeNull()
    expect(badge?.textContent).toContain(cleanServerName)

    // 2. Select proxy action in the form
    const actionSelect = container!.querySelector('select.editor-select') as HTMLSelectElement
    await act(async () => {
      actionSelect.value = 'proxy'
      actionSelect.dispatchEvent(new Event('change', { bubbles: true }))
    })

    // 3. Find server select dropdown
    const serverSelects = container!.querySelectorAll('select.editor-select')
    const serverSelect = serverSelects[1] as HTMLSelectElement
    expect(serverSelect).not.toBeNull()

    // 4. Verify options: option text is clean name, option value is raw ID
    const options = Array.from(serverSelect.querySelectorAll('option'))
    const targetOpt = options.find((o) => o.value === rawServerId)
    expect(targetOpt).toBeDefined()
    expect(targetOpt?.textContent).toBe(cleanServerName)

    // 5. Select this server and submit form
    await act(async () => {
      serverSelect.value = rawServerId
      serverSelect.dispatchEvent(new Event('change', { bubbles: true }))
    })

    const form = container!.querySelector('form') as HTMLFormElement
    await act(async () => {
      form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }))
    })

    // 6. Verify apiPost was called with rawServerId in target_server
    expect(apiPostSpy).toHaveBeenCalled()
    const callArgs = apiPostSpy.mock.calls[0]
    expect(callArgs[0]).toBe('schedules')
    const postBody = callArgs[1] as any
    const addedSched = postBody.schedules.find((s: any) => s.id !== 'sched-1')
    expect(addedSched).toBeDefined()
    expect(addedSched.target_server).toBe(rawServerId)
    expect(addedSched.action).toBe('proxy')
  })
})
