import { useState, useEffect, useRef, useCallback } from 'react'
import {
  getVkTurnStatus,
  getVkTurnLogs,
  saveVkTurnConfig,
  vkTurnAction,
} from '../api'
import type { VkTurnStatus, VkTurnPhase, VkTurnCoreInstall } from '../types'

interface VkTurnProps {
  notify: (msg: string, error?: boolean) => void
}

/** PEER-порт сервера csqtt на VPS по умолчанию */
export const DEFAULT_CSQTT_PORT = 46000
/** Клиент csqtt использует до шести ссылок одновременно */
const MAX_LINKS_FALLBACK = 6

/** Хост из "host:port" / "[v6]:port" для подстановки в поле адреса сервера. */
export function hostFromPeer(peer?: string | null): string {
  if (!peer) return ''
  const i = peer.lastIndexOf(':')
  const host = i > 0 ? peer.slice(0, i) : peer
  return host.replace(/^\[|\]$/g, '').trim()
}

export function formatTraffic(bytes?: number | null): string {
  const b = Number(bytes) || 0
  if (b < 1024) return `${b} Б`
  if (b < 1024 ** 2) return `${(b / 1024).toFixed(1)} КБ`
  if (b < 1024 ** 3) return `${(b / 1024 ** 2).toFixed(1)} МБ`
  return `${(b / 1024 ** 3).toFixed(2)} ГБ`
}

/** Сколько скачано из компонента: «3.2 МБ из 10.3 МБ (31%)» */
export function installProgressText(install?: VkTurnCoreInstall | null): string {
  const done = Number(install?.downloaded) || 0
  const total = Number(install?.total) || 0
  if (total > 0) {
    const pct = Math.min(100, Math.floor((done / total) * 100))
    return `${formatTraffic(done)} из ${formatTraffic(total)} (${pct}%)`
  }
  return done > 0 ? formatTraffic(done) : 'подключаюсь к серверу загрузки'
}

/**
 * Компонент скачивается на роутере в фоне — на медленном канале это минуты, дольше таймаута
 * одного запроса панели. Ждём окончания по статусу.
 */
export async function waitForCoreInstall(
  fetchStatus: () => Promise<VkTurnStatus>,
  onStatus: (s: VkTurnStatus) => void,
  pollMs = 1500,
  maxWaitMs = 16 * 60 * 1000,
): Promise<void> {
  const deadline = Date.now() + maxWaitMs
  while (Date.now() < deadline) {
    await new Promise((r) => setTimeout(r, pollMs))
    let st: VkTurnStatus
    try {
      st = await fetchStatus()
    } catch {
      continue // роутер мог на секунду не ответить — продолжаем ждать
    }
    onStatus(st)
    if (st.install?.running) continue
    if (st.install?.error) throw new Error(`Не удалось скачать компонент: ${st.install.error}`)
    if (st.installed) return
    throw new Error('Компонент не установился — попробуйте ещё раз')
  }
  throw new Error('Компонент скачивается слишком долго — проверьте интернет на роутере и попробуйте ещё раз')
}

/** Интервал опроса статуса во время установки компонента (в тестах уменьшается). */
export const vkTurnTiming = { installPollMs: 1500 }

const badgeStyle = { fontSize: 13, padding: '4px 10px' }

