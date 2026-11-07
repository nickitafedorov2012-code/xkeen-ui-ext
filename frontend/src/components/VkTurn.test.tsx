import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import VkTurn, { hostFromPeer, formatTraffic, installProgressText, waitForCoreInstall, vkTurnTiming } from './VkTurn'
import * as api from '../api'
import type { VkTurnStatus, VkTurnPhase } from '../types'

// @ts-ignore
globalThis.IS_REACT_ACT_ENVIRONMENT = true

const inputSetter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value')?.set!

async function typeInto(el: HTMLInputElement, value: string) {
  await act(async () => {
    inputSetter.call(el, value)
    el.dispatchEvent(new Event('input', { bubbles: true }))
  })
}

describe('VkTurn Component (ядро csqtt)', () => {
  let container: HTMLDivElement | null = null
  let root: ReturnType<typeof createRoot> | null = null
  const notifyMock = vi.fn()

  const defaultUnconfiguredStatus: VkTurnStatus = {
    installed: true,
    core_version: 'csqtt-core-0e90df2',
    phase: 'disabled',
    pid: null,
    connected_streams: 0,
    target_streams: 100,
    bytes_up: 0,
    bytes_down: 0,
    links_count: 0,
    links: [],
    max_links: 6,
    restarts: 0,
    last_error: null,
    enabled: false,
    peer: '',
    has_password: false,
    tunnel_ready: false,
    tunnel_ip: null,
    tunnel_dns: null,
    interface: 'csq0',
    default_port: 46000,
  }

  const defaultConnectedStatus: VkTurnStatus = {
    installed: true,
    core_version: 'csqtt-core-0e90df2',
    phase: 'connected',
    pid: 12345,
    connected_streams: 3,
    target_streams: 100,
    bytes_up: 1536,
    bytes_down: 3 * 1024 * 1024,
    links_count: 2,
    links: ['https://vk.com/call/join/abcd…', 'https://vk.com/call/join/efgh…'],
    max_links: 6,
    restarts: 1,
    last_error: null,
    enabled: true,
    peer: '1.2.3.4:46000',
    has_password: true,
    tunnel_ready: true,
    tunnel_ip: '10.66.67.3',
    tunnel_dns: '1.1.1.1',
    interface: 'csq0',
    default_port: 46000,
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
    expect(container?.textContent).toContain('Сервер csqtt на VPS')
    expect(container?.textContent).toContain('Пароль подключения')
    expect(container?.textContent).toContain('Установить на мой VPS автоматически')
    // csqtt: адрес туннеля выдаёт сервер — вставлять конфиги не нужно
    expect(container?.querySelector('textarea')).toBeNull()
    expect(container?.textContent).not.toContain('Телемост')

    await fillWizard({ host: '198.51.100.1', password: 'router-pass', link: 'https://vk.com/call/join/abcdef123' })

    const enableBtn = findButton('Включить')!
    await act(async () => {
      enableBtn.click()
    })

    // Одно сохранение, без отдельного вызова enable; PEER-порт по умолчанию 46000
    expect(saveConfigSpy).toHaveBeenCalledTimes(1)
    expect(saveConfigSpy).toHaveBeenCalledWith({
      enabled: true,
      peer: '198.51.100.1:46000',
      password: 'router-pass',
      links: ['https://vk.com/call/join/abcdef123'],
    })
    // Пароль не остаётся в поле после сохранения
    expect((container?.querySelector('input[type="password"]') as HTMLInputElement).value).toBe('')
  })

  it('does not show first-time setup when the tunnel status is unavailable and allows retry', async () => {
    vi.spyOn(api, 'getVkTurnStatus').mockRejectedValueOnce(new Error('offline')).mockResolvedValue(defaultUnconfiguredStatus)
    await act(async () => { root!.render(<VkTurn notify={notifyMock} />) })
    expect(container!.textContent).toContain('Не удалось получить состояние VK-туннеля')
    expect(container!.textContent).not.toContain('Мастер настройки VK-туннеля')

    const retry = findButton('Повторить')!
    await act(async () => { retry.click() })
    expect(container!.textContent).toContain('Мастер настройки VK-туннеля')
  })

  it('marks the tunnel status unavailable after a polling failure and recovers', async () => {
    vi.useFakeTimers()
    try {
      vi.spyOn(api, 'getVkTurnStatus')
        .mockResolvedValueOnce(defaultConnectedStatus)
        .mockRejectedValueOnce(new Error('offline'))
        .mockResolvedValue(defaultConnectedStatus)
      await act(async () => { root!.render(<VkTurn notify={notifyMock} />) })
      expect(container!.textContent).toContain('Работает · соединений')
      await act(async () => { await vi.advanceTimersByTimeAsync(3000) })
      expect(container!.textContent).toContain('Не удалось получить состояние VK-туннеля')
      await act(async () => { await vi.advanceTimersByTimeAsync(3000) })
      expect(container!.textContent).not.toContain('Не удалось получить состояние VK-туннеля')
      expect(container!.textContent).toContain('Работает · соединений')
    } finally {
      vi.useRealTimers()
    }
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
    expect(notifyMock).toHaveBeenCalledWith('Укажите пароль подключения к серверу csqtt', true)

    await fillWizard({ password: 'pw', link: '' })
    await act(async () => {
      enableBtn.click()
    })
    expect(notifyMock).toHaveBeenCalledWith('Укажите хотя бы одну ссылку на звонок ВКонтакте', true)
    expect(saveConfigSpy).not.toHaveBeenCalled()

    // Свой порт сервера сохраняется как есть
    const portInput = container?.querySelector('input[aria-label="Порт сервера csqtt"]') as HTMLInputElement
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
    for (let i = 0; i < 5; i++) {
      await act(async () => {
        findButton('Добавить запасную ссылку')!.click()
      })
    }
    expect(container?.querySelectorAll('input[placeholder*="ссылка"]').length).toBe(6)
    expect(findButton('Добавить запасную ссылку')).toBeUndefined()
  })

  // Регрессия: скачивание компонента шло одним запросом и обрывалось таймаутом панели (30 с).
  // Теперь компонент качается в фоне, мастер ждёт окончания по статусу и только потом сохраняет.
  it('in wizard mode installs the component in background and saves after it is installed', async () => {
    vkTurnTiming.installPollMs = 0
    vi.spyOn(api, 'getVkTurnStatus')
      .mockResolvedValueOnce({ ...defaultUnconfiguredStatus, installed: false })
      .mockResolvedValueOnce({
        ...defaultUnconfiguredStatus,
        installed: false,
        install: { running: true, downloaded: 3 * 1024 * 1024, total: 10 * 1024 * 1024, error: null },
      })
      .mockResolvedValue({ ...defaultUnconfiguredStatus, installed: true, install: { running: false, downloaded: 1, total: 1, error: null } })
    const actionSpy = vi.spyOn(api, 'vkTurnAction').mockResolvedValue({ success: true })
    const saveConfigSpy = vi.spyOn(api, 'saveVkTurnConfig').mockResolvedValue({ success: true })

    await act(async () => {
      root!.render(<VkTurn notify={notifyMock} />)
    })
    await fillWizard({ host: '198.51.100.7', password: 'pw', link: 'https://vk.com/call/join/link12345' })
    await act(async () => {
      findButton('Включить')!.click()
    })
    await act(async () => {
      await vi.waitFor(() => expect(saveConfigSpy).toHaveBeenCalled())
    })

    expect(actionSpy).toHaveBeenCalledWith('install_core')
    expect(actionSpy.mock.invocationCallOrder[0]).toBeLessThan(saveConfigSpy.mock.invocationCallOrder[0])
    expect(notifyMock).not.toHaveBeenCalledWith(expect.anything(), true)
  })

  it('in wizard mode reports a failed background download and does not save', async () => {
    vkTurnTiming.installPollMs = 0
    vi.spyOn(api, 'getVkTurnStatus')
      .mockResolvedValueOnce({ ...defaultUnconfiguredStatus, installed: false })
      .mockResolvedValue({
        ...defaultUnconfiguredStatus,
        installed: false,
        install: { running: false, downloaded: 4096, total: 10_000_000, error: 'Не удалось скачать (direct): данные перестали приходить' },
      })
    vi.spyOn(api, 'vkTurnAction').mockResolvedValue({ success: true })
    const saveConfigSpy = vi.spyOn(api, 'saveVkTurnConfig').mockResolvedValue({ success: true })

    await act(async () => {
      root!.render(<VkTurn notify={notifyMock} />)
    })
    await fillWizard({ host: '198.51.100.7', password: 'pw', link: 'https://vk.com/call/join/link12345' })
    await act(async () => {
      findButton('Включить')!.click()
    })
    await act(async () => {
      await vi.waitFor(() =>
        expect(notifyMock).toHaveBeenCalledWith(
          'Не удалось скачать компонент: Не удалось скачать (direct): данные перестали приходить',
          true,
        ),
      )
    })
    expect(saveConfigSpy).not.toHaveBeenCalled()
  })

  it('shows download progress while the component is being installed', async () => {
    vi.spyOn(api, 'getVkTurnStatus').mockResolvedValue({
      ...defaultConnectedStatus,
      phase: 'not_installed',
      installed: false,
      install: { running: true, downloaded: 3 * 1024 * 1024, total: 10 * 1024 * 1024, error: null },
    })
    await act(async () => {
      root!.render(<VkTurn notify={notifyMock} />)
    })
    expect(container?.querySelector('[data-testid="install-progress"]')?.textContent).toContain('3.0 МБ из 10.0 МБ (30%)')
    expect(findButton('Установить компонент')).toBeUndefined()
  })

  // Регрессия: пока статус «Подключаюсь…», поменять пароль было негде — поле появлялось только
  // в фазах отказа, а ядро csqtt при неверном пароле до них не доходило.
  it('lets the user change the connection password in tunnel settings while connecting', async () => {
    vi.spyOn(api, 'getVkTurnStatus').mockResolvedValue({
      ...defaultConnectedStatus,
      phase: 'authorizing',
      connected_streams: 0,
      tunnel_ready: false,
      tunnel_ip: null,
    })
    const saveConfigSpy = vi.spyOn(api, 'saveVkTurnConfig').mockResolvedValue({ success: true })
    await act(async () => {
      root!.render(<VkTurn notify={notifyMock} />)
    })
    expect(container?.querySelector('[data-testid="password-fix"]')).toBeNull()
    const accordionToggle = Array.from(container?.querySelectorAll('[role="button"]') || []).find((el) =>
      el.textContent?.includes('Настройки туннеля и подробности'),
    ) as HTMLElement
    await act(async () => {
      accordionToggle.click()
    })
    const pw = container?.querySelector('input[placeholder="Сменить пароль подключения"]') as HTMLInputElement
    expect(pw).toBeTruthy()
    await typeInto(pw, 'TEST')
    await act(async () => {
      findButton('Сменить пароль')!.click()
    })
    expect(saveConfigSpy).toHaveBeenCalledWith({ password: 'TEST' })
    expect(notifyMock).toHaveBeenCalledWith('Пароль подключения обновлён, переподключаюсь')
  })

  it('lets the user change stream count and saves only a supported value', async () => {
    vi.spyOn(api, 'getVkTurnStatus').mockResolvedValue(defaultConnectedStatus)
    const saveConfigSpy = vi.spyOn(api, 'saveVkTurnConfig').mockResolvedValue({ success: true })
    await act(async () => {
      root!.render(<VkTurn notify={notifyMock} />)
    })
    const accordionToggle = Array.from(container?.querySelectorAll('[role="button"]') || []).find((el) =>
      el.textContent?.includes('Настройки туннеля и подробности'),
    ) as HTMLElement
    await act(async () => accordionToggle.click())

    const input = container?.querySelector('[data-testid="stream-count"]') as HTMLInputElement
    expect(input).toBeTruthy()
    expect(input.value).toBe('100')
    await act(async () => {
      inputSetter.call(input, '40')
      input.dispatchEvent(new Event('change', { bubbles: true }))
    })
    expect(saveConfigSpy).toHaveBeenCalledWith({ streams: 40 })
    expect(notifyMock).toHaveBeenCalledWith('Число потоков изменено на 40; туннель переподключается')
  })

  it('installProgressText and waitForCoreInstall', async () => {
    expect(installProgressText({ running: true, downloaded: 0, total: null })).toBe('подключаюсь к серверу загрузки')
    expect(installProgressText({ running: true, downloaded: 2048, total: null })).toBe('2.0 КБ')
    expect(installProgressText({ running: true, downloaded: 512, total: 1024 })).toBe('512 Б из 1.0 КБ (50%)')

    const seen: VkTurnStatus[] = []
    const seq: VkTurnStatus[] = [
      { ...defaultUnconfiguredStatus, installed: false, install: { running: true, downloaded: 1, total: 2 } },
      { ...defaultUnconfiguredStatus, installed: true, install: { running: false, downloaded: 2, total: 2 } },
    ]
    let i = 0
    await waitForCoreInstall(async () => seq[Math.min(i++, seq.length - 1)], (s) => seen.push(s), 0)
    expect(seen.length).toBe(2)

    // Роутер на мгновение не ответил — ждём дальше, а не падаем
    let calls = 0
    await waitForCoreInstall(
      async () => {
        calls++
        if (calls === 1) throw new Error('network')
        return { ...defaultUnconfiguredStatus, installed: true, install: { running: false, downloaded: 2, total: 2 } }
      },
      () => {},
      0,
    )
    expect(calls).toBe(2)

    await expect(
      waitForCoreInstall(
        async () => ({ ...defaultUnconfiguredStatus, installed: false, install: { running: true, downloaded: 1 } }),
        () => {},
        5,
        30,
      ),
    ).rejects.toThrow('слишком долго')
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
    expect(container?.textContent).toContain('сервер csqtt: 1.2.3.4:46000')
    expect(container?.textContent).toContain('Ядро csqtt')
    expect(container?.querySelector('[data-testid="traffic-line"]')?.textContent).toContain('↑ 1.5 КБ ↓ 3.0 МБ')
    expect(container?.querySelector('[data-testid="waiting-tunnel"]')).toBeNull()

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
    expect(container?.querySelector('[data-testid="tunnel-ip"]')?.textContent).toBe('10.66.67.3 · csq0')
    expect(container?.textContent).toContain('2 из 6')
    expect(container?.querySelector('[data-testid="links-list"]')?.textContent).toContain('abcd… (основная)')
    expect(container?.textContent).toContain('Туннель готов к работе')
  })

  it('shows waiting banner and disables check while the tunnel is not attached yet', async () => {
    vi.spyOn(api, 'getVkTurnStatus').mockResolvedValue({
      ...defaultConnectedStatus,
      tunnel_ready: false,
      tunnel_ip: null,
    })
    await act(async () => {
      root!.render(<VkTurn notify={notifyMock} />)
    })
    expect(container?.querySelector('[data-testid="waiting-tunnel"]')?.textContent).toContain('подключаю туннель')
    expect(findButton('Проверить')!.disabled).toBe(true)
  })

  // Сервер csqtt при неверном пароле молчит так же, как выключенный: поле нового пароля нужно и здесь
  it('server_unreachable phase explains silent wrong password and lets user re-enter it', async () => {
    vi.spyOn(api, 'getVkTurnStatus').mockResolvedValue({
      ...defaultConnectedStatus,
      phase: 'server_unreachable',
      connected_streams: 0,
      tunnel_ready: false,
      last_error: 'Сервер csqtt на VPS не отвечает',
    })
    const saveConfigSpy = vi.spyOn(api, 'saveVkTurnConfig').mockResolvedValue({ success: true })
    await act(async () => {
      root!.render(<VkTurn notify={notifyMock} />)
    })
    expect(container?.querySelector('[data-testid="status-badge"]')?.textContent).toContain('🔴 Сервер на VPS не отвечает')
    expect(container?.querySelector('[data-testid="password-fix"]')?.textContent).toContain('При неверном пароле сервер тоже молчит')
    expect(container?.querySelector('[data-testid="last-error"]')).toBeNull()
    const pw = container?.querySelector('input[placeholder="Новый пароль подключения"]') as HTMLInputElement
    await typeInto(pw, 'fixed-pass')
    await act(async () => {
      findButton('Сохранить пароль')!.click()
    })
    expect(saveConfigSpy).toHaveBeenCalledWith({ password: 'fixed-pass' })
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
      last_error: 'Срок действия пароля подключения истёк — получите новый у владельца сервера',
    })
    const saveConfigSpy = vi.spyOn(api, 'saveVkTurnConfig').mockResolvedValue({ success: true })
    await act(async () => {
      root!.render(<VkTurn notify={notifyMock} />)
    })
    expect(container?.querySelector('[data-testid="status-badge"]')?.textContent).toContain('🔴 Сервер отклонил пароль')
    expect(container?.textContent).toContain('Срок действия пароля')
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
    vkTurnTiming.installPollMs = 0
    await act(async () => {
      root!.render(<VkTurn notify={notifyMock} />)
    })
    expect(container?.querySelector('[data-testid="status-badge"]')?.textContent).toContain('Компонент не установлен')
    await act(async () => {
      findButton('Установить компонент')!.click()
    })
    expect(actionSpy).toHaveBeenCalledWith('install_core')
    // Статус так и не показал установленный компонент — пользователь видит понятную ошибку
    await act(async () => {
      await vi.waitFor(() => expect(notifyMock).toHaveBeenCalledWith('Компонент не установился — попробуйте ещё раз', true))
    })
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
    expect(hostFromPeer('198.51.100.1:46000')).toBe('198.51.100.1')
    expect(hostFromPeer('vps.example.com:46000')).toBe('vps.example.com')
    expect(hostFromPeer('[2001:db8::1]:46000')).toBe('2001:db8::1')
    expect(hostFromPeer(undefined)).toBe('')
    expect(formatTraffic(0)).toBe('0 Б')
    expect(formatTraffic(2048)).toBe('2.0 КБ')
    expect(formatTraffic(5 * 1024 ** 3)).toBe('5.00 ГБ')
    expect(formatTraffic(undefined)).toBe('0 Б')
  })
})
