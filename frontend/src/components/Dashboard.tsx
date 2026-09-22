import { useCallback, useEffect, useState } from 'react'
import { apiGet, apiPost, apiPut } from '../api'
import { pingClass, type FailoverEventInfo, type StatusInfo } from '../types'

interface Props {
  status: StatusInfo | null
  notify: (msg: string, isError?: boolean) => void
  refresh?: () => void
}

export default function Dashboard({ status, notify, refresh }: Props) {
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
  return (
    <div className="grid2">
      <section className="card">
        <h2>Роутер</h2>
        {status?.router ? (
          <ul className="kv">
            <li><span>Модель</span><b>{status.router.model || '—'}</b></li>
            <li><span>KeeneticOS</span><b>{status.router.version || '—'}</b></li>
            <li><span>RCI</span><b>{status.rci.host}:{status.rci.port}</b></li>
          </ul>
        ) : (
          <p className="muted">Роутер недоступен — проверьте настройки RCI.</p>
        )}
      </section>

      <section className="card">
        <h2>Mihomo</h2>
        <ul className="kv">
          <li><span>API</span><b>{status ? `${status.mihomo.host}:${status.mihomo.port}` : '—'}</b></li>
          <li>
            <span>Активный сервер</span>
            {status?.active_server ? (
              <b>
                {status.active_server.name}{' '}
                <span className={'ping ' + pingClass(status.active_server.ping_ms)}>
                  {status.active_server.ping_ms > 0 ? `${status.active_server.ping_ms} мс` : '—'}
                </span>
              </b>
            ) : (
              <b className="muted">—</b>
            )}
          </li>
        </ul>
        {pingHistory.length >= 2 && <PingSparkline data={pingHistory} />}
        {pingHistory.length >= 2 && (
          <p className="muted small">Стабильность за последние {pingHistory.length} опросов (выше = хуже пинг)</p>
        )}
      </section>

      <section className="card">
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 12 }}>
          <h2 style={{ margin: 0 }}>Failover</h2>
          <span className={`badge ${f?.enabled ? 'badge-online' : ''}`}>
            {f?.enabled ? '🟢 включён' : '⚪ выключен'}
          </span>
        </div>
        <ul className="kv">
          <li>
            <span>Автоматический failover</span>
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
              <b style={{ color: f?.enabled ? 'var(--green)' : 'var(--muted)', fontSize: 13 }}>
                {togglingFailover ? 'Сохранение…' : f?.enabled ? 'Включён' : 'Выключен'}
              </b>
            </div>
          </li>
          <li><span>Порог пинга</span><b>{f ? `${f.ping_threshold_ms} мс` : '—'}</b></li>
          <li>
            <span>Приоритетный</span>
            <b>
              {f?.priority_server || 'не задан'}
              {f?.priority_chain && f.priority_chain.length > 0 && status?.active_server?.id && f.priority_chain.indexOf(status.active_server.id) > 0 ? (
                <span className="muted small" style={{ marginLeft: 6, fontWeight: 'normal' }}>(ожидает восстановления)</span>
              ) : null}
            </b>
          </li>
          {(f?.priority_chain?.length ?? 0) > 0 && status?.active_server?.id && f!.priority_chain!.indexOf(status.active_server.id) >= 0 && (
            <li>
              <span>Позиция в цепочке</span>
              <b>
                {f!.priority_chain!.indexOf(status.active_server.id) === 0 ? (
                  <span style={{ color: 'var(--green)' }}>ОСН (основной)</span>
                ) : (
                  <span style={{ color: 'var(--yellow)' }}>РЕЗ{f!.priority_chain!.indexOf(status.active_server.id)} (резервный)</span>
                )}
              </b>
            </li>
          )}
          {(f?.priority_chain?.length ?? 0) > 1 && (
            <li><span>Резервы</span><b>{f!.priority_chain!.slice(1).length} сервер(ов)</b></li>
          )}
          <li><span>Интервал</span><b>{f ? `${f.interval_secs} с` : '—'}</b></li>
        </ul>
        <button className="btn primary" onClick={runCheck} disabled={checking}>
          {checking ? 'Проверка…' : '🔍 Проверить сейчас'}
        </button>
      </section>

      <section className="card">
        <h2>События</h2>
        {events.length === 0 ? (
          <p className="muted">Пока пусто.</p>
        ) : (
          <ul className="events">
            {events.map((e, i) => (
              <li key={i} className={e.switched ? 'ev-switch' : ''}>
                <span className="ev-time">{e.time}</span> {e.message}
              </li>
            ))}
          </ul>
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
