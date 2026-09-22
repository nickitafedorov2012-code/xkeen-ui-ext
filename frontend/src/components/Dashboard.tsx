import { useCallback, useEffect, useState } from 'react'
import { apiGet, apiPost, apiPut } from '../api'
import { pingClass, type FailoverEventInfo, type StatusInfo } from '../types'

interface Props {
  status: StatusInfo | null
  notify: (msg: string, isError?: boolean) => void
  refresh?: () => void
  onSwitchTab?: (tab: 'dashboard' | 'servers' | 'devices' | 'settings' | 'help') => void
}

interface GroupedEvent {
  time: string
  message: string
  switched: boolean
  count: number
  type: 'ok' | 'warn' | 'err' | 'switch'
}

function groupEvents(rawEvents: FailoverEventInfo[]): GroupedEvent[] {
  if (!rawEvents || rawEvents.length === 0) return []

  const classify = (e: FailoverEventInfo): 'ok' | 'warn' | 'err' | 'switch' => {
    if (e.switched) return 'switch'
    const m = e.message.toLowerCase()
    if (
      m.includes('ошибка') ||
      m.includes('недоступен') ||
      m.includes('таймаут') ||
      m.includes('failed') ||
      m.includes('timeout')
    )
      return 'err'
    if (m.includes('превышен') || m.includes('высокий') || m.includes('задержк') || m.includes('резерв'))
      return 'warn'
    return 'ok'
  }

  const normMsg = (msg: string) => {
    return msg.replace(/\(\d+\s*мс\)/i, '').trim()
  }

  const grouped: GroupedEvent[] = []
  for (const ev of rawEvents) {
    const t = classify(ev)
    const norm = normMsg(ev.message)
    const last = grouped[grouped.length - 1]
    if (last && normMsg(last.message) === norm && last.type === t) {
      last.count += 1
      last.time = ev.time
      last.message = ev.message
    } else {
      grouped.push({
        time: ev.time,
        message: ev.message,
        switched: ev.switched,
        count: 1,
        type: t,
      })
    }
  }
  return grouped
}

