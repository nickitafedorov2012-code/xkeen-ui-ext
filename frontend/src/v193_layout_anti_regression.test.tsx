import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import DeviceRoutingModal from './components/DeviceRoutingModal'
import TrafficGraph from './components/TrafficGraph'
import Gaming from './components/Gaming'
import * as api from './api'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

describe('v1.9.3 Anti-Regression: Real Component Behavior', () => {
  let container: HTMLDivElement
  let root: ReturnType<typeof createRoot>

  beforeEach(() => {
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
  })

  it('DeviceRoutingModal renames Default (PROXY) to 🌐 Общий активный узел and filters PROXY & pools', async () => {
    vi.spyOn(api, 'apiGet').mockResolvedValue({ rules: [] })

    await act(async () => {
      root.render(
        <DeviceRoutingModal
          modal={{
            ip: '192.168.1.50',
            name: 'Phone',
            servers: [],
            threshold: 250,
            autoRestore: true,
          }}
          servers={[
            { id: 'PROXY', name: 'PROXY', protocol: 'Selector', host: '', port: 0, is_active: false, is_priority: false, is_pool: true, ping_ms: 0 },
            { id: 'pool_auto', name: 'Auto', protocol: 'URLTest', host: '', port: 0, is_active: false, is_priority: false, is_pool: true, ping_ms: 0 },
            { id: 'vless_nl', name: 'Netherlands', protocol: 'VLESS', host: 'nl.example.com', port: 443, is_active: true, is_priority: false, is_pool: false, ping_ms: 40 },
          ]}
          busy={false}
          hasExisting={false}
          onChange={() => {}}
          onClose={() => {}}
          onSave={() => {}}
          serverLabel={(id: string) => id}
        />
      )
    })

    const html = container.innerHTML
    expect(html).toContain('🌐 Общий активный узел')
    expect(html).not.toContain('Default (PROXY)')
    expect(html).toContain('Цепочка пуста — устройство использует общий активный узел.')

    const options = Array.from(container.querySelectorAll('select option')).map(
      (o) => (o as HTMLOptionElement).value
    )
    expect(options).toContain('vless_nl')
    expect(options).not.toContain('pool_auto')
  })

  it('TrafficGraph hides Proxy badge during active Whitelist mode and renders reset buttons', async () => {
    HTMLCanvasElement.prototype.getContext = vi.fn(() => null) as unknown as typeof HTMLCanvasElement.prototype.getContext
    vi.spyOn(api, 'apiGet').mockImplementation(async (path: string) => {
      if (path === 'traffic/poll') {
        return {
          whitelist_active: true,
          direct: { down: 100, up: 50 },
          proxy: { down: 0, up: 0 },
          whitelist: { down: 500, up: 200 },
          direct_bytes: 1024,
          proxy_bytes: 2048,
          whitelist_bytes: 4096,
          history: [],
        }
      }
      return {}
    })

    await act(async () => {
      root.render(<TrafficGraph notify={() => {}} />)
    })

    expect(container.querySelector('.traffic-badge.proxy')).toBeNull()
    expect(container.querySelector('.traffic-badge.whitelist')).not.toBeNull()
    expect(container.querySelectorAll('.traffic-reset-btn').length).toBeGreaterThanOrEqual(2)
  })

  it('Gaming renames PROXY to 🌐 Общий активный узел in server selector', async () => {
    vi.spyOn(api, 'apiGet').mockImplementation(async (path: string) => {
      if (path === 'gaming/status') {
        return {
          enabled: false,
          server: 'PROXY',
          platforms: [],
          devices: [],
        }
      }
      if (path === 'servers') {
        return {
          servers: [{ id: 'vless_de', name: 'DE', protocol: 'VLESS', host: 'de.example.com', port: 443, is_active: true, is_priority: false, is_pool: false, ping_ms: 35 }],
        }
      }
      if (path === 'devices') {
        return { devices: [] }
      }
      return {}
    })

    await act(async () => {
      root.render(<Gaming notify={() => {}} />)
    })

    const html = container.innerHTML
    expect(html).toContain('🌐 Общий активный узел')
    expect(html).not.toContain('🔒 PROXY (Основной туннель)')
  })
})
