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
  const [ignored, setIgnored] = useState<Set<string>>(new Set())
  const [ignoreOpen, setIgnoreOpen] = useState(false)
  const [ignoreDraft, setIgnoreDraft] = useState<Set<string>>(new Set())
  const [ignoreSaving, setIgnoreSaving] = useState(false)

  const load = useCallback(async () => {
    try {
      const [data, ig] = await Promise.all([
        apiGet<{ servers: ServerInfo[] }>('servers'),
        apiGet<{ servers: string[] }>('ignore').catch(() => ({ servers: [] as string[] })),
      ])
      setServers(data.servers)
      setIgnored(new Set(ig.servers))
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

  const openIgnore = () => {
    setIgnoreDraft(new Set(ignored))
    setIgnoreOpen(true)
  }

  const toggleIgnoreDraft = (id: string, on: boolean) => {
    setIgnoreDraft((prev) => {
      const next = new Set(prev)
      if (on) next.add(id)
      else next.delete(id)
      return next
    })
  }

  const saveIgnore = async () => {
    setIgnoreSaving(true)
    try {
      await apiPost<{ applied: number }>('ignore', { servers: [...ignoreDraft] })
      notify(`Игнор-лист применён: исключено из Fastest/Fallback — ${ignoreDraft.size}`)
      setIgnoreOpen(false)
      load()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка применения игнор-листа', true)
    } finally {
      setIgnoreSaving(false)
    }
  }

  // для игнор-листа годятся только реальные серверы (не синтетические Fastest/Fallback)
  const ignoreCandidates = servers.filter((s) => s.protocol !== 'URL-TEST' && s.protocol !== 'FALLBACK')

  const [fixing, setFixing] = useState(false)
  const fixNames = async () => {
    setFixing(true)
    try {
      const data = await apiPost<{ fixed: number; names: string[] }>('servers/fix-names', {})
      notify(
        data.fixed > 0
          ? `Исправлено имён: ${data.fixed} — ${data.names.join('; ')}`
          : 'Битых (mojibake) имён не найдено',
      )
      load()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка исправления имён', true)
    } finally {
      setFixing(false)
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
        <button className="btn" onClick={openIgnore}>
          🚫 Игнор-лист{ignored.size > 0 ? ` (${ignored.size})` : ''}
        </button>
        <button className="btn" onClick={fixNames} disabled={fixing} title="Починить битые (mojibake) имена серверов в config.yaml">
          {fixing ? 'Исправление…' : '🩹 Исправить имена'}
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
                {ignored.has(s.id) && <span className="tag ignored">ИГНОР</span>}
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

      {ignoreOpen && (
        <div className="modal-overlay" onClick={() => setIgnoreOpen(false)}>
          <div className="modal" onClick={(e) => e.stopPropagation()}>
            <h2>🚫 Игнор-лист серверов</h2>
            <p className="muted small">
              Отмеченные серверы будут исключены из авто-групп <b>Fastest</b> (url-test) и <b>Fallback</b> —
              они не будут выбираться автоматически. Ручное подключение к ним остаётся доступным.
            </p>
            <div className="modal-list">
              {ignoreCandidates.length === 0 && <p className="muted">Нет реальных серверов.</p>}
              {ignoreCandidates.map((s) => (
                <label key={s.id} className="check-row">
                  <input
                    type="checkbox"
                    checked={ignoreDraft.has(s.id)}
                    onChange={(e) => toggleIgnoreDraft(s.id, e.target.checked)}
                  />
                  <span className="badge">{s.protocol}</span>
                  <span className="server-name" title={s.name}>{s.name}</span>
                  {s.ping_ms > 0 && <span className={'ping ' + pingClass(s.ping_ms)}>{s.ping_ms} мс</span>}
                </label>
              ))}
            </div>
            <div className="modal-actions">
              <button className="btn" onClick={() => setIgnoreOpen(false)}>Отмена</button>
              <button className="btn ghost" disabled={ignoreSaving || ignoreDraft.size === 0}
                onClick={() => { setIgnoreDraft(new Set()); }}>
                Снять все
              </button>
              <button className="btn primary" onClick={saveIgnore} disabled={ignoreSaving}>
                {ignoreSaving ? 'Применение…' : `Применить (${ignoreDraft.size})`}
              </button>
            </div>
          </div>
        </div>
      )}
    </section>
  )
}
