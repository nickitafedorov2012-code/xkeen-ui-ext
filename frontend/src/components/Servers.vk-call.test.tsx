import { afterEach, describe, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import Servers from './Servers'

import ShareNodeModal from './ShareNodeModal'
import OutboundGeneratorModal from './OutboundGeneratorModal'
import Help from './Help'

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
      const pingCount = () => fetchMock.mock.calls.filter(([url]) => String(url) === '/api/servers/ping').length
      expect(pingCount()).toBe(0)
      await act(async () => { connect!.click() })
      const request = fetchMock.mock.calls.find(([url]) => String(url) === '/api/servers/switch')
      expect(request?.[1]?.method).toBe('POST')
      expect(JSON.parse(String(request?.[1]?.body))).toEqual({ server_id: id })
      expect(pingCount()).toBe(0)
    } finally {
      await act(async () => { root.unmount() })
      container.remove()
    }
  })
})

describe('regression_vkturn_25_frontend — Block B cross-panel integration', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
    localStorage.clear()
  })

  it('hides vkturn provider defensively, renders VPS peer host:port & VK settings button on card, ShareNodeModal, OutboundGeneratorModal csqtt, and Help FAQ', async () => {
    const id = '📞 VK-туннель [БС]'
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      let data: unknown = {}
      if (path === '/api/servers') {
        data = {
          servers: [{
            id, name: '📞 VK-туннель [БС]', protocol: 'VK-CALL',
            host: '203.0.113.10', port: 46000, ping_ms: 82,
            provider: 'vkturn', provider_name: 'Звонки ВКонтакте',
            is_active: true, is_priority: false, is_pool: false, is_whitelist: true,
          }],
        }
      } else if (path === '/api/providers') {
        data = {
          providers: [
            { id: 'vkturn', name: 'vkturn', count: 1, vehicle_type: 'File', enabled: true },
            { id: 'sub1', name: 'Main Sub', count: 5, vehicle_type: 'HTTP', enabled: true },
          ],
        }
      } else if (path === '/api/ignore') {
        data = { servers: [] }
      } else if (path === '/api/vkturn/config' && init?.method === 'POST') {
        data = { success: true, message: 'Сохранено' }
      }
      return new Response(JSON.stringify({ success: true, data }), {
        headers: { 'Content-Type': 'application/json' },
      })
    })
    vi.stubGlobal('fetch', fetchMock)

    const switched: string[] = []
    const onSwitch = (e: Event) => {
      const ce = e as CustomEvent<string>
      switched.push(ce.detail)
    }
    window.addEventListener('xr:switch-tab', onSwitch)

    const container = document.createElement('div')
    document.body.appendChild(container)
    const root = createRoot(container)
    try {
      await act(async () => {
        root.render(<Servers notify={vi.fn()} />)
      })

      expect(container.textContent).toContain('203.0.113.10:46000')
      expect(container.textContent).toContain('Адрес VPS:')
      expect(container.textContent).toContain('Настройки ВК')
      expect(container.textContent).not.toContain('🔗 Ссылка')

      // Check provider modal hides vkturn
      const subBtn = Array.from(container.querySelectorAll('button')).find((b) => b.textContent?.includes('Подписки'))
      if (subBtn) {
        await act(async () => { subBtn.click() })
        expect(container.textContent).toContain('Main Sub')
        expect(container.textContent).not.toContain('vkturn')
      }

      // Click Настройки ВК -> xr:switch-tab 'vkturn'
      const vkSettingsBtn = Array.from(container.querySelectorAll('button')).find((b) => b.textContent?.includes('Настройки ВК'))
      expect(vkSettingsBtn).toBeDefined()
      await act(async () => { vkSettingsBtn!.click() })
      expect(switched).toContain('vkturn')

      // ShareNodeModal for VK-CALL shows dedicated VK settings button instead of generic URI error
      await act(async () => {
        root.render(
          <ShareNodeModal
            isOpen
            onClose={vi.fn()}
            notify={vi.fn()}
            server={{
              id, name: '📞 VK-туннель [БС]', protocol: 'VK-CALL',
              host: '203.0.113.10', port: 46000, ping_ms: 82,
              is_active: true, is_priority: false, is_pool: false,
            }}
          />
        )
      })
      expect(container.textContent).toContain('настройки VK-туннеля')
      expect(container.textContent).not.toContain('невозможно экспортировать')

      // OutboundGeneratorModal imports csqtt:// via saveVkTurnConfig
      await act(async () => {
        root.render(<OutboundGeneratorModal isOpen onClose={vi.fn()} notify={vi.fn()} />)
      })
      const ta = container.querySelector('textarea') as HTMLTextAreaElement
      expect(ta).not.toBeNull()
      await act(async () => {
        const setter = Object.getOwnPropertyDescriptor(window.HTMLTextAreaElement.prototype, 'value')!.set!
        setter.call(ta, 'csqtt://connect?v=2&host=203.0.113.10&peer=46000&password=my_secret_pass')
        ta.dispatchEvent(new Event('input', { bubbles: true }))
      })
      expect(container.textContent).toContain('Профиль VK-туннеля')

      // Help includes VK Call FAQ
      await act(async () => {
        root.render(<Help status={null} />)
      })
      expect(container.textContent).toContain('Звонки ВКонтакте')
      expect(container.textContent).toContain('csqtt://')
    } finally {
      window.removeEventListener('xr:switch-tab', onSwitch)
      await act(async () => { root.unmount() })
      container.remove()
    }
  })
})
