import { useCallback, useEffect, useMemo, useState } from 'react'
import { apiGet, apiPost } from '../api'
import { pingClass, type ServerInfo } from '../types'

interface Props {
  notify: (msg: string, isError?: boolean) => void
}

const PAGE = 15

export default function Servers({ notify }: Props) {
  const [servers, setServers] = useState<ServerInfo[]>([])
  const [filter, setFilter] = useState('')
  const [limit, setLimit] = useState(PAGE)
  const [loading, setLoading] = useState(true)
  const [pinging, setPinging] = useState(false)

  const load = useCallback(async () => {
    try {
      const data = await apiGet<{ servers: ServerInfo[] }>('servers')
      setServers(data.servers)
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка загрузки серверов', true)
    } finally {
      setLoading(false)
    }
  }, [notify])

  useEffect(() => {
    load()
  }, [load])

  const filtered = useMemo(() => {
    const q = filter.trim().toLowerCase()
    return q ? servers.filter((s) => s.name.toLowerCase().includes(q)) : servers
  }, [servers, filter])

  const pingAll = async () => {
    setPinging(true)
    try {
      const data = await apiPost<{ pings: Record<string, number> }>('servers/ping', {})
      setServers((prev) => prev.map((s) => ({ ...s, ping_ms: data.pings[s.id] ?? s.ping_ms })))
      notify('Пинг завершён')
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка пинга', true)
    } finally {
      setPinging(false)
    }
  }

  const activate = async (s: ServerInfo) => {
    try {
      const data = await apiPost<{ message: string }>('servers/switch', { server_id: s.id })
      notify(data.message)
      load()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка переключения', true)
    }
  }

  const setPriority = async (s: ServerInfo) => {
    try {
      const data = await apiPost<{ message: string }>('settings/priority', { server_id: s.is_priority ? '' : s.id })
      notify(data.message)
      load()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка', true)
    }
  }

  return (
    <section className="card">
      <div className="toolbar">
        <input
          className="input"
          placeholder="Поиск сервера…"
          value={filter}
          onChange={(e) => {
            setFilter(e.target.value)
            setLimit(PAGE)
          }}
        />
        <button className="btn" onClick={pingAll} disabled={pinging}>
          {pinging ? 'Пинг…' : '📡 Пинг всех'}
        </button>
        <button className="btn" onClick={load}>🔄 Обновить</button>
        <span className="muted">{filtered.length} шт.</span>
      </div>

      {loading ? (
        <p className="muted">Загрузка…</p>
      ) : (
        <div className="server-list">
          {filtered.slice(0, limit).map((s) => (
            <div key={s.id} className={'server-card' + (s.is_active ? ' active' : '')}>
              <div className="server-head">
                <span className="badge">{s.protocol}</span>
                <span className="server-name" title={s.name}>{s.name}</span>
                <span className={'ping ' + pingClass(s.ping_ms)}>
                  {s.ping_ms > 0 ? `${s.ping_ms} мс` : '—'}
                </span>
              </div>
              <div className="server-host">{s.host}{s.port ? `:${s.port}` : ''}</div>
              <div className="server-actions">
                {s.is_active && <span className="tag current">ТЕКУЩИЙ</span>}
                {s.is_priority && <span className="tag priority">ПРИОРИТЕТ</span>}
                <span className="spacer" />
                {!s.is_active && (
                  <button className="btn sm" onClick={() => activate(s)}>Подключить</button>
                )}
                <button className="btn sm ghost" onClick={() => setPriority(s)}>
                  {s.is_priority ? 'Снять приоритет' : '★ Приоритет'}
                </button>
              </div>
            </div>
          ))}
          {filtered.length > limit && (
            <button className="btn wide" onClick={() => setLimit(limit + PAGE)}>
              Показать ещё ({filtered.length - limit})
            </button>
          )}
        </div>
      )}
    </section>
  )
}
