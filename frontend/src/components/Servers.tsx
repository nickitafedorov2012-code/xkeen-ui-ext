import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { apiGet, apiPost } from '../api'
import { pingClass, getFlowStatus, type ProviderInfo, type ServerInfo, type SpeedtestResult, type StatusInfo } from '../types'
import OutboundGeneratorModal from './OutboundGeneratorModal'
import ShareNodeModal from './ShareNodeModal'
import FlowRepairModal from './FlowRepairModal'
import { copyToClipboard } from '../utils/clipboard'
import { Sparkline, PROVIDER_COLORS, formatSpeed, formatBytes, formatQuotaBytes, isPoolServer } from './servers/serversHelpers'
import { IgnoreListModal } from './servers/IgnoreListModal'
import { SubscriptionsModal } from './servers/SubscriptionsModal'

export { formatQuotaBytes, isPoolServer } from './servers/serversHelpers'
interface Props {
  notify: (msg: string, isError?: boolean) => void
  status?: StatusInfo | null
  refresh?: () => void
}

const PAGE = 20

export default function Servers({ notify, status, refresh }: Props) {
  const [servers, setServers] = useState<ServerInfo[]>([])
  const [providers, setProviders] = useState<ProviderInfo[]>([])
  const [providersLoading, setProvidersLoading] = useState(false)
  const [providersLoaded, setProvidersLoaded] = useState(false)
  const [providersError, setProvidersError] = useState(false)
  const providersRequestId = useRef(0)
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
  const [poolOnly, setPoolOnly] = useState(false)
  const [showPools, setShowPools] = useState(false)
  const [lastPingTime, setLastPingTime] = useState<string | null>(() => {
    try {
      return localStorage.getItem('xkeen_last_ping_check')
    } catch {
      return null
    }
  })
  const [flowRepairOpen, setFlowRepairOpen] = useState(false)
  const [limit, setLimit] = useState(PAGE)
  const [loading, setLoading] = useState(true)
  const [pinging, setPinging] = useState(false)
  const [moreMenuOpen, setMoreMenuOpen] = useState(false)

  // Apple-grade фильтры и сортировка
  const [hideOffline, setHideOffline] = useState<boolean>(() => {
    return localStorage.getItem('xr_servers_hide_offline') === 'true'
  })
  const [sortMode, setSortMode] = useState<'default' | 'ping_asc' | 'ping_desc' | 'name_asc' | 'provider_asc' | 'priority_first'>('default')

  // Псевдонимы серверов (Rename / Alias)
  const [editingAliasId, setEditingAliasId] = useState<string | null>(null)
  const [aliasInput, setAliasInput] = useState('')
  const [aliasSaving, setAliasSaving] = useState(false)

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
  const [confirmDeleteSub, setConfirmDeleteSub] = useState<{ id: string; name: string } | null>(null)
  const [deletingSub, setDeletingSub] = useState(false)
  const [editingAliases, setEditingAliases] = useState<Record<string, string>>({})
  const [togglingProvider, setTogglingProvider] = useState<string | null>(null)
  const [updatingProvider, setUpdatingProvider] = useState<string | null>(null)
  const [updatingAllProviders, setUpdatingAllProviders] = useState(false)
  const [urlVisibility, setUrlVisibility] = useState<Record<string, boolean>>({})
  const [copiedUrlId, setCopiedUrlId] = useState<string | null>(null)
  const [editingProviderNameId, setEditingProviderNameId] = useState<string | null>(null)
  const [editingHwidId, setEditingHwidId] = useState<string | null>(null)
  const [hwidInput, setHwidInput] = useState('')
  const [copiedHwidId, setCopiedHwidId] = useState<string | null>(null)
  const [savingHwidId, setSavingHwidId] = useState<string | null>(null)

  // Добавление новой подписки
  const [newSubOpen, setNewSubOpen] = useState(false)
  const [newSubId, setNewSubId] = useState('')
  const [newSubUrl, setNewSubUrl] = useState('')
  const [newSubName, setNewSubName] = useState('')
  const [newSubHwid, setNewSubHwid] = useState('')
  const [newSubAppendHwid, setNewSubAppendHwid] = useState(false)
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

  const loadProviders = useCallback(async () => {
    const requestId = ++providersRequestId.current
    setProvidersLoading(true)
    try {
      const data = await apiGet<{ providers: ProviderInfo[] }>('providers')
      if (!Array.isArray(data.providers)) throw new Error('Некорректный список подписок')
      if (requestId !== providersRequestId.current) return
      setProviders(data.providers.filter((p) => p.id !== 'vkturn'))
      setProvidersLoaded(true)
      setProvidersError(false)
    } catch {
      if (requestId !== providersRequestId.current) return
      setProvidersError(true)
    } finally {
      if (requestId === providersRequestId.current) setProvidersLoading(false)
    }
  }, [])

  const load = useCallback(async () => {
    void loadProviders()
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
  }, [loadProviders, notify])

  const pingAll = useCallback(async () => {
    setPinging(true)
    try {
      const data = await apiPost<{ pings: Record<string, number> }>('servers/ping', {})
      setServers((prev) => prev.map((s) => ({ ...s, ping_ms: data.pings[s.id] ?? s.ping_ms })))
      const nowStr = new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' })
      setLastPingTime(nowStr)
      try {
        localStorage.setItem('xkeen_last_ping_check', nowStr)
      } catch {}
      notify('Пинг всех серверов завершён')
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка пинга', true)
    } finally {
      setPinging(false)
    }
  }, [notify])

  useEffect(() => {
    load()
    const onSync = () => load()
    window.addEventListener('xr:data-changed', onSync)
    window.addEventListener('xr:refresh-status', onSync)
    // Фоновое обновление статуса пулов и серверов каждые 3 секунды для мгновенной реактивности без F5
    const bgTimer = setInterval(() => {
      apiGet<{ servers: ServerInfo[] }>('servers')
        .then((data) => {
          if (data?.servers) {
            setServers((prev) => {
              if (prev.length !== data.servers.length) {
                return data.servers
              }
              return prev.map((s) => {
                const fresh = data.servers.find((f) => f.id === s.id)
                if (!fresh) return s
                return {
                  ...s,
                  name: fresh.name,
                  host: fresh.host,
                  resolved_ip: fresh.resolved_ip,
                  is_active: fresh.is_active,
                  now_node: fresh.now_node,
                  ping_ms: fresh.ping_ms > 0 ? fresh.ping_ms : s.ping_ms,
                }
              })
            })
          }
        })
        .catch(() => {})
    }, 3000)
    return () => {
      window.removeEventListener('xr:data-changed', onSync)
      window.removeEventListener('xr:refresh-status', onSync)
      clearInterval(bgTimer)
    }
  }, [load])

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

  // Живая телеметрия активного узла: трафик и история пинга
  const [liveTraffic, setLiveTraffic] = useState<{
    proxy?: { down: number; up: number }
    whitelist?: { down: number; up: number }
    whitelist_active?: boolean
    down?: number
    up?: number
    proxy_bytes?: number
    whitelist_bytes?: number
  } | null>(null)
  const [pingHistory, setPingHistory] = useState<number[]>([])
  const [jitterMeasuring, setJitterMeasuring] = useState(false)
  const lastActiveServerIdRef = useRef<string | undefined>(undefined)

  const jitterVal = useMemo(() => {
    if (pingHistory.length < 2) return 0
    let sum = 0
    for (let i = 1; i < pingHistory.length; i++) {
      sum += Math.abs(pingHistory[i] - pingHistory[i - 1])
    }
    return Math.round(sum / (pingHistory.length - 1))
  }, [pingHistory])

  const failoverChain = useMemo(() => {
    return status?.failover?.priority_chain ?? (status?.failover?.priority_server ? [status.failover.priority_server] : [])
  }, [status?.failover?.priority_chain, status?.failover?.priority_server])

  useEffect(() => {
    let active = true
    const pollTraffic = async () => {
      try {
        const snap = await apiGet<any>('traffic/poll')
        if (active && snap) {
          setLiveTraffic(snap)
        }
      } catch {}
    }
    pollTraffic()
    const interval = setInterval(pollTraffic, 1500)
    return () => {
      active = false
      clearInterval(interval)
    }
  }, [])

  const flowCount = useMemo(() => {
    return servers.filter((s) => getFlowStatus(s) === 'ok').length
  }, [servers])

  const poolsCount = useMemo(() => {
    return servers.filter(isPoolServer).length
  }, [servers])

  const saveAliasWithVal = async (serverId: string, val: string) => {
    setAliasSaving(true)
    try {
      const trimmed = val.trim()
      await apiPost('servers/alias', { server_id: serverId, alias: trimmed })
      setServers((prev) =>
        prev.map((s) => (s.id === serverId ? { ...s, custom_alias: trimmed || undefined } : s))
      )
      notify(trimmed ? `Имя сервера изменено на «${trimmed}»` : 'Пользовательский псевдоним сброшен')
      setEditingAliasId(null)
      refresh?.()
    } catch (e: any) {
      notify(e instanceof Error ? e.message : 'Ошибка сохранения имени', true)
    } finally {
      setAliasSaving(false)
    }
  }

  const saveAlias = (serverId: string) => saveAliasWithVal(serverId, aliasInput)

  const toggleWhitelist = async (serverId: string) => {
    try {
      const isCurrentlyWhitelisted = servers.find((s) => s.id === serverId)?.is_whitelist ?? false
      const nextWhitelisted = !isCurrentlyWhitelisted
      await apiPost('servers/whitelist', { server_id: serverId, enabled: nextWhitelisted })
      setServers((prev) =>
        prev.map((s) => (s.id === serverId ? { ...s, is_whitelist: nextWhitelisted } : s))
      )
      notify(nextWhitelisted ? 'Сервер добавлен в белый список аварийного туннеля' : 'Сервер удалён из белого списка')
      refresh?.()
    } catch (e: any) {
      notify(e instanceof Error ? e.message : 'Ошибка обновления белого списка', true)
    }
  }

  const makeServerNormal = async (serverId: string) => {
    try {
      await apiPost('servers/whitelist', { server_id: serverId, normal: true })
      setServers((prev) =>
        prev.map((s) => (s.id === serverId ? { ...s, is_whitelist: false } : s))
      )
      notify('Узел помечен как обычный (теги белых списков отключены)')
      refresh?.()
    } catch (e: any) {
      notify(e instanceof Error ? e.message : 'Ошибка сохранения настройки узла', true)
    }
  }

  // Фильтрация и сортировка полного пула серверов в памяти (Apple-Style Full Pool Sorting)
  const filtered = useMemo(() => {
    const q = filter.trim().toLowerCase()
    const list = servers.filter((s) => {
      // Исключённые серверы скрываем полностью из общего списка (доступны только в модалке Игнор-лист)
      if (ignored.has(s.id)) {
        return false
      }
      const isPool = isPoolServer(s)
      if (poolOnly && !isPool) {
        return false
      }
      if (!poolOnly && !showPools && isPool && !selectedProviders.has('__pool__')) {
        return false
      }
      if (flowOnly && getFlowStatus(s) !== 'ok') {
        return false
      }
      if (hideOffline && !s.is_active && (s.ping_ms <= 0 || s.ping_ms >= 9999)) {
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
      const aliasMatch = s.custom_alias ? s.custom_alias.toLowerCase().includes(q) : false
      return (
        aliasMatch ||
        s.name.toLowerCase().includes(q) ||
        (s.provider_name && s.provider_name.toLowerCase().includes(q)) ||
        (s.provider && s.provider.toLowerCase().includes(q)) ||
        s.host.toLowerCase().includes(q)
      )
    })

    const sorted = [...list]
    if (sortMode === 'ping_asc') {
      sorted.sort((a, b) => {
        const aHealthy = a.ping_ms > 0 && a.ping_ms < 9999
        const bHealthy = b.ping_ms > 0 && b.ping_ms < 9999
        if (aHealthy && bHealthy) return a.ping_ms - b.ping_ms
        if (aHealthy) return -1
        if (bHealthy) return 1
        return 0
      })
    } else if (sortMode === 'ping_desc') {
      sorted.sort((a, b) => {
        const aPing = a.ping_ms > 0 && a.ping_ms < 9999 ? a.ping_ms : -1
        const bPing = b.ping_ms > 0 && b.ping_ms < 9999 ? b.ping_ms : -1
        return bPing - aPing
      })
    } else if (sortMode === 'name_asc') {
      sorted.sort((a, b) => (a.custom_alias || a.name).localeCompare(b.custom_alias || b.name, 'ru'))
    } else if (sortMode === 'provider_asc') {
      sorted.sort((a, b) => {
        const provA = a.provider_name || a.provider || 'ZZZ'
        const provB = b.provider_name || b.provider || 'ZZZ'
        const cmp = provA.localeCompare(provB, 'ru')
        if (cmp !== 0) return cmp
        return (a.custom_alias || a.name).localeCompare(b.custom_alias || b.name, 'ru')
      })
    } else if (sortMode === 'priority_first') {
      sorted.sort((a, b) => {
        if (a.is_priority !== b.is_priority) return a.is_priority ? -1 : 1
        return 0
      })
    }
    return sorted
  }, [servers, filter, selectedProviders, flowOnly, poolOnly, showPools, hideOffline, sortMode, ignored])

  // Активный сервер (из списка или глобального статуса)
  const activeServer = useMemo(() => {
    return servers.find((s) => s.is_active) || (status?.active_server ? servers.find((s) => s.id === status.active_server?.id) : undefined)
  }, [servers, status?.active_server])

  useEffect(() => {
    const sId = activeServer?.id
    if (!sId) {
      setPingHistory([])
      lastActiveServerIdRef.current = undefined
      return
    }

    const ping = activeServer.ping_ms
    const isValidPing = typeof ping === 'number' && ping > 0 && ping < 9999

    if (lastActiveServerIdRef.current !== sId) {
      lastActiveServerIdRef.current = sId
      let stored: number[] = []
      try {
        const raw = sessionStorage.getItem(`xr_pings_${sId}`)
        if (raw) stored = JSON.parse(raw)
      } catch {}

      if (isValidPing) {
        if (stored.length === 0 || stored[stored.length - 1] !== ping) {
          stored = [...stored, ping].slice(-15)
          try {
            sessionStorage.setItem(`xr_pings_${sId}`, JSON.stringify(stored))
          } catch {}
        }
      }
      setPingHistory(stored)
    } else if (isValidPing) {
      setPingHistory((prev) => {
        if (prev.length > 0 && prev[prev.length - 1] === ping) {
          return prev
        }
        const next = [...prev, ping].slice(-15)
        try {
          sessionStorage.setItem(`xr_pings_${sId}`, JSON.stringify(next))
        } catch {}
        return next
      })
    }
  }, [activeServer?.id, activeServer?.ping_ms])

  const measureLiveJitter = async () => {
    if (!activeServer?.id || jitterMeasuring) return
    setJitterMeasuring(true)
    try {
      const res = await apiPost<{ pings: Record<string, number> }>('servers/ping', { server_id: activeServer.id })
      const liveDelay = res?.pings?.[activeServer.id]
      if (liveDelay && liveDelay > 0 && liveDelay < 9999) {
        setPingHistory((prev) => {
          const next = [...prev, liveDelay].slice(-15)
          try {
            sessionStorage.setItem(`xr_pings_${activeServer.id}`, JSON.stringify(next))
          } catch {}
          return next
        })
        notify(`Замер задержки: ${liveDelay} мс`)
      } else {
        notify('Сервер не ответил на пинг', true)
      }
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка замера пинга', true)
    } finally {
      setJitterMeasuring(false)
    }
  }

  // FE-04: защита от двойного клика (двойной switch) и параллельных мутаций цепочки
  const switchingRef = useRef(false)
  const chainMutatingRef = useRef(false)

  const activate = async (s: ServerInfo) => {
    if (switchingRef.current) return
    switchingRef.current = true
    try {
      const data = await apiPost<{ message: string }>('servers/switch', { server_id: s.id })
      notify(data.message || `Подключено: ${s.name}`)
      // Выбранный узел проверяет бэкенд. Не запускаем замер всей подписки при переключении.
      window.dispatchEvent(new CustomEvent('xr:refresh-status'))
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка переключения', true)
    } finally {
      switchingRef.current = false
    }
  }

  const addToFailover = async (serverId: string, position: 'start' | 'end') => {
    if (chainMutatingRef.current) return
    chainMutatingRef.current = true
    try {
      // FE-04: цепочку берём свежую с бэкенда — read-modify-write от stale-статуса
      // терял быстрые последовательные изменения
      let currentChain: string[] = []
      try {
        const st = await apiGet<StatusInfo>('status')
        currentChain = st?.failover?.priority_chain ?? (st?.failover?.priority_server ? [st.failover.priority_server] : [])
      } catch {
        currentChain = status?.failover?.priority_chain ?? (status?.failover?.priority_server ? [status.failover.priority_server] : [])
      }
      const filteredChain = currentChain.filter((id) => id !== serverId)
      const isAtStart = currentChain[0] === serverId
      const isAtEnd = currentChain[currentChain.length - 1] === serverId && currentChain.length > 1
      const isRemoving = (position === 'start' && isAtStart) || (position === 'end' && isAtEnd)

      const nextChain = isRemoving
        ? filteredChain
        : position === 'start'
        ? [serverId, ...filteredChain]
        : [...filteredChain, serverId]

      const data = await apiPost<{ message?: string }>('settings/priority', { server_ids: nextChain })
      notify(data.message || (isRemoving ? 'Узел удалён из Failover' : position === 'start' ? 'Узел перемещён в начало Failover (#1)' : 'Узел добавлен в конец Failover'))
      load()
      // PING-AUTO: обновляем пинги сразу после правки цепочки приоритета
      pingAll()
      if (refresh) refresh()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка настройки Failover', true)
    } finally {
      chainMutatingRef.current = false
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
    setUrlVisibility({})
    setCopiedUrlId(null)
    setNewSubOpen(false)
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

  const toggleProviderEnable = async (id: string, currentEnabled: boolean) => {
    setTogglingProvider(id)
    try {
      const target = !currentEnabled
      const res = await apiPost<{ message: string; enabled: boolean }>('providers/toggle', { id, enabled: target })
      notify(res.message || (target ? 'Подписка включена' : 'Подписка отключена'))
      setProviders((prev) => prev.map((p) => (p.id === id ? { ...p, enabled: target } : p)))
      load()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка переключения подписки', true)
    } finally {
      setTogglingProvider(null)
    }
  }

  const [togglingWhitelistSub, setTogglingWhitelistSub] = useState<string | null>(null)

  const toggleProviderWhitelist = async (provId: string, currentWhitelist: boolean) => {
    setTogglingWhitelistSub(provId)
    try {
      const target = !currentWhitelist
      await apiPost('whitelist/providers', { provider_id: provId, enabled: target })
      notify(target ? `Подписка "${provId}" добавлена в белые списки (аварийный туннель)` : `Подписка "${provId}" исключена из белых списков`)
      setProviders((prev) => prev.map((p) => (p.id === provId ? { ...p, is_whitelist: target } : p)))
      load()
      refresh?.()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка переключения белого списка для подписки', true)
    } finally {
      setTogglingWhitelistSub(null)
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

  const generateHwid = () => {
    const num = Math.floor(100 + Math.random() * 900)
    setNewSubHwid(`keenetic-router-${num}`)
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
        hwid: newSubHwid.trim() || undefined,
        append_hwid_to_url: newSubAppendHwid,
      })
      notify(`Подписка '${newSubId.trim()}' успешно добавлена`)
      setNewSubId('')
      setNewSubUrl('')
      setNewSubName('')
      setNewSubHwid('')
      setNewSubAppendHwid(false)
      setNewSubOpen(false)
      load()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка добавления подписки', true)
    } finally {
      setAddingSub(false)
    }
  }

  const handleSaveHwid = async (id: string, hwid: string) => {
    setSavingHwidId(id)
    try {
      const clean = hwid.trim()
      await apiPost('providers/hwid', { id, hwid: clean || undefined })
      notify(clean ? `HWID подписки '${id}' сохранён` : `HWID подписки '${id}' удалён`)
      setProviders((prev) =>
        prev.map((p) => (p.id === id ? { ...p, hwid: clean || undefined } : p)),
      )
      setEditingHwidId(null)
      load()
    } catch (e: any) {
      notify(e instanceof Error ? e.message : 'Ошибка сохранения HWID', true)
    } finally {
      setSavingHwidId(null)
    }
  }

  const handleCopyHwid = async (id: string, hwid: string) => {
    await copyToClipboard(hwid)
    setCopiedHwidId(id)
    notify('HWID скопирован в буфер обмена')
    setTimeout(() => setCopiedHwidId(null), 2500)
  }

  const handleDeleteSubscription = (id: string, name: string) => {
    setConfirmDeleteSub({ id, name })
  }

  const executeDeleteSubscription = async (id: string, name: string) => {
    setDeletingSub(true)
    try {
      await apiPost('providers/delete', { id })
      notify(`Подписка "${name}" успешно удалена`)
      setProviders((prev) => prev.filter((p) => p.id !== id))
      setConfirmDeleteSub(null)
      load()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка удаления подписки', true)
    } finally {
      setDeletingSub(false)
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
      {/* ВЕРХНИЙ ДЕК: Активный узел слева, Поиск/Фильтры/Действия справа — сбалансированная командная строка */}
      <div className="servers-top-deck">
        {/* Активный узел */}
        <div className="servers-active-deck-card">
          {activeServer ? (
            <>
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', gap: 6, flexWrap: 'wrap' }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: 6, flexWrap: 'wrap' }}>
                  <span className="pinned-header-tag" style={{ background: '#22c55e', color: '#0f172a', fontWeight: 700, padding: '2px 8px', borderRadius: 4, fontSize: 10 }}>
                    🟢 АКТИВНЫЙ УЗЕЛ
                  </span>
                  <span className="badge protocol-badge">{activeServer.protocol}</span>
                  {activeServer.provider && (
                    <span className="tag-provider" onClick={openProvidersModal} style={{ cursor: 'pointer' }}>
                      <span className="sub-marker-dot" style={{ backgroundColor: providerColorMap.get(activeServer.provider) || '#a855f7' }} />
                      <span className="tag-provider-name">{activeServer.provider_name || activeServer.provider}</span>
                    </span>
                  )}
                  {status?.whitelist_mode_active && (
                    <span className="badge" style={{ background: 'rgba(168, 85, 247, 0.25)', color: '#c084fc', border: '1px solid rgba(168, 85, 247, 0.5)', fontSize: 10, fontWeight: 700 }}>
                      🟣 ОБХОД БЕЛЫХ СПИСКОВ
                    </span>
                  )}
                </div>

                <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                  {getFlowStatus(activeServer) === 'ok' && (
                    <span className="badge badge-flow-ok" style={{ fontSize: 10, padding: '1px 5px' }}>🟢 Flow</span>
                  )}
                  {speedResults[activeServer.id] && (
                    <span className="badge badge-speed" style={{ fontSize: 10, padding: '1px 5px' }}>
                      ⚡ {speedResults[activeServer.id].speed_mbps.toFixed(1)} М
                    </span>
                  )}
                  <span className={'ping ' + pingClass(activeServer.ping_ms)}>
                    {activeServer.ping_ms > 0 ? `${activeServer.ping_ms} мс` : '—'}
                  </span>
                  <Sparkline ping={activeServer.ping_ms} />
                </div>
              </div>

              {/* Название и псевдоним */}
              <div>
                {editingAliasId === activeServer.id ? (
                  <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
                    <input
                      className="input sm"
                      style={{ flex: 1, fontSize: 13 }}
                      value={aliasInput}
                      onChange={(e) => setAliasInput(e.target.value)}
                      placeholder="Пользовательский псевдоним (пусто = сброс)"
                      autoFocus
                      onKeyDown={(e) => {
                        if (e.key === 'Enter') saveAlias(activeServer.id)
                        if (e.key === 'Escape') setEditingAliasId(null)
                      }}
                    />
                    <button className="btn sm primary" onClick={() => saveAlias(activeServer.id)} disabled={aliasSaving}>
                      ✓
                    </button>
                    <button className="btn sm ghost" onClick={() => setEditingAliasId(null)}>
                      ✕
                    </button>
                  </div>
                ) : (
                  <div style={{ display: 'flex', alignItems: 'center', gap: 6, flexWrap: 'wrap' }}>
                    <b style={{ fontSize: 14, color: 'var(--text-bright)' }}>
                      {activeServer.custom_alias ? `${activeServer.custom_alias} (${activeServer.name})` : activeServer.name}
                    </b>
                    <button
                      type="button"
                      className="btn sm ghost"
                      style={{ padding: '0 4px', fontSize: 12, height: 22, opacity: 0.7 }}
                      onClick={() => {
                        setEditingAliasId(activeServer.id)
                        setAliasInput(activeServer.custom_alias || '')
                      }}
                      title="Переименовать узел (задать псевдоним)"
                    >
                      ✏️
                    </button>
                    {activeServer.custom_alias && (
                      <button
                        type="button"
                        className="btn sm ghost"
                        style={{ padding: '0 4px', fontSize: 12, height: 22, opacity: 0.7, color: 'var(--text-danger, #ef4444)' }}
                        onClick={() => saveAliasWithVal(activeServer.id, '')}
                        title="Сбросить пользовательский псевдоним"
                      >
                        ✕
                      </button>
                    )}
                  </div>
                )}
                <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginTop: 4, flexWrap: 'wrap' }}>
                  <div className="muted small" style={{ fontSize: 11 }}>
                    {activeServer.protocol === 'VK-CALL' ? 'Адрес VPS:' : 'IP сервера:'}{' '}
                    <code>
                      {activeServer.protocol === 'VK-CALL' && (!activeServer.port || activeServer.port <= 0)
                        ? activeServer.host || 'Интерфейс csq0 (Звонки ВК)'
                        : (
                          <>
                            {activeServer.resolved_ip || activeServer.host}
                            {activeServer.resolved_ip && activeServer.resolved_ip !== activeServer.host
                              ? ` (${activeServer.host})`
                              : ''}
                            {activeServer.port ? `:${activeServer.port}` : ''}
                          </>
                        )}
                    </code>
                  </div>
                  {/* Роль в Failover-цепочке */}
                  {status?.failover && (
                    <span
                      className="badge"
                      style={{
                        fontSize: 10.5,
                        fontWeight: 700,
                        padding: '2px 7px',
                        borderRadius: 4,
                        background:
                          status.failover.priority_server === activeServer.id || status.failover.priority_chain?.[0] === activeServer.id
                            ? 'rgba(56, 189, 248, 0.2)'
                            : status.failover.priority_chain?.includes(activeServer.id)
                            ? 'rgba(245, 158, 11, 0.2)'
                            : 'rgba(255, 255, 255, 0.06)',
                        color:
                          status.failover.priority_server === activeServer.id || status.failover.priority_chain?.[0] === activeServer.id
                            ? '#38bdf8'
                            : status.failover.priority_chain?.includes(activeServer.id)
                            ? '#fbbf24'
                            : 'var(--muted)',
                        border: '1px solid rgba(255, 255, 255, 0.1)',
                      }}
                      title="Позиция узла в автоматической цепочке защиты от сбоев (Failover)"
                    >
                      {status.failover.priority_server === activeServer.id || status.failover.priority_chain?.[0] === activeServer.id
                        ? '👑 Основной сервер'
                        : status.failover.priority_chain?.includes(activeServer.id)
                        ? `⚡ Резерв #${status.failover.priority_chain.indexOf(activeServer.id) + 1}`
                        : '🌐 Ручной выбор'}
                    </span>
                  )}
                </div>

                {/* Apple-style Live-телеметрия узла: скорость, трафик и спарклайн задержки */}
                <div
                  className="active-node-telemetry"
                  style={{
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'space-between',
                    gap: 12,
                    padding: '6px 12px',
                    background: 'rgba(15, 23, 42, 0.55)',
                    border: '1px solid rgba(56, 189, 248, 0.18)',
                    borderRadius: 8,
                    marginTop: 8,
                    marginBottom: 4,
                    flexWrap: 'wrap',
                  }}
                >
                  {(() => {
                    const isWl = Boolean(status?.whitelist_mode_active || activeServer.is_whitelist)
                    const downSpeed = isWl
                      ? (liveTraffic?.whitelist?.down ?? liveTraffic?.down ?? 0)
                      : (liveTraffic?.proxy?.down ?? liveTraffic?.down ?? 0)
                    const upSpeed = isWl
                      ? (liveTraffic?.whitelist?.up ?? liveTraffic?.up ?? 0)
                      : (liveTraffic?.proxy?.up ?? liveTraffic?.up ?? 0)
                    const totalBytes = (liveTraffic?.proxy_bytes ?? 0) + (liveTraffic?.whitelist_bytes ?? 0)

                    return (
                      <div style={{ display: 'flex', alignItems: 'center', gap: 8, fontSize: 12 }}>
                        <span style={{ fontSize: 11, color: '#94a3b8', fontWeight: 600 }}>Live-трафик:</span>
                        <span style={{ color: '#38bdf8', fontWeight: 700, fontFamily: 'Consolas, monospace' }}>
                          ↓ {formatSpeed(downSpeed)}
                        </span>
                        <span style={{ color: '#c084fc', fontWeight: 700, fontFamily: 'Consolas, monospace' }}>
                          ↑ {formatSpeed(upSpeed)}
                        </span>
                        {totalBytes > 0 && (
                          <span className="muted" style={{ fontSize: 11, marginLeft: 2 }}>
                            (всего {formatBytes(totalBytes)})
                          </span>
                        )}
                      </div>
                    )
                  })()}

                  {/* Квота подписки активного сервера */}
                  {(() => {
                    const prov = providers.find((p) => p.id === activeServer.provider)
                    if (!prov || !prov.total_bytes || prov.total_bytes <= 0) return null
                    const used = (prov.upload_bytes || 0) + (prov.download_bytes || 0)
                    const total = prov.total_bytes
                    const remaining = Math.max(0, total - used)
                    const percent = Math.min(100, Math.round((used / total) * 100))
                    return (
                      <div
                        style={{
                          display: 'flex',
                          alignItems: 'center',
                          gap: 6,
                          fontSize: 11,
                          padding: '2px 8px',
                          background: 'rgba(56, 189, 248, 0.08)',
                          border: '1px solid rgba(56, 189, 248, 0.25)',
                          borderRadius: 6,
                        }}
                        title={`Лимит: ${formatQuotaBytes(used)} из ${formatQuotaBytes(total)} (${percent}%)${prov.expire_timestamp ? ` · До ${new Date(prov.expire_timestamp * 1000).toLocaleDateString()}` : ''}`}
                      >
                        <span style={{ color: '#38bdf8', fontWeight: 600 }}>📊 Трафик подписки:</span>
                        <span style={{ color: remaining === 0 ? '#ef4444' : '#4ade80', fontWeight: 700 }}>
                          {formatQuotaBytes(remaining)} ост.
                        </span>
                        <span className="muted" style={{ fontSize: 10 }}>({percent}%)</span>
                      </div>
                    )
                  })()}

                  {activeServer && (
                    <div
                      onClick={measureLiveJitter}
                      style={{
                        display: 'inline-flex',
                        alignItems: 'center',
                        gap: 5,
                        padding: '2px 8px',
                        borderRadius: 6,
                        cursor: jitterMeasuring ? 'wait' : 'pointer',
                        background:
                          pingHistory.length < 2
                            ? 'rgba(148, 163, 184, 0.1)'
                            : jitterVal > 25
                            ? 'rgba(239, 68, 68, 0.12)'
                            : jitterVal > 10
                            ? 'rgba(234, 179, 8, 0.12)'
                            : 'rgba(34, 197, 94, 0.12)',
                        border: `1px solid ${
                          pingHistory.length < 2
                            ? 'rgba(148, 163, 184, 0.25)'
                            : jitterVal > 25
                            ? 'rgba(239, 68, 68, 0.35)'
                            : jitterVal > 10
                            ? 'rgba(234, 179, 8, 0.35)'
                            : 'rgba(34, 197, 94, 0.35)'
                        }`,
                        fontSize: 11,
                        userSelect: 'none',
                        transition: 'all 0.2s ease',
                      }}
                      title={
                        jitterMeasuring
                          ? 'Выполняется измерение задержки...'
                          : pingHistory.length < 2
                          ? `Задержка: ${activeServer.ping_ms > 0 ? `${activeServer.ping_ms} мс` : '—'} | Джиттер: требуется от 2 замеров (нажмите для экспресс-замера)`
                          : `Задержка: ${activeServer.ping_ms} мс | Джиттер: ±${jitterVal} мс (разброс по ${pingHistory.length} точкам) | Мин: ${Math.min(...pingHistory)} мс, Макс: ${Math.max(...pingHistory)} мс | Нажмите для экспресс-замера`
                      }
                    >
                      <span>{jitterMeasuring ? '⏳' : '📶'}</span>
                      <span style={{ color: 'var(--muted)', fontSize: 10.5 }}>Джиттер:</span>
                      <span
                        style={{
                          fontFamily: 'Consolas, monospace',
                          fontWeight: 600,
                          color:
                            pingHistory.length < 2
                              ? 'var(--muted)'
                              : jitterVal > 25
                              ? '#ef4444'
                              : jitterVal > 10
                              ? '#eab308'
                              : '#22c55e',
                        }}
                      >
                        {jitterMeasuring ? '...' : pingHistory.length < 2 ? '—' : `±${jitterVal} мс`}
                      </span>
                    </div>
                  )}
                </div>
              </div>

              {/* Действия */}
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
                <span className="tag current-active-badge" style={{ fontSize: 11, padding: '2px 8px' }}>
                  <span className="dot on" style={{ width: 6, height: 6, marginRight: 4, display: 'inline-block' }} />
                  Активен
                </span>
                {/* FE-MOBILE: перенос кнопок на новую строку вместо распирания карточки на мобильных */}
                <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap' }}>
                  <button
                    type="button"
                    className={`btn sm ghost btn-speedtest ${speedtestingId === activeServer.id ? 'loading' : ''}`}
                    onClick={() => runSpeedtest(activeServer)}
                    disabled={speedtestingId !== null}
                    title="Замерить скорость"
                  >
                    <span>{speedtestingId === activeServer.id ? '⏳' : '🚀'}</span>
                    <span>{speedtestingId === activeServer.id ? 'Замер…' : 'Скорость'}</span>
                  </button>
                  {activeServer.protocol === 'VK-CALL' ? (
                    <button
                      type="button"
                      className="btn sm ghost"
                      onClick={() => window.dispatchEvent(new CustomEvent('xr:switch-tab', { detail: 'vkturn' }))}
                      title="Открыть настройки VK-туннеля"
                    >
                      📞 Настройки ВК
                    </button>
                  ) : (
                    <button
                      type="button"
                      className="btn sm ghost"
                      onClick={() => setShareServer(activeServer)}
                      title="Ссылка подключения и QR-код"
                    >
                      🔗 Ссылка
                    </button>
                  )}
                  {(() => {
                    const chainIdx = failoverChain.indexOf(activeServer.id)
                    const isStart = chainIdx === 0

                    return (
                      <>
                        <button
                          type="button"
                          className={`btn sm ghost ${isStart ? 'is-priority' : ''}`}
                          onClick={() => addToFailover(activeServer.id, 'start')}
                          style={isStart ? { background: 'rgba(56, 189, 248, 0.2)', color: '#38bdf8', borderColor: '#38bdf8', fontWeight: 700 } : undefined}
                          title={isStart ? 'Основной узел Failover (#1). Нажмите для снятия' : 'Сделать основным узлом (в начало Failover)'}
                        >
                          <span>{isStart ? '👑 Основной' : '⚡↑ В начало'}</span>
                        </button>
                        <button
                          type="button"
                          className={`btn sm ghost ${chainIdx > 0 ? 'is-priority' : ''}`}
                          onClick={() => addToFailover(activeServer.id, 'end')}
                          style={chainIdx > 0 ? { background: 'rgba(245, 158, 11, 0.15)', color: '#fbbf24', borderColor: '#fbbf24', fontWeight: 600 } : undefined}
                          title={chainIdx > 0 ? `Резерв #${chainIdx + 1} в Failover. Нажмите для снятия` : 'Добавить резервом (в конец Failover)'}
                        >
                          <span>{chainIdx > 0 ? `⚡#${chainIdx + 1} Резерв` : '⚡↓ В конец'}</span>
                        </button>
                      </>
                    )
                  })()}
                  <button
                    type="button"
                    className={`btn sm btn-whitelist ${activeServer.is_whitelist ? 'is-whitelist' : 'ghost'}`}
                    onClick={() => toggleWhitelist(activeServer.id)}
                    title={activeServer.is_whitelist ? 'Удалить из белого списка' : 'Включить в белый список (аварийный туннель)'}
                  >
                    <span style={{ fontSize: 11 }}>{activeServer.is_whitelist ? '🟣' : '⚪'}</span>
                    <span style={{ fontSize: 11, fontWeight: 600 }}>{activeServer.is_whitelist ? 'В белом списке' : 'Белый список'}</span>
                  </button>
                  {activeServer.is_whitelist && (
                    <button
                      type="button"
                      className="btn sm ghost"
                      onClick={() => makeServerNormal(activeServer.id)}
                      title="Сделать узел обычным: перекрыть тег БС и убрать из аварийного туннеля"
                    >
                      <span style={{ fontSize: 11 }}>Сделать обычным</span>
                    </button>
                  )}
                </div>
              </div>
            </>
          ) : (
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', minHeight: 90 }} className="muted">
              Нет подключенного сервера
            </div>
          )}
        </div>

        {/* Поиск, фильтры и управление */}
        <div className="servers-controls-deck-card">
          {/* Строка поиска + счетчик */}
          <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
            <div style={{ position: 'relative', flex: 1 }}>
              <input
                ref={searchInputRef}
                className="input"
                style={{ width: '100%', height: 34, paddingRight: 40, fontSize: 13 }}
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
                    fontSize: 13,
                  }}
                  title="Очистить поиск"
                >
                  ✕
                </button>
              )}
            </div>
            <span className="muted" style={{ fontWeight: 600, fontSize: 12, whiteSpace: 'nowrap' }}>
              {filtered.length} шт.
            </span>
          </div>

          {/* Строка сортировки, фильтра скрытия оффлайна и переключателей вида */}
          <div style={{ display: 'flex', gap: 8, alignItems: 'center', flexWrap: 'wrap' }}>
            {/* Чип "⚡ Скрыть оффлайн" */}
            <button
              type="button"
              className={`btn sm ${hideOffline ? 'primary' : 'ghost'}`}
              style={{
                height: 32,
                fontSize: 12,
                borderRadius: 6,
                fontWeight: 600,
                border: hideOffline ? '1px solid var(--accent)' : '1px solid var(--border)',
              }}
              onClick={() => {
                const next = !hideOffline
                setHideOffline(next)
                localStorage.setItem('xr_servers_hide_offline', String(next))
                setLimit(PAGE)
              }}
              title="Скрыть серверы с таймаутом (пинг 0 или 9999 мс)"
            >
              ⚡ Скрыть оффлайн
            </button>

            {/* Выпадающий список сортировки */}
            <select
              className="select"
              value={sortMode}
              onChange={(e) => {
                setSortMode(e.target.value as any)
                setLimit(PAGE)
              }}
              style={{ height: 32, fontSize: 12, padding: '2px 8px', flex: '1 1 140px' }}
            >
              <option value="default">↕ По умолчанию</option>
              <option value="ping_asc">⚡ Пинг: по возрастанию</option>
              <option value="ping_desc">💤 Пинг: по убыванию</option>
              <option value="name_asc">🔤 По имени (А-Я)</option>
              <option value="provider_asc">📦 По подписке / провайдеру</option>
              <option value="priority_first">★ Приоритетные первыми</option>
            </select>

            {/* Мультивыбор подписок */}
            {providers.length > 0 && (
              <div className="subscription-dropdown" ref={subDropdownRef}>
                <button
                  type="button"
                  className="subscription-dropdown-btn"
                  style={{ height: 32, fontSize: 12, padding: '4px 10px' }}
                  onClick={() => setSubDropdownOpen((prev) => !prev)}
                  title="Фильтр по подпискам"
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
                          <span style={{ flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', opacity: p.enabled === false ? 0.6 : 1 }}>
                            {p.name} {p.enabled === false && <span style={{ fontSize: 10, color: 'var(--muted)' }}>(откл.)</span>}
                          </span>
                          <span className="sub-count-badge">({p.count})</span>
                        </div>
                      )
                    })}
                  </div>
                )}
              </div>
            )}

            {/* Режим отображения */}
            <div className="view-toggle" style={{ height: 32 }} title="Режим отображения">
              <button
                type="button"
                className={`view-toggle-btn ${viewMode === 'detailed' ? 'active' : ''}`}
                style={{ padding: '0 8px', fontSize: 12 }}
                onClick={() => setAndSaveViewMode('detailed')}
              >
                ☷ Карточки
              </button>
              <button
                type="button"
                className={`view-toggle-btn ${viewMode === 'compact' ? 'active' : ''}`}
                style={{ padding: '0 8px', fontSize: 12 }}
                onClick={() => setAndSaveViewMode('compact')}
              >
                ☰ Компактно
              </button>
            </div>

            {/* Столбцы */}
            <div className="view-toggle cols-toggle" style={{ height: 32 }} title="Столбцы">
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
                  style={{ padding: '0 6px', fontSize: 11 }}
                  onClick={() => setAndSaveColumns(col.id)}
                >
                  {col.label}
                </button>
              ))}
            </div>
          </div>

          {/* Строка дополнительных кнопок (Пинг, Обновить, Подписки, Импорт, Игнор, Меню) */}
          <div style={{ display: 'flex', gap: 6, alignItems: 'center', flexWrap: 'wrap' }}>
            <button className="btn sm" onClick={pingAll} disabled={pinging} title="Измерить пинг всех">
              {pinging ? '📡 Пинг…' : '📡 Пинг всех'}
            </button>
            {lastPingTime && (
              <span
                style={{
                  fontSize: 11,
                  color: 'var(--muted)',
                  background: 'rgba(255, 255, 255, 0.04)',
                  padding: '3px 8px',
                  borderRadius: 6,
                  border: '1px solid rgba(255, 255, 255, 0.06)',
                  whiteSpace: 'nowrap',
                  display: 'inline-flex',
                  alignItems: 'center',
                  gap: 4,
                }}
                title="Время последней проверки пинга серверов"
              >
                ⏱ Проверен: <b style={{ color: 'var(--text)' }}>{lastPingTime}</b>
              </span>
            )}
            <button className="btn sm" onClick={load} title="Обновить список">
              🔄 Обновить
            </button>
            <button className="btn sm" onClick={() => setGeneratorOpen(true)} title="Импорт ссылок">
              📥 Импорт
            </button>
            <button className="btn sm" onClick={openProvidersModal} title="Управление подписками">
              📦 Подписки{providersLoaded ? ` (${providers.length})` : ''}{providersError ? ' ⚠' : ''}
            </button>
            <button className="btn sm" onClick={openIgnore} title="Игнор-лист">
              🚫 Игнор{ignored.size > 0 ? ` (${ignored.size})` : ''}
            </button>
            <button
              type="button"
              className={`btn sm ${flowOnly ? 'primary' : ''}`}
              onClick={() => {
                setFlowOnly((prev) => !prev)
                if (!flowOnly) setPoolOnly(false)
                setLimit(PAGE)
              }}
              title="Google Flow совместимые"
            >
              ✨ Flow ({flowCount})
            </button>
            {poolsCount > 0 && (
              <button
                type="button"
                className={`btn sm ${showPools ? 'primary' : 'ghost'}`}
                onClick={() => {
                  setShowPools((prev) => !prev)
                  setLimit(PAGE)
                }}
                title={showPools ? 'Скрыть пулы серверов' : 'Показать пулы серверов в общем списке'}
              >
                🔀 Пулы ({poolsCount}) {showPools ? '▼' : '▶'}
              </button>
            )}
            {flowOnly && (
              <button
                type="button"
                className="btn sm"
                style={{
                  background: 'linear-gradient(135deg, #ec4899 0%, #8b5cf6 100%)',
                  border: 'none',
                  color: '#fff',
                  fontWeight: 600,
                }}
                onClick={() => setFlowRepairOpen(true)}
              >
                🔓 Анлок Flow
              </button>
            )}

            {/* ⋯ Меню */}
            <div style={{ position: 'relative' }} ref={moreMenuRef}>
              <button
                type="button"
                className="btn sm ghost"
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
                  >
                    {fixing ? 'Исправление…' : '🩹 Исправить битые имена'}
                  </button>
                </div>
              )}
            </div>
          </div>
        </div>
      </div>

      {/* ОСНОВНОЙ СПИСОК СЕРВЕРОВ: на ВСЮ ширину страницы (полноценные 3-4 колонки) */}
      <div className="servers-main-content">
        {loading ? (
        /* Скелетоны при загрузке (Skeleton Shimmers) */
        <div className={`server-list cols-${columns}`} style={{ marginTop: 12 }}>
          {Array.from({ length: 8 }).map((_, idx) => (
            <div
              key={idx}
              className="server-card skeleton-shimmer"
              style={{
                minHeight: 110,
                display: 'flex',
                flexDirection: 'column',
                justifyContent: 'space-between',
                padding: '12px 14px',
                border: '1px solid rgba(255, 255, 255, 0.06)',
              }}
            >
              <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                <div style={{ width: 60, height: 16, borderRadius: 4, background: 'rgba(255, 255, 255, 0.06)' }} />
                <div style={{ width: 50, height: 16, borderRadius: 4, background: 'rgba(255, 255, 255, 0.06)' }} />
              </div>
              <div style={{ width: '70%', height: 18, borderRadius: 4, background: 'rgba(255, 255, 255, 0.08)', margin: '10px 0' }} />
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                <div style={{ width: 80, height: 24, borderRadius: 6, background: 'rgba(255, 255, 255, 0.06)' }} />
                <div style={{ display: 'flex', gap: 6 }}>
                  <div style={{ width: 24, height: 24, borderRadius: 6, background: 'rgba(255, 255, 255, 0.05)' }} />
                  <div style={{ width: 24, height: 24, borderRadius: 6, background: 'rgba(255, 255, 255, 0.05)' }} />
                </div>
              </div>
            </div>
          ))}
        </div>
      ) : (
        <div>
          <div className="pinned-divider" style={{ margin: '14px 0 12px' }}>
            <span>Список серверов ({filtered.length})</span>
          </div>

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
                      <span
                        className="badge protocol-badge"
                        style={s.protocol === 'VK-CALL' ? {
                          background: 'rgba(34, 197, 94, 0.16)',
                          border: '1px solid rgba(34, 197, 94, 0.42)',
                          color: '#4ade80',
                        } : undefined}
                      >{s.protocol}</span>
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
                      {(s.is_whitelist || status?.whitelist_servers?.includes(s.id)) && (
                        <span className="badge" style={{ background: 'rgba(168, 85, 247, 0.2)', color: '#c084fc', border: '1px solid rgba(168, 85, 247, 0.4)', fontSize: 10 }}>
                          🟣 Белый список
                        </span>
                      )}
                      {ignored.has(s.id) && (
                        <span className="tag ignored" title="Сервер исключён из авто-выбора (Fastest / Fallback)">
                          🚫 Исключён
                        </span>
                      )}
                    </div>

                    <div className="server-card-metrics">
                      {getFlowStatus(s) === 'ok' ? (
                        <span className="badge badge-flow-ok" title="Сервер подходит для Google Flow и Gemini Labs">
                          🟢 Flow OK
                        </span>
                      ) : getFlowStatus(s) === 'blocked' ? (
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
                      <Sparkline ping={s.ping_ms} />
                    </div>
                  </div>

                  {/* 2. Средняя строка: Название сервера или inline-редактирование псевдонима */}
                  <div className="server-card-main">
                    {editingAliasId === s.id ? (
                      <div style={{ display: 'flex', gap: 6, alignItems: 'center', width: '100%', margin: '4px 0' }}>
                        <input
                          className="input sm"
                          style={{ flex: 1, fontSize: 13 }}
                          value={aliasInput}
                          onChange={(e) => setAliasInput(e.target.value)}
                          placeholder="Пользовательское имя (или пусто для сброса)"
                          autoFocus
                          onKeyDown={(e) => {
                            if (e.key === 'Enter') saveAlias(s.id)
                            if (e.key === 'Escape') setEditingAliasId(null)
                          }}
                        />
                        <button className="btn sm primary" onClick={() => saveAlias(s.id)} disabled={aliasSaving}>
                          ✓
                        </button>
                        <button className="btn sm ghost" onClick={() => setEditingAliasId(null)}>
                          ✕
                        </button>
                      </div>
                    ) : (
                      <div className="server-card-name" style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                        <span title={`${s.name}\nХост: ${s.host}${s.port ? `:${s.port}` : ''}\nПротокол: ${s.protocol}`}>
                          {s.custom_alias ? `${s.custom_alias} (${s.name})` : s.name}
                        </span>
                        <button
                          type="button"
                          className="btn sm ghost"
                          style={{ padding: '0 4px', fontSize: 11, height: 20, opacity: 0.6 }}
                          onClick={(e) => {
                            e.stopPropagation()
                            setEditingAliasId(s.id)
                            setAliasInput(s.custom_alias || '')
                          }}
                          title="Задать псевдоним (пользовательское имя)"
                        >
                          ✏️
                        </button>
                        {s.custom_alias && (
                          <button
                            type="button"
                            className="btn sm ghost"
                            style={{ padding: '0 4px', fontSize: 11, height: 20, opacity: 0.6, color: 'var(--text-danger, #ef4444)' }}
                            onClick={(e) => {
                              e.stopPropagation()
                              saveAliasWithVal(s.id, '')
                            }}
                            title="Сбросить пользовательский псевдоним"
                          >
                            ✕
                          </button>
                        )}
                      </div>
                    )}
                  </div>

                  {/* 3. Нижняя строка: Кнопки действий без разрыва строк */}
                  <div className="server-card-actions">
                    <div className="server-card-action-primary">
                      {s.is_active ? (
                        <span className="tag current-active-badge">
                          <span>✓</span>
                          <span className="btn-label-optional">ПОДКЛЮЧЁН</span>
                        </span>
                      ) : (
                        <button
                          type="button"
                          className="btn sm btn-connect"
                          onClick={() => activate(s)}
                          title="Сделать этот сервер активным"
                        >
                          <span>🔌</span>
                          <span className="btn-label-optional">Подключить</span>
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
                        <span className="btn-label-optional">{speedtestingId === s.id ? 'Замер…' : 'Скорость'}</span>
                      </button>

                      {s.protocol === 'VK-CALL' ? (
                        <button
                          type="button"
                          className="btn sm ghost"
                          onClick={() => window.dispatchEvent(new CustomEvent('xr:switch-tab', { detail: 'vkturn' }))}
                          title="Открыть настройки VK-туннеля"
                        >
                          <span>📞</span>
                          <span className="btn-label-optional">Настройки ВК</span>
                        </button>
                      ) : (
                        <button
                          type="button"
                          className="btn sm ghost"
                          onClick={() => setShareServer(s)}
                          title="Сгенерировать ссылку подключения (vless/vmess/ss) и QR-код"
                        >
                          <span>🔗</span>
                          <span className="btn-label-optional">Ссылка</span>
                        </button>
                      )}

                      {(() => {
                        const chainIdx = failoverChain.indexOf(s.id)
                        const isStart = chainIdx === 0

                        return (
                          <>
                            <button
                              type="button"
                              className={`btn sm ghost ${isStart ? 'is-priority' : ''}`}
                              onClick={() => addToFailover(s.id, 'start')}
                              style={isStart ? { background: 'rgba(56, 189, 248, 0.2)', color: '#38bdf8', borderColor: '#38bdf8', fontWeight: 700 } : undefined}
                              title={isStart ? 'Основной узел Failover (#1). Нажмите для снятия' : 'Сделать основным узлом (в начало Failover)'}
                            >
                              <span>{isStart ? '👑' : '⚡↑'}</span>
                              <span className="btn-label-optional">{isStart ? 'Основной' : 'В начало'}</span>
                            </button>
                            <button
                              type="button"
                              className={`btn sm ghost ${chainIdx > 0 ? 'is-priority' : ''}`}
                              onClick={() => addToFailover(s.id, 'end')}
                              style={chainIdx > 0 ? { background: 'rgba(245, 158, 11, 0.15)', color: '#fbbf24', borderColor: '#fbbf24', fontWeight: 600 } : undefined}
                              title={chainIdx > 0 ? `Резерв #${chainIdx + 1} в Failover. Нажмите для снятия` : 'Добавить резервом (в конец Failover)'}
                            >
                              <span>{chainIdx > 0 ? `⚡#${chainIdx + 1}` : '⚡↓'}</span>
                              <span className="btn-label-optional">{chainIdx > 0 ? 'Резерв' : 'В конец'}</span>
                            </button>
                          </>
                        )
                      })()}

                      <button
                        type="button"
                        className={`btn sm btn-whitelist ${s.is_whitelist ? 'is-whitelist' : 'ghost'}`}
                        onClick={() => toggleWhitelist(s.id)}
                        title={s.is_whitelist ? 'Удалить из белого списка (аварийный туннель)' : 'Включить в белый список (аварийный 100% туннель)'}
                      >
                        <span style={{ fontSize: 11 }}>{s.is_whitelist ? '🟣' : '⚪'}</span>
                        <span className="btn-label-full">{s.is_whitelist ? 'В белом списке' : 'Белый список'}</span>
                        <span className="btn-label-short">{s.is_whitelist ? 'БС ON' : 'БС'}</span>
                      </button>
                      {s.is_whitelist && (
                        <button
                          type="button"
                          className="btn sm ghost"
                          onClick={() => makeServerNormal(s.id)}
                          title="Сделать узел обычным: перекрыть тег БС и убрать из аварийного туннеля"
                        >
                          <span className="btn-label-full">Сделать обычным</span>
                          <span className="btn-label-short">Обычный</span>
                        </button>
                      )}
                    </div>
                  </div>
                </div>
              ))}
              {filtered.length > limit && (
                <button
                  className="btn wide"
                  style={{ gridColumn: '1 / -1', marginTop: 12, padding: '10px 16px', fontWeight: 600 }}
                  onClick={() => setLimit(limit + PAGE)}
                >
                  Показать ещё {PAGE} (осталось {filtered.length - limit})
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
                  {(s.is_whitelist || status?.whitelist_servers?.includes(s.id)) && (
                    <span className="badge" style={{ background: 'rgba(168, 85, 247, 0.2)', color: '#c084fc', border: '1px solid rgba(168, 85, 247, 0.4)', fontSize: 9, padding: '1px 4px' }}>
                      🟣
                    </span>
                  )}
                  {editingAliasId === s.id ? (
                    <div style={{ display: 'inline-flex', gap: 4, alignItems: 'center', flex: 1 }}>
                      <input
                        className="input sm"
                        style={{ height: 24, fontSize: 12 }}
                        value={aliasInput}
                        onChange={(e) => setAliasInput(e.target.value)}
                        placeholder="Псевдоним"
                        autoFocus
                        onKeyDown={(e) => {
                          if (e.key === 'Enter') saveAlias(s.id)
                          if (e.key === 'Escape') setEditingAliasId(null)
                        }}
                      />
                      <button className="btn sm primary" style={{ padding: '0 6px', height: 24 }} onClick={() => saveAlias(s.id)}>✓</button>
                      <button className="btn sm ghost" style={{ padding: '0 6px', height: 24 }} onClick={() => setEditingAliasId(null)}>✕</button>
                    </div>
                  ) : (
                    <span
                      className="server-compact-name"
                      title={`${s.name}\nХост: ${s.host}${s.port ? `:${s.port}` : ''}`}
                      style={{ display: 'inline-flex', alignItems: 'center', gap: 4 }}
                    >
                      <span>{s.custom_alias ? `${s.custom_alias} (${s.name})` : s.name}</span>
                      <button
                        type="button"
                        className="btn sm ghost"
                        style={{ padding: '0 2px', fontSize: 10, height: 18, opacity: 0.5 }}
                        onClick={() => {
                          setEditingAliasId(s.id)
                          setAliasInput(s.custom_alias || '')
                        }}
                        title="Переименовать"
                      >
                        ✏️
                      </button>
                      {s.custom_alias && (
                        <button
                          type="button"
                          className="btn sm ghost"
                          style={{ padding: '0 2px', fontSize: 10, height: 18, opacity: 0.5, color: 'var(--text-danger, #ef4444)' }}
                          onClick={() => saveAliasWithVal(s.id, '')}
                          title="Сбросить псевдоним"
                        >
                          ✕
                        </button>
                      )}
                    </span>
                  )}
                  {getFlowStatus(s) === 'ok' ? (
                    <span className="badge badge-flow-ok" style={{ fontSize: 10, padding: '1px 5px' }} title="Подходит для Google Flow">
                      🟢 Flow
                    </span>
                  ) : getFlowStatus(s) === 'blocked' ? (
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
                  <Sparkline ping={s.ping_ms} />
                  <div style={{ display: 'inline-flex', gap: 6, alignItems: 'center' }}>
                    {s.protocol === 'VK-CALL' ? (
                      <button
                        className="btn sm ghost"
                        onClick={() => window.dispatchEvent(new CustomEvent('xr:switch-tab', { detail: 'vkturn' }))}
                        style={{ padding: '3px 6px' }}
                        title="Открыть настройки VK-туннеля"
                      >
                        📞
                      </button>
                    ) : (
                      <button
                        className="btn sm ghost"
                        onClick={() => setShareServer(s)}
                        style={{ padding: '3px 6px' }}
                        title="Ссылка подключения и QR-код"
                      >
                        🔗
                      </button>
                    )}
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
                    {(() => {
                      const chainIdx = failoverChain.indexOf(s.id)
                      const isStart = chainIdx === 0
                      return (
                        <>
                          <button
                            type="button"
                            className={`btn sm ghost ${isStart ? 'is-priority' : ''}`}
                            onClick={() => addToFailover(s.id, 'start')}
                            style={{
                              padding: '3px 6px',
                              ...(isStart ? { background: 'rgba(56, 189, 248, 0.2)', color: '#38bdf8', borderColor: '#38bdf8', fontWeight: 700 } : {}),
                            }}
                            title={isStart ? 'Основной узел Failover (#1). Нажмите для снятия' : 'В начало Failover (основной)'}
                          >
                            {isStart ? '👑' : '⚡↑'}
                          </button>
                          <button
                            type="button"
                            className={`btn sm ghost ${chainIdx > 0 ? 'is-priority' : ''}`}
                            onClick={() => addToFailover(s.id, 'end')}
                            style={{
                              padding: '3px 6px',
                              ...(chainIdx > 0 ? { background: 'rgba(245, 158, 11, 0.15)', color: '#fbbf24', borderColor: '#fbbf24', fontWeight: 600 } : {}),
                            }}
                            title={chainIdx > 0 ? `Резерв #${chainIdx + 1} в Failover. Нажмите для снятия` : 'В конец Failover (резерв)'}
                          >
                            {chainIdx > 0 ? `⚡#${chainIdx + 1}` : '⚡↓'}
                          </button>
                        </>
                      )
                    })()}
                    <button
                      type="button"
                      className={`btn sm btn-whitelist ${s.is_whitelist ? 'is-whitelist' : 'ghost'}`}
                      onClick={() => toggleWhitelist(s.id)}
                      style={{ padding: '3px 8px' }}
                      title={s.is_whitelist ? 'Удалить из белого списка' : 'В белый список (аварийный туннель)'}
                    >
                      <span style={{ fontSize: 11 }}>{s.is_whitelist ? '🟣' : '⚪'}</span>
                      <span style={{ fontSize: 11, fontWeight: 600 }}>{s.is_whitelist ? 'БС ON' : 'БС'}</span>
                    </button>
                    {s.is_whitelist && (
                      <button
                        type="button"
                        className="btn sm ghost"
                        onClick={() => makeServerNormal(s.id)}
                        style={{ padding: '3px 8px' }}
                        title="Сделать узел обычным: перекрыть тег БС и убрать из аварийного туннеля"
                      >
                        <span style={{ fontSize: 11 }}>Обычный</span>
                      </button>
                    )}
                  </div>
                </div>
              ))}
              {filtered.length > limit && (
                <button
                  className="btn wide"
                  style={{ gridColumn: '1 / -1', marginTop: 12, padding: '10px 16px', fontWeight: 600 }}
                  onClick={() => setLimit(limit + PAGE)}
                >
                  Показать ещё {PAGE} (осталось {filtered.length - limit})
                </button>
              )}
            </div>
          )}
        </div>
      )}
      </div>

      {/* МОДАЛЬНОЕ ОКНО ИГНОР-ЛИСТА */}
      <IgnoreListModal
        ignoreOpen={ignoreOpen}
        setIgnoreOpen={setIgnoreOpen}
        providers={providers}
        providerColorMap={providerColorMap}
        ignoreCandidates={ignoreCandidates}
        filteredIgnoreCandidates={filteredIgnoreCandidates}
        ignoreDraft={ignoreDraft}
        setIgnoreDraft={setIgnoreDraft}
        initialIgnored={initialIgnored}
        ignoreSearch={ignoreSearch}
        setIgnoreSearch={setIgnoreSearch}
        ignoreProviderFilter={ignoreProviderFilter}
        setIgnoreProviderFilter={setIgnoreProviderFilter}
        ignoreSaving={ignoreSaving}
        toggleIgnoreDraft={toggleIgnoreDraft}
        selectAllFilteredIgnore={selectAllFilteredIgnore}
        unselectAllFilteredIgnore={unselectAllFilteredIgnore}
        saveIgnore={saveIgnore}
      />

      {/* МОДАЛЬНОЕ ОКНО УПРАВЛЕНИЯ ПОДПИСКАМИ */}
      <SubscriptionsModal
        providersOpen={providersOpen}
        setProvidersOpen={setProvidersOpen}
        providers={providers}
        providersLoading={providersLoading}
        providersLoaded={providersLoaded}
        providersError={providersError}
        confirmDeleteSub={confirmDeleteSub}
        setConfirmDeleteSub={setConfirmDeleteSub}
        deletingSub={deletingSub}
        editingAliases={editingAliases}
        setEditingAliases={setEditingAliases}
        togglingProvider={togglingProvider}
        updatingProvider={updatingProvider}
        updatingAllProviders={updatingAllProviders}
        urlVisibility={urlVisibility}
        setUrlVisibility={setUrlVisibility}
        copiedUrlId={copiedUrlId}
        editingProviderNameId={editingProviderNameId}
        setEditingProviderNameId={setEditingProviderNameId}
        editingHwidId={editingHwidId}
        setEditingHwidId={setEditingHwidId}
        hwidInput={hwidInput}
        setHwidInput={setHwidInput}
        copiedHwidId={copiedHwidId}
        savingHwidId={savingHwidId}
        newSubOpen={newSubOpen}
        setNewSubOpen={setNewSubOpen}
        newSubId={newSubId}
        setNewSubId={setNewSubId}
        newSubUrl={newSubUrl}
        setNewSubUrl={setNewSubUrl}
        newSubName={newSubName}
        setNewSubName={setNewSubName}
        newSubHwid={newSubHwid}
        setNewSubHwid={setNewSubHwid}
        newSubAppendHwid={newSubAppendHwid}
        setNewSubAppendHwid={setNewSubAppendHwid}
        addingSub={addingSub}
        providerColorMap={providerColorMap}
        togglingWhitelistSub={togglingWhitelistSub}
        loadProviders={loadProviders}
        saveProviderAlias={saveProviderAlias}
        toggleProviderEnable={toggleProviderEnable}
        toggleProviderWhitelist={toggleProviderWhitelist}
        updateProviderNow={updateProviderNow}
        updateAllProvidersNow={updateAllProvidersNow}
        copySubUrl={copySubUrl}
        generateHwid={generateHwid}
        handleAddSubscription={handleAddSubscription}
        handleSaveHwid={handleSaveHwid}
        handleCopyHwid={handleCopyHwid}
        handleDeleteSubscription={handleDeleteSubscription}
        executeDeleteSubscription={executeDeleteSubscription}
      />

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

      {/* Модальное окно починки Google Flow */}
      <FlowRepairModal
        isOpen={flowRepairOpen}
        onClose={() => setFlowRepairOpen(false)}
        notify={notify}
        onRepaired={() => load()}
      />
    </section>
  )
}
