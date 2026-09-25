import { describe, it, expect, vi } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import ShareNodeModal from './ShareNodeModal'
import ConfigEditor from './ConfigEditor'

// @ts-ignore
globalThis.IS_REACT_ACT_ENVIRONMENT = true

describe('Modals React Hook Order and State Lifecycle', () => {
  it('regression: ShareNodeModal open/close cycles maintain hook consistency and handle null server', async () => {
    const container = document.createElement('div')
    document.body.appendChild(container)
    const root = createRoot(container)
    const notify = vi.fn()
    const onClose = vi.fn()

    // 1. Initial render when closed
    await act(async () => {
      root.render(<ShareNodeModal isOpen={false} server={null} onClose={onClose} notify={notify} />)
    })
    expect(container.innerHTML).toBe('')

    // 2. Open with null server (tests null safety before hooks return, renders null safely)
    await act(async () => {
      root.render(<ShareNodeModal isOpen={true} server={null} onClose={onClose} notify={notify} />)
    })
    expect(container.innerHTML).toBe('')

    // 3. Open with active server
    await act(async () => {
      root.render(
        <ShareNodeModal
          isOpen={true}
          server={{
            id: 'server-1',
            name: 'NL-Amsterdam-Fast',
            protocol: 'Trojan',
            host: '1.2.3.4',
            port: 443,
            is_active: true,
            is_priority: false,
            ping_ms: 25,
            raw: { password: 'secretpassword' },
          }}
          onClose={onClose}
          notify={notify}
        />
      )
    })
    expect(container.innerHTML).toContain('NL-Amsterdam-Fast')

    // 4. Close modal again
    await act(async () => {
      root.render(<ShareNodeModal isOpen={false} server={null} onClose={onClose} notify={notify} />)
    })
    expect(container.innerHTML).toBe('')

    // 5. Reopen modal with a different server
    await act(async () => {
      root.render(
        <ShareNodeModal
          isOpen={true}
          server={{
            id: 'server-2',
            name: 'DE-Frankfurt-Vless',
            protocol: 'VLESS',
            host: '5.6.7.8',
            port: 8443,
            is_active: false,
            is_priority: true,
            ping_ms: 40,
            raw: { uuid: '1111-2222' },
          }}
          onClose={onClose}
          notify={notify}
        />
      )
    })
    expect(container.innerHTML).toContain('DE-Frankfurt-Vless')

    // 6. Final close
    await act(async () => {
      root.render(<ShareNodeModal isOpen={false} server={null} onClose={onClose} notify={notify} />)
    })
    expect(container.innerHTML).toBe('')

    act(() => {
      root.unmount()
      container.remove()
    })
  })

  it('regression: ConfigEditor open/close cycles execute all hooks in stable order', async () => {
    const container = document.createElement('div')
    document.body.appendChild(container)
    const root = createRoot(container)
    const notify = vi.fn()
    const onClose = vi.fn()

    // Mock API fetch
    const originalFetch = global.fetch
    global.fetch = vi.fn().mockImplementation((url: string) => {
      if (url.includes('config-files/list')) {
        return Promise.resolve({
          ok: true,
          json: () => Promise.resolve({ ok: true, data: [{ id: 'mihomo', name: 'Mihomo', path: '/opt/etc/mihomo/config.yaml', syntax: 'yaml' }] }),
        })
      }
      if (url.includes('config-files/read')) {
        return Promise.resolve({
          ok: true,
          json: () => Promise.resolve({ ok: true, data: { file: 'mihomo', content: 'port: 7890\n' } }),
        })
      }
      return Promise.resolve({
        ok: true,
        json: () => Promise.resolve({ ok: true }),
      })
    })

    // 1. Initial render when closed
    await act(async () => {
      root.render(<ConfigEditor isOpen={false} onClose={onClose} notify={notify} />)
    })
    expect(container.innerHTML).toBe('')

    // 2. Open editor
    await act(async () => {
      root.render(<ConfigEditor isOpen={true} onClose={onClose} notify={notify} />)
    })
    expect(container.innerHTML).toContain('Редактор конфигураций')

    // 3. Close editor
    await act(async () => {
      root.render(<ConfigEditor isOpen={false} onClose={onClose} notify={notify} />)
    })
    expect(container.innerHTML).toBe('')

    // 4. Re-open editor
    await act(async () => {
      root.render(<ConfigEditor isOpen={true} onClose={onClose} notify={notify} />)
    })
    expect(container.innerHTML).toContain('Редактор конфигураций')

    // 5. Final close
    await act(async () => {
      root.render(<ConfigEditor isOpen={false} onClose={onClose} notify={notify} />)
    })
    expect(container.innerHTML).toBe('')

    act(() => {
      root.unmount()
      container.remove()
      global.fetch = originalFetch
    })
  })

  it('regression: Settings component renders without hook order errors during loading and loaded states', async () => {
    const originalFetch = global.fetch
    global.fetch = vi.fn().mockImplementation((url: string) => {
      if (url.includes('settings')) {
        return Promise.resolve({
          ok: true,
          json: () =>
            Promise.resolve({
              success: true,
              data: {
                failover: {
                  enabled: true,
                  ping_threshold_ms: 300,
                  priority_chain: [],
                  auto_restore_priority: true,
                  interval_secs: 60,
                },
                rci: { host: '127.0.0.1', port: 79, login: '', password: '', token: '' },
                mihomo: { host: '127.0.0.1', port: 9090 },
                auth: { enabled: false },
                notifications: {},
              },
            }),
        })
      }
      if (url.includes('servers')) {
        return Promise.resolve({
          ok: true,
          json: () => Promise.resolve({ success: true, data: { servers: [] } }),
        })
      }
      if (url.includes('domains')) {
        return Promise.resolve({
          ok: true,
          json: () => Promise.resolve({ success: true, data: { direct: [], force: [] } }),
        })
      }
      return Promise.resolve({
        ok: true,
        json: () => Promise.resolve({ success: true, data: {} }),
      })
    })

    const Settings = (await import('./Settings')).default
    const container = document.createElement('div')
    document.body.appendChild(container)
    const root = createRoot(container)
    const notify = vi.fn()

    // 1. Initial render (settings is null -> renders loading placeholder)
    await act(async () => {
      root.render(<Settings notify={notify} />)
    })
    expect(container.innerHTML).toContain('Загрузка')

    // 2. Wait for async fetch to populate settings -> transition to full view without React invariant 310 error
    await act(async () => {
      await new Promise((r) => setTimeout(r, 10))
    })
    expect(container.innerHTML).toContain('Failover')

    act(() => {
      root.unmount()
      container.remove()
      global.fetch = originalFetch
    })
  })
})
