import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import VkTurn, { hostFromPeer, formatTraffic } from './VkTurn'
import * as api from '../api'
import type { VkTurnStatus, VkTurnPhase } from '../types'

// @ts-ignore
globalThis.IS_REACT_ACT_ENVIRONMENT = true

const inputSetter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!

async function typeInto(el: HTMLInputElement, value: string) {
  await act(async () => {
    inputSetter.call(el, value)
    el.dispatchEvent(new Event('input', { bubbles: true }))
  })
}

describe('VkTurn Component (ядро WDTT)', () => {
  let container: HTMLDivElement | null = null
  let root: ReturnType<typeof createRoot> | null = null
  const notifyMock = vi.fn()

  const defaultUnconfiguredStatus: VkTurnStatus = {
    installed: true,
    core_version: 'wdtt-core-7355598',
    phase: 'disabled',
    pid: null,
    connected_streams: 0,
    target_streams: 18,
    bytes_up: 0,
    bytes_down: 0,
    links_count: 0,
    links: [],
    max_links: 3,
    restarts: 0,
    last_error: null,
    enabled: false,
    peer: '',
    has_password: false,
    device_id: '',
    wg_ready: false,
    wg: { address: '', mtu: 1280, dns: [] },
  }

  const defaultConnectedStatus: VkTurnStatus = {
    installed: true,
    core_version: 'wdtt-core-7355598',
    phase: 'connected',
    pid: 12345,
    connected_streams: 3,
    target_streams: 18,
    bytes_up: 1536,
    bytes_down: 3 * 1024 * 1024,
    links_count: 2,
    links: ['https://vk.com/call/join/abcd…', 'https://vk.com/call/join/efgh…'],
    max_links: 3,
    restarts: 1,
    last_error: null,
    enabled: true,
    peer: '1.2.3.4:56000',
    has_password: true,
    device_id: 'xkeen-route-0011223344556677',
    wg_ready: true,
    wg: { address: '10.66.66.5', mtu: 1280, dns: ['1.1.1.1'] },
  }

  const findButton = (text: string) =>
    Array.from(container?.querySelectorAll('button') || []).find((b) => b.textContent?.includes(text)) as
      | HTMLButtonElement
      | undefined

  const fillWizard = async (opts: { host?: string; password?: string; link?: string }) => {
    const hostInput = container?.querySelector('input[placeholder*="203.0.113.10"]') as HTMLInputElement
    const pwInput = container?.querySelector('input[type="password"]') as HTMLInputElement
    const linkInput = container?.querySelector('input[placeholder*="Основная ссылка"]') as HTMLInputElement
    expect(hostInput).not.toBeNull()
    expect(pwInput).not.toBeNull()
    expect(linkInput).not.toBeNull()
    if (opts.host !== undefined) await typeInto(hostInput, opts.host)
    if (opts.password !== undefined) await typeInto(pwInput, opts.password)
    if (opts.link !== undefined) await typeInto(linkInput, opts.link)
  }

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

  it('wizard asks for server, password and VK links — no WireGuard config paste', async () => {
    vi.spyOn(api, 'getVkTurnStatus').mockResolvedValue(defaultUnconfiguredStatus)
    const saveConfigSpy = vi.spyOn(api, 'saveVkTurnConfig').mockResolvedValue({ success: true })

    await act(async () => {
      root!.render(<VkTurn notify={notifyMock} />)
    })

    expect(container?.textContent).toContain('Мастер настройки VK-туннеля')
    expect(container?.textContent).toContain('Сервер WDTT на VPS')
    expect(container?.textContent).toContain('Пароль подключения')
    expect(container?.textContent).toContain('Установить на мой VPS автоматически')
    // WDTT: wg-конфиг выдаёт сервер — поля для вставки .conf больше нет
    expect(container?.querySelector('textarea')).toBeNull()
    expect(container?.textContent).not.toContain('Телемост')

    await fillWizard({ host: '198.51.100.1', password: 'router-pass', link: 'https://vk.com/call/join/abcdef123' })

    const enableBtn = findButton('Включить')!
    await act(async () => {
      enableBtn.click()
    })

    // Одно сохранение, без отдельного вызова enable; порт по умолчанию 56000
    expect(saveConfigSpy).toHaveBeenCalledTimes(1)
    expect(saveConfigSpy).toHaveBeenCalledWith({
      enabled: true,
      peer: '198.51.100.1:56000',
      password: 'router-pass',
      links: ['https://vk.com/call/join/abcdef123'],
    })
    // Пароль не остаётся в поле после сохранения
    expect((container?.querySelector('input[type="password"]') as HTMLInputElement).value).toBe('')
  })

  it('wizard refuses to save without server address, password or link', async () => {
    vi.spyOn(api, 'getVkTurnStatus').mockResolvedValue(defaultUnconfiguredStatus)
    const saveConfigSpy = vi.spyOn(api, 'saveVkTurnConfig').mockResolvedValue({ success: true })

    await act(async () => {
      root!.render(<VkTurn notify={notifyMock} />)
    })

    const enableBtn = findButton('Включить')!
    await fillWizard({ password: 'pw', link: 'https://vk.com/call/join/abcdef123' })
    await act(async () => {
      enableBtn.click()
    })
    expect(notifyMock).toHaveBeenCalledWith('Укажите адрес сервера VPS (IP или домен)', true)

    await fillWizard({ host: 'vps.example.com', password: '' })
    await act(async () => {
      enableBtn.click()
    })
    expect(notifyMock).toHaveBeenCalledWith('Укажите пароль подключения к серверу WDTT', true)

    await fillWizard({ password: 'pw', link: '' })
    await act(async () => {
      enableBtn.click()
    })
    expect(notifyMock).toHaveBeenCalledWith('Укажите хотя бы одну ссылку на звонок ВКонтакте', true)
    expect(saveConfigSpy).not.toHaveBeenCalled()

    // Свой порт сервера сохраняется как есть
    const portInput = container?.querySelector('input[aria-label="Порт сервера WDTT"]') as HTMLInputElement
    await typeInto(portInput, '443')
    await fillWizard({ link: 'https://vk.com/call/join/abcdef123' })
    await act(async () => {
      enableBtn.click()
    })
    expect(saveConfigSpy).toHaveBeenCalledWith(expect.objectContaining({ peer: 'vps.example.com:443' }))
  })

  it('wizard allows at most max_links call links', async () => {
    vi.spyOn(api, 'getVkTurnStatus').mockResolvedValue(defaultUnconfiguredStatus)
    await act(async () => {
      root!.render(<VkTurn notify={notifyMock} />)
    })
    for (let i = 0; i < 2; i++) {
      await act(async () => {
        findButton('Добавить запасную ссылку')!.click()
      })
    }
    expect(container?.querySelectorAll('input[placeholder*="ссылка"]').length).toBe(3)
    expect(findButton('Добавить запасную ссылку')).toBeUndefined()
  })

  it('in wizard mode triggers install_core first if component not installed', async () => {
    vi.spyOn(api, 'getVkTurnStatus').mockResolvedValue({ ...defaultUnconfiguredStatus, installed: false })
    const actionSpy = vi.spyOn(api, 'vkTurnAction').mockResolvedValue({ success: true })
    const saveConfigSpy = vi.spyOn(api, 'saveVkTurnConfig').mockResolvedValue({ success: true })

    await act(async () => {
      root!.render(<VkTurn notify={notifyMock} />)
    })
    await fillWizard({ host: '198.51.100.7', password: 'pw', link: 'https://vk.com/call/join/link12345' })
    await act(async () => {
      findButton('Включить')!.click()
    })

    expect(actionSpy).toHaveBeenCalledWith('install_core')
    expect(saveConfigSpy).toHaveBeenCalled()
    expect(actionSpy.mock.invocationCallOrder[0]).toBeLessThan(saveConfigSpy.mock.invocationCallOrder[0])
  })

  it('renders monitoring card for connected phase and handles toggle actions', async () => {
    vi.spyOn(api, 'getVkTurnStatus').mockResolvedValue(defaultConnectedStatus)
    const saveConfigSpy = vi.spyOn(api, 'saveVkTurnConfig').mockResolvedValue({ success: true })
    const actionSpy = vi.spyOn(api, 'vkTurnAction').mockResolvedValue({ delay_ms: 42, success: true })
    const logsSpy = vi.spyOn(api, 'getVkTurnLogs').mockResolvedValue({
      lines: ['[ГРУППА #1] Креды OK', '[ВОРКЕР #1] [READY] Туннель готов к работе ✓'],
    })

    await act(async () => {
      root!.render(<VkTurn notify={notifyMock} />)
    })

    const badge = container?.querySelector('[data-testid="status-badge"]')
    expect(badge?.textContent).toContain('🟢 Работает · соединений: 3')
    expect(container?.textContent).toContain('сервер WDTT: 1.2.3.4:56000')
    expect(container?.textContent).toContain('Ядро WDTT')
    expect(container?.querySelector('[data-testid="traffic-line"]')?.textContent).toContain('↑ 1.5 КБ ↓ 3.0 МБ')
    expect(container?.querySelector('[data-testid="waiting-wg"]')).toBeNull()

    const toggleInput = container?.querySelector('input[type="checkbox"][aria-label*="VK-туннель"]') as HTMLInputElement
    expect(toggleInput.checked).toBe(true)
    await act(async () => {
      toggleInput.click()
    })
    expect(saveConfigSpy).toHaveBeenCalledWith({ enabled: false })

    await act(async () => {
      findButton('Проверить')!.click()
    })
    expect(actionSpy).toHaveBeenCalledWith('test')
    expect(container?.textContent).toContain('Задержка 42 мс')

    await act(async () => {
      findButton('Перезапустить')!.click()
    })
    expect(actionSpy).toHaveBeenCalledWith('restart')

    const accordionToggle = Array.from(container?.querySelectorAll('[role="button"]') || []).find((el) =>
      el.textContent?.includes('Настройки туннеля и подробности')
    ) as HTMLElement
    await act(async () => {
      accordionToggle.click()
    })
    expect(logsSpy).toHaveBeenCalled()
    expect(container?.textContent).toContain('Число перезапусков')
    expect(container?.textContent).toContain('xkeen-route-0011223344556677')
    expect(container?.textContent).toContain('10.66.66.5')
    expect(container?.textContent).toContain('2 из 3')
    expect(container?.querySelector('[data-testid="links-list"]')?.textContent).toContain('abcd… (основная)')
    expect(container?.textContent).toContain('Туннель готов к работе')
  })

  it('shows waiting banner and disables check while server has not sent WireGuard config', async () => {
    vi.spyOn(api, 'getVkTurnStatus').mockResolvedValue({
      ...defaultConnectedStatus,
      wg_ready: false,
      wg: { address: '', mtu: 1280, dns: [] },
    })
    await act(async () => {
      root!.render(<VkTurn notify={notifyMock} />)
    })
    expect(container?.querySelector('[data-testid="waiting-wg"]')?.textContent).toContain('жду от него WireGuard-конфиг')
    expect(findButton('Проверить')!.disabled).toBe(true)
  })

  it('renders captcha_required phase banner with auto-retry hint', async () => {
    vi.spyOn(api, 'getVkTurnStatus').mockResolvedValue({ ...defaultConnectedStatus, phase: 'captcha_required' })
    await act(async () => {
      root!.render(<VkTurn notify={notifyMock} />)
    })
    const badge = container?.querySelector('[data-testid="status-badge"]')
    expect(badge?.textContent).toContain('🔴 ВК запросил капчу, повторю через 10 минут')
    expect(container?.textContent).toContain('через 10 минут')
  })

  it('auth_failed phase shows server reason and lets user save a new password', async () => {
    vi.spyOn(api, 'getVkTurnStatus').mockResolvedValue({
      ...defaultConnectedStatus,
      phase: 'auth_failed',
      last_error: 'Пароль уже привязан к другому устройству — нужен отдельный пароль для роутера',
    })
    const saveConfigSpy = vi.spyOn(api, 'saveVkTurnConfig').mockResolvedValue({ success: true })
    await act(async () => {
      root!.render(<VkTurn notify={notifyMock} />)
    })
    expect(container?.querySelector('[data-testid="status-badge"]')?.textContent).toContain('🔴 Сервер отклонил пароль')
    expect(container?.textContent).toContain('привязан к другому устройству')
    // Причина показана один раз, без дубля в строке ошибки
    expect(container?.querySelector('[data-testid="last-error"]')).toBeNull()

    const pw = container?.querySelector('input[placeholder="Новый пароль подключения"]') as HTMLInputElement
    await typeInto(pw, 'fresh-pass')
    await act(async () => {
      findButton('Сохранить пароль')!.click()
    })
    expect(saveConfigSpy).toHaveBeenCalledWith({ password: 'fresh-pass' })
  })

  it('renders link_invalid phase with input to update call link', async () => {
    vi.spyOn(api, 'getVkTurnStatus').mockResolvedValue({ ...defaultConnectedStatus, phase: 'link_invalid' })
    const saveConfigSpy = vi.spyOn(api, 'saveVkTurnConfig').mockResolvedValue({ success: true })
    await act(async () => {
      root!.render(<VkTurn notify={notifyMock} />)
    })
    expect(container?.querySelector('[data-testid="status-badge"]')?.textContent).toContain('🔴 Ссылка на звонок не работает')

    const newLinkInput = container?.querySelector('input[placeholder*="Новая ссылка на звонок"]') as HTMLInputElement
    await typeInto(newLinkInput, 'https://vk.com/call/join/freshlink')
    await act(async () => {
      findButton('Обновить ссылку')!.click()
    })
    // Новая ссылка становится основной, запасные не удаляются
    expect(saveConfigSpy).toHaveBeenCalledWith({ primary_link: 'https://vk.com/call/join/freshlink' })
  })

  it('shows partial-failure warning while still connected', async () => {
    vi.spyOn(api, 'getVkTurnStatus').mockResolvedValue({
      ...defaultConnectedStatus,
      last_error: 'Ссылка на звонок ВК не работает: звонок завершён или удалён',
    })
    await act(async () => {
      root!.render(<VkTurn notify={notifyMock} />)
    })
    expect(container?.querySelector('[data-testid="status-badge"]')?.textContent).toContain('Работает')
    expect(container?.querySelector('[data-testid="last-error"]')?.textContent).toContain('звонок завершён')
  })

  it('renders not_installed phase with installation button', async () => {
    vi.spyOn(api, 'getVkTurnStatus').mockResolvedValue({ ...defaultConnectedStatus, phase: 'not_installed', installed: false })
    const actionSpy = vi.spyOn(api, 'vkTurnAction').mockResolvedValue({ success: true })
    await act(async () => {
      root!.render(<VkTurn notify={notifyMock} />)
    })
    expect(container?.querySelector('[data-testid="status-badge"]')?.textContent).toContain('Компонент не установлен')
    await act(async () => {
      findButton('Установить компонент')!.click()
    })
    expect(actionSpy).toHaveBeenCalledWith('install_core')
  })

  it('renders all phases correctly', async () => {
    const phases: Array<{ phase: VkTurnPhase; expectedText: string }> = [
      { phase: 'starting', expectedText: '🟡 Подключаюсь…' },
      { phase: 'authorizing', expectedText: '🟡 Подключаюсь…' },
      { phase: 'solving_captcha', expectedText: '🟡 ВК проверяет, что мы не робот…' },
      { phase: 'degraded', expectedText: '🟡 Переподключаюсь' },
      { phase: 'backoff', expectedText: '🟡 Переподключаюсь' },
      { phase: 'crashed', expectedText: '🔴 Сбой процесса' },
      { phase: 'server_unreachable', expectedText: '🔴 Сервер на VPS не отвечает' },
      { phase: 'auth_failed', expectedText: '🔴 Сервер отклонил пароль' },
    ]

    for (const item of phases) {
      if (root && container) {
        act(() => {
          root!.unmount()
        })
        container.remove()
      }
      container = document.createElement('div')
      document.body.appendChild(container)
      root = createRoot(container)

      vi.spyOn(api, 'getVkTurnStatus').mockResolvedValue({ ...defaultConnectedStatus, phase: item.phase })
      await act(async () => {
        root!.render(<VkTurn notify={notifyMock} />)
      })
      const badge = container?.querySelector('[data-testid="status-badge"]')
      expect(badge?.textContent).toContain(item.expectedText)
      expect(container?.textContent).not.toContain('undefined')
    }
  })

  it('polls status every 3s while component is mounted', async () => {
    vi.useFakeTimers()
    const getStatusSpy = vi.spyOn(api, 'getVkTurnStatus').mockResolvedValue(defaultConnectedStatus)
    await act(async () => {
      root!.render(<VkTurn notify={notifyMock} />)
    })
    expect(getStatusSpy).toHaveBeenCalledTimes(1)
    await act(async () => {
      vi.advanceTimersByTime(3000)
    })
    expect(getStatusSpy).toHaveBeenCalledTimes(2)
    await act(async () => {
      vi.advanceTimersByTime(3000)
    })
    expect(getStatusSpy).toHaveBeenCalledTimes(3)
    vi.useRealTimers()
  })

  it('handles test latency error gracefully and notifies user', async () => {
    vi.spyOn(api, 'getVkTurnStatus').mockResolvedValue(defaultConnectedStatus)
    vi.spyOn(api, 'vkTurnAction').mockRejectedValue(new Error('Connection timed out'))
    await act(async () => {
      root!.render(<VkTurn notify={notifyMock} />)
    })
    await act(async () => {
      findButton('Проверить')!.click()
    })
    expect(notifyMock).toHaveBeenCalledWith('Connection timed out', true)
  })

  it('hostFromPeer and formatTraffic helpers', () => {
    expect(hostFromPeer('198.51.100.1:56000')).toBe('198.51.100.1')
    expect(hostFromPeer('vps.example.com:56000')).toBe('vps.example.com')
    expect(hostFromPeer('[2001:db8::1]:56000')).toBe('2001:db8::1')
    expect(hostFromPeer(undefined)).toBe('')
    expect(formatTraffic(0)).toBe('0 Б')
    expect(formatTraffic(2048)).toBe('2.0 КБ')
    expect(formatTraffic(5 * 1024 ** 3)).toBe('5.00 ГБ')
    expect(formatTraffic(undefined)).toBe('0 Б')
  })
})
