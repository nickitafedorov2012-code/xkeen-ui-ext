import { afterEach, describe, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import Servers from './Servers'

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })
const response = (data: unknown) => new Response(JSON.stringify({ success: true, data }), { headers: { 'Content-Type': 'application/json' } })
function button(container: HTMLElement, label: string) {
  const found = Array.from(container.querySelectorAll('button')).find((item) => item.textContent?.includes(label))
  if (!found) throw new Error(`Button not found: ${label}`)
  return found
}
describe('subscription controls', () => {
  afterEach(() => { vi.unstubAllGlobals(); localStorage.clear() })
  it('allows adding the first subscription when the list is empty', async () => {
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => response(String(input) === '/api/providers' ? { providers: [] } : { servers: [] })))
    const container = document.createElement('div'); document.body.appendChild(container)
    const root = createRoot(container)
    try {
      await act(async () => { root.render(<Servers notify={vi.fn()} />) })
      await act(async () => { button(container, 'Подписки').click() })
      expect(container.textContent).toContain('Нет активных подписок')
      await act(async () => { button(container, 'Добавить новую подписку').click() })
      expect(container.querySelector('input[placeholder^="ID"]')).not.toBeNull()
    } finally { await act(async () => { root.unmount() }); container.remove() }
  })
  it('shows the loading failure and retries without confusing it with an empty list', async () => {
    let fail = true
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
      if (String(input) === '/api/providers') {
        if (fail) throw new Error('timeout')
        return response({ providers: [{ id: 'sample', name: 'Sample', count: 1, enabled: true }] })
      }
      return response({ servers: [] })
    }))
    const container = document.createElement('div'); document.body.appendChild(container)
    const root = createRoot(container)
    try {
      await act(async () => { root.render(<Servers notify={vi.fn()} />) })
      await act(async () => { button(container, 'Подписки').click() })
      expect(container.querySelector('[role="alert"]')?.textContent).toContain('Не удалось загрузить список подписок')
      expect(container.textContent).not.toContain('Нет активных подписок')
      expect(container.textContent).toContain('Добавить новую подписку')
      fail = false
      await act(async () => { button(container, 'Повторить').click() })
      expect(container.querySelector('[role="alert"]')).toBeNull()
      expect(container.textContent).toContain('Sample')
    } finally { await act(async () => { root.unmount() }); container.remove() }
  })
  it('keeps the last subscription list if a later refresh fails', async () => {
    let fail = false
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
      if (String(input) === '/api/providers') {
        if (fail) throw new Error('timeout')
        return response({ providers: [{ id: 'sample', name: 'Sample', count: 1, enabled: true }] })
      }
      return response({ servers: [] })
    }))
    const container = document.createElement('div'); document.body.appendChild(container)
    const root = createRoot(container)
    try {
      await act(async () => { root.render(<Servers notify={vi.fn()} />) })
      fail = true
      await act(async () => { container.querySelector<HTMLButtonElement>('button[title="Обновить список"]')!.click() })
      await act(async () => { button(container, 'Подписки').click() })
      expect(container.textContent).toContain('Sample')
      expect(container.querySelector('[role="alert"]')?.textContent).toContain('Показаны последние загруженные данные')
    } finally { await act(async () => { root.unmount() }); container.remove() }
  })

  it('renders servers and the subscription button while providers are still loading', async () => {
    let finish!: (value: Response) => void
    const pending = new Promise<Response>((resolve) => { finish = resolve })
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
      if (String(input) === '/api/providers') return pending
      if (String(input) === '/api/servers') return response({ servers: [{
        id: 'slow-test', name: 'Пример сервера', protocol: 'VLESS', host: 'example.com', port: 443,
        ping_ms: 10, is_active: false, is_priority: false, is_pool: false, is_whitelist: false,
      }] })
      return response({ servers: [] })
    }))
    const container = document.createElement('div'); document.body.appendChild(container)
    const root = createRoot(container)
    try {
      await act(async () => { root.render(<Servers notify={vi.fn()} />) })
      expect(container.textContent).toContain('Пример сервера')
      await act(async () => { button(container, 'Подписки').click() })
      expect(container.textContent).toContain('Загрузка подписок…')
      expect(container.textContent).not.toContain('Нет активных подписок')
    } finally {
      await act(async () => { finish(response({ providers: [] })) })
      await act(async () => { root.unmount() }); container.remove()
    }
  })

})
