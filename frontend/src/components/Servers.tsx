import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { apiGet, apiPost } from '../api'
import { pingClass, getFlowStatus, type ProviderInfo, type ServerInfo, type SpeedtestResult } from '../types'
import OutboundGeneratorModal from './OutboundGeneratorModal'
import ShareNodeModal from './ShareNodeModal'
import { copyToClipboard } from '../utils/clipboard'

interface Props {
  notify: (msg: string, isError?: boolean) => void
}

const PAGE = 24

const PROVIDER_COLORS = [
  '#22c55e', // green
  '#a855f7', // purple
  '#38bdf8', // sky blue
  '#f59e0b', // amber
  '#ec4899', // pink
  '#14b8a6', // teal
  '#6366f1', // indigo
]

export default function Servers({ notify }: Props) {
  const [servers, setServers] = useState<ServerInfo[]>([])
  const [providers, setProviders] = useState<ProviderInfo[]>([])
  const [selectedProviders, setSelectedProviders] = useState<Set<string>>(new Set())
  const [subDropdownOpen, setSubDropdownOpen] = useState(false)
  const [viewMode, setViewMode] = useState<'detailed' | 'compact'>(() => {
    return (localStorage.getItem('xr_servers_view') as 'detailed' | 'compact') || 'detailed'
  })
  const [columns, setColumns] = useState<number>(() => {
    const saved = localStorage.getItem('xr_servers_columns')
    if (saved) {
      const n = parseInt(saved, 10)
      if (n >= 0 && n <= 4) return n
    }
    return 0 // 0 = Автоматическое заполнение (Auto-fill)
  })

  const [filter, setFilter] = useState('')
  const [flowOnly, setFlowOnly] = useState(false)
  const [limit, setLimit] = useState(PAGE)
  const [loading, setLoading] = useState(true)
  const [pinging, setPinging] = useState(false)
  const [moreMenuOpen, setMoreMenuOpen] = useState(false)

  // Генератор и импорт ссылок
  const [generatorOpen, setGeneratorOpen] = useState(false)
  const [shareServer, setShareServer] = useState<ServerInfo | null>(null)

  // Speedtest
  const [speedtestingId, setSpeedtestingId] = useState<string | null>(null)
  const [speedResults, setSpeedResults] = useState<Record<string, SpeedtestResult>>({})

  // Игнор-лист
  const [ignored, setIgnored] = useState<Set<string>>(new Set())
  const [ignoreOpen, setIgnoreOpen] = useState(false)
  const [ignoreDraft, setIgnoreDraft] = useState<Set<string>>(new Set())
  const [initialIgnored, setInitialIgnored] = useState<Set<string>>(new Set())
  const [ignoreSearch, setIgnoreSearch] = useState('')
  const [ignoreProviderFilter, setIgnoreProviderFilter] = useState('')
  const [ignoreSaving, setIgnoreSaving] = useState(false)

  // Управление подписками
  const [providersOpen, setProvidersOpen] = useState(false)
  const [editingAliases, setEditingAliases] = useState<Record<string, string>>({})
  const [savedAliasFeedback, setSavedAliasFeedback] = useState<Record<string, boolean>>({})
  const [updatingProvider, setUpdatingProvider] = useState<string | null>(null)
  const [updatingAllProviders, setUpdatingAllProviders] = useState(false)
  const [urlVisibility, setUrlVisibility] = useState<Record<string, boolean>>({})
  const [copiedUrlId, setCopiedUrlId] = useState<string | null>(null)

  // Добавление новой подписки
  const [newSubOpen, setNewSubOpen] = useState(false)
  const [newSubId, setNewSubId] = useState('')
  const [newSubUrl, setNewSubUrl] = useState('')
  const [newSubName, setNewSubName] = useState('')
  const [addingSub, setAddingSub] = useState(false)

  const searchInputRef = useRef<HTMLInputElement>(null)
  const subDropdownRef = useRef<HTMLDivElement>(null)
  const moreMenuRef = useRef<HTMLDivElement>(null)

  // Закрытие выпадающих списков при клике вне их
  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (subDropdownRef.current && !subDropdownRef.current.contains(e.target as Node)) {
        setSubDropdownOpen(false)
      }
      if (moreMenuRef.current && !moreMenuRef.current.contains(e.target as Node)) {
        setMoreMenuOpen(false)
      }
    }
    document.addEventListener('mousedown', handleClickOutside)
    return () => document.removeEventListener('mousedown', handleClickOutside)
  }, [])

  // Цветовая карта подписок
  const providerColorMap = useMemo(() => {
    const m = new Map<string, string>()
    providers.forEach((p, idx) => {
      m.set(p.id, PROVIDER_COLORS[idx % PROVIDER_COLORS.length])
    })
    return m
  }, [providers])

  const setAndSaveViewMode = (mode: 'detailed' | 'compact') => {
    setViewMode(mode)
    localStorage.setItem('xr_servers_view', mode)
  }

  const setAndSaveColumns = (cols: number) => {
    setColumns(cols)
    localStorage.setItem('xr_servers_columns', String(cols))
  }

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

  const pingAll = useCallback(async () => {
    setPinging(true)
    try {
      const data = await apiPost<{ pings: Record<string, number> }>('servers/ping', {})
      setServers((prev) => prev.map((s) => ({ ...s, ping_ms: data.pings[s.id] ?? s.ping_ms })))
      notify('Пинг всех серверов завершён')
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка пинга', true)
    } finally {
      setPinging(false)
    }
  }, [notify])

  const runSpeedtest = async (s: ServerInfo) => {
    setSpeedtestingId(s.id)
    try {
      const res = await apiPost<SpeedtestResult>('servers/speedtest', { server_id: s.id })
      setSpeedResults((prev) => ({ ...prev, [s.id]: res }))
      notify(`Скорость ${s.name}: ${res.speed_mbps.toFixed(1)} Мбит/с (${res.latency_ms} мс)`)
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка замера скорости', true)
    } finally {
      setSpeedtestingId(null)
    }
  }

  // Горячие клавиши и события переключения
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault()
        searchInputRef.current?.focus()
        searchInputRef.current?.select()
      }
    }
    const handleFocusSearch = () => {
      searchInputRef.current?.focus()
      searchInputRef.current?.select()
    }
    const handlePingAll = () => {
      pingAll()
    }
    window.addEventListener('keydown', handleKeyDown)
    window.addEventListener('xr:focus-search', handleFocusSearch)
    window.addEventListener('xr:ping-all', handlePingAll)
    return () => {
      window.removeEventListener('keydown', handleKeyDown)
      window.removeEventListener('xr:focus-search', handleFocusSearch)
      window.removeEventListener('xr:ping-all', handlePingAll)
    }
  }, [pingAll])

  const flowCount = useMemo(() => {
    return servers.filter((s) => getFlowStatus(s.name) === 'ok').length
  }, [servers])

  // Фильтрация серверов
  const filtered = useMemo(() => {
    const q = filter.trim().toLowerCase()
    return servers.filter((s) => {
      if (flowOnly && getFlowStatus(s.name) !== 'ok') {
        return false
      }
      if (selectedProviders.size > 0) {
        if (selectedProviders.has('__static__') && !s.provider) {
          // подходит статический
        } else if (s.provider && selectedProviders.has(s.provider)) {
          // подходит провайдер
        } else {
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
  }, [servers, filter, selectedProviders, flowOnly])

  // Активный сервер для закрепления наверху
  const activeServer = useMemo(() => servers.find((s) => s.is_active), [servers])

  const activate = async (s: ServerInfo) => {
    try {
      const data = await apiPost<{ message: string }>('servers/switch', { server_id: s.id })
      notify(data.message || `Подключено: ${s.name}`)
      load()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка переключения', true)
    }
  }

  const setPriority = async (s: ServerInfo) => {
    try {
      const nextPriority = !s.is_priority
      const data = await apiPost<{ message?: string }>('settings/priority', { server_id: nextPriority ? s.id : '' })
      notify(data.message || (nextPriority ? `Сервер ${s.name} сделан приоритетным` : 'Приоритет снят'))
      load()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка установки приоритета', true)
    }
  }

  // --- Игнор-лист ---
  const openIgnore = () => {
    setIgnoreDraft(new Set(ignored))
    setInitialIgnored(new Set(ignored))
    setIgnoreSearch('')
    setIgnoreProviderFilter('')
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
      notify(`Игнор-лист обновлён: исключено серверов — ${ignoreDraft.size}`)
      setIgnoreOpen(false)
      load()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка применения игнор-листа', true)
    } finally {
      setIgnoreSaving(false)
    }
  }

  const ignoreCandidates = useMemo(() => {
    return servers.filter((s) => s.protocol !== 'URL-TEST' && s.protocol !== 'FALLBACK')
  }, [servers])

  const filteredIgnoreCandidates = useMemo(() => {
    const q = ignoreSearch.trim().toLowerCase()
    return ignoreCandidates.filter((s) => {
      if (ignoreProviderFilter) {
        if (ignoreProviderFilter === '__static__') {
          if (s.provider) return false
        } else if (s.provider !== ignoreProviderFilter) {
          return false
        }
      }
      if (!q) return true
      return s.name.toLowerCase().includes(q) || s.host.toLowerCase().includes(q)
    })
  }, [ignoreCandidates, ignoreSearch, ignoreProviderFilter])

  const selectAllFilteredIgnore = () => {
    setIgnoreDraft((prev) => {
      const next = new Set(prev)
      filteredIgnoreCandidates.forEach((s) => next.add(s.id))
      return next
    })
  }

  const unselectAllFilteredIgnore = () => {
    setIgnoreDraft((prev) => {
      const next = new Set(prev)
      filteredIgnoreCandidates.forEach((s) => next.delete(s.id))
      return next
    })
  }

  // --- Исправление mojibake ---
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
      setMoreMenuOpen(false)
    }
  }

  // --- Управление подписками ---
  const openProvidersModal = () => {
    const initial: Record<string, string> = {}
    for (const p of providers) {
      initial[p.id] = p.name === p.id ? '' : p.name
    }
    setEditingAliases(initial)
    setSavedAliasFeedback({})
    setUrlVisibility({})
    setCopiedUrlId(null)
    setNewSubOpen(false)
    setProvidersOpen(true)
  }

  const saveProviderAlias = async (id: string, alias: string) => {
    try {
      await apiPost('providers/rename', { id, alias: alias.trim() })
      setSavedAliasFeedback((prev) => ({ ...prev, [id]: true }))
      setTimeout(() => {
        setSavedAliasFeedback((prev) => ({ ...prev, [id]: false }))
      }, 2000)
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

  const updateAllProvidersNow = async () => {
    setUpdatingAllProviders(true)
    try {
      const res = await apiPost<{ message: string }>('providers/update', {})
      notify(res.message || 'Все подписки обновлены')
      load()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка обновления всех подписок', true)
    } finally {
      setUpdatingAllProviders(false)
    }
  }

  const copySubUrl = async (id: string, url: string) => {
    await copyToClipboard(url)
    setCopiedUrlId(id)
    notify('URL подписки скопирован в буфер обмена')
    setTimeout(() => setCopiedUrlId(null), 2500)
  }

  const handleAddSubscription = async (e: React.FormEvent) => {
    e.preventDefault()
    if (!newSubId.trim() || !newSubUrl.trim()) {
      notify('Заполните ID и URL подписки', true)
      return
    }
    setAddingSub(true)
    try {
      await apiPost('providers/add', {
        id: newSubId.trim(),
        url: newSubUrl.trim(),
        name: newSubName.trim() || undefined,
      })
      notify(`Подписка '${newSubId.trim()}' успешно добавлена`)
      setNewSubId('')
      setNewSubUrl('')
      setNewSubName('')
      setNewSubOpen(false)
      load()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка добавления подписки', true)
    } finally {
      setAddingSub(false)
    }
  }

  const handleDeleteSubscription = async (id: string, name: string) => {
    if (!window.confirm(`Вы уверены, что хотите удалить подписку "${name}" (${id}) из config.yaml?`)) {
      return
    }
    try {
      await apiPost('providers/delete', { id })
      notify(`Подписка "${name}" удалена`)
      load()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка удаления подписки', true)
    }
  }

  const toggleProviderFilter = (provId: string) => {
    setSelectedProviders((prev) => {
      const next = new Set(prev)
      if (next.has(provId)) next.delete(provId)
      else next.add(provId)
      return next
    })
    setLimit(PAGE)
  }

  const clearProviderFilter = () => {
    setSelectedProviders(new Set())
    setLimit(PAGE)
  }

  return (
    <section className="card">
      {/* ПАНЕЛЬ ИНСТРУМЕНТОВ */}
      <div className="toolbar">
        {/* Поле поиска */}
        <div style={{ position: 'relative', flex: '1 1 220px', minWidth: 180 }}>
          <input
            ref={searchInputRef}
            className="input"
            style={{ width: '100%', paddingRight: 60 }}
            placeholder="Поиск сервера… (Ctrl+K)"
            value={filter}
            onChange={(e) => {
              setFilter(e.target.value)
              setLimit(PAGE)
            }}
          />
          {filter && (
            <button
              type="button"
              onClick={() => {
                setFilter('')
                setLimit(PAGE)
              }}
              style={{
                position: 'absolute',
                right: 8,
                top: '50%',
                transform: 'translateY(-50%)',
                background: 'transparent',
                border: 'none',
                color: 'var(--muted)',
                cursor: 'pointer',
                fontSize: 14,
              }}
              title="Очистить поиск"
            >
              ✕
            </button>
          )}
        </div>

        {/* Счётчик найденных */}
        <span className="muted" style={{ fontWeight: 600, fontSize: 13 }}>
          {filtered.length} шт.
        </span>

        {/* Мультивыбор фильтра подписок */}
        {providers.length > 0 && (
          <div className="subscription-dropdown" ref={subDropdownRef}>
            <button
              type="button"
              className="subscription-dropdown-btn"
              onClick={() => setSubDropdownOpen((prev) => !prev)}
              title="Фильтр по подпискам (поддерживается мультивыбор)"
            >
              <span>📦</span>
              <span>
                {selectedProviders.size === 0
                  ? 'Все подписки'
                  : selectedProviders.size === 1
                  ? providers.find((p) => selectedProviders.has(p.id))?.name || '1 подписка'
                  : `Подписки (${selectedProviders.size})`}
              </span>
              <span style={{ fontSize: 10, opacity: 0.7 }}>▾</span>
            </button>

            {subDropdownOpen && (
              <div className="subscription-dropdown-panel">
                <div
                  className="sub-dropdown-item"
                  onClick={clearProviderFilter}
                  style={{ fontWeight: selectedProviders.size === 0 ? 700 : 400 }}
                >
                  <input
                    type="checkbox"
                    checked={selectedProviders.size === 0}
                    onChange={clearProviderFilter}
                    style={{ pointerEvents: 'none' }}
                  />
                  <span>Все подписки</span>
                  <span className="sub-count-badge">({servers.length})</span>
                </div>

                <div style={{ height: 1, background: 'var(--border)', margin: '4px 0' }} />

                {providers.map((p) => {
                  const isChecked = selectedProviders.has(p.id)
                  const dotColor = providerColorMap.get(p.id) || '#a855f7'
                  return (
                    <div
                      key={p.id}
                      className="sub-dropdown-item"
                      onClick={() => toggleProviderFilter(p.id)}
                    >
                      <input
                        type="checkbox"
                        checked={isChecked}
                        onChange={() => {}}
                        style={{ pointerEvents: 'none' }}
                      />
                      <span className="sub-marker-dot" style={{ backgroundColor: dotColor }} />
                      <span style={{ flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                        {p.name}
                      </span>
                      <span className="sub-count-badge">({p.count})</span>
                    </div>
                  )
                })}
              </div>
            )}
          </div>
        )}

        {/* Пинг всех */}
        <button className="btn" onClick={pingAll} disabled={pinging} title="Измерить пинг всех серверов">
          {pinging ? '📡 Пинг…' : '📡 Пинг всех'}
        </button>

        {/* Обновить */}
        <button className="btn" onClick={load} title="Обновить список">
          🔄 Обновить
        </button>

        {/* Фильтр Google Flow */}
        <button
          type="button"
          className={`btn ${flowOnly ? 'primary' : ''}`}
          onClick={() => {
            setFlowOnly((prev) => !prev)
            setLimit(PAGE)
          }}
          title="Показать только серверы, совместимые с Google Flow и Gemini Labs"
        >
          ✨ Только Flow ({flowCount})
        </button>

        {/* Переключатель вида (Компактный / Подробный) */}
        <div className="view-toggle" title="Режим отображения списка">
          <button
            type="button"
            className={`view-toggle-btn ${viewMode === 'detailed' ? 'active' : ''}`}
            onClick={() => setAndSaveViewMode('detailed')}
            title="Подробный карточный вид"
          >
            ☷ Карточки
          </button>
          <button
            type="button"
            className={`view-toggle-btn ${viewMode === 'compact' ? 'active' : ''}`}
            onClick={() => setAndSaveViewMode('compact')}
            title="Компактный табличный вид"
          >
            ☰ Компактно
          </button>
        </div>

        {/* Выбор количества столбцов (Авто, 1, 2, 3, 4) */}
        <div className="view-toggle cols-toggle" title="Количество столбцов в списке">
          <span className="cols-toggle-label">
            Столбцы:
          </span>
          {[
            { id: 0, label: 'Авто' },
            { id: 1, label: '1' },
            { id: 2, label: '2' },
            { id: 3, label: '3' },
            { id: 4, label: '4' },
          ].map((col) => (
            <button
              key={col.id}
              type="button"
              className={`view-toggle-btn ${columns === col.id ? 'active' : ''}`}
              onClick={() => setAndSaveColumns(col.id)}
              title={col.id === 0 ? 'Адаптивное заполнение экрана' : `${col.id} ${col.id === 1 ? 'столбец' : col.id < 5 ? 'столбца' : 'столбцов'}`}
            >
              {col.label}
            </button>
          ))}
        </div>

        {/* Управление списками */}
        <button className="btn" onClick={() => setGeneratorOpen(true)} title="Импортировать vless/vmess/ss/trojan/hy2/tuic ссылки">
          🪄 Импорт ссылок
        </button>

        {providers.length > 0 && (
          <button className="btn" onClick={openProvidersModal} title="Управление и переименование подписок">
            📦 Подписки ({providers.length})
          </button>
        )}

        <button className="btn" onClick={openIgnore} title="Исключить серверы из Fastest / Fallback">
          🚫 Игнор-лист{ignored.size > 0 ? ` (${ignored.size})` : ''}
        </button>

        {/* Меню дополнительных действий (⋯ Ещё) */}
        <div style={{ position: 'relative' }} ref={moreMenuRef}>
          <button
            type="button"
            className="btn ghost"
            onClick={() => setMoreMenuOpen((prev) => !prev)}
            title="Дополнительные действия"
          >
            ⋯
          </button>
          {moreMenuOpen && (
            <div
              style={{
                position: 'absolute',
                right: 0,
                top: 'calc(100% + 4px)',
                background: '#181d28',
                border: '1px solid var(--border)',
                borderRadius: 8,
                padding: '6px',
                minWidth: 200,
                zIndex: 60,
                boxShadow: '0 8px 24px rgba(0,0,0,0.5)',
              }}
            >
              <button
                type="button"
                className="btn sm ghost"
                style={{ width: '100%', justifyContent: 'flex-start', textAlign: 'left' }}
                onClick={fixNames}
                disabled={fixing}
                title="Починить битые (mojibake) имена серверов в config.yaml"
              >
                {fixing ? 'Исправление…' : '🩹 Исправить битые имена'}
              </button>
            </div>
          )}
        </div>
      </div>

      {loading ? (
        <p className="muted">Загрузка серверов…</p>
      ) : (
        <div>
          {/* 2.6 ЗАКРЕПЛЁННЫЙ АКТИВНЫЙ СЕРВЕР НАВЕРХУ СПИСКА */}
          {activeServer && (
            <div style={{ marginBottom: 16 }}>
              {viewMode === 'detailed' ? (
                <div className="server-card pinned">
                  <div className="server-card-top">
                    <div className="server-card-badges">
                      <span className="pinned-header-tag">✓ ТЕКУЩИЙ СЕРВЕР</span>
                      <span className="badge protocol-badge">{activeServer.protocol}</span>
                      {activeServer.provider && (
                        <span
                          className="tag-provider"
                          title={`Подписка: ${activeServer.provider_name || activeServer.provider}`}
                          onClick={openProvidersModal}
                        >
                          <span
                            className="sub-marker-dot"
                            style={{ backgroundColor: providerColorMap.get(activeServer.provider) || '#a855f7' }}
                          />
                          <span className="tag-provider-name">{activeServer.provider_name || activeServer.provider}</span>
                        </span>
                      )}
                      {ignored.has(activeServer.id) && (
                        <span className="tag ignored" title="Сервер исключён из авто-выбора (Fastest/Fallback)">
                          🚫 Исключён
                        </span>
                      )}
                    </div>

                    <div className="server-card-metrics">
                      {getFlowStatus(activeServer.name) === 'ok' ? (
                        <span className="badge badge-flow-ok" title="Сервер подходит для Google Flow и Gemini Labs">
                          🟢 Flow OK
                        </span>
                      ) : getFlowStatus(activeServer.name) === 'blocked' ? (
                        <span className="badge badge-flow-blocked" title="Заблокирован для Google Flow">
                          🔴 Flow Блок
                        </span>
                      ) : null}
                      {speedResults[activeServer.id] && (
                        <span
                          className="badge badge-speed"
                          title={`Замер скорости: ${speedResults[activeServer.id].speed_mbps.toFixed(2)} Мбит/с (${speedResults[activeServer.id].latency_ms} мс)`}
                        >
                          ⚡ {speedResults[activeServer.id].speed_mbps.toFixed(1)} М
                        </span>
                      )}
                      <span className={'ping ' + pingClass(activeServer.ping_ms)}>
                        {activeServer.ping_ms > 0 ? `${activeServer.ping_ms} мс` : '—'}
                      </span>
                    </div>
                  </div>

                  <div className="server-card-main">
                    <div
                      className="server-card-name"
                      title={`${activeServer.name}\nХост: ${activeServer.host}${activeServer.port ? `:${activeServer.port}` : ''}`}
                    >
                      {activeServer.name}
                    </div>
                  </div>

                  <div className="server-card-actions">
                    <div className="server-card-action-primary">
                      <span className="tag current-active-badge">✓ ПОДКЛЮЧЁН</span>
                    </div>

                    <div className="server-card-action-secondary">
                      <button
                        type="button"
                        className={`btn sm ghost btn-speedtest ${speedtestingId === activeServer.id ? 'loading' : ''}`}
                        onClick={() => runSpeedtest(activeServer)}
                        disabled={speedtestingId !== null}
                        title="Замерить реальную скорость загрузки через этот прокси"
                      >
                        <span>{speedtestingId === activeServer.id ? '⏳' : '🚀'}</span>
                        <span>{speedtestingId === activeServer.id ? 'Замер…' : 'Скорость'}</span>
                      </button>

                      <button
                        type="button"
                        className="btn sm ghost"
                        onClick={() => setShareServer(activeServer)}
                        title="Сгенерировать ссылку подключения (vless/vmess/ss) и QR-код"
                      >
                        <span>🔗</span>
                        <span className="btn-label-optional">Ссылка</span>
                      </button>

                      <button
                        type="button"
                        className={`btn sm ghost btn-priority ${activeServer.is_priority ? 'is-priority' : ''}`}
                        onClick={() => setPriority(activeServer)}
                        title={activeServer.is_priority ? 'Снять приоритет' : 'Сделать приоритетным'}
                      >
                        <span>{activeServer.is_priority ? '★' : '☆'}</span>
                        <span className="btn-label-optional">{activeServer.is_priority ? 'В приоритете' : 'Приоритет'}</span>
                      </button>
                    </div>
                  </div>
                </div>
              ) : (
                <div className="server-compact-row pinned">
                  <span className="pinned-header-tag" style={{ fontSize: 10 }}>✓ ТЕКУЩИЙ</span>
                  <span className="badge">{activeServer.protocol}</span>
                  {activeServer.provider && (
                    <span
                      className="sub-marker-dot"
                      title={activeServer.provider_name || activeServer.provider}
                      style={{ backgroundColor: providerColorMap.get(activeServer.provider) || '#a855f7' }}
                    />
                  )}
                  <span
                    className="server-compact-name"
                    title={`${activeServer.name}\nХост: ${activeServer.host}:${activeServer.port}`}
                  >
                    {activeServer.name}
                  </span>
                  {getFlowStatus(activeServer.name) === 'ok' ? (
                    <span className="badge badge-flow-ok" style={{ fontSize: 10, padding: '1px 5px' }} title="Подходит для Google Flow">
                      🟢 Flow
                    </span>
                  ) : getFlowStatus(activeServer.name) === 'blocked' ? (
                    <span className="badge badge-flow-blocked" style={{ fontSize: 10, padding: '1px 5px' }} title="Заблокирован для Flow">
                      🔴 Блок
                    </span>
                  ) : null}
                  {ignored.has(activeServer.id) && (
                    <span className="tag ignored" style={{ fontSize: 10 }}>🚫</span>
                  )}
                  {speedResults[activeServer.id] && (
                    <span className="badge badge-speed" style={{ fontSize: 10, padding: '1px 5px' }}>
                      ⚡ {speedResults[activeServer.id].speed_mbps.toFixed(1)}M
                    </span>
                  )}
                  <span className={'ping ' + pingClass(activeServer.ping_ms)}>
                    {activeServer.ping_ms > 0 ? `${activeServer.ping_ms} мс` : '—'}
                  </span>
                  <div style={{ display: 'inline-flex', gap: 6, alignItems: 'center' }}>
                    <button
                      className="btn sm ghost"
                      onClick={() => setShareServer(activeServer)}
                      style={{ padding: '3px 6px' }}
                      title="Ссылка подключения и QR-код"
                    >
                      🔗
                    </button>
                    <button
                      className={`btn sm ghost btn-speedtest ${speedtestingId === activeServer.id ? 'loading' : ''}`}
                      onClick={() => runSpeedtest(activeServer)}
                      disabled={speedtestingId !== null}
                      style={{ padding: '3px 6px' }}
                      title="Замерить скорость"
                    >
                      {speedtestingId === activeServer.id ? '⏳' : '🚀'}
                    </button>
                    <button
                      className={`btn sm ghost btn-priority ${activeServer.is_priority ? 'is-priority' : ''}`}
                      onClick={() => setPriority(activeServer)}
                    >
                      {activeServer.is_priority ? '★' : '☆'}
                    </button>
                  </div>
                </div>
              )}

              <div className="pinned-divider">
                <span>Список всех серверов ({filtered.length})</span>
              </div>
            </div>
          )}

          {/* ОСНОВНОЙ СПИСОК СЕРВЕРОВ */}
          {filtered.length === 0 ? (
            <p className="muted">Серверов по заданному фильтру не найдено.</p>
          ) : viewMode === 'detailed' ? (
            /* ПОДРОБНЫЙ ВИД (КАРТОЧКИ) */
            <div className={`server-list cols-${columns}`}>
              {filtered.slice(0, limit).map((s) => (
                <div key={s.id} className={'server-card' + (s.is_active ? ' active' : '')}>
                  {/* 1. Верхняя строка: метаданные слева, метрики справа */}
                  <div className="server-card-top">
                    <div className="server-card-badges">
                      <span className="badge protocol-badge">{s.protocol}</span>
                      {s.provider && (
                        <span
                          className="tag-provider"
                          title={`Подписка: ${s.provider_name || s.provider} (кликните для управления)`}
                          onClick={openProvidersModal}
                        >
                          <span
                            className="sub-marker-dot"
                            style={{ backgroundColor: providerColorMap.get(s.provider) || '#a855f7' }}
                          />
                          <span className="tag-provider-name">{s.provider_name || s.provider}</span>
                        </span>
                      )}
                      {ignored.has(s.id) && (
                        <span className="tag ignored" title="Сервер исключён из авто-выбора (Fastest / Fallback)">
                          🚫 Исключён
                        </span>
                      )}
                    </div>

                    <div className="server-card-metrics">
                      {getFlowStatus(s.name) === 'ok' ? (
                        <span className="badge badge-flow-ok" title="Сервер подходит для Google Flow и Gemini Labs">
                          🟢 Flow OK
                        </span>
                      ) : getFlowStatus(s.name) === 'blocked' ? (
                        <span className="badge badge-flow-blocked" title="Заблокирован для Google Flow">
                          🔴 Flow Блок
                        </span>
                      ) : null}
                      {speedResults[s.id] && (
                        <span
                          className="badge badge-speed"
                          title={`Замер скорости: ${speedResults[s.id].speed_mbps.toFixed(2)} Мбит/с (${speedResults[s.id].latency_ms} мс)`}
                        >
                          ⚡ {speedResults[s.id].speed_mbps.toFixed(1)} М
                        </span>
                      )}
                      <span className={'ping ' + pingClass(s.ping_ms)}>
                        {s.ping_ms > 0 ? `${s.ping_ms} мс` : '—'}
                      </span>
                    </div>
                  </div>

                  {/* 2. Средняя строка: Название сервера во всю ширину */}
                  <div className="server-card-main">
                    <div
                      className="server-card-name"
                      title={`${s.name}\nХост: ${s.host}${s.port ? `:${s.port}` : ''}\nПротокол: ${s.protocol}`}
                    >
                      {s.name}
                    </div>
                  </div>

                  {/* 3. Нижняя строка: Кнопки действий без разрыва строк */}
                  <div className="server-card-actions">
                    <div className="server-card-action-primary">
                      {s.is_active ? (
                        <span className="tag current-active-badge">✓ ПОДКЛЮЧЁН</span>
                      ) : (
                        <button
                          type="button"
                          className="btn sm btn-connect"
                          onClick={() => activate(s)}
                          title="Сделать этот сервер активным"
                        >
                          🔌 Подключить
                        </button>
                      )}
                    </div>

                    <div className="server-card-action-secondary">
                      <button
                        type="button"
                        className={`btn sm ghost btn-speedtest ${speedtestingId === s.id ? 'loading' : ''}`}
                        onClick={() => runSpeedtest(s)}
                        disabled={speedtestingId !== null}
                        title="Замерить реальную скорость загрузки через этот прокси"
                      >
                        <span>{speedtestingId === s.id ? '⏳' : '🚀'}</span>
                        <span>{speedtestingId === s.id ? 'Замер…' : 'Скорость'}</span>
                      </button>

                      <button
                        type="button"
                        className="btn sm ghost"
                        onClick={() => setShareServer(s)}
                        title="Сгенерировать ссылку подключения (vless/vmess/ss) и QR-код"
                      >
                        <span>🔗</span>
                        <span className="btn-label-optional">Ссылка</span>
                      </button>

                      <button
                        type="button"
                        className={`btn sm ghost btn-priority ${s.is_priority ? 'is-priority' : ''}`}
                        onClick={() => setPriority(s)}
                        title={s.is_priority ? 'Снять приоритет' : 'Сделать приоритетным'}
                      >
                        <span>{s.is_priority ? '★' : '☆'}</span>
                        <span className="btn-label-optional">{s.is_priority ? 'В приоритете' : 'Приоритет'}</span>
                      </button>
                    </div>
                  </div>
                </div>
              ))}
              {filtered.length > limit && (
                <button
                  className="btn wide"
                  style={{ gridColumn: '1 / -1', marginTop: 8 }}
                  onClick={() => setLimit(limit + PAGE)}
                >
                  Показать ещё ({filtered.length - limit})
                </button>
              )}
            </div>
          ) : (
            /* КОМПАКТНЫЙ ВИД (ОДНОСТРОЧНАЯ ТАБЛИЦА) */
            <div className={`server-compact-list cols-${columns}`}>
              {filtered.slice(0, limit).map((s) => (
                <div key={s.id} className={'server-compact-row' + (s.is_active ? ' active' : '')}>
                  <span className="badge" style={{ minWidth: 46, textAlign: 'center' }}>
                    {s.protocol}
                  </span>
                  {s.provider && (
                    <span
                      className="sub-marker-dot"
                      title={s.provider_name || s.provider}
                      style={{ backgroundColor: providerColorMap.get(s.provider) || '#a855f7' }}
                    />
                  )}
                  <span
                    className="server-compact-name"
                    title={`${s.name}\nХост: ${s.host}${s.port ? `:${s.port}` : ''}`}
                  >
                    {s.name}
                  </span>
                  {getFlowStatus(s.name) === 'ok' ? (
                    <span className="badge badge-flow-ok" style={{ fontSize: 10, padding: '1px 5px' }} title="Подходит для Google Flow">
                      🟢 Flow
                    </span>
                  ) : getFlowStatus(s.name) === 'blocked' ? (
                    <span className="badge badge-flow-blocked" style={{ fontSize: 10, padding: '1px 5px' }} title="Заблокирован для Flow">
                      🔴 Блок
                    </span>
                  ) : null}
                  {ignored.has(s.id) && (
                    <span className="tag ignored" style={{ fontSize: 10, padding: '1px 5px' }} title="Исключён из авто-выбора">
                      🚫 Исключён
                    </span>
                  )}
                  {speedResults[s.id] && (
                    <span className="badge badge-speed" style={{ fontSize: 10, padding: '1px 5px' }}>
                      ⚡ {speedResults[s.id].speed_mbps.toFixed(1)}M
                    </span>
                  )}
                  <span
                    className={'ping ' + pingClass(s.ping_ms)}
                    style={{ minWidth: 60, textAlign: 'right' }}
                  >
                    {s.ping_ms > 0 ? `${s.ping_ms} мс` : '—'}
                  </span>
                  <div style={{ display: 'inline-flex', gap: 6, alignItems: 'center' }}>
                    <button
                      className="btn sm ghost"
                      onClick={() => setShareServer(s)}
                      style={{ padding: '3px 6px' }}
                      title="Ссылка подключения и QR-код"
                    >
                      🔗
                    </button>
                    <button
                      className={`btn sm ghost btn-speedtest ${speedtestingId === s.id ? 'loading' : ''}`}
                      onClick={() => runSpeedtest(s)}
                      disabled={speedtestingId !== null}
                      style={{ padding: '3px 6px' }}
                      title="Замерить скорость"
                    >
                      {speedtestingId === s.id ? '⏳' : '🚀'}
                    </button>
                    {s.is_active ? (
                      <span className="tag current" style={{ fontSize: 10, padding: '2px 6px' }}>✓</span>
                    ) : (
                      <button className="btn sm btn-connect" onClick={() => activate(s)} style={{ padding: '3px 8px' }}>
                        🔌
                      </button>
                    )}
                    <button
                      className={`btn sm ghost btn-priority ${s.is_priority ? 'is-priority' : ''}`}
                      onClick={() => setPriority(s)}
                      style={{ padding: '3px 8px' }}
                      title={s.is_priority ? 'Снять приоритет' : 'Сделать приоритетным'}
                    >
                      {s.is_priority ? '★' : '☆'}
                    </button>
                  </div>
                </div>
              ))}
              {filtered.length > limit && (
                <button
                  className="btn wide"
                  style={{ gridColumn: '1 / -1', marginTop: 8 }}
                  onClick={() => setLimit(limit + PAGE)}
                >
                  Показать ещё ({filtered.length - limit})
                </button>
              )}
            </div>
          )}
        </div>
      )}

      {/* МОДАЛЬНОЕ ОКНО ИГНОР-ЛИСТА */}
      {ignoreOpen && (
        <div className="modal-overlay" onClick={() => setIgnoreOpen(false)}>
          <div className="modal" onClick={(e) => e.stopPropagation()}>
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
              <h2>🚫 Игнор-лист серверов</h2>
              <button
                type="button"
                className="btn sm ghost"
                onClick={() => setIgnoreOpen(false)}
                style={{ fontSize: 16, padding: '2px 8px' }}
              >
                ✕
              </button>
            </div>
            <p className="muted small" style={{ margin: '0 0 6px' }}>
              Отмеченные серверы исключаются из авто-групп <b>Fastest</b> и <b>Fallback</b>. Ручное подключение остаётся доступным.
            </p>

            {/* Поиск и фильтр по подписке внутри модалки */}
            <div className="modal-filter-row">
              <input
                className="input sm"
                placeholder="Поиск серверов в модалке…"
                value={ignoreSearch}
                onChange={(e) => setIgnoreSearch(e.target.value)}
                style={{ flex: 1 }}
              />
              {providers.length > 0 && (
                <select
                  className="input sm"
                  value={ignoreProviderFilter}
                  onChange={(e) => setIgnoreProviderFilter(e.target.value)}
                  style={{ maxWidth: 180 }}
                >
                  <option value="">Все подписки ({ignoreCandidates.length})</option>
                  {providers.map((p) => (
                    <option key={p.id} value={p.id}>
                      {p.name} ({p.count})
                    </option>
                  ))}
                  <option value="__static__">Статические</option>
                </select>
              )}
              <button
                type="button"
                className="btn sm ghost"
                onClick={selectAllFilteredIgnore}
                title="Отметить все отфильтрованные серверы"
              >
                Выбрать все
              </button>
              <button
                type="button"
                className="btn sm ghost"
                onClick={unselectAllFilteredIgnore}
                title="Снять отметки с отфильтрованных"
              >
                Снять все
              </button>
            </div>

            {/* Список серверов */}
            <div className="modal-list">
              {filteredIgnoreCandidates.length === 0 ? (
                <p className="muted">Серверов не найдено.</p>
              ) : (
                filteredIgnoreCandidates.map((s) => {
                  const isChecked = ignoreDraft.has(s.id)
                  const wasInitiallyChecked = initialIgnored.has(s.id)
                  return (
                    <label key={s.id} className="check-row">
                      <input
                        type="checkbox"
                        checked={isChecked}
                        onChange={(e) => toggleIgnoreDraft(s.id, e.target.checked)}
                      />
                      <span className="badge">{s.protocol}</span>
                      {s.provider && (
                        <span
                          className="sub-marker-dot"
                          style={{ backgroundColor: providerColorMap.get(s.provider) || '#a855f7' }}
                        />
                      )}
                      <span className="server-name" title={s.name}>
                        {s.name}
                      </span>
                      {/* Индикация изменений сессии */}
                      {isChecked && !wasInitiallyChecked && (
                        <span className="diff-added-badge">+добавлен</span>
                      )}
                      {!isChecked && wasInitiallyChecked && (
                        <span className="diff-removed-badge">-снят</span>
                      )}
                      {s.ping_ms > 0 && (
                        <span className={'ping ' + pingClass(s.ping_ms)}>{s.ping_ms} мс</span>
                      )}
                    </label>
                  )
                })
              )}
            </div>

            {/* Липкий футер (Sticky actions) */}
            <div className="modal-actions">
              <button className="btn" onClick={() => setIgnoreOpen(false)}>
                Отмена
              </button>
              <button
                className="btn ghost"
                disabled={ignoreSaving || ignoreDraft.size === 0}
                onClick={() => setIgnoreDraft(new Set())}
              >
                Сбросить всё
              </button>
              <button className="btn primary" onClick={saveIgnore} disabled={ignoreSaving}>
                {ignoreSaving ? 'Применение…' : `Применить (${ignoreDraft.size})`}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* МОДАЛЬНОЕ ОКНО УПРАВЛЕНИЯ ПОДПИСКАМИ */}
      {providersOpen && (
        <div className="modal-overlay" onClick={() => setProvidersOpen(false)}>
          <div className="modal" style={{ width: 'min(720px, 94vw)' }} onClick={(e) => e.stopPropagation()}>
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
              <h2>📦 Управление подписками</h2>
              <div style={{ display: 'flex', gap: 6 }}>
                <button
                  type="button"
                  className="btn sm"
                  onClick={updateAllProvidersNow}
                  disabled={updatingAllProviders}
                  title="Принудительно обновить все подписки"
                >
                  {updatingAllProviders ? 'Обновление…' : '↻ Обновить все'}
                </button>
                <button
                  type="button"
                  className="btn sm ghost"
                  onClick={() => setProvidersOpen(false)}
                  style={{ fontSize: 16, padding: '2px 8px' }}
                >
                  ✕
                </button>
              </div>
            </div>

            <p className="muted small" style={{ margin: '0 0 8px' }}>
              Вы можете переименовывать подписки, копировать их URL, добавлять новые или удалять неиспользуемые.
            </p>

            <div className="modal-list" style={{ maxHeight: '52vh' }}>
              {providers.length === 0 && <p className="muted">Нет активных подписок.</p>}
              {providers.map((p) => {
                const draft = editingAliases[p.id] ?? (p.name === p.id ? '' : p.name)
                const isChanged = (p.name === p.id ? '' : p.name) !== draft.trim()
                const isSaved = savedAliasFeedback[p.id]
                const dotColor = providerColorMap.get(p.id) || '#a855f7'
                const showUrl = urlVisibility[p.id]

                return (
                  <div key={p.id} className="provider-edit-row" style={{ flexDirection: 'column', alignItems: 'stretch', gap: 6, padding: '10px 0', borderBottom: '1px solid var(--border)' }}>
                    <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 10, flexWrap: 'wrap' }}>
                      <div className="provider-meta" style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                        <span className="sub-marker-dot" style={{ backgroundColor: dotColor }} />
                        <b style={{ fontSize: 14 }}>{p.name}</b>
                        <span className="provider-meta-id" style={{ fontFamily: 'Consolas, monospace', fontSize: 12, color: 'var(--muted)' }}>
                          ({p.id})
                        </span>
                        <span style={{ fontSize: 11, color: 'var(--muted)' }}>
                          · {p.count} серв.{p.updated_at ? ` · ${p.updated_at.slice(0, 10)}` : ''}
                        </span>
                      </div>

                      {/* Кнопки обновления и удаления */}
                      <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
                        <button
                          type="button"
                          className="btn sm ghost"
                          title="Загрузить свежие серверы из этой подписки"
                          disabled={updatingProvider === p.id}
                          onClick={() => updateProviderNow(p.id)}
                        >
                          {updatingProvider === p.id ? '…' : '↻ Обновить'}
                        </button>
                        <button
                          type="button"
                          className="btn sm ghost"
                          style={{ color: '#ef4444' }}
                          title="Удалить подписку"
                          onClick={() => handleDeleteSubscription(p.id, p.name)}
                        >
                          🗑
                        </button>
                      </div>
                    </div>

                    {/* Поле переименования */}
                    <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
                      <input
                        className="input sm"
                        placeholder="Своё название подписки…"
                        value={draft}
                        onChange={(e) =>
                          setEditingAliases((prev) => ({ ...prev, [p.id]: e.target.value }))
                        }
                        onKeyDown={(e) => {
                          if (e.key === 'Enter') saveProviderAlias(p.id, draft)
                        }}
                        style={{ flex: 1 }}
                      />
                      <button
                        type="button"
                        className={`btn sm ${isSaved ? 'primary' : ''}`}
                        disabled={!isChanged && !isSaved}
                        onClick={() => saveProviderAlias(p.id, draft)}
                        style={{ minWidth: 95 }}
                      >
                        {isSaved ? '✓ Сохранено' : 'Сохранить'}
                      </button>
                    </div>

                    {/* Строка URL подписки */}
                    {p.url && (
                      <div style={{ display: 'flex', alignItems: 'center', gap: 8, background: 'rgba(0,0,0,0.2)', padding: '4px 8px', borderRadius: 6 }}>
                        <span className="muted small" style={{ flexShrink: 0 }}>URL:</span>
                        <span style={{ flex: 1, fontFamily: 'Consolas, monospace', fontSize: 11.5, color: showUrl ? 'var(--text)' : 'var(--muted)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                          {showUrl ? p.url : '••••••••••••••••••••••••••••••••••••••••••••'}
                        </span>
                        <button
                          type="button"
                          className="btn sm ghost"
                          onClick={() => setUrlVisibility((prev) => ({ ...prev, [p.id]: !showUrl }))}
                          title={showUrl ? 'Скрыть URL' : 'Показать URL'}
                          style={{ padding: '2px 6px', fontSize: 11 }}
                        >
                          {showUrl ? '🙈 Скрыть' : '👁 Показать'}
                        </button>
                        <button
                          type="button"
                          className="btn sm ghost"
                          onClick={() => copySubUrl(p.id, p.url!)}
                          title="Скопировать URL в буфер обмена"
                          style={{ padding: '2px 6px', fontSize: 11 }}
                        >
                          {copiedUrlId === p.id ? '✓ Скопировано' : '📋 Копировать'}
                        </button>
                      </div>
                    )}
                  </div>
                )
              })}

              {/* Форма добавления новой подписки */}
              <div style={{ marginTop: 12, padding: '10px 12px', background: 'rgba(255,255,255,0.03)', border: '1px solid var(--border)', borderRadius: 8 }}>
                {!newSubOpen ? (
                  <button
                    type="button"
                    className="btn sm ghost"
                    onClick={() => setNewSubOpen(true)}
                    style={{ width: '100%', color: 'var(--accent)' }}
                  >
                    ➕ Добавить новую подписку
                  </button>
                ) : (
                  <form onSubmit={handleAddSubscription} style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
                    <b style={{ fontSize: 13 }}>➕ Добавление подписки</b>
                    <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
                      <input
                        className="input sm"
                        placeholder="ID (латиница, напр. sub_work)"
                        value={newSubId}
                        onChange={(e) => setNewSubId(e.target.value)}
                        style={{ flex: '1 1 180px' }}
                        required
                      />
                      <input
                        className="input sm"
                        placeholder="Отображаемое имя (напр. Рабочая)"
                        value={newSubName}
                        onChange={(e) => setNewSubName(e.target.value)}
                        style={{ flex: '1 1 180px' }}
                      />
                    </div>
                    <input
                      className="input sm"
                      placeholder="URL подписки (https://...)"
                      value={newSubUrl}
                      onChange={(e) => setNewSubUrl(e.target.value)}
                      required
                    />
                    <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 6, marginTop: 4 }}>
                      <button type="button" className="btn sm ghost" onClick={() => setNewSubOpen(false)}>
                        Отмена
                      </button>
                      <button type="submit" className="btn sm primary" disabled={addingSub}>
                        {addingSub ? 'Добавление…' : 'Добавить подписку'}
                      </button>
                    </div>
                  </form>
                )}
              </div>
            </div>

            {/* Липкий футер */}
            <div className="modal-actions">
              <button className="btn primary" onClick={() => setProvidersOpen(false)}>
                Закрыть
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Модальное окно импорта ссылок */}
      <OutboundGeneratorModal
        isOpen={generatorOpen}
        onClose={() => setGeneratorOpen(false)}
        onImportSuccess={() => {
          load()
          notify('Прокси успешно импортированы')
        }}
        notify={notify}
      />

      {/* Модальное окно экспорта и генерации ссылки / QR-кода */}
      <ShareNodeModal
        server={shareServer}
        isOpen={!!shareServer}
        onClose={() => setShareServer(null)}
        notify={notify}
      />
    </section>
  )
}
