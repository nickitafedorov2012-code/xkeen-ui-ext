import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { apiGet, apiPost } from '../api'
import { getFlowStatus, type ProviderInfo, type ServerInfo, type SpeedtestResult, type StatusInfo } from '../types'
import OutboundGeneratorModal from './OutboundGeneratorModal'
import ShareNodeModal from './ShareNodeModal'
import FlowRepairModal from './FlowRepairModal'
import { copyToClipboard } from '../utils/clipboard'
import { loadServersColumns, saveServersColumns } from '../utils/serversLayout'
import { calcJitter, loadServerPingHistory, saveServerPingHistory } from '../utils/jitter'
import { PROVIDER_COLORS, isPoolServer } from './servers/serversHelpers'
import { IgnoreListModal } from './servers/IgnoreListModal'
import { SubscriptionsModal } from './servers/SubscriptionsModal'
import { ActiveServerBanner } from './servers/ActiveServerBanner'
import { ServerList } from './servers/ServerList'

export { formatQuotaBytes, isPoolServer } from './servers/serversHelpers'
interface Props {
  notify: (msg: string, isError?: boolean) => void
  status?: StatusInfo | null
  refresh?: () => void
}

const PAGE = 20

export function mergePolledServerInfo(prev: ServerInfo, fresh: ServerInfo): ServerInfo {
  return {
    ...prev,
    name: fresh.name,
    host: fresh.host,
    resolved_ip: fresh.resolved_ip,
    is_active: fresh.is_active,
    now_node: fresh.now_node,
    ping_ms: fresh.ping_ms,
  }
}

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
  const [columns, setColumns] = useState<number>(loadServersColumns)

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
    saveServersColumns(cols)
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
      setServers(Array.isArray(data?.servers) ? data.servers : [])
      setIgnored(new Set(Array.isArray(ig?.servers) ? ig.servers : []))
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка загрузки серверов', true)
    } finally {
      setLoading(false)
    }
  }, [loadProviders, notify])

  const pingAll = useCallback(async () => {
    setPinging(true)
    try {
      const data = await apiPost<{ pings?: Record<string, number> }>('servers/ping', {
        include_pools: showPools,
      })
      const pingsMap = data?.pings && typeof data.pings === 'object' ? data.pings : {}
      setServers((prev) => prev.map((s) => ({ ...s, ping_ms: pingsMap[s.id] ?? s.ping_ms })))
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
  }, [notify, showPools])

  useEffect(() => {
    load()
    const onSync = () => load()
    window.addEventListener('xr:data-changed', onSync)
    window.addEventListener('xr:refresh-status', onSync)
    // Фоновое обновление статуса пулов и серверов каждые 3 секунды для мгновенной реактивности без F5
    const bgTimer = setInterval(() => {
      apiGet<{ servers: ServerInfo[] }>('servers')
        .then((data) => {
          if (Array.isArray(data?.servers)) {
            setServers((prev) => {
              if (prev.length !== data.servers.length) {
                return data.servers
              }
              return prev.map((s) => {
                const fresh = data.servers.find((f) => f.id === s.id)
                if (!fresh) return s
                return mergePolledServerInfo(s, fresh)
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

  const jitterVal = useMemo(() => calcJitter(pingHistory), [pingHistory])

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
      const isFreeSubCard = s.protocol === 'FREE-VPN' || s.protocol === 'FREE-WL'
      if (hideOffline && !isFreeSubCard && !s.is_active && (s.ping_ms <= 0 || s.ping_ms >= 9999)) {
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
      let stored = loadServerPingHistory(sId)

      if (isValidPing) {
        if (stored.length === 0 || stored[stored.length - 1] !== ping) {
          stored = [...stored, ping].slice(-15)
          saveServerPingHistory(sId, stored)
        }
      }
      setPingHistory(stored)
    } else if (isValidPing) {
      setPingHistory((prev) => {
        if (prev.length > 0 && prev[prev.length - 1] === ping) {
          return prev
        }
        const next = [...prev, ping].slice(-15)
        saveServerPingHistory(sId, next)
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
        currentChain = Array.isArray(st?.failover?.priority_chain)
          ? st.failover.priority_chain
          : (st?.failover?.priority_server ? [st.failover.priority_server] : [])
      } catch {
        currentChain = Array.isArray(status?.failover?.priority_chain)
          ? status.failover.priority_chain
          : (status?.failover?.priority_server ? [status.failover.priority_server] : [])
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
        data?.fixed > 0
          ? `Исправлено имён: ${data.fixed}${Array.isArray(data?.names) && data.names.length ? ` — ${data.names.join('; ')}` : ''}`
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
        <ActiveServerBanner
          activeServer={activeServer ?? null}
          status={status}
          providers={providers}
          providerColorMap={providerColorMap}
          failoverChain={failoverChain}
          pingHistory={pingHistory}
          jitterVal={jitterVal}
          jitterMeasuring={jitterMeasuring}
          liveTraffic={liveTraffic}
          speedResults={speedResults}
          speedtestingId={speedtestingId}
          editingAliasId={editingAliasId}
          aliasInput={aliasInput}
          aliasSaving={aliasSaving}
          setEditingAliasId={setEditingAliasId}
          setAliasInput={setAliasInput}
          setShareServer={setShareServer}
          openProvidersModal={openProvidersModal}
          saveAlias={saveAlias}
          saveAliasWithVal={saveAliasWithVal}
          runSpeedtest={runSpeedtest}
          measureLiveJitter={measureLiveJitter}
          toggleWhitelist={toggleWhitelist}
          makeServerNormal={makeServerNormal}
          addToFailover={addToFailover}
        />

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
            {(!servers.some((srv) => srv.id === 'free_vpn') || !servers.some((srv) => srv.id === 'free_wl')) && (
              <button
                type="button"
                className="btn sm primary"
                data-testid="add-free-subs-btn"
                onClick={async () => {
                  try {
                    await apiPost('providers/free-sub/enable', {})
                    notify('Бесплатные подписки (VPN и Обход БС) добавлены в список серверов')
                    await load()
                    refresh?.()
                  } catch (e) {
                    notify(`Ошибка добавления бесплатных подписок: ${e instanceof Error ? e.message : String(e)}`, true)
                  }
                }}
                title="В 1 клик добавить карточки Бесплатный VPN (Авто) и Бесплатный обход БС (Авто)"
              >
                🆓 Добавить бесплатные подписки
              </button>
            )}
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
      <ServerList
        loading={loading}
        columns={columns}
        viewMode={viewMode}
        filtered={filtered}
        limit={limit}
        pageSize={PAGE}
        setLimit={setLimit}
        status={status}
        providerColorMap={providerColorMap}
        failoverChain={failoverChain}
        ignored={ignored}
        speedResults={speedResults}
        speedtestingId={speedtestingId}
        editingAliasId={editingAliasId}
        aliasInput={aliasInput}
        aliasSaving={aliasSaving}
        setEditingAliasId={setEditingAliasId}
        setAliasInput={setAliasInput}
        setShareServer={setShareServer}
        openProvidersModal={openProvidersModal}
        saveAlias={saveAlias}
        saveAliasWithVal={saveAliasWithVal}
        activate={activate}
        runSpeedtest={runSpeedtest}
        toggleWhitelist={toggleWhitelist}
        makeServerNormal={makeServerNormal}
        addToFailover={addToFailover}
        notify={notify}
        onFreeSubChanged={() => {
          void load()
          refresh?.()
        }}
      />

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
