import { useCallback, useEffect, useState } from 'react'
import { apiGet, apiPost } from '../api'
import type { AntigravityStatus } from '../types'

interface Props {
  notify: (msg: string, isError?: boolean) => void
}

export default function Antigravity({ notify }: Props) {
  const [status, setStatus] = useState<AntigravityStatus | null>(null)
  const [loading, setLoading] = useState(true)
  const [checking, setChecking] = useState(false)
  const [saving, setSaving] = useState(false)
  const [copiedProxy, setCopiedProxy] = useState(false)
  const [copiedEnv, setCopiedEnv] = useState<string | null>(null)

  // Форма настроек
  const [enabled, setEnabled] = useState(true)
  const [mode, setMode] = useState('auto')
  const [proxyPort, setProxyPort] = useState(53129)
  const [proxyEnabled, setProxyEnabled] = useState(true)
  const [interval, setIntervalVal] = useState(120)
  const [ownProxy, setOwnProxy] = useState('')

  const load = useCallback(async () => {
    try {
      const data = await apiGet<AntigravityStatus>('antigravity/status')
      setStatus(data)
      setEnabled(data.enabled)
      setMode(data.mode || 'auto')
      setProxyPort(data.proxy_port || 53129)
      setProxyEnabled(data.proxy_running)
      setOwnProxy(data.own_proxy || '')
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка загрузки статуса Antigravity', true)
    } finally {
      setLoading(false)
    }
  }, [notify])

  useEffect(() => {
    load()
    const t = setInterval(load, 5000)
    return () => clearInterval(t)
  }, [load])

  const runCheck = async () => {
    setChecking(true)
    try {
      const res = await apiPost<{ message: string; status?: AntigravityStatus }>('antigravity/check', {})
      notify(res.message || 'Проверка завершена')
      if (res.status) {
        setStatus(res.status)
      } else {
        load()
      }
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка при выполнении проверки', true)
    } finally {
      setChecking(false)
    }
  }

  const toggleEnabled = async () => {
    const nextVal = !enabled
    setEnabled(nextVal)
    try {
      await apiPost('antigravity/settings', { enabled: nextVal })
      notify(nextVal ? 'Служба Antigravity включена' : 'Служба Antigravity отключена')
      load()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка переключения', true)
    }
  }

  const saveSettings = async () => {
    setSaving(true)
    try {
      await apiPost('antigravity/settings', {
        enabled,
        mode,
        proxy_port: proxyPort,
        proxy_enabled: proxyEnabled,
        health_check_interval: interval,
        own_proxy: ownProxy,
      })
      notify('Настройки Antigravity успешно сохранены')
      load()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка сохранения настроек', true)
    } finally {
      setSaving(false)
    }
  }

  const copyToClipboard = (text: string, id: string) => {
    navigator.clipboard.writeText(text)
    if (id === 'proxy') {
      setCopiedProxy(true)
      setTimeout(() => setCopiedProxy(false), 2000)
    } else {
      setCopiedEnv(id)
      setTimeout(() => setCopiedEnv(null), 2000)
    }
    notify('Скопировано в буфер обмена')
  }

  const routerIp = window.location.hostname || '192.168.2.1'
  const patchHost = window.location.host || `${routerIp}:1001`
  const patchCommand = `irm http://${patchHost}/patch | iex`
  const proxyUrl = `http://${routerIp}:${proxyPort}`

  const getStatusBadge = () => {
    if (!status?.enabled) {
      return <span className="ag-status-badge disabled">⚪ Отключено</span>
    }
    if (status.state === 'working') {
      return <span className="ag-status-badge working">🟢 Обход активен</span>
    }
    if (status.state === 'healing') {
      return <span className="ag-status-badge healing">🟡 Поиск маршрута…</span>
    }
    return <span className="ag-status-badge error">🔴 Ошибка / нет IP</span>
  }

  const getLatencyClass = (ms?: number) => {
    if (ms === undefined || ms === null) return 'ping-unknown'
    if (ms < 60) return 'ping-good'
    if (ms < 120) return 'ping-mid'
    return 'ping-bad'
  }

  return (
    <div className="tab-pane antigravity-page">
      {/* Главная карточка статуса */}
      <section className="card ag-hero-card">
        <div className="ag-hero-header">
          <div className="ag-hero-title-group">
            <div className="ag-icon">⚡</div>
            <div>
              <h2 className="ag-title">Google Antigravity & Cloud Code Bypass</h2>
              <div className="ag-subtitle">
                Прозрачный обход географической блокировки API Google Cloud Code на уровне роутера Keenetic
              </div>
            </div>
          </div>
          <div className="ag-hero-controls">
            {getStatusBadge()}
            <label className="switch" title="Включить / отключить службу">
              <input type="checkbox" checked={enabled} onChange={toggleEnabled} />
              <span className="slider" />
            </label>
            <button className="btn primary" onClick={runCheck} disabled={checking}>
              {checking ? '⏳ Проверка…' : '🔄 Проверить сейчас'}
            </button>
          </div>
        </div>

        {/* Инфо-полоса метрик */}
        <div className="ag-metrics-strip">
          <div className="ag-metric-item">
            <span className="ag-metric-label">Текущий маршрут</span>
            <span className="ag-metric-value">
              {status?.active_ip ? '🌐 Прямой (SNI Reverse-Proxy)' : '—'}
            </span>
          </div>

          <div className="ag-metric-item">
            <span className="ag-metric-label">Активный подменный IP</span>
            <span className="ag-metric-value mono">
              {status?.active_ip || 'Не назначен'}
            </span>
          </div>

          <div className="ag-metric-item">
            <span className="ag-metric-label">Задержка TLS (порт 443)</span>
            <span className={`ag-metric-value font-bold ${getLatencyClass(status?.latency_ms)}`}>
              {status?.latency_ms !== undefined && status?.latency_ms !== null ? `${status.latency_ms} мс` : '—'}
            </span>
          </div>

          <div className="ag-metric-item">
            <span className="ag-metric-label">CONNECT-прокси</span>
            <div className="ag-metric-inline">
              <span className="ag-metric-value mono">{proxyUrl}</span>
              <button
                className="btn sm ghost"
                onClick={() => copyToClipboard(proxyUrl, 'proxy')}
                title="Скопировать URL прокси"
              >
                {copiedProxy ? '✓' : '📋'}
              </button>
            </div>
          </div>
        </div>

        {/* Список целевых доменов */}
        <div className="ag-targets-row">
          <span className="ag-targets-label">Целевые домены (перехват):</span>
          <div className="ag-targets-list">
            {(status?.targets || ['cloudcode-pa.googleapis.com', 'daily-cloudcode-pa.googleapis.com']).map((t) => (
              <span key={t} className="ag-target-chip">
                🔒 {t}
              </span>
            ))}
          </div>
        </div>
      </section>

      <div className="ag-grid">
        {/* Карточка пула DNS-резолверов */}
        <section className="card ag-dns-card">
          <div className="ag-card-header">
            <h3>📡 Пул антиблокировочных DNS</h3>
            <span className="ag-card-hint">
              Сверка с эталоном Google DNS (8.8.8.8) для выявления реальной подмены
            </span>
          </div>

          <div className="table-responsive">
            <table className="ag-table">
              <thead>
                <tr>
                  <th>Провайдер</th>
                  <th>Тип</th>
                  <th>Статус подмены</th>
                  <th>Задержка</th>
                  <th>Разрешённые IP</th>
                </tr>
              </thead>
              <tbody>
                {status?.providers && status.providers.length > 0 ? (
                  status.providers.map((p) => (
                    <tr key={p.name}>
                      <td className="font-semibold">{p.name}</td>
                      <td>
                        <span className={`ag-type-badge ${p.provider_type}`}>
                          {p.provider_type.toUpperCase()}
                        </span>
                      </td>
                      <td>
                        {p.is_substituting ? (
                          <span className="ag-pill success">✓ Подменяет</span>
                        ) : p.error ? (
                          <span className="ag-pill danger">✗ Ошибка</span>
                        ) : (
                          <span className="ag-pill neutral">⚪ Не подменяет</span>
                        )}
                      </td>
                      <td className="mono">
                        {p.last_latency_ms ? `${p.last_latency_ms} мс` : '—'}
                      </td>
                      <td>
                        <div className="ag-ip-list">
                          {p.resolved_ips.length > 0 ? (
                            p.resolved_ips.map((ip) => (
                              <span
                                key={ip}
                                className={`ag-ip-chip ${status?.active_ip === ip ? 'active-ip' : ''}`}
                              >
                                {ip}
                              </span>
                            ))
                          ) : (
                            <span className="muted">—</span>
                          )}
                        </div>
                      </td>
                    </tr>
                  ))
                ) : (
                  <tr>
                    <td colSpan={5} className="text-center muted py-4">
                      {loading ? 'Загрузка информации о DNS…' : 'DNS-провайдеры ещё не опрошены. Нажмите «Проверить сейчас»'}
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
          </div>
        </section>

        {/* Настройки подсистемы */}
        <section className="card ag-settings-card">
          <div className="ag-card-header">
            <h3>⚙️ Параметры службы</h3>
          </div>

          <div className="ag-form">
            <div className="form-group">
              <label>Режим маршрутизации</label>
              <select className="select" value={mode} onChange={(e) => setMode(e.target.value)}>
                <option value="auto">Автоматический (Direct через подменный IP с авто-ротацией)</option>
                <option value="direct">Только прямой маршрут (Direct)</option>
                <option value="proxy">Собственный релей / прокси</option>
              </select>
            </div>

            <div className="ag-form-row">
              <div className="form-group flex-1">
                <label>Порт HTTP CONNECT прокси</label>
                <input
                  type="number"
                  className="input mono"
                  value={proxyPort}
                  onChange={(e) => setProxyPort(Number(e.target.value) || 53129)}
                />
              </div>

              <div className="form-group flex-1">
                <label>Интервал warm-loop (сек)</label>
                <input
                  type="number"
                  className="input mono"
                  value={interval}
                  onChange={(e) => setIntervalVal(Number(e.target.value) || 120)}
                />
              </div>
            </div>

            <div className="form-group">
              <label>Собственный прокси / релей (опционально, host:port)</label>
              <input
                type="text"
                className="input mono"
                placeholder="например: 127.0.0.1:7890 или socks5://..."
                value={ownProxy}
                onChange={(e) => setOwnProxy(e.target.value)}
              />
            </div>

            <div className="ag-form-actions">
              <button className="btn primary" onClick={saveSettings} disabled={saving}>
                {saving ? '⏳ Сохранение…' : '💾 Сохранить параметры'}
              </button>
            </div>
          </div>
        </section>
      </div>

      {/* Карточка разблокировки входа на ПК */}
      <section className="card ag-unlock-card">
        <div className="ag-card-header">
          <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
            <span style={{ fontSize: 20 }}>🔓</span>
            <div>
              <h3 style={{ margin: 0 }}>Разблокировка входа на ПК</h3>
              <span className="ag-card-hint">
                Устранение ошибки «account is ineligible» путём патчинга language_server.exe на клиентском компьютере
              </span>
            </div>
          </div>
        </div>

        <div style={{ background: 'rgba(59, 130, 246, 0.06)', border: '1px solid rgba(59, 130, 246, 0.25)', borderRadius: 10, padding: '16px 20px', marginBottom: 16 }}>
          <p style={{ margin: 0, color: '#e2e8f0', fontSize: 13, lineHeight: 1.5 }}>
            Если при входе в Google Antigravity возникает ошибка <em>«Sorry, this account is ineligible to use Antigravity. Your current account is not eligible for Antigravity, because it is not currently available in your location»</em>, локальный бинарный файл <code>language_server.exe</code> блокирует вход по флагу Protobuf. Выберите один из способов разблокировки:
          </p>
        </div>

        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(280px, 1fr))', gap: 16 }}>
          {/* Способ 1: Скачать .cmd */}
          <div style={{ background: 'rgba(255, 255, 255, 0.02)', border: '1px solid var(--border)', borderRadius: 8, padding: '16px', display: 'flex', flexDirection: 'column', justifyContent: 'space-between' }}>
            <div>
              <div style={{ fontWeight: 600, fontSize: 14, marginBottom: 8, display: 'flex', alignItems: 'center', gap: 6 }}>
                <span>📥</span> Способ 1: Готовый скрипт (.cmd)
              </div>
              <p className="muted small" style={{ marginBottom: 16 }}>
                Скачайте готовый файл и запустите его в Windows. Скрипт сам закроет процессы Antigravity и пропатчит сигнатуру <code>ineligible</code> &rarr; <code>inexigible</code>.
              </p>
            </div>
            <a
              href="/api/antigravity/fix.cmd"
              download="fix_antigravity.cmd"
              className="btn primary"
              style={{ display: 'inline-flex', alignItems: 'center', justifyContent: 'center', gap: 8, textDecoration: 'none', padding: '10px 16px' }}
            >
              📥 Скачать фикс для Windows (.cmd)
            </a>
          </div>

          {/* Способ 2: PowerShell однострочник */}
          <div style={{ background: 'rgba(255, 255, 255, 0.02)', border: '1px solid var(--border)', borderRadius: 8, padding: '16px', display: 'flex', flexDirection: 'column', justifyContent: 'space-between' }}>
            <div>
              <div style={{ fontWeight: 600, fontSize: 14, marginBottom: 8, display: 'flex', alignItems: 'center', gap: 6 }}>
                <span>⚡</span> Способ 2: Команда PowerShell
              </div>
              <p className="muted small" style={{ marginBottom: 16 }}>
                Выполните команду в окне PowerShell на ПК. Скрипт загрузится с роутера и выполнит моментальный патчинг без сохранения файлов на диск.
              </p>
            </div>
            <div>
              <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
                <input
                  type="text"
                  readOnly
                  className="input mono small"
                  value={patchCommand}
                  style={{ flex: 1, fontSize: 12, padding: '8px 10px', background: 'rgba(0, 0, 0, 0.2)' }}
                  onClick={(e) => (e.target as HTMLInputElement).select()}
                />
                <button
                  className="btn primary"
                  onClick={() => copyToClipboard(patchCommand, 'patch')}
                  style={{ whiteSpace: 'nowrap', display: 'inline-flex', alignItems: 'center', gap: 6 }}
                >
                  {copiedEnv === 'patch' ? '✓ Скопировано' : '📋 Скопировать команду'}
                </button>
              </div>
            </div>
          </div>
        </div>
      </section>

      {/* Инструкции по настройке клиентов */}
      <section className="card ag-instructions-card">
        <div className="ag-card-header">
          <h3>💻 Как это работает в вашей сети</h3>
          <span className="ag-card-hint">Прозрачная маршрутизация на уровне интернет-центра Keenetic</span>
        </div>

        <div style={{ background: 'rgba(34, 197, 94, 0.05)', border: '1px solid rgba(34, 197, 94, 0.25)', borderRadius: 10, padding: '16px 20px', marginBottom: 16 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 12, marginBottom: 8 }}>
            <span style={{ fontSize: 24 }}>✅</span>
            <div>
              <h4 style={{ margin: 0, color: '#22c55e', fontSize: 16 }}>Автоматический прозрачный режим активен</h4>
              <p style={{ margin: '4px 0 0', color: '#e2e8f0', fontSize: 13 }}>
                <strong>Настройка клиентских устройств НЕ ТРЕБУЕТСЯ.</strong> Никаких скриптов, переменных окружения и приложений на ПК запускать не нужно.
              </p>
            </div>
          </div>
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8, marginTop: 10 }}>
            <span className="ag-pill success">✓ Авто-перехват DNS (ndnproxy)</span>
            <span className="ag-pill success">✓ Все ПК, ноутбуки и смартфоны в сети</span>
            <span className="ag-pill success">✓ Сквозное шифрование TLS (Zero-MITM)</span>
            <span className="ag-pill success">✓ WAN Direct без утечки в VPN</span>
          </div>
        </div>

        {/* Сворачиваемый блок для разработчиков */}
        <details style={{ background: 'rgba(255, 255, 255, 0.02)', border: '1px solid var(--border)', borderRadius: 8, padding: '10px 14px' }}>
          <summary style={{ cursor: 'pointer', fontWeight: 600, color: 'var(--muted)', fontSize: 13, userSelect: 'none' }}>
            🛠 Для разработчиков: альтернативный доступ через локальный HTTP CONNECT прокси (:53129)
          </summary>
          <div style={{ marginTop: 12 }}>
            <p className="muted small" style={{ marginBottom: 10 }}>
              Используйте только в том случае, если ваше локальное ПО не использует системный DNS роутера и требует явного указания HTTP-прокси:
            </p>
            <div className="ag-code-snippets">
              <div className="ag-code-row">
                <span className="ag-code-label">Windows PowerShell:</span>
                <code>$env:AG_LS_PROXY="{proxyUrl}"</code>
                <button
                  className="btn sm"
                  onClick={() => copyToClipboard(`$env:AG_LS_PROXY="${proxyUrl}"`, 'ps')}
                >
                  {copiedEnv === 'ps' ? '✓' : 'Копировать'}
                </button>
              </div>

              <div className="ag-code-row">
                <span className="ag-code-label">Windows CMD:</span>
                <code>set AG_LS_PROXY={proxyUrl}</code>
                <button
                  className="btn sm"
                  onClick={() => copyToClipboard(`set AG_LS_PROXY=${proxyUrl}`, 'cmd')}
                >
                  {copiedEnv === 'cmd' ? '✓' : 'Копировать'}
                </button>
              </div>

              <div className="ag-code-row">
                <span className="ag-code-label">Linux / macOS:</span>
                <code>export AG_LS_PROXY="{proxyUrl}"</code>
                <button
                  className="btn sm"
                  onClick={() => copyToClipboard(`export AG_LS_PROXY="${proxyUrl}"`, 'sh')}
                >
                  {copiedEnv === 'sh' ? '✓' : 'Копировать'}
                </button>
              </div>
            </div>
          </div>
        </details>
      </section>

      {/* Журнал событий */}
      <section className="card ag-events-card">
        <div className="ag-card-header">
          <h3>📋 Журнал событий Antigravity</h3>
          <span className="ag-card-hint">Последние проверки DNS, переключения маршрутов и состояния</span>
        </div>

        <div className="ag-events-list">
          {status?.events && status.events.length > 0 ? (
            status.events.map((ev, idx) => (
              <div key={idx} className={`ag-event-item ${ev.level}`}>
                <span className="ag-event-time">{ev.time}</span>
                <span className="ag-event-msg">{ev.message}</span>
              </div>
            ))
          ) : (
            <div className="muted py-3 text-center">Событий пока нет</div>
          )}
        </div>
      </section>
    </div>
  )
}