export default function VkTurn({ notify }: VkTurnProps) {
  const [status, setStatus] = useState<VkTurnStatus | null>(null)
  const [loading, setLoading] = useState(true)
  const [statusUnavailable, setStatusUnavailable] = useState(false)
  const [actionPending, setActionPending] = useState(false)
  const [logs, setLogs] = useState<string[]>([])
  const [loadingLogs, setLoadingLogs] = useState(false)
  const [accordionOpen, setAccordionOpen] = useState(false)
  const [testLatency, setTestLatency] = useState<number | null>(null)
  const [testingPing, setTestingPing] = useState(false)

  // Мастер
  const [peerHost, setPeerHost] = useState('')
  const [serverPort, setServerPort] = useState<number>(DEFAULT_CSQTT_PORT)
  const [password, setPassword] = useState('')
  const [callLinks, setCallLinks] = useState<string[]>([''])

  // Карточка: новая ссылка / новый пароль
  const [newLinkInput, setNewLinkInput] = useState('')
  const [updatingLink, setUpdatingLink] = useState(false)
  const [newPassword, setNewPassword] = useState('')
  const [updatingPassword, setUpdatingPassword] = useState(false)

  const pollingRef = useRef<boolean>(false)

  const loadStatus = useCallback(async (isInitial = false) => {
    if (document.hidden && !isInitial) return
    if (pollingRef.current) return
    pollingRef.current = true
    try {
      const data = await getVkTurnStatus()
      setStatus(data)
      setStatusUnavailable(false)
    } catch {
      setStatusUnavailable(true)
    } finally {
      pollingRef.current = false
      if (isInitial) setLoading(false)
    }
  }, [])

  useEffect(() => {
    loadStatus(true)
    const interval = setInterval(() => {
      loadStatus(false)
    }, 3000)
    return () => clearInterval(interval)
  }, [loadStatus])

  const fetchLogs = async () => {
    setLoadingLogs(true)
    try {
      const res = await getVkTurnLogs()
      setLogs(res.lines || [])
    } catch (err: any) {
      notify(err?.message || 'Не удалось загрузить логи VK-туннеля', true)
    } finally {
      setLoadingLogs(false)
    }
  }

  const maxLinks = status?.max_links || MAX_LINKS_FALLBACK

  const installCore = async () => {
    await vkTurnAction('install_core')
    await waitForCoreInstall(getVkTurnStatus, setStatus, vkTurnTiming.installPollMs)
  }

  const handleCallLinkChange = (index: number, val: string) => {
    setCallLinks((prev) => {
      const copy = [...prev]
      copy[index] = val
      return copy
    })
  }

  // Мастер: «Включить»
  const handleWizardEnable = async () => {
    const host = peerHost.trim()
    if (!host) {
      notify('Укажите адрес сервера VPS (IP или домен)', true)
      return
    }
    if (!password.trim()) {
      notify('Укажите пароль подключения к серверу csqtt', true)
      return
    }
    const links = callLinks.map((l) => l.trim()).filter(Boolean)
    if (links.length === 0) {
      notify('Укажите хотя бы одну ссылку на звонок ВКонтакте', true)
      return
    }

    setActionPending(true)
    try {
      if (status && !status.installed) {
        notify('Скачиваю компонент VK-туннеля (около 11 МБ) — на медленном интернете это займёт несколько минут…')
        await installCore()
      }

      notify('Сохранение конфигурации и запуск...')
      // Одно сохранение с enabled: true уже запускает клиент — отдельный вызов «enable» не нужен
      await saveVkTurnConfig({
        enabled: true,
        peer: `${host}:${serverPort || DEFAULT_CSQTT_PORT}`,
        password: password.trim(),
        links,
      })
      setPassword('')
      notify('VK-туннель настроен. WireGuard-конфиг придёт от сервера автоматически.')
      await loadStatus(true)
    } catch (err: any) {
      notify(err?.message || 'Ошибка настройки VK-туннеля', true)
    } finally {
      setActionPending(false)
    }
  }

  const handleToggleEnabled = async (checked: boolean) => {
    setActionPending(true)
    try {
      await saveVkTurnConfig({ enabled: checked })
      notify(checked ? 'VK-туннель включен' : 'VK-туннель выключен')
      await loadStatus(true)
    } catch (err: any) {
      notify(err?.message || 'Ошибка переключения туннеля', true)
    } finally {
      setActionPending(false)
    }
  }

  const handleTest = async () => {
    setTestingPing(true)
    try {
      const res = await vkTurnAction('test')
      if (res.delay_ms !== undefined) {
        setTestLatency(res.delay_ms)
        notify(`Проверка связи: задержка ${res.delay_ms} мс`)
      } else {
        notify('Проверка завершена успешно')
      }
    } catch (err: any) {
      notify(err?.message || 'Ошибка проверки туннеля', true)
    } finally {
      setTestingPing(false)
    }
  }

  const handleRestart = async () => {
    setActionPending(true)
    try {
      await vkTurnAction('restart')
      notify('VK-туннель перезапущен')
      await loadStatus(true)
    } catch (err: any) {
      notify(err?.message || 'Ошибка перезапуска туннеля', true)
    } finally {
      setActionPending(false)
    }
  }

  const handleInstallCore = async () => {
    setActionPending(true)
    try {
      notify('Скачиваю компонент VK-туннеля (около 11 МБ) — на медленном интернете это займёт несколько минут…')
      await installCore()
      notify('Компонент успешно установлен')
      await loadStatus(true)
    } catch (err: any) {
      notify(err?.message || 'Ошибка установки компонента', true)
    } finally {
      setActionPending(false)
    }
  }

  const handleUpdateCallLink = async () => {
    const trimmed = newLinkInput.trim()
    if (!trimmed) {
      notify('Введите новую ссылку на звонок', true)
      return
    }
    setUpdatingLink(true)
    try {
      // Новая ссылка становится основной, запасные сохраняются
      await saveVkTurnConfig({ primary_link: trimmed })
      notify('Ссылка на звонок обновлена')
      setNewLinkInput('')
      await loadStatus(true)
    } catch (err: any) {
      notify(err?.message || 'Ошибка обновления ссылки', true)
    } finally {
      setUpdatingLink(false)
    }
  }

  const handleUpdatePassword = async () => {
    const trimmed = newPassword.trim()
    if (!trimmed) {
      notify('Введите новый пароль подключения', true)
      return
    }
    setUpdatingPassword(true)
    try {
      await saveVkTurnConfig({ password: trimmed })
      notify('Пароль подключения обновлён, переподключаюсь')
      setNewPassword('')
      await loadStatus(true)
    } catch (err: any) {
      notify(err?.message || 'Ошибка обновления пароля', true)
    } finally {
      setUpdatingPassword(false)
    }
  }

  const handleUpdateStreams = async (raw: string) => {
    const streams = Number(raw)
    if (!Number.isInteger(streams) || streams < 3 || streams > 100) return
    setActionPending(true)
    try {
      await saveVkTurnConfig({ streams })
      notify(`Число потоков изменено на ${streams}; туннель переподключается`)
      await loadStatus(true)
    } catch (err: any) {
      notify(err?.message || 'Не удалось изменить число потоков', true)
      await loadStatus(true)
    } finally {
      setActionPending(false)
    }
  }

  if (loading && !status) {
    return (
      <div className="card" style={{ padding: 40, textAlign: 'center' }}>
        <div className="spinner" style={{ margin: '0 auto 16px' }} />
        <div className="muted">Загрузка модуля VK-туннель…</div>
      </div>
    )
  }

  if (statusUnavailable || !status) {
    return (
      <section className="card" role="alert">
        <h2>Не удалось получить состояние VK-туннеля</h2>
        <p className="muted">Состояние туннеля сейчас неизвестно. Повторите попытку после восстановления связи.</p>
        <button type="button" className="btn" onClick={() => void loadStatus(true)}>Повторить</button>
      </section>
    )
  }

  const installing = Boolean(status?.install?.running)
  const isConfigured = Boolean(status?.peer && status?.has_password && (status?.links_count ?? 0) > 0)

  const renderPhaseBadge = (phase: VkTurnPhase = 'disabled', connected = 0) => {
    switch (phase) {
      case 'connected':
        return (
          <span className="badge badge-success" data-testid="status-badge" style={badgeStyle}>
            🟢 Работает · соединений: {connected}
          </span>
        )
      case 'starting':
      case 'authorizing':
        return (
          <span className="badge badge-warning" data-testid="status-badge" style={badgeStyle}>
            🟡 Подключаюсь…
          </span>
        )
      case 'solving_captcha':
        return (
          <span className="badge badge-warning" data-testid="status-badge" style={badgeStyle}>
            🟡 ВК проверяет, что мы не робот…
          </span>
        )
      case 'captcha_required':
        return (
          <span className="badge badge-danger" data-testid="status-badge" style={badgeStyle}>
            🔴 ВК запросил капчу, повторю через 10 минут
          </span>
        )
      case 'link_invalid':
        return (
          <span className="badge badge-danger" data-testid="status-badge" style={badgeStyle}>
            🔴 Ссылка на звонок не работает
          </span>
        )
      case 'auth_failed':
        return (
          <span className="badge badge-danger" data-testid="status-badge" style={badgeStyle}>
            🔴 Сервер отклонил пароль
          </span>
        )
      case 'server_unreachable':
        return (
          <span className="badge badge-danger" data-testid="status-badge" style={badgeStyle}>
            🔴 Сервер на VPS не отвечает
          </span>
        )
      case 'degraded':
      case 'backoff':
        return (
          <span className="badge badge-warning" data-testid="status-badge" style={badgeStyle}>
            🟡 Переподключаюсь
          </span>
        )
      case 'not_installed':
        return (
          <span className="badge badge-neutral" data-testid="status-badge" style={badgeStyle}>
            Компонент не установлен
          </span>
        )
      case 'crashed':
        return (
          <span className="badge badge-danger" data-testid="status-badge" style={badgeStyle}>
            🔴 Сбой процесса
          </span>
        )
      case 'disabled':
      default:
        return (
          <span className="badge badge-neutral" data-testid="status-badge" style={badgeStyle}>
            Отключен
          </span>
        )
    }
  }

  // ---------------------------------------------------------
  // A. Мастер (туннель ещё не настроен)
  // ---------------------------------------------------------
  if (!isConfigured) {
    return (
      <div className="vkturn-container" style={{ display: 'flex', flexDirection: 'column', gap: 20 }}>
        <section
          className="card"
          style={{
            position: 'relative',
            overflow: 'hidden',
            padding: '24px 26px',
            background: 'linear-gradient(135deg, rgba(37, 99, 235, 0.08) 0%, rgba(15, 23, 42, 0.6) 100%)',
            border: '1px solid rgba(59, 130, 246, 0.3)',
            borderRadius: 16,
          }}
        >
          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 16, flexWrap: 'wrap' }}>
            <div>
              <div style={{ display: 'flex', alignItems: 'center', gap: 12, marginBottom: 6 }}>
                <span style={{ fontSize: 28 }}>📞</span>
                <h2 style={{ margin: 0, fontSize: 20, fontWeight: 700 }}>Мастер настройки VK-туннеля</h2>
                <span className="badge badge-warning" style={{ fontSize: 12 }}>Первоначальная настройка</span>
              </div>
              <p className="muted" style={{ margin: 0, fontSize: 14 }}>
                Трафик идёт через серверы звонков ВКонтакте до вашего сервера csqtt на VPS — работает даже при белых списках.
              </p>
            </div>
            <button
              className="btn"
              disabled
              style={{
                display: 'inline-flex',
                alignItems: 'center',
                gap: 8,
                opacity: 0.6,
                cursor: 'not-allowed',
              }}
              title="Скоро: этап 2"
            >
              <span>⚙️ Установить на мой VPS автоматически</span>
              <span className="badge badge-neutral" style={{ fontSize: 11, background: 'rgba(255,255,255,0.1)' }}>скоро</span>
            </button>
          </div>
        </section>

        <section className="card" style={{ padding: '24px 26px' }}>
          <h3 style={{ margin: '0 0 16px 0', fontSize: 16 }}>1️⃣ Сервер csqtt на VPS</h3>
          <div style={{ display: 'flex', gap: 16, flexWrap: 'wrap', marginBottom: 16 }}>
            <div style={{ flex: '1 1 260px', maxWidth: 420 }}>
              <label style={{ display: 'block', fontSize: 13, marginBottom: 6, fontWeight: 600 }}>
                Адрес сервера (IP или домен):
              </label>
              <input
                type="text"
                className="input"
                placeholder="Например, 203.0.113.10"
                value={peerHost}
                onChange={(e) => setPeerHost(e.target.value)}
                style={{ width: '100%' }}
              />
            </div>
            <div style={{ flex: '0 1 160px' }}>
              <label style={{ display: 'block', fontSize: 13, marginBottom: 6, fontWeight: 600 }}>
                Порт:
              </label>
              <input
                type="number"
                className="input"
                aria-label="Порт сервера csqtt"
                value={serverPort}
                onChange={(e) => setServerPort(Number(e.target.value) || DEFAULT_CSQTT_PORT)}
                style={{ width: '100%' }}
              />
            </div>
          </div>

          <h3 style={{ margin: '24px 0 16px 0', fontSize: 16 }}>2️⃣ Пароль подключения</h3>
          <div style={{ marginBottom: 8, maxWidth: 420 }}>
            <input
              type="password"
              className="input"
              autoComplete="new-password"
              placeholder="Пароль от сервера csqtt"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              style={{ width: '100%' }}
            />
          </div>
          <p className="muted" style={{ fontSize: 13, marginTop: 0, marginBottom: 0 }}>
            🔑 Это пароль туннеля, заданный при установке сервера csqtt (или выданный владельцем сервера).
            Адрес внутри туннеля сервер выдаст сам.
          </p>

          <h3 style={{ margin: '24px 0 16px 0', fontSize: 16 }}>3️⃣ Ссылки на звонки ВКонтакте</h3>
          <p className="muted" style={{ fontSize: 13, marginTop: -8, marginBottom: 14 }}>
            💡 Создайте звонок в ВК и не нажимайте «Завершить для всех» — тогда ссылка живёт долго. Можно добавить
            до {maxLinks} ссылок: они работают одновременно, и если одна умрёт, остальные продолжат.
          </p>

          <div style={{ display: 'flex', flexDirection: 'column', gap: 10, marginBottom: 16 }}>
            {callLinks.map((link, idx) => (
              <div key={idx} style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
                <input
                  type="text"
                  className="input"
                  placeholder={idx === 0 ? 'Основная ссылка на звонок (https://vk.com/call/join/...)' : 'Запасная ссылка'}
                  value={link}
                  onChange={(e) => handleCallLinkChange(idx, e.target.value)}
                  style={{ flex: 1 }}
                />
                {callLinks.length > 1 && (
                  <button
                    className="btn btn-secondary"
                    onClick={() => setCallLinks((prev) => prev.filter((_, i) => i !== idx))}
                    title="Удалить ссылку"
                    style={{ padding: '6px 10px' }}
                  >
                    ✕
                  </button>
                )}
              </div>
            ))}
            {callLinks.length < maxLinks && (
              <div>
                <button
                  className="btn btn-secondary"
                  onClick={() => setCallLinks((prev) => [...prev, ''])}
                  style={{ fontSize: 13 }}
                >
                  + Добавить запасную ссылку
                </button>
              </div>
            )}
          </div>

          <div style={{ marginTop: 24, paddingTop: 16, borderTop: '1px solid var(--border)' }}>
            <button
              className="btn btn-primary"
              disabled={actionPending}
              onClick={handleWizardEnable}
              style={{ padding: '10px 24px', fontSize: 14, fontWeight: 600 }}
            >
              {actionPending ? (installing ? '⏳ Скачиваю компонент…' : '⏳ Активация…') : 'Включить'}
            </button>
            {installing && (
              <div className="muted" style={{ marginTop: 10, fontSize: 13 }} data-testid="install-progress">
                ⏳ Скачиваю компонент VK-туннеля: {installProgressText(status?.install)}
              </div>
            )}
          </div>
        </section>
      </div>
    )
  }

  // ---------------------------------------------------------
  // B. Карточка мониторинга
  // ---------------------------------------------------------
  const phase = status?.phase || 'disabled'
  const isRunning = phase === 'connected'
  const waitingTunnel = Boolean(status?.enabled && !status?.tunnel_ready && phase !== 'not_installed')
  const showPasswordFix = phase === 'auth_failed' || phase === 'server_unreachable'
  const showLastError = Boolean(
    status?.enabled && status?.last_error && !['captcha_required', 'disabled', 'not_installed'].includes(phase)
  )

  const bannerStyle = (danger: boolean) => ({
    marginTop: 16,
    padding: '12px 16px',
    borderRadius: 8,
    background: danger ? 'rgba(239, 68, 68, 0.1)' : 'rgba(234, 179, 8, 0.1)',
    border: danger ? '1px solid rgba(239, 68, 68, 0.3)' : '1px solid rgba(234, 179, 8, 0.3)',
    fontSize: 13.5,
  })

  return (
    <div className="vkturn-container" style={{ display: 'flex', flexDirection: 'column', gap: 20 }}>
      <section
        className="card"
        style={{
          position: 'relative',
          overflow: 'hidden',
          padding: '24px 26px',
          background: isRunning
            ? 'linear-gradient(135deg, rgba(34, 197, 94, 0.08) 0%, rgba(15, 23, 42, 0.6) 100%)'
            : 'linear-gradient(135deg, rgba(239, 68, 68, 0.06) 0%, rgba(15, 23, 42, 0.6) 100%)',
          border: isRunning
            ? '1px solid rgba(34, 197, 94, 0.3)'
            : '1px solid rgba(239, 68, 68, 0.25)',
          borderRadius: 16,
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', flexWrap: 'wrap', gap: 16 }}>
          <div>
            <div style={{ display: 'flex', alignItems: 'center', gap: 12, marginBottom: 8 }}>
              <span style={{ fontSize: 32 }}>📞</span>
              <div>
                <h2 style={{ margin: 0, fontSize: 20, fontWeight: 700 }}>VK-туннель</h2>
                <div style={{ fontSize: 12, color: 'var(--muted)', marginTop: 2 }}>
                  Ядро csqtt {status?.core_version ? `· ${status.core_version}` : ''} {status?.pid ? `(PID: ${status.pid})` : ''}
                </div>
              </div>
              <div style={{ marginLeft: 8 }}>
                {renderPhaseBadge(phase, status?.connected_streams)}
              </div>
            </div>
            <div style={{ fontSize: 13, color: 'var(--muted)' }}>
              Звонки ВКонтакте → сервер csqtt: <b>{status?.peer || 'не указан'}</b>
              {isRunning && (
                <span data-testid="traffic-line">
                  {' '}• ↑ {formatTraffic(status?.bytes_up)} ↓ {formatTraffic(status?.bytes_down)}
                </span>
              )}
            </div>
          </div>

          <div style={{ display: 'flex', alignItems: 'center', gap: 14 }}>
            <label className="switch" title={status?.enabled ? 'Отключить туннель' : 'Включить туннель'}>
              <input
                type="checkbox"
                aria-label="Включить или отключить VK-туннель"
                checked={Boolean(status?.enabled)}
                disabled={actionPending}
                onChange={(e) => handleToggleEnabled(e.target.checked)}
              />
              <span className="slider" />
            </label>
          </div>
        </div>

        {installing && (
          <div style={bannerStyle(false)} data-testid="install-progress">
            ⏳ Скачиваю компонент VK-туннеля: {installProgressText(status?.install)}
          </div>
        )}

        {phase === 'not_installed' && !installing && (
          <div style={{ ...bannerStyle(false), display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 12 }}>
            <div>Компонент ядра VK-туннеля не установлен на роутере.</div>
            <button
              className="btn btn-primary"
              disabled={actionPending}
              onClick={handleInstallCore}
              style={{ fontSize: 13 }}
            >
              Установить компонент
            </button>
          </div>
        )}

        {waitingTunnel && isRunning && (
          <div style={bannerStyle(false)} data-testid="waiting-tunnel">
            ⏳ Соединение с сервером есть, подключаю туннель к роутеру — узел заработает в списке серверов автоматически.
          </div>
        )}

        {phase === 'captcha_required' && (
          <div style={{ ...bannerStyle(true), color: 'var(--danger-fg, #ef4444)' }}>
            ⚠️ ВКонтакте запросил капчу, и автоматически пройти её не удалось. Повторная попытка будет предпринята
            автоматически через 10 минут.
          </div>
        )}

        {showPasswordFix && (
          <div style={bannerStyle(true)} data-testid="password-fix">
            <div style={{ marginBottom: 8, fontWeight: 600, color: 'var(--danger-fg, #ef4444)' }}>
              {phase === 'auth_failed'
                ? status?.last_error || 'Сервер csqtt отклонил пароль подключения.'
                : 'Сервер на VPS не отвечает. Проверьте, что сервер csqtt запущен и открыт его UDP-порт. ' +
                  'При неверном пароле сервер тоже молчит — если пароль мог измениться, введите его заново.'}
            </div>
            <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
              <input
                type="password"
                className="input"
                autoComplete="new-password"
                placeholder="Новый пароль подключения"
                value={newPassword}
                onChange={(e) => setNewPassword(e.target.value)}
                style={{ flex: 1 }}
              />
              <button
                className="btn btn-primary"
                disabled={updatingPassword || !newPassword.trim()}
                onClick={handleUpdatePassword}
                style={{ fontSize: 13 }}
              >
                {updatingPassword ? 'Сохранение…' : 'Сохранить пароль'}
              </button>
            </div>
          </div>
        )}

        {phase === 'link_invalid' && (
          <div style={bannerStyle(true)}>
            <div style={{ marginBottom: 8, fontWeight: 600, color: 'var(--danger-fg, #ef4444)' }}>
              Ссылка на звонок больше не действует или звонок был завершён.
            </div>
            <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
              <input
                type="text"
                className="input"
                placeholder="Новая ссылка на звонок (https://vk.com/call/join/...)"
                value={newLinkInput}
                onChange={(e) => setNewLinkInput(e.target.value)}
                style={{ flex: 1 }}
              />
              <button
                className="btn btn-primary"
                disabled={updatingLink || !newLinkInput.trim()}
                onClick={handleUpdateCallLink}
                style={{ fontSize: 13 }}
              >
                {updatingLink ? 'Обновление…' : 'Обновить ссылку'}
              </button>
            </div>
          </div>
        )}

        {showLastError && !showPasswordFix && (
          <div className="muted" data-testid="last-error" style={{ marginTop: 12, fontSize: 13 }}>
            ⚠️ {status?.last_error}
          </div>
        )}

        <div style={{ display: 'flex', alignItems: 'center', gap: 12, marginTop: 20, flexWrap: 'wrap' }}>
          <button
            className="btn btn-secondary"
            disabled={testingPing || actionPending || !status?.enabled || !status?.tunnel_ready}
            onClick={handleTest}
            title={status?.tunnel_ready ? '' : 'Станет доступно, когда туннель подключится к серверу'}
            style={{ display: 'inline-flex', alignItems: 'center', gap: 6 }}
          >
            <span>{testingPing ? '⏳' : '⚡'}</span>
            <span>{testingPing ? 'Проверяю…' : 'Проверить'}</span>
          </button>

          <button
            className="btn btn-secondary"
            disabled={actionPending || !status?.enabled}
            onClick={handleRestart}
            style={{ display: 'inline-flex', alignItems: 'center', gap: 6 }}
          >
            <span>🔄</span>
            <span>Перезапустить</span>
          </button>

          {testLatency !== null && (
            <span className="badge badge-success" data-testid="latency-badge" style={{ fontSize: 13 }}>
              Задержка {testLatency} мс ✓
            </span>
          )}
        </div>
      </section>

      <section className="card" style={{ padding: '20px 24px' }}>
        <div
          role="button"
          tabIndex={0}
          onClick={() => {
            const next = !accordionOpen
            setAccordionOpen(next)
            if (next && logs.length === 0) {
              fetchLogs()
            }
          }}
          onKeyDown={(e) => {
            if (e.key === 'Enter' || e.key === ' ') {
              e.preventDefault()
              setAccordionOpen((prev) => !prev)
            }
          }}
          style={{
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            cursor: 'pointer',
            userSelect: 'none',
          }}
        >
          <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
            <span style={{ fontSize: 18 }}>📋</span>
            <span style={{ fontWeight: 600, fontSize: 15 }}>Настройки туннеля и подробности</span>
          </div>
          <span style={{ fontSize: 16, transform: accordionOpen ? 'rotate(180deg)' : 'none', transition: 'transform 0.2s' }}>
            ▼
          </span>
        </div>

        {accordionOpen && (
          <div style={{ marginTop: 20, paddingTop: 16, borderTop: '1px solid var(--border)' }}>
            <div style={{ marginBottom: 20, maxWidth: 640 }} data-testid="stream-controls">
              <label htmlFor="vkturn-stream-count" style={{ display: 'block', fontSize: 13, fontWeight: 600, marginBottom: 8 }}>
                Потоки звонков (Workers / Streams)
              </label>
              <div style={{ display: 'flex', gap: 10, alignItems: 'center' }}>
                <input
                  id="vkturn-stream-count"
                  type="number"
                  min={3}
                  max={100}
                  className="input"
                  data-testid="stream-count"
                  value={status?.target_streams || 20}
                  disabled={actionPending || !status?.enabled}
                  onChange={(e) => handleUpdateStreams(e.target.value)}
                  style={{ width: 140, fontWeight: 600 }}
                />
                <span className="muted" style={{ fontSize: 13 }}>
                  активно: {status?.connected_streams ?? 0} из {status?.target_streams ?? 0}
                </span>
              </div>
              <div className="muted" style={{ fontSize: 12, marginTop: 6, lineHeight: 1.4 }}>
                Каждый поток создаёт независимый шифрованный DTLS-канал к медиа-серверам звонков ВКонтакте.
                Для слабых роутеров (Keenetic) рекомендуется 10–20 потоков, для мощных ПК/серверов — 30–50.
              </div>
            </div>
            <div
              style={{
                display: 'grid',
                gridTemplateColumns: 'repeat(auto-fit, minmax(200px, 1fr))',
                gap: 16,
                marginBottom: 20,
              }}
            >
              <div className="stat-card" style={{ padding: 12, background: 'rgba(255,255,255,0.03)', borderRadius: 8 }}>
                <div className="muted" style={{ fontSize: 12 }}>Соединений (сейчас / потоков)</div>
                <div style={{ fontSize: 18, fontWeight: 700, marginTop: 4 }}>
                  {status?.connected_streams ?? 0} / {status?.target_streams ?? 0}
                </div>
              </div>
              <div className="stat-card" style={{ padding: 12, background: 'rgba(255,255,255,0.03)', borderRadius: 8 }}>
                <div className="muted" style={{ fontSize: 12 }}>Ссылки на звонки</div>
                <div style={{ fontSize: 18, fontWeight: 700, marginTop: 4 }}>
                  {status?.links_count ?? 0} из {maxLinks}
                </div>
              </div>
              <div className="stat-card" style={{ padding: 12, background: 'rgba(255,255,255,0.03)', borderRadius: 8 }}>
                <div className="muted" style={{ fontSize: 12 }}>Число перезапусков</div>
                <div style={{ fontSize: 18, fontWeight: 700, marginTop: 4 }}>{status?.restarts ?? 0}</div>
              </div>
              <div className="stat-card" style={{ padding: 12, background: 'rgba(255,255,255,0.03)', borderRadius: 8 }}>
                <div className="muted" style={{ fontSize: 12 }}>Адрес роутера в туннеле</div>
                <div style={{ fontSize: 16, fontWeight: 600, marginTop: 4, fontFamily: 'monospace' }} data-testid="tunnel-ip">
                  {status?.tunnel_ip ? `${status.tunnel_ip} · ${status.interface}` : 'выдаст сервер при подключении'}
                </div>
              </div>
            </div>

            <div style={{ marginBottom: 20 }}>
              <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 8 }}>Ссылки на звонки</div>
              <ul style={{ margin: '0 0 10px 0', paddingLeft: 18, fontSize: 13, fontFamily: 'monospace' }} data-testid="links-list">
                {(status?.links || []).map((l, i) => (
                  <li key={i}>{l}{i === 0 ? ' (основная)' : ''}</li>
                ))}
              </ul>
              <div style={{ display: 'flex', gap: 8, alignItems: 'center', maxWidth: 640 }}>
                <input
                  type="text"
                  className="input"
                  placeholder="Добавить ссылку как основную (https://vk.com/call/join/...)"
                  value={phase === 'link_invalid' ? '' : newLinkInput}
                  disabled={phase === 'link_invalid'}
                  onChange={(e) => setNewLinkInput(e.target.value)}
                  style={{ flex: 1 }}
                />
                <button
                  className="btn btn-secondary"
                  disabled={updatingLink || !newLinkInput.trim() || phase === 'link_invalid'}
                  onClick={handleUpdateCallLink}
                  style={{ fontSize: 13 }}
                >
                  Добавить
                </button>
              </div>
            </div>

            {!showPasswordFix && (
              <div style={{ marginBottom: 20 }} data-testid="password-change">
                <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 8 }}>Пароль подключения к серверу</div>
                <div style={{ display: 'flex', gap: 8, alignItems: 'center', maxWidth: 640 }}>
                  <input
                    type="password"
                    className="input"
                    autoComplete="new-password"
                    placeholder="Сменить пароль подключения"
                    value={newPassword}
                    onChange={(e) => setNewPassword(e.target.value)}
                    style={{ flex: 1 }}
                  />
                  <button
                    className="btn btn-secondary"
                    disabled={updatingPassword || !newPassword.trim()}
                    onClick={handleUpdatePassword}
                    style={{ fontSize: 13 }}
                  >
                    {updatingPassword ? 'Сохранение…' : 'Сменить пароль'}
                  </button>
                </div>
              </div>
            )}

            <div>
              <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 10 }}>
                <span style={{ fontSize: 13, fontWeight: 600 }}>Логи службы (Tail)</span>
                <button
                  className="btn btn-secondary"
                  disabled={loadingLogs}
                  onClick={fetchLogs}
                  style={{ fontSize: 12, padding: '4px 8px' }}
                >
                  {loadingLogs ? 'Обновление…' : '🔄 Обновить логи'}
                </button>
              </div>
              <pre
                data-testid="log-tail-pre"
                style={{
                  background: 'var(--code-bg, #090d16)',
                  color: 'var(--code-fg, #e2e8f0)',
                  padding: 12,
                  borderRadius: 8,
                  fontSize: 12,
                  fontFamily: 'Consolas, monospace',
                  maxHeight: 260,
                  overflowY: 'auto',
                  border: '1px solid var(--border)',
                  whiteSpace: 'pre-wrap',
                  wordBreak: 'break-all',
                }}
              >
                {logs.length > 0 ? logs.join('\n') : 'Логи пока пусты.'}
              </pre>
            </div>
          </div>
        )}
      </section>
    </div>
  )
}
