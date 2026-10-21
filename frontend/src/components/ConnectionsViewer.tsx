import { useEffect, useState, useRef, useCallback, useMemo } from 'react'
import { apiGet, apiPost, apiDelete } from '../api'
import Diagnostics from './Diagnostics'

export interface ConnectionMetadata {
  network: string
  type: string
  sourceIP: string
  sourcePort: string
  destinationIP: string
  destinationPort: string
  host: string
  dnsMode?: string
  processPath?: string
}

export interface ConnectionItem {
  id: string
  metadata: ConnectionMetadata
  upload: number
  download: number
  start: string
  chains: string[]
  rule: string
  rulePayload: string
  isClosed?: boolean
  closedAt?: number
}

interface ConnectionsResponse {
  downloadTotal: number
  uploadTotal: number
  connections: ConnectionItem[]
}

interface ConnectionsViewerProps {
  notify: (msg: string, error?: boolean) => void
}

interface ForceProxyModalState {
  isOpen: boolean
  connectionId?: string
  rawHost: string
  targetDomain: string
  rootDomain: string
  fullHost: string
  clientIp: string
  clientName?: string
  scope: 'global' | 'device'
  closeCurrent: boolean
  submitting: boolean
}

function formatBytes(bytes: number): string {
  if (!bytes || bytes < 0) return '0 B'
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(2)} MB`
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`
}

/**
 * Очистка и извлечение корневого и полного домена из хоста соединения
 */
export function extractCleanDomain(rawHost: string): { host: string; rootDomain: string; isIp: boolean } {
  if (!rawHost) return { host: '', rootDomain: '', isIp: false }
  const clean = rawHost.split(':')[0].trim().toLowerCase()
  const isIp = /^(\d{1,3}\.){3}\d{1,3}$/.test(clean) || clean.includes(':')
  if (isIp) {
    return { host: clean, rootDomain: clean, isIp: true }
  }

  const parts = clean.split('.')
  if (parts.length > 2) {
    const lastTwo = parts.slice(-2).join('.')
    const knownTwoPartTlds = [
      'co.uk', 'com.ru', 'org.ru', 'net.ru', 'msk.ru', 'spb.ru', 'gov.ru', 'edu.ru',
      'com.ua', 'net.ua', 'org.ua', 'com.tr', 'co.il', 'com.br', 'co.jp'
    ]
    if (knownTwoPartTlds.includes(lastTwo) && parts.length > 2) {
      return { host: clean, rootDomain: parts.slice(-3).join('.'), isIp: false }
    }
    return { host: clean, rootDomain: parts.slice(-2).join('.'), isIp: false }
  }

  return { host: clean, rootDomain: clean, isIp: false }
}

export function detectAppFromConn(c: ConnectionItem): { name: string; icon: string; category: string } {
  const host = (c.metadata.host || c.metadata.destinationIP || '').toLowerCase()
  const proc = (c.metadata.processPath || '').toLowerCase()

  if (proc.includes('telegram') || host.includes('telegram') || host.includes('t.me') || host.includes('telesco.pe')) {
    return { name: 'Telegram', icon: '✈️', category: 'Мессенджеры' }
  }
  if (proc.includes('discord') || host.includes('discord') || host.includes('discordapp') || host.includes('discord.gg')) {
    return { name: 'Discord', icon: '💬', category: 'Мессенджеры' }
  }
  if (proc.includes('youtube') || host.includes('youtube') || host.includes('googlevideo') || host.includes('ytimg')) {
    return { name: 'YouTube', icon: '▶️', category: 'Медиа & Видео' }
  }
  if (host.includes('openai') || host.includes('chatgpt') || host.includes('anthropic') || host.includes('claude') || host.includes('gemini') || host.includes('generativelanguage')) {
    return { name: 'AI / ChatGPT & Claude', icon: '🤖', category: 'Искусственный интеллект' }
  }
  if (proc.includes('steam') || host.includes('steam') || host.includes('valve') || host.includes('epicgames') || host.includes('riotgames') || host.includes('playstation') || host.includes('xbox')) {
    return { name: 'Игры & Платформы', icon: '🎮', category: 'Гейминг' }
  }
  if (proc.includes('github') || host.includes('github') || host.includes('gitlab') || host.includes('npmjs') || host.includes('docker')) {
    return { name: 'GitHub & Разработка', icon: '🐙', category: 'Разработка' }
  }
  if (host.includes('netflix') || host.includes('spotify') || host.includes('twitch') || host.includes('tiktok') || host.includes('instagram') || host.includes('facebook') || host.includes('twitter') || host.includes('x.com')) {
    return { name: 'Медиа & Стриминг', icon: '🎬', category: 'Медиа & Видео' }
  }
  if (host.includes('apple') || host.includes('icloud') || host.includes('aaplimg') || host.includes('mzstatic')) {
    return { name: 'Apple Services', icon: '🍎', category: 'Экосистемы' }
  }
  if (host.includes('microsoft') || host.includes('windowsupdate') || host.includes('live.com') || host.includes('office')) {
    return { name: 'Microsoft / Windows', icon: '🪟', category: 'Экосистемы' }
  }
  if (host.includes('google') || host.includes('gstatic') || host.includes('1e100')) {
    return { name: 'Google Services', icon: '🔍', category: 'Поиск & Сервисы' }
  }
  if (host.startsWith('192.168.') || host.startsWith('10.') || host.startsWith('172.16.') || host.includes('keenetic') || host.includes('local')) {
    return { name: 'Локальная сеть / Keenetic', icon: '📡', category: 'LAN' }
  }

  const { rootDomain } = extractCleanDomain(host)
  return { name: rootDomain || host || 'Прочий трафик', icon: '🌐', category: 'Веб-трафик' }
}

