import { useCallback, useEffect, useState } from 'react'
import { apiGet, apiPost } from '../api'
import { pingClass, type FailoverEventInfo, type StatusInfo } from '../types'

interface Props {
  status: StatusInfo | null
  notify: (msg: string, isError?: boolean) => void
}

export default function Dashboard({ status, notify }: Props) {
  const [events, setEvents] = useState<FailoverEventInfo[]>([])
  const [checking, setChecking] = useState(false)

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
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка проверки', true)
    } finally {
      setChecking(false)
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
      </section>

      <section className="card">
        <h2>Failover</h2>
        <ul className="kv">
          <li><span>Состояние</span><b>{f?.enabled ? '🟢 включён' : '⚪ выключен'}</b></li>
          <li><span>Порог пинга</span><b>{f ? `${f.ping_threshold_ms} мс` : '—'}</b></li>
          <li><span>Приоритетный</span><b>{f?.priority_server || 'не задан'}</b></li>
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
