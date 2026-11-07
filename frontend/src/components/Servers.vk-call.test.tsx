import { afterEach, describe, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import Servers from './Servers'

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })

describe('VK Call server selection', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
    localStorage.clear()
  })

  it('renders a selectable VK Call card and switches using the original Mihomo ID', async () => {
    const id = '📞 VK-туннель [БС]'
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      let data: unknown = {}
      if (path === '/api/servers') {
        data = { servers: [{
          id, name: '📞 Сервер звонков ВК', protocol: 'VK-CALL',
          host: 'Туннель через звонки ВКонтакте', port: 0, ping_ms: 73,
          provider: 'vkturn', provider_name: 'Звонки ВКонтакте',
          is_active: false, is_priority: false, is_pool: false, is_whitelist: true,
        }] }
      } else if (path === '/api/providers') {
        data = { providers: [] }
      } else if (path === '/api/ignore') {
        data = { servers: [] }
      } else if (path === '/api/servers/ping') {
        data = { pings: { [id]: 73 } }
      } else if (path === '/api/servers/switch' && init?.method === 'POST') {
        data = { message: 'Подключено' }
      }
      return new Response(JSON.stringify({ success: true, data }), {
        headers: { 'Content-Type': 'application/json' },
      })
    })
    vi.stubGlobal('fetch', fetchMock)
    const container = document.createElement('div')
    document.body.appendChild(container)
    const root = createRoot(container)
    try {
      await act(async () => { root.render(<Servers notify={vi.fn()} />) })
      const card = container.querySelector('.server-card')
      expect(card?.textContent).toContain('Сервер звонков ВК')
      expect(card?.textContent).toContain('VK-CALL')
      expect(card?.textContent).toContain('Звонки ВКонтакте')
      const connect = card?.querySelector<HTMLButtonElement>('button[title="Сделать этот сервер активным"]')
      expect(connect).not.toBeNull()
      await act(async () => { connect!.click() })
      const request = fetchMock.mock.calls.find(([url]) => String(url) === '/api/servers/switch')
      expect(request?.[1]?.method).toBe('POST')
      expect(JSON.parse(String(request?.[1]?.body))).toEqual({ server_id: id })
    } finally {
      await act(async () => { root.unmount() })
      container.remove()
    }
  })
})