export default function ConnectionsViewer({ notify }: ConnectionsViewerProps) {
  const [connections, setConnections] = useState<ConnectionItem[]>([])
  const [closedConnections, setClosedConnections] = useState<ConnectionItem[]>([])
  const [downloadTotal, setDownloadTotal] = useState(0)
  const [uploadTotal, setUploadTotal] = useState(0)
  const [loading, setLoading] = useState(true)
  const [autoRefresh, setAutoRefresh] = useState(true)
  const [search, setSearch] = useState('')
  const [sortBy, setSortBy] = useState<'traffic' | 'download' | 'upload' | 'time'>('traffic')
  const [mainView, setMainView] = useState<'connections' | 'diagnostics'>('connections')
  const [filterTab, setFilterTab] = useState<'all' | 'active' | 'proxy' | 'direct' | 'closed'>('all')
  const [groupByApp, setGroupByApp] = useState(false)

  // Имена устройств локальной сети (IP -> имя)
  const [deviceNames, setDeviceNames] = useState<Record<string, string>>({})
  // Списки проксируемых и direct доменов
  const [forceDomains, setForceDomains] = useState<string[]>([])
  const [directDomains, setDirectDomains] = useState<string[]>([])

  // Состояние модального окна «Жестко отправить в прокси»
  const [modal, setModal] = useState<ForceProxyModalState>({
    isOpen: false,
    rawHost: '',
    targetDomain: '',
    rootDomain: '',
    fullHost: '',
    clientIp: '',
    clientName: '',
    scope: 'global',
    closeCurrent: true,
    submitting: false,
  })

  const activeRef = useRef(true)
  const prevActiveMapRef = useRef<Map<string, ConnectionItem>>(new Map())

  // Загрузка вспомогательных данных (устройства и списки доменов)
  const loadAuxData = useCallback(async () => {
    try {
      const [devRes, domRes] = await Promise.allSettled([
        apiGet<{ devices: Array<{ ip: string; name: string }> }>('devices'),
        apiGet<{ direct: string[]; force: string[] }>('domains'),
      ])
      if (devRes.status === 'fulfilled' && devRes.value.devices) {
        const map: Record<string, string> = {}
        for (const d of devRes.value.devices) {
          if (d.ip && d.name) map[d.ip] = d.name
        }
        setDeviceNames(map)
      }
      if (domRes.status === 'fulfilled' && domRes.value) {
        if (domRes.value.force) setForceDomains(domRes.value.force.map((d) => d.toLowerCase()))
        if (domRes.value.direct) setDirectDomains(domRes.value.direct.map((d) => d.toLowerCase()))
      }
    } catch {
      // Игнорируем ошибки фоновой загрузки доп. данных
    }
  }, [])

  useEffect(() => {
    loadAuxData()
  }, [loadAuxData])

  // Загрузка активных соединений и расчёт истории закрытых
  const loadData = useCallback(async (isManual = false) => {
    try {
      if (isManual) setLoading(true)
      const res = await apiGet<ConnectionsResponse>('connections')
      if (!activeRef.current) return

      const live = res.connections || []
      setConnections(live)
      setDownloadTotal(res.downloadTotal || 0)
      setUploadTotal(res.uploadTotal || 0)

      // Вычисляем только что закрывшиеся соединения
      const currentIds = new Set(live.map((c) => c.id))
      const newlyClosed: ConnectionItem[] = []
      const now = Date.now()

      for (const [id, item] of prevActiveMapRef.current.entries()) {
        if (!currentIds.has(id)) {
          newlyClosed.push({
            ...item,
            isClosed: true,
            closedAt: now,
          })
        }
      }

      if (newlyClosed.length > 0) {
        setClosedConnections((prev) => {
          const merged = [...newlyClosed, ...prev]
          // Ограничиваем историю 150 последними закрытыми сессиями
          return merged.slice(0, 150)
        })
      }

      // Обновляем карту активных ID для следующего цикла
      const nextMap = new Map<string, ConnectionItem>()
      for (const c of live) {
        nextMap.set(c.id, c)
      }
      prevActiveMapRef.current = nextMap
    } catch (e: any) {
      if (isManual) notify('Ошибка загрузки соединений: ' + e.message, true)
    } finally {
      if (isManual && activeRef.current) setLoading(false)
    }
  }, [notify])

  useEffect(() => {
    activeRef.current = true
    loadData(true)
    let interval: any = null
    if (autoRefresh) {
      interval = setInterval(() => {
        if (document.hidden) return
        loadData(false)
      }, 2500)
    }
    return () => {
      activeRef.current = false
      if (interval) clearInterval(interval)
    }
  }, [loadData, autoRefresh])

  const handleCloseAll = async () => {
    if (!window.confirm('Закрыть все активные сетевые соединения?')) return
    try {
      await apiDelete('connections')
      notify('Все соединения закрыты')
      loadData(false)
    } catch (e: any) {
      notify('Ошибка закрытия соединений: ' + e.message, true)
    }
  }

  const handleCloseOne = async (id: string) => {
    try {
      await apiDelete(`connections/${id}`)
      setConnections((prev) => prev.filter((c) => c.id !== id))
      notify('Соединение разорвано')
    } catch (e: any) {
      notify('Ошибка: ' + e.message, true)
    }
  }

  const handleClearClosed = () => {
    setClosedConnections([])
    notify('История завершённых сессий очищена')
  }

  // Открытие модального окна для жесткой отправки в прокси
  const openForceProxyModal = (c?: ConnectionItem) => {
    if (!c) {
      // Ручной ввод
      setModal({
        isOpen: true,
        rawHost: '',
        targetDomain: '',
        rootDomain: '',
        fullHost: '',
        clientIp: '',
        clientName: '',
        scope: 'global',
        closeCurrent: false,
        submitting: false,
      })
      return
    }

    const host = c.metadata.host || c.metadata.destinationIP || ''
    const { host: cleanHost, rootDomain } = extractCleanDomain(host)
    const clientIp = c.metadata.sourceIP || ''
    const clientName = deviceNames[clientIp] || ''

    setModal({
      isOpen: true,
      connectionId: c.id,
      rawHost: host,
      targetDomain: rootDomain || cleanHost,
      rootDomain: rootDomain || cleanHost,
      fullHost: cleanHost,
      clientIp,
      clientName,
      scope: 'global',
      closeCurrent: !c.isClosed,
      submitting: false,
    })
  }

  // Применение принудительного направления в прокси
  const handleForceProxySubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    const domain = modal.targetDomain.trim().toLowerCase()
    if (!domain) {
      notify('Введите домен для направления в прокси', true)
      return
    }

    setModal((prev) => ({ ...prev, submitting: true }))
    try {
      // 1. Пробуем атомарный эндпоинт POST /api/domains/force-add
      try {
        await apiPost('domains/force-add', {
          domain,
          client_ip: modal.scope === 'device' ? modal.clientIp : undefined,
          close_connection_id: modal.closeCurrent ? modal.connectionId : undefined,
        })
      } catch (err: any) {
        // Fallback на раздельные API, если force-add не ответил
        if (modal.scope === 'device' && modal.clientIp) {
          const res = await apiGet<{ rules?: Record<string, Array<{ domain: string; target: string }>> } | Record<string, Array<{ domain: string; target: string }>>>('devices/domain-rules')
          const rulesMap = (res as any)?.rules ?? (res as any) ?? {}
          const curRules: Array<{ domain: string; target: string }> = rulesMap[modal.clientIp] || []
          if (!curRules.some((r: { domain: string; target: string }) => r.domain === domain)) {
            const nextRules = [...curRules, { domain, target: 'PROXY' }]
            await apiPost('devices/domain-rules', {
              ip: modal.clientIp,
              rules: nextRules,
            })
          }
        } else {
          const domRes = await apiGet<{ direct: string[]; force: string[] }>('domains')
          const curForce = domRes.force || []
          if (!curForce.some((d) => d.toLowerCase() === domain)) {
            await apiPost('domains', {
              direct: domRes.direct || [],
              force: [...curForce, domain],
            })
          }
        }
        if (modal.closeCurrent && modal.connectionId) {
          await apiDelete(`connections/${modal.connectionId}`).catch(() => {})
        }
      }

      notify(
        `✓ Домен «${domain}» успешно отправлен жестко в прокси ${
          modal.scope === 'device' ? `для устройства ${modal.clientName || modal.clientIp}` : 'глобально (все устройства)'
        }`
      )

      // Обновляем список force доменов и перезагружаем соединения
      setForceDomains((prev) => (prev.includes(domain) ? prev : [...prev, domain]))
      setModal((prev) => ({ ...prev, isOpen: false, submitting: false }))
      loadData(false)
    } catch (e: any) {
      notify('Ошибка отправки в прокси: ' + e.message, true)
      setModal((prev) => ({ ...prev, submitting: false }))
    }
  }

  // 1-клик отправка домена соединения в обход (Direct)
  const handleOneClickDirect = async (c: ConnectionItem) => {
    const host = c.metadata.host || c.metadata.destinationIP || ''
    const { host: cleanHost, rootDomain } = extractCleanDomain(host)
    const domain = rootDomain || cleanHost
    if (!domain) return
    try {
      const domRes = await apiGet<{ direct: string[]; force: string[] }>('domains')
      const curDirect = domRes.direct || []
      const curForce = domRes.force || []
      if (!curDirect.some((d) => d.toLowerCase() === domain)) {
        await apiPost('domains', {
          direct: [...curDirect, domain],
          force: curForce.filter((d) => d.toLowerCase() !== domain),
        })
      }
      if (!c.isClosed) {
        await apiDelete(`connections/${c.id}`).catch(() => {})
      }
      setDirectDomains((prev) => (prev.includes(domain) ? prev : [...prev, domain]))
      setForceDomains((prev) => prev.filter((d) => d !== domain))
      notify(`✓ Домен «${domain}» направлен напрямую (Direct)`)
      loadData(false)
    } catch (e: any) {
      notify('Ошибка направления напрямую: ' + e.message, true)
    }
  }

  // Проверка: находится ли домен хоста уже в force_domains
  const isHostAlreadyForced = useCallback(
    (rawHost: string): boolean => {
      if (!rawHost) return false
      const { host, rootDomain } = extractCleanDomain(rawHost)
      return (
        forceDomains.includes(host) ||
        forceDomains.includes(rootDomain) ||
        forceDomains.some((fd) => host.endsWith(`.${fd}`))
      )
    },
    [forceDomains]
  )

  // Проверка: находится ли домен хоста уже в direct_domains
  const isHostAlreadyDirect = useCallback(
    (rawHost: string): boolean => {
      if (!rawHost) return false
      const { host, rootDomain } = extractCleanDomain(rawHost)
      return (
        directDomains.includes(host) ||
        directDomains.includes(rootDomain) ||
        directDomains.some((dd) => host.endsWith(`.${dd}`))
      )
    },
    [directDomains]
  )

  // Объединяем активные и завершённые сессии
  const allConnections = useMemo(() => {
    return [...connections, ...closedConnections]
  }, [connections, closedConnections])

  // Фильтрация по табам и поиску
  const filtered = useMemo(() => {
    return allConnections.filter((c) => {
      // Фильтр по типу таба
      const isProxy = c.chains && c.chains.length > 0 && c.chains[0].toUpperCase() !== 'DIRECT'
      const isDirect = !isProxy

      if (filterTab === 'active' && c.isClosed) return false
      if (filterTab === 'closed' && !c.isClosed) return false
      if (filterTab === 'proxy' && !isProxy) return false
      if (filterTab === 'direct' && !isDirect) return false

      // Поиск
      if (!search.trim()) return true
      const q = search.toLowerCase()
      const host = (c.metadata.host || '').toLowerCase()
      const srcIp = (c.metadata.sourceIP || '').toLowerCase()
      const dstIp = (c.metadata.destinationIP || '').toLowerCase()
      const devName = (deviceNames[c.metadata.sourceIP] || '').toLowerCase()
      const rule = (c.rule || '').toLowerCase()
      const chainStr = (c.chains || []).join(' ').toLowerCase()

      return (
        host.includes(q) ||
        srcIp.includes(q) ||
        dstIp.includes(q) ||
        devName.includes(q) ||
        rule.includes(q) ||
        chainStr.includes(q)
      )
    })
  }, [allConnections, filterTab, search, deviceNames])

  // Сортировка
  const sorted = useMemo(() => {
    const copy = [...filtered]
    copy.sort((a, b) => {
      if (sortBy === 'traffic') return (b.download + b.upload) - (a.download + a.upload)
      if (sortBy === 'download') return b.download - a.download
      if (sortBy === 'upload') return b.upload - a.upload
      const timeA = a.closedAt || new Date(a.start).getTime()
      const timeB = b.closedAt || new Date(b.start).getTime()
      return timeB - timeA
    })
    return copy
  }, [filtered, sortBy])

  // Группировка потоков по приложениям (Live Flow Inspector)
  const groupedApps = useMemo(() => {
    if (!groupByApp) return []
    const map = new Map<string, { app: { name: string; icon: string; category: string }; conns: ConnectionItem[]; download: number; upload: number }>()
    for (const c of sorted) {
      const app = detectAppFromConn(c)
      const existing = map.get(app.name)
      if (existing) {
        existing.conns.push(c)
        existing.download += c.download
        existing.upload += c.upload
      } else {
        map.set(app.name, {
          app,
          conns: [c],
          download: c.download,
          upload: c.upload,
        })
      }
    }
    return Array.from(map.values()).sort((a, b) => (b.download + b.upload) - (a.download + a.upload))
  }, [groupByApp, sorted])

  // Подсчёт количеств для табов
  const counts = useMemo(() => {
    const total = allConnections.length
    const activeCount = connections.length
    const closedCount = closedConnections.length
    const proxyCount = allConnections.filter(
      (c) => c.chains && c.chains.length > 0 && c.chains[0].toUpperCase() !== 'DIRECT'
    ).length
    const directCount = total - proxyCount
    return { total, activeCount, closedCount, proxyCount, directCount }
  }, [allConnections, connections, closedConnections])

  return (
    <div className="connections-view">
      <div className="section-header" style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: 12, marginBottom: 16 }}>
        <div>
          <h2>🌐 {mainView === 'connections' ? 'Соединения и диагностика' : 'Диагностика сети'}</h2>
          <p className="muted" style={{ margin: 0 }}>
            {mainView === 'connections'
              ? 'Мониторинг сетевых сессий ядра Mihomo в реальном времени и диагностика доступности узлов.'
              : 'Проверка здоровья компонентов маршрутизации, DNS и сетевых интерфейсов роутера.'}
          </p>
        </div>
        <div style={{ display: 'inline-flex', padding: 3, background: 'rgba(255, 255, 255, 0.05)', borderRadius: 8, border: '1px solid var(--border)' }}>
          <button
            type="button"
            className={`btn sm ${mainView === 'connections' ? 'primary' : 'ghost'}`}
            style={{ fontWeight: 600, border: 'none', borderRadius: 6 }}
            onClick={() => setMainView('connections')}
          >
            🌐 Активные соединения
          </button>
          <button
            type="button"
            className={`btn sm ${mainView === 'diagnostics' ? 'primary' : 'ghost'}`}
            style={{ fontWeight: 600, border: 'none', borderRadius: 6 }}
            onClick={() => setMainView('diagnostics')}
          >
            🩺 Диагностика сети
          </button>
        </div>
      </div>

      {mainView === 'diagnostics' ? (
        <Diagnostics notify={notify} />
      ) : (
        <>

      {/* Метрики (плитки в 1/4 ширины) */}
      <div className="stats-grid four-col">
        <div className="stat-card">
          <div className="stat-label">Активных сессий</div>
          <div className="stat-value">{connections.length}</div>
          <div className="muted small">Открытых сетевых сокетов</div>
        </div>
        <div className="stat-card">
          <div className="stat-label">Всего скачано (Down)</div>
          <div className="stat-value down-color">{formatBytes(downloadTotal)}</div>
          <div className="muted small">Входящий трафик сессий</div>
        </div>
        <div className="stat-card">
          <div className="stat-label">Всего отдано (Up)</div>
          <div className="stat-value up-color">{formatBytes(uploadTotal)}</div>
          <div className="muted small">Исходящий трафик сессий</div>
        </div>
        <div className="stat-card">
          <div className="stat-label">В таблице</div>
          <div className="stat-value" style={{ color: 'var(--accent)' }}>
            {sorted.length}
          </div>
          <div className="muted small">{search ? 'С учётом поиска' : 'Все соединения'}</div>
        </div>
      </div>

      {/* Панель фильтров и управления */}
      <div className="card toolbar-card">
        {/* Сегментированные вкладки-фильтры */}
        <div className="conn-filter-tabs">
          <button
            type="button"
            className={`conn-tab-btn ${filterTab === 'all' ? 'active' : ''}`}
            onClick={() => setFilterTab('all')}
          >
            🌐 Все соединения <span className="tab-count">{counts.total}</span>
          </button>
          <button
            type="button"
            className={`conn-tab-btn ${filterTab === 'active' ? 'active' : ''}`}
            onClick={() => setFilterTab('active')}
          >
            <span className="conn-dot conn-dot-active" /> Активные <span className="tab-count">{counts.activeCount}</span>
          </button>
          <button
            type="button"
            className={`conn-tab-btn ${filterTab === 'proxy' ? 'active' : ''}`}
            onClick={() => setFilterTab('proxy')}
          >
            ⚡ Через прокси <span className="tab-count">{counts.proxyCount}</span>
          </button>
          <button
            type="button"
            className={`conn-tab-btn ${filterTab === 'direct' ? 'active' : ''}`}
            onClick={() => setFilterTab('direct')}
          >
            ➡️ Прямые (DIRECT) <span className="tab-count">{counts.directCount}</span>
          </button>
          <button
            type="button"
            className={`conn-tab-btn ${filterTab === 'closed' ? 'active' : ''}`}
            onClick={() => setFilterTab('closed')}
          >
            <span className="conn-dot conn-dot-closed" /> Завершённые <span className="tab-count">{counts.closedCount}</span>
          </button>
        </div>

        <div className="connections-controls" style={{ marginTop: 12 }}>
          <input
            type="text"
            className="input-text connections-search"
            placeholder="🔍 Поиск по хосту, клиенту (192.168...), назначению или правилу…"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />

          <div className="connections-actions-group">
            <button
              type="button"
              className={`btn btn-sm ${groupByApp ? 'btn-primary' : 'btn-secondary'}`}
              onClick={() => setGroupByApp((prev) => !prev)}
              title="Инспектор потоков: группировать по приложениям"
            >
              📦 {groupByApp ? 'Группировка: Вкл' : 'По приложениям'}
            </button>
            <button
              className="btn btn-sm btn-primary"
              onClick={() => openForceProxyModal()}
              title="Добавить любой домен принудительно в прокси вручную"
            >
              ➕ В прокси по домену
            </button>
            <button
              className="btn btn-sm btn-secondary"
              onClick={() => loadData(true)}
              title="Обновить список соединений вручную"
            >
              🔄 Обновить
            </button>
            <button
              className="btn btn-sm btn-danger"
              onClick={handleCloseAll}
              disabled={connections.length === 0}
              title="Закрыть все открытые соединения"
            >
              💥 Закрыть все ({connections.length})
            </button>
            {closedConnections.length > 0 && (
              <button
                className="btn btn-sm btn-secondary"
                onClick={handleClearClosed}
                title="Очистить историю завершённых сессий"
              >
                🗑️ Очистить завершённые
              </button>
            )}
          </div>

          <div className="controls-group">
            <label className="checkbox-label">
              <input
                type="checkbox"
                checked={autoRefresh}
                onChange={(e) => setAutoRefresh(e.target.checked)}
              />
              <span>Автообновление (2.5с)</span>
            </label>

            <select
              className="select editor-select"
              value={sortBy}
              onChange={(e: any) => setSortBy(e.target.value)}
            >
              <option value="traffic">Сортировка: По объёму трафика (всего)</option>
              <option value="download">Сортировка: По скачиванию</option>
              <option value="upload">Сортировка: По отдаче</option>
              <option value="time">Сортировка: По времени</option>
            </select>
          </div>
        </div>
      </div>

      {/* Список соединений */}
      {loading && allConnections.length === 0 ? (
        <div className="card loading-placeholder">⏳ Загрузка списка соединений…</div>
      ) : sorted.length === 0 ? (
        <div className="card empty-placeholder">
          {search ? '🔍 Соединений по данному запросу не найдено' : 'Подключения в данном разделе отсутствуют'}
        </div>
      ) : groupByApp ? (
        /* РЕЖИМ ГРУППИРОВКИ ПО ПРИЛОЖЕНИЯМ (Live Flow Inspector) */
        <div style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
          {groupedApps.map(({ app, conns, download, upload }) => (
            <div
              key={app.name}
              className="card"
              style={{
                background: 'rgba(255, 255, 255, 0.02)',
                border: '1px solid var(--border)',
                borderRadius: 10,
                padding: '12px 16px',
              }}
            >
              <div
                style={{
                  display: 'flex',
                  justifyContent: 'space-between',
                  alignItems: 'center',
                  flexWrap: 'wrap',
                  gap: 10,
                  marginBottom: 10,
                  borderBottom: '1px solid rgba(255,255,255,0.06)',
                  paddingBottom: 8,
                }}
              >
                <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                  <span style={{ fontSize: 20 }}>{app.icon}</span>
                  <div>
                    <b style={{ fontSize: 14, color: 'var(--text-bright)' }}>{app.name}</b>
                    <span className="badge" style={{ marginLeft: 8, fontSize: 10, background: 'rgba(255,255,255,0.06)' }}>
                      {app.category}
                    </span>
                    <span className="muted small" style={{ marginLeft: 8, fontSize: 11 }}>
                      {conns.length} сессий
                    </span>
                  </div>
                </div>

                <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
                  <span style={{ fontSize: 12, fontFamily: 'monospace', color: '#38bdf8' }}>{formatBytes(download)}</span>
                  <span style={{ fontSize: 12, fontFamily: 'monospace', color: '#a855f7' }}>{formatBytes(upload)}</span>
                  <button
                    type="button"
                    className="btn btn-xs"
                    style={{
                      background: 'rgba(56, 189, 248, 0.15)',
                      color: '#38bdf8',
                      border: '1px solid rgba(56, 189, 248, 0.4)',
                      borderRadius: 6,
                      padding: '3px 8px',
                      fontWeight: 600,
                    }}
                    onClick={() => {
                      if (conns[0]) handleOneClickDirect(conns[0])
                    }}
                    title="Направить домен первого соединения этого приложения напрямую (Direct)"
                  >
                    ⚡ В обход (Direct)
                  </button>
                  <button
                    type="button"
                    className="btn btn-xs btn-force-proxy"
                    style={{ borderRadius: 6, padding: '3px 8px', fontWeight: 600 }}
                    onClick={() => {
                      if (conns[0]) openForceProxyModal(conns[0])
                    }}
                    title="Направить в прокси"
                  >
                    ⚡ В прокси
                  </button>
                </div>
              </div>

              {/* Мини-таблица соединений приложения */}
              <div className="table-responsive">
                <table className="connections-table" style={{ fontSize: 12 }}>
                  <tbody>
                    {conns.map((c) => {
                      const host = c.metadata.host || c.metadata.destinationIP || 'Неизвестно'
                      const isProxy = c.chains && c.chains.length > 0 && c.chains[0].toUpperCase() !== 'DIRECT'
                      const isForced = isHostAlreadyForced(host)
                      return (
                        <tr key={c.id}>
                          <td style={{ width: 60 }}>
                            <span className={`net-badge ${c.metadata.network.toLowerCase()}`}>{c.metadata.network}</span>
                          </td>
                          <td className="host-cell" title={`${host}:${c.metadata.destinationPort}`}>
                            <span className="host-name">{host}</span>
                            <span className="host-port muted">:{c.metadata.destinationPort}</span>
                          </td>
                          <td style={{ width: 140 }}>
                            <span className="client-ip">{c.metadata.sourceIP}</span>
                          </td>
                          <td style={{ width: 100 }}>
                            <span className="chain-badge">{c.chains?.[0] || 'DIRECT'}</span>
                          </td>
                          <td className="traffic-cell down-color" style={{ width: 90 }}>{formatBytes(c.download)}</td>
                          <td className="traffic-cell up-color" style={{ width: 90 }}>{formatBytes(c.upload)}</td>
                          <td style={{ width: 130, textAlign: 'right' }}>
                            <div style={{ display: 'inline-flex', gap: 6, alignItems: 'center' }}>
                              {isProxy ? (
                                isHostAlreadyDirect(host) ? (
                                  <span
                                    className="badge-already-forced"
                                    style={{
                                      background: 'rgba(56, 189, 248, 0.12)',
                                      color: '#38bdf8',
                                      borderColor: 'rgba(56, 189, 248, 0.35)',
                                      padding: '2px 4px',
                                      fontSize: 10,
                                    }}
                                    title="Уже в обходе (Direct)"
                                  >
                                    ✓ В обход
                                  </span>
                                ) : (
                                  <button
                                    type="button"
                                    className="btn btn-xs"
                                    style={{
                                      background: 'rgba(56, 189, 248, 0.12)',
                                      color: '#38bdf8',
                                      border: '1px solid rgba(56, 189, 248, 0.35)',
                                      padding: '2px 5px',
                                      fontSize: 10,
                                      fontWeight: 600,
                                    }}
                                    title="Направить напрямую (Direct)"
                                    onClick={() => handleOneClickDirect(c)}
                                  >
                                    ⚡ Обход
                                  </button>
                                )
                              ) : isForced ? (
                                <span
                                  className="badge-already-forced"
                                  style={{ padding: '2px 4px', fontSize: 10 }}
                                  title="Уже в прокси"
                                >
                                  ✓ В прокси
                                </span>
                              ) : (
                                <button
                                  type="button"
                                  className="btn btn-xs btn-force-proxy"
                                  style={{ padding: '2px 5px', fontSize: 10 }}
                                  title="Направить в прокси"
                                  onClick={() => openForceProxyModal(c)}
                                >
                                  ⚡ Прокси
                                </button>
                              )}
                              {!c.isClosed && (
                                <button
                                  type="button"
                                  className="btn-icon-danger"
                                  style={{ width: 20, height: 20, fontSize: 10 }}
                                  onClick={() => handleCloseOne(c.id)}
                                >
                                  ✕
                                </button>
                              )}
                            </div>
                          </td>
                        </tr>
                      )
                    })}
                  </tbody>
                </table>
              </div>
            </div>
          ))}
        </div>
      ) : (
        <div className="card table-card">
          <div className="table-responsive">
            <table className="connections-table">
              <thead>
                <tr>
                  <th style={{ width: 70 }}>Сеть</th>
                  <th>Хост / Назначение</th>
                  <th>Клиент (IP)</th>
                  <th>Маршрут / Цепочка</th>
                  <th>Правило</th>
                  <th
                    style={{ cursor: 'pointer', userSelect: 'none' }}
                    onClick={() => setSortBy((prev) => (prev === 'download' ? 'traffic' : 'download'))}
                    title="Сортировка по объёму входящего трафика"
                  >
                    Скачано{sortBy === 'download' ? ' •' : ''}
                  </th>
                  <th
                    style={{ cursor: 'pointer', userSelect: 'none' }}
                    onClick={() => setSortBy((prev) => (prev === 'upload' ? 'traffic' : 'upload'))}
                    title="Сортировка по объёму исходящего трафика"
                  >
                    Отдано{sortBy === 'upload' ? ' •' : ''}
                  </th>
                  <th style={{ width: 160, textAlign: 'center' }}>Действие</th>
                </tr>
              </thead>
              <tbody>
                {sorted.map((c) => {
                  const host = c.metadata.host || c.metadata.destinationIP || 'Неизвестно'
                  const client = c.metadata.sourceIP
                  const clientName = deviceNames[client]
                  const chain = c.chains && c.chains.length > 0 ? c.chains.join(' → ') : 'DIRECT'
                  const isForced = isHostAlreadyForced(host)
                  const isProxy = c.chains && c.chains.length > 0 && c.chains[0].toUpperCase() !== 'DIRECT'

                  return (
                    <tr key={c.id} className={c.isClosed ? 'conn-row-closed' : ''}>
                      <td>
                        <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                          <span
                            className={`conn-dot ${c.isClosed ? 'conn-dot-closed' : 'conn-dot-active'}`}
                            title={c.isClosed ? 'Сессия завершена' : 'Активная сессия'}
                          />
                          <span className={`net-badge ${c.metadata.network.toLowerCase()}`}>
                            {c.metadata.network}
                          </span>
                        </div>
                      </td>
                      <td className="host-cell" title={`${host}:${c.metadata.destinationPort}`}>
                        <div className="host-name">{host}</div>
                        <div className="host-port muted">:{c.metadata.destinationPort}</div>
                      </td>
                      <td>
                        <div className="client-cell">
                          <span className="client-ip">{client}</span>
                          {clientName && <span className="client-device-name">{clientName}</span>}
                        </div>
                      </td>
                      <td>
                        <span className="chain-badge" title={chain}>
                          {c.chains?.[0] || 'DIRECT'}
                        </span>
                      </td>
                      <td>
                        <span className="rule-badge" title={c.rulePayload || c.rule}>
                          {c.rule}
                        </span>
                      </td>
                      <td className="traffic-cell down-color">{formatBytes(c.download)}</td>
                      <td className="traffic-cell up-color">{formatBytes(c.upload)}</td>
                      <td style={{ textAlign: 'center' }}>
                        <div style={{ display: 'inline-flex', alignItems: 'center', gap: 6 }}>
                          {isForced ? (
                            <span className="badge-already-forced" title="Домен уже принудительно направляется в прокси">
                              ✓ В прокси
                            </span>
                          ) : (
                            <button
                              type="button"
                              className="btn btn-xs btn-force-proxy"
                              title="Жестко отправить домен этого соединения в прокси"
                              onClick={() => openForceProxyModal(c)}
                            >
                              ⚡ В прокси
                            </button>
                          )}
                          {isProxy && (
                            isHostAlreadyDirect(host) ? (
                              <span
                                className="badge-already-forced"
                                style={{
                                  background: 'rgba(56, 189, 248, 0.15)',
                                  color: '#38bdf8',
                                  borderColor: 'rgba(56, 189, 248, 0.35)',
                                  borderRadius: 4,
                                  padding: '2px 6px',
                                  fontSize: 11,
                                }}
                                title="Домен уже в списке прямого подключения (Direct)"
                              >
                                ✓ В обход
                              </span>
                            ) : (
                              <button
                                type="button"
                                className="btn btn-xs"
                                style={{
                                  background: 'rgba(56, 189, 248, 0.15)',
                                  color: '#38bdf8',
                                  border: '1px solid rgba(56, 189, 248, 0.35)',
                                  borderRadius: 4,
                                  padding: '2px 6px',
                                  fontSize: 11,
                                  fontWeight: 600,
                                }}
                                title="Направить домен в обход прокси (Direct)"
                                onClick={() => handleOneClickDirect(c)}
                              >
                                ⚡ В обход
                              </button>
                            )
                          )}
                          {!c.isClosed && (
                            <button
                              type="button"
                              className="btn-icon-danger"
                              title="Разорвать активное соединение"
                              onClick={() => handleCloseOne(c.id)}
                            >
                              ✕
                            </button>
                          )}
                        </div>
                      </td>
                    </tr>
                  )
                })}
              </tbody>
            </table>
          </div>
        </div>
      )}

      {/* МОДАЛЬНОЕ ОКНО: «ЖЕСТКО ОТПРАВИТЬ СОЕДИНЕНИЕ В ПРОКСИ» */}
      {modal.isOpen && (
        <div className="modal-overlay" onClick={() => !modal.submitting && setModal((prev) => ({ ...prev, isOpen: false }))}>
          <div className="modal force-proxy-modal" style={{ maxWidth: 540 }} onClick={(e) => e.stopPropagation()}>
            <div className="modal-header">
              <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                <span style={{ fontSize: 22 }}>⚡</span>
                <h2 style={{ margin: 0, fontSize: 18 }}>Жестко отправить в прокси</h2>
              </div>
              <button
                type="button"
                className="btn btn-sm ghost"
                disabled={modal.submitting}
                onClick={() => setModal((prev) => ({ ...prev, isOpen: false }))}
              >
                ✕
              </button>
            </div>

            <form onSubmit={handleForceProxySubmit} className="modal-body" style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
              {modal.rawHost && (
                <div className="force-proxy-info-box">
                  <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 4 }}>
                    <span className="muted small">Исходный хост:</span>
                    <b style={{ fontFamily: 'monospace', fontSize: 13 }}>{modal.rawHost}</b>
                  </div>
                  {modal.clientIp && (
                    <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                      <span className="muted small">Устройство клиента:</span>
                      <span style={{ fontSize: 12.5 }}>
                        {modal.clientName ? `${modal.clientName} (${modal.clientIp})` : modal.clientIp}
                      </span>
                    </div>
                  )}
                </div>
              )}

              {/* Выбор целевого домена */}
              <div className="form-group" style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                <label style={{ fontSize: 13, fontWeight: 600 }}>Домен для перенаправления:</label>
                {modal.rootDomain && modal.fullHost && modal.rootDomain !== modal.fullHost && (
                  <div style={{ display: 'flex', gap: 8, marginBottom: 4 }}>
                    <button
                      type="button"
                      className={`btn btn-xs ${modal.targetDomain === modal.rootDomain ? 'btn-primary' : 'btn-secondary'}`}
                      onClick={() => setModal((prev) => ({ ...prev, targetDomain: modal.rootDomain }))}
                    >
                      🌟 Корневой: <b>{modal.rootDomain}</b> (рекомендуется)
                    </button>
                    <button
                      type="button"
                      className={`btn btn-xs ${modal.targetDomain === modal.fullHost ? 'btn-primary' : 'btn-secondary'}`}
                      onClick={() => setModal((prev) => ({ ...prev, targetDomain: modal.fullHost }))}
                    >
                      🎯 Точный: <b>{modal.fullHost}</b>
                    </button>
                  </div>
                )}
                <input
                  type="text"
                  className="input sm"
                  style={{ fontFamily: 'Consolas, monospace', fontSize: 13.5 }}
                  placeholder="например: discord.gg, googlevideo.com, habr.com"
                  value={modal.targetDomain}
                  onChange={(e) => setModal((prev) => ({ ...prev, targetDomain: e.target.value }))}
                  required
                />
                <span className="muted small">
                  Правило <code style={{ color: 'var(--accent)' }}>DOMAIN-SUFFIX</code> охватит указанный домен и все его поддомены.
                </span>
              </div>

              {/* Область действия: Глобально или для конкретного устройства */}
              <div className="form-group" style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
                <label style={{ fontSize: 13, fontWeight: 600 }}>Область применения:</label>
                <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                  <label className="radio-label" style={{ display: 'flex', alignItems: 'center', gap: 8, cursor: 'pointer' }}>
                    <input
                      type="radio"
                      name="proxy_scope"
                      value="global"
                      checked={modal.scope === 'global'}
                      onChange={() => setModal((prev) => ({ ...prev, scope: 'global' }))}
                    />
                    <div>
                      <div style={{ fontWeight: 600, fontSize: 13 }}>🌐 Для всех устройств сети (Глобально)</div>
                      <div className="muted small">
                        Добавляет в «Принудительно через прокси», автоматически находит сопутствующие CDN и синхронизирует с ipset <code style={{ color: 'var(--accent)' }}>geo_override</code> ядра роутера.
                      </div>
                    </div>
                  </label>

                  {modal.clientIp && (
                    <label className="radio-label" style={{ display: 'flex', alignItems: 'center', gap: 8, cursor: 'pointer' }}>
                      <input
                        type="radio"
                        name="proxy_scope"
                        value="device"
                        checked={modal.scope === 'device'}
                        onChange={() => setModal((prev) => ({ ...prev, scope: 'device' }))}
                      />
                      <div>
                        <div style={{ fontWeight: 600, fontSize: 13 }}>
                          📱 Только для устройства {modal.clientName ? `${modal.clientName} (${modal.clientIp})` : modal.clientIp}
                        </div>
                        <div className="muted small">
                          Создает персональное правило перенаправления для этого клиента без влияния на другие домашние гаджеты.
                        </div>
                      </div>
                    </label>
                  )}
                </div>
              </div>

              {/* Чекбокс немедленного разрыва текущей сессии */}
              {modal.connectionId && (
                <label className="checkbox-label" style={{ display: 'flex', alignItems: 'center', gap: 8, cursor: 'pointer' }}>
                  <input
                    type="checkbox"
                    checked={modal.closeCurrent}
                    onChange={(e) => setModal((prev) => ({ ...prev, closeCurrent: e.target.checked }))}
                  />
                  <span style={{ fontSize: 13 }}>
                    ⚡ Немедленно разорвать текущее соединение (чтобы трафик сразу пошёл через прокси)
                  </span>
                </label>
              )}

              <div className="modal-actions" style={{ display: 'flex', justifyContent: 'flex-end', gap: 8, marginTop: 10 }}>
                <button
                  type="button"
                  className="btn sm secondary"
                  disabled={modal.submitting}
                  onClick={() => setModal((prev) => ({ ...prev, isOpen: false }))}
                >
                  Отмена
                </button>
                <button
                  type="submit"
                  className="btn sm primary"
                  disabled={modal.submitting || !modal.targetDomain.trim()}
                  style={{ minWidth: 160 }}
                >
                  {modal.submitting ? '⏳ Применение...' : '🚀 Отправить жестко в прокси'}
                </button>
              </div>
            </form>
          </div>
        </div>
      )}
      </>
      )}
    </div>
  )
}