export default function Dashboard({ status, notify, refresh, onSwitchTab }: Props) {
  const [events, setEvents] = useState<FailoverEventInfo[]>([])
  const [checking, setChecking] = useState(false)
  const [togglingFailover, setTogglingFailover] = useState(false)
  // История пинга активного сервера (для sparkline): {значение, было ли измерение}
  const [pingHistory, setPingHistory] = useState<{ ms: number; ok: boolean }[]>([])

  useEffect(() => {
    const ping = status?.active_server?.ping_ms
    if (ping === undefined) return
    setPingHistory((prev) => {
      const next = [...prev, { ms: ping, ok: ping > 0 }].slice(-40)
      return next
    })
  }, [status?.active_server?.ping_ms, status?.active_server?.name])

  const loadEvents = useCallback(async () => {
    try {
      const data = await apiGet<{ events: FailoverEventInfo[] }>('failover/events')
      setEvents(data.events)
    } catch {
      /* ignore */
    }
  }, [])

  useEffect(() => {
    loadEvents()
    const t = setInterval(loadEvents, 10000)
    return () => clearInterval(t)
  }, [loadEvents])

  const runCheck = async () => {
    setChecking(true)
    try {
      const data = await apiPost<{ message: string }>('failover/check')
      notify(data.message)
      loadEvents()
      refresh?.()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка проверки', true)
    } finally {
      setChecking(false)
    }
  }

  const toggleFailover = async (enabled: boolean) => {
    setTogglingFailover(true)
    try {
      await apiPut('settings', { failover: { enabled } })
      notify(enabled ? 'Failover включён' : 'Failover выключен')
      refresh?.()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка переключения failover', true)
    } finally {
      setTogglingFailover(false)
    }
  }

  const f = status?.failover
  const groupedEvents = groupEvents(events)
  const mihomoVer = status?.mihomo_version || 'v1.19.29'

  return (
    <div>
      {/* 1.1 Компактная статусная полоска роутера и ядра вместо двух статичных карточек */}
      <div className="dash-info-strip">
        <div className="dash-info-group">
          <span className="dash-info-badge" title="Информация об интернет-центре Keenetic">
            <span>🌐 Роутер:</span>
            <b>{status?.router?.model || 'Keenetic'}</b>
            <span className="dash-info-sep">·</span>
            <span>KeeneticOS {status?.router?.version || '—'}</span>
            <span className="dash-info-sep">·</span>
            <span className="muted">RCI {status?.rci ? `${status.rci.host}:${status.rci.port}` : '127.0.0.1:79'}</span>
          </span>
        </div>

        <div className="dash-info-group">
          <span className="dash-info-badge" title="Mihomo core proxy engine">
            <span>⚙️ Ядро:</span>
            <b>Mihomo {mihomoVer}</b>
            <span className="dash-info-sep">·</span>
            <span className="muted">API {status?.mihomo ? `${status.mihomo.host}:${status.mihomo.port}` : '127.0.0.1:9090'}</span>
          </span>
        </div>
      </div>

      {/* Оперативные виджеты */}
      <div className="grid2">
        {/* КАРТОЧКА 1: Активный сервер с историей пинга */}
        <section className="card">
          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 12 }}>
            <h2 style={{ margin: 0 }}>🛰 Активный сервер</h2>
            {onSwitchTab && (
              <button
                type="button"
                className="btn sm ghost"
                onClick={() => onSwitchTab('servers')}
                title="Перейти к полному списку серверов"
              >
                Все серверы →
              </button>
            )}
          </div>

          {status?.active_server ? (
            <div>
              <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap', marginBottom: 8 }}>
                {status.active_server.provider && (
                  <span className="tag-provider" style={{ cursor: 'default' }}>
                    📦 {status.active_server.provider_name || status.active_server.provider}
                  </span>
                )}
                <b style={{ fontSize: 15, color: '#f8fafc' }}>{status.active_server.name}</b>
                <span className={'ping ' + pingClass(status.active_server.ping_ms)}>
                  {status.active_server.ping_ms > 0 ? `${status.active_server.ping_ms} мс` : '—'}
                </span>
              </div>
              {pingHistory.length >= 2 && <PingSparkline data={pingHistory} />}
              {pingHistory.length >= 2 && (
                <p className="muted small" style={{ marginTop: 6, marginBottom: 0 }}>
                  Стабильность за последние {pingHistory.length} опросов (всплески = задержка)
                </p>
              )}
            </div>
          ) : (
            <p className="muted">Сервер не выбран или Mihomo не запущен.</p>
          )}
        </section>

        {/* КАРТОЧКА 2: Failover статус и управление */}
        <section className="card">
          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 12 }}>
            <h2 style={{ margin: 0 }}>⚡ Failover контроль</h2>
            <span className={`badge ${f?.enabled ? 'badge-online' : ''}`} style={{ color: f?.enabled ? '#22c55e' : 'var(--muted)' }}>
              {f?.enabled ? '🟢 включён' : '⚪ выключен'}
            </span>
          </div>
          <ul className="kv">
            <li>
              <span>Автоматический мониторинг</span>
              <div style={{ display: 'inline-flex', alignItems: 'center', gap: 10 }}>
                <label className="switch" title={f?.enabled ? 'Выключить failover' : 'Включить failover'}>
                  <input
                    type="checkbox"
                    checked={f?.enabled ?? false}
                    disabled={togglingFailover}
                    onChange={(e) => toggleFailover(e.target.checked)}
                  />
                  <span className="slider" />
                </label>
                <b style={{ color: f?.enabled ? '#22c55e' : 'var(--muted)', fontSize: 13 }}>
                  {togglingFailover ? 'Сохранение…' : f?.enabled ? 'Включён' : 'Выключен'}
                </b>
              </div>
            </li>
            <li><span>Порог пинга</span><b>{f ? `${f.ping_threshold_ms} мс` : '—'}</b></li>
            <li>
              <span>Приоритетный</span>
              <b>
                {f?.priority_server || 'не задан'}
                {f?.priority_chain &&
                f.priority_chain.length > 0 &&
                status?.active_server?.id &&
                f.priority_chain.indexOf(status.active_server.id) > 0 ? (
                  <span className="muted small" style={{ marginLeft: 6, fontWeight: 'normal' }}>
                    (ожидает восстановления)
                  </span>
                ) : null}
              </b>
            </li>
            {(f?.priority_chain?.length ?? 0) > 0 &&
              status?.active_server?.id &&
              f!.priority_chain!.indexOf(status.active_server.id) >= 0 && (
                <li>
                  <span>Позиция в цепочке</span>
                  <b>
                    {f!.priority_chain!.indexOf(status.active_server.id) === 0 ? (
                      <span style={{ color: '#22c55e' }}>ОСН (основной)</span>
                    ) : (
                      <span style={{ color: '#eab308' }}>
                        РЕЗ{f!.priority_chain!.indexOf(status.active_server.id)} (резервный)
                      </span>
                    )}
                  </b>
                </li>
              )}
            {(f?.priority_chain?.length ?? 0) > 1 && (
              <li><span>Резервы</span><b>{f!.priority_chain!.slice(1).length} сервер(ов)</b></li>
            )}
            <li><span>Интервал проверки</span><b>{f ? `${f.interval_secs} с` : '—'}</b></li>
          </ul>
          <div style={{ display: 'flex', gap: 8, marginTop: 12 }}>
            <button className="btn primary" onClick={runCheck} disabled={checking} style={{ flex: 1 }}>
              {checking ? 'Проверка…' : '🔍 Проверить сейчас'}
            </button>
            {onSwitchTab && (
              <button
                type="button"
                className="btn ghost"
                onClick={() => onSwitchTab('settings')}
                title="Настроить порог пинга и интервал"
              >
                ⚙️
              </button>
            )}
          </div>
        </section>
      </div>

      {/* КАРТОЧКА 3: Блок событий с группировкой и цветовой разметкой */}
      <section className="card" style={{ marginTop: 14 }}>
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 10 }}>
          <h2 style={{ margin: 0 }}>📋 Журнал событий Failover</h2>
          {onSwitchTab && (
            <button
              type="button"
              className="btn sm ghost"
              onClick={() => onSwitchTab('settings')}
              title="Открыть системный лог и настройки"
            >
              Системный журнал →
            </button>
          )}
        </div>

        {groupedEvents.length === 0 ? (
          <p className="muted">Событий пока нет. Запустите проверку для получения данных.</p>
        ) : (
          <div className="events-container">
            {groupedEvents.map((e, i) => (
              <div key={i} className={`event-entry ev-${e.type}`}>
                <span className="ev-type-icon">
                  {e.type === 'switch' ? '🔵' : e.type === 'err' ? '🔴' : e.type === 'warn' ? '🟡' : '🟢'}
                </span>
                <span className="ev-time-pill">{e.time}</span>
                <span className="ev-msg-text" title={e.message}>{e.message}</span>
                {e.count > 1 && (
                  <span className="ev-repeat-badge" title={`Повторено ${e.count} раз(а) подряд`}>
                    ×{e.count}
                  </span>
                )}
              </div>
            ))}
          </div>
        )}
      </section>
    </div>
  )
}

/// Мини-график пинга (SVG sparkline): последняя точка справа, провалы — красные.
function PingSparkline({ data }: { data: { ms: number; ok: boolean }[] }) {
  const W = 280
  const H = 46
  const okVals = data.filter((d) => d.ok).map((d) => d.ms)
  const max = Math.max(100, ...okVals) * 1.15
  const step = data.length > 1 ? W / (data.length - 1) : W
  const y = (d: { ms: number; ok: boolean }) =>
    d.ok ? H - 4 - (d.ms / max) * (H - 10) : H - 2
  const pts = data.map((d, i) => `${(i * step).toFixed(1)},${y(d).toFixed(1)}`).join(' ')
  const last = data[data.length - 1]
  return (
    <svg width="100%" height={H} viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" style={{ display: 'block', marginTop: 8 }}>
      <polyline points={pts} fill="none" stroke="var(--accent)" strokeWidth="1.6" />
      {data.map((d, i) =>
        d.ok ? null : (
          <circle key={i} cx={i * step} cy={y(d)} r="2.6" fill="var(--red, #e5484d)" />
        ),
      )}
      <circle cx={(data.length - 1) * step} cy={y(last)} r="2.6" fill="var(--accent)" />
    </svg>
  )
}
