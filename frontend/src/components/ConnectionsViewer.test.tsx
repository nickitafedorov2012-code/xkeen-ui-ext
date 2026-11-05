import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { createRoot } from 'react-dom/client'
import { act } from 'react'
import ConnectionsViewer from './ConnectionsViewer'
import * as api from '../api'

vi.mock('../api', () => ({
  apiGet: vi.fn(),
  apiPost: vi.fn(),
  apiDelete: vi.fn(),
}))
vi.mock('./Diagnostics', () => ({ default: () => null }))

describe('ConnectionsViewer — «В прокси»', () => {
  let container: HTMLDivElement | null = null
  let root: ReturnType<typeof createRoot> | null = null
  const notifyMock = vi.fn()

  beforeEach(() => {
    container = document.createElement('div')
    document.body.appendChild(container)
    root = createRoot(container)
    vi.clearAllMocks()
    vi.mocked(api.apiGet).mockImplementation((path: string) => {
      if (path === 'connections') return Promise.resolve({ connections: [], downloadTotal: 0, uploadTotal: 0 })
      if (path === 'devices') return Promise.resolve({ devices: [] })
      if (path === 'domains') return Promise.resolve({ direct: ['keep.example'], force: ['old.example'] })
      if (path === 'devices/domain-rules') return Promise.resolve({ rules: {} })
      return Promise.resolve({})
    })
    vi.mocked(api.apiDelete).mockResolvedValue({})
  })

  afterEach(() => {
    act(() => {
      root?.unmount()
    })
    container?.remove()
    container = null
    root = null
  })

  it('bug 136: force-add failure reports the error and never falls back to read-modify-write of domains', async () => {
    // Баг 136: при любой ошибке force-add (таймаут, когда бэкенд уже применил; ошибка валидации) шёл запасной
    // путь — повторное применение, read-modify-write `domains` (гонка с другими вкладками) и правило с
    // жёсткой целью `PROXY` в обход проверок сервера.
    vi.mocked(api.apiPost).mockImplementation((path: string) => {
      if (path === 'domains/force-add') return Promise.reject(new Error('Некорректный домен'))
      return Promise.resolve({ success: true })
    })
    await act(async () => {
      root!.render(<ConnectionsViewer notify={notifyMock} />)
    })
    const openBtn = Array.from(container?.querySelectorAll('button') || []).find((b) =>
      b.textContent?.includes('В прокси по домену')
    ) as HTMLButtonElement
    await act(async () => {
      openBtn.click()
    })
    const form = container?.querySelector('form') as HTMLFormElement
    const input = form.querySelector('input[type="text"]') as HTMLInputElement
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!
    await act(async () => {
      setter.call(input, 'new.example')
      input.dispatchEvent(new Event('input', { bubbles: true }))
    })
    await act(async () => {
      form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }))
    })
    const posted = vi.mocked(api.apiPost).mock.calls.map((c) => c[0])
    expect(posted).toEqual(['domains/force-add'])
    expect(vi.mocked(api.apiGet).mock.calls.map((c) => c[0])).not.toContain('devices/domain-rules')
    expect(notifyMock).toHaveBeenCalledWith(expect.stringContaining('Некорректный домен'), true)
    expect(notifyMock.mock.calls.some((c) => String(c[0]).startsWith('✓'))).toBe(false)
  })
})
