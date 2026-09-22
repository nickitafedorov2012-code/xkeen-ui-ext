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

      {/* Инструкции по настройке клиентов */}
      <section className="card ag-instructions-card">
        <div className="ag-card-header">
          <h3>💻 Настройка клиентских устройств и IDE</h3>
          <span className="ag-card-hint">Два способа использования разблокировки в вашей сети</span>
        </div>

        <div className="ag-instructions-grid">
          <div className="ag-instruction-box">
            <h4>Способ 1: Прозрачный режим (по умолчанию)</h4>
            <p>
              Роутер Keenetic через собственный DNS-сервер <code>ndnproxy</code> автоматически перехватывает запросы
              к целевым доменам и подставляет проверенные IP-адреса.
            </p>
            <div className="ag-badge-row">
              <span className="ag-pill success">✓ Настройка на клиентах НЕ требуется</span>
              <span className="ag-pill success">✓ Работает для всех ПК, ноутбуков и телефонов в сети</span>
            </div>
          </div>

          <div className="ag-instruction-box">
            <h4>Способ 2: Через переменную окружения AG_LS_PROXY</h4>
            <p>
              Если на вашем ПК установлен бинарный патч Antigravity с переменной <code>AG_LS_PROXY</code>:
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
        </div>

        <div className="ag-note-banner">
          ℹ️ <strong>Обратите внимание:</strong> Бинарный патч самого Antigravity (замена <code>ineligible</code> →{' '}
          <code>inexigible</code>) выполняется на клиентском компьютере в исполняемом файле IDE / расширения. Роутер
          обеспечивает надёжную сетевую доставку запросов в обход гео-блокировок.
        </div>
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
