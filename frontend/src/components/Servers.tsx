import { useCallback, useEffect, useMemo, useState } from 'react'
import { apiGet, apiPost } from '../api'
import { pingClass, type ProviderInfo, type ServerInfo } from '../types'

interface Props {
  notify: (msg: string, isError?: boolean) => void
}

const PAGE = 15

export default function Servers({ notify }: Props) {
  const [servers, setServers] = useState<ServerInfo[]>([])
  const [providers, setProviders] = useState<ProviderInfo[]>([])
  const [selectedProvider, setSelectedProvider] = useState('')
  const [providersOpen, setProvidersOpen] = useState(false)
  const [editingAliases, setEditingAliases] = useState<Record<string, string>>({})
  const [updatingProvider, setUpdatingProvider] = useState<string | null>(null)
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
      const [data, ig, provData] = await Promise.all([
        apiGet<{ servers: ServerInfo[] }>('servers'),
        apiGet<{ servers: string[] }>('ignore').catch(() => ({ servers: [] as string[] })),
        apiGet<{ providers: ProviderInfo[] }>('providers').catch(() => ({ providers: [] as ProviderInfo[] })),
      ])
      setServers(data.servers)
      setIgnored(new Set(ig.servers))
      setProviders(provData.providers)
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
    return servers.filter((s) => {
      if (selectedProvider) {
        if (selectedProvider === '__static__') {
          if (s.provider) return false
        } else if (s.provider !== selectedProvider) {
          return false
        }
      }
      if (!q) return true
      return (
        s.name.toLowerCase().includes(q) ||
        (s.provider_name && s.provider_name.toLowerCase().includes(q)) ||
        (s.provider && s.provider.toLowerCase().includes(q)) ||
        s.host.toLowerCase().includes(q)
      )
    })
  }, [servers, filter, selectedProvider])

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
      const data = await apiPost<{ message?: string }>('settings/priority', { server_id: s.is_priority ? '' : s.id })
      if (data.message) notify(data.message)
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

  const openProvidersModal = () => {
    const initial: Record<string, string> = {}
    for (const p of providers) {
      initial[p.id] = p.name === p.id ? '' : p.name
    }
    setEditingAliases(initial)
    setProvidersOpen(true)
  }

  const saveProviderAlias = async (id: string, alias: string) => {
    try {
      await apiPost('providers/rename', { id, alias: alias.trim() })
      notify(`Подписка сохранена: ${alias.trim() || id}`)
      load()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка сохранения названия', true)
    }
  }

  const updateProviderNow = async (id: string) => {
    setUpdatingProvider(id)
    try {
      const res = await apiPost<{ message: string }>('providers/update', { id })
      notify(res.message || 'Подписка обновлена')
      load()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка обновления подписки', true)
    } finally {
      setUpdatingProvider(null)
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
        {providers.length > 0 && (
          <select
            className="input"
            value={selectedProvider}
            onChange={(e) => {
              setSelectedProvider(e.target.value)
              setLimit(PAGE)
            }}
            title="Фильтр по подписке"
            style={{ maxWidth: 180 }}
          >
            <option value="">Все подписки ({servers.length})</option>
            {providers.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name} ({p.count})
              </option>
            ))}
          </select>
        )}
        <button className="btn" onClick={pingAll} disabled={pinging}>
          {pinging ? 'Пинг…' : '📡 Пинг всех'}
        </button>
        {providers.length > 0 && (
          <button className="btn" onClick={openProvidersModal} title="Управление и переименование подписок">
            📦 Подписки ({providers.length})
          </button>
        )}
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
                {s.provider && (
                  <span
                    className="tag-provider"
                    title={`Подписка: ${s.provider_name || s.provider} (кликните для управления подписками)`}
                    onClick={openProvidersModal}
                  >
                    📦 {s.provider_name || s.provider}
                  </span>
                )}
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
                  {s.provider && (
                    <span className="tag-provider" style={{ fontSize: 10, padding: '1px 5px' }}>
                      📦 {s.provider_name || s.provider}
                    </span>
                  )}
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

      {providersOpen && (
        <div className="modal-overlay" onClick={() => setProvidersOpen(false)}>
          <div className="modal" onClick={(e) => e.stopPropagation()}>
            <h2>📦 Управление подписками</h2>
            <p className="muted small">
              Вы можете переименовать подписки (proxy-providers), чтобы легко различать их в списке серверов,
              фильтрах, цепочке failover и на дашборде.
            </p>
            <div className="modal-list">
              {providers.length === 0 && <p className="muted">Нет активных подписок.</p>}
              {providers.map((p) => {
                const draft = editingAliases[p.id] ?? (p.name === p.id ? '' : p.name)
                const isChanged = (p.name === p.id ? '' : p.name) !== draft.trim()
                return (
                  <div key={p.id} className="provider-edit-row">
                    <div className="provider-meta">
                      <b style={{ fontSize: 13 }}>{p.name}</b>
                      <span className="provider-meta-id">{p.id}</span>
                      <span className="provider-meta-count">
                        {p.count} серв.{p.updated_at ? ` · ${p.updated_at.slice(0, 10)}` : ''}
                      </span>
                    </div>
                    <div className="provider-inputs">
                      <input
                        className="input sm"
                        placeholder="Своё название…"
                        value={draft}
                        onChange={(e) =>
                          setEditingAliases((prev) => ({ ...prev, [p.id]: e.target.value }))
                        }
                        onKeyDown={(e) => {
                          if (e.key === 'Enter') saveProviderAlias(p.id, draft)
                        }}
                      />
                      <button
                        className="btn sm primary"
                        disabled={!isChanged}
                        onClick={() => saveProviderAlias(p.id, draft)}
                      >
                        Сохранить
                      </button>
                      <button
                        className="btn sm ghost"
                        title="Обновить подписку (скачать свежие серверы)"
                        disabled={updatingProvider === p.id}
                        onClick={() => updateProviderNow(p.id)}
                      >
                        {updatingProvider === p.id ? '…' : '🔄'}
                      </button>
                    </div>
                  </div>
                )
              })}
            </div>
            <div className="modal-actions">
              <button className="btn primary" onClick={() => setProvidersOpen(false)}>
                Закрыть
              </button>
            </div>
          </div>
        </div>
      )}
    </section>
  )
}
