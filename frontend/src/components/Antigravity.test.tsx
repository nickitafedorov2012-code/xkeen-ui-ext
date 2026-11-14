import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import Antigravity from './Antigravity'
import * as api from '../api'

// @ts-ignore
globalThis.IS_REACT_ACT_ENVIRONMENT = true

const mockAntigravityStatus = {
  enabled: true,
  mode: 'auto',
  proxy_port: 53129,
  proxy_running: true,
  health_check_interval: 120,
  own_proxy: '',
  current_route: 'Mihomo',
  google_reachable: true,
  last_check: '2026-11-14T08:00:00Z',
  events: [],
}

describe('Antigravity Flow selector (Bug 100)', () => {
  let container: HTMLDivElement
  let root: ReturnType<typeof createRoot>

  beforeEach(() => {
    localStorage.clear()
    container = document.createElement('div')
    document.body.appendChild(container)
    root = createRoot(container)
  })

  afterEach(async () => {
    await act(async () => {
      root.unmount()
    })
    container.remove()
    vi.restoreAllMocks()
    localStorage.clear()
  })

  it('bug 100: flow selector shows backend value only', async () => {
    localStorage.setItem('xr_flow_server', 'stale_local_node')

    vi.spyOn(api, 'apiGet').mockImplementation(async (path: string) => {
      if (path === 'antigravity/status') return mockAntigravityStatus as any
      if (path === 'servers') {
        return {
          servers: [
            { id: 'us_node_1', name: '🇺🇸 US Node 1', ping_ms: 42, is_pool: false },
            { id: 'de_node_2', name: '🇩🇪 DE Node 2', ping_ms: 55, is_pool: false },
          ],
        } as any
      }
      if (path === 'flow/status') {
        return { flow_server: '', flow_auto_select: false } as any
      }
      return {} as any
    })

    await act(async () => {
      root.render(<Antigravity notify={() => {}} />)
    })
    await act(async () => {
      await Promise.resolve()
      await Promise.resolve()
    })

    const select = container.querySelector('select') as HTMLSelectElement | null
    expect(select).not.toBeNull()
    expect(select!.value).toBe('')
    const emptyOpt = select!.querySelector('option[value=""]')
    expect(emptyOpt?.textContent).toContain('не выбрано')
  })
})
