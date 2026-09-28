import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import Diagnostics from './Diagnostics'
import FlowRepairModal from './FlowRepairModal'
import * as api from '../api'

// @ts-ignore
globalThis.IS_REACT_ACT_ENVIRONMENT = true

describe('Diagnostics & FlowRepair Truthfulness Components (Milestone 5)', () => {
  let container: HTMLDivElement | null = null
  let root: ReturnType<typeof createRoot> | null = null

  beforeEach(() => {
    container = document.createElement('div')
    document.body.appendChild(container)
    root = createRoot(container)
    vi.restoreAllMocks()
  })

  afterEach(() => {
    if (root && container) {
      act(() => {
        root!.unmount()
      })
      container.remove()
    }
  })

  it('Diagnostics renders health checks on success', async () => {
    const mockHealth = {
      checks: [
        { id: 'gateway', name: 'Шлюз Keenetic', status: 'ok', message: 'KeeneticOS 4.2', measurement_point: 'router_local' },
        { id: 'storage', name: 'Хранилище /opt', status: 'ok', message: 'Использовано 45%', measurement_point: 'router_local' },
      ],
    }

    vi.spyOn(api, 'apiGet').mockImplementation(async (path: string) => {
      if (path === 'diagnostics/health') return mockHealth as any
      return {} as any
    })

    await act(async () => {
      root!.render(<Diagnostics notify={vi.fn()} />)
    })

    expect(container!.textContent).toContain('Шлюз Keenetic')
    expect(container!.textContent).toContain('KeeneticOS 4.2')
    expect(container!.textContent).toContain('Последнее успешное обновление:')
  })

  it('UI-05: Diagnostics renders stale error banner when health check fails', async () => {
    vi.spyOn(api, 'apiGet').mockRejectedValue(new Error('Сетевой таймаут к роутеру'))

    await act(async () => {
      root!.render(<Diagnostics notify={vi.fn()} />)
    })

    expect(container!.textContent).toContain('Ошибка диагностики:')
    expect(container!.textContent).toContain('Сетевой таймаут к роутеру')
  })

  it('UI-05: DNS test clears previous result on new run and does NOT display stale success on error', async () => {
    const notifyMock = vi.fn()
    let postCallCount = 0

    vi.spyOn(api, 'apiGet').mockResolvedValue({ checks: [] } as any)
    vi.spyOn(api, 'apiPost').mockImplementation(async (path: string, _body?: any) => {
      if (path === 'diagnostics/dns-test') {
        postCallCount++
        if (postCallCount === 1) {
          return {
            domain: 'good-site.com',
            resolved_ips: ['93.184.216.34'],
            is_poisoned: false,
            http_direct_ok: true,
            verdict: 'Домен доступен напрямую без ограничений.',
            recommendation: 'Дополнительных действий не требуется.',
          } as any
        } else {
          throw new Error('Таймаут DNS резолвера')
        }
      }
      return {} as any
    })

    await act(async () => {
      root!.render(<Diagnostics notify={notifyMock} />)
    })

    const input = container!.querySelector('input.input-text') as HTMLInputElement
    const form = container!.querySelector('form.smart-dns-form') as HTMLFormElement

    // Первый успешный тест: good-site.com
    await act(async () => {
      input.value = 'good-site.com'
      input.dispatchEvent(new Event('input', { bubbles: true }))
      form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }))
    })

    expect(container!.textContent).toContain('Анализ: good-site.com')
    expect(container!.textContent).toContain('Прямой доступ OK')

    // Второй тест: bad-site.com с ошибкой
    await act(async () => {
      input.value = 'bad-site.com'
      input.dispatchEvent(new Event('input', { bubbles: true }))
      form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }))
    })

    // UI-05: Старый результат good-site.com должен быть очищен, отображен баннер ошибки bad-site.com
    expect(container!.textContent).toContain('Ошибка DNS-теста: Таймаут DNS резолвера')
    expect(container!.textContent).not.toContain('Анализ: good-site.com')
    expect(container!.textContent).not.toContain('Прямой доступ OK')
  })

  it('DIAG-04: FlowRepairModal displays error banner when repair fails and does not claim verified success', async () => {
    vi.spyOn(api, 'apiPost').mockRejectedValue(new Error('Маршрут Google AI в ядре Mihomo не подтвердил выбор узла в runtime'))

    await act(async () => {
      root!.render(<FlowRepairModal isOpen={true} onClose={vi.fn()} notify={vi.fn()} />)
    })

    expect(container!.textContent).toContain('Не удалось подтвердить маршрут Flow')
    expect(container!.textContent).toContain('Маршрут Google AI в ядре Mihomo не подтвердил выбор узла в runtime')
    expect(container!.textContent).not.toContain('Маршрут подтвержден и активен')
  })

  it('DIAG-04: FlowRepairModal displays verified status only when verified: true is returned', async () => {
    vi.spyOn(api, 'apiPost').mockResolvedValue({
      success: true,
      flow_server: '🇺🇸 US Chicago',
      verified: true,
      message: 'Маршрут подтвержден',
    } as any)

    await act(async () => {
      root!.render(<FlowRepairModal isOpen={true} onClose={vi.fn()} notify={vi.fn()} />)
    })

    expect(container!.textContent).toContain('Маршрут подтвержден и активен')
    expect(container!.textContent).toContain('🇺🇸 US Chicago')
    expect(container!.textContent).toContain('Проверен в рантайме ядра')
  })
})
