import { useEffect, useState, useRef, useCallback, useMemo } from 'react'
import { apiGet, apiPost, apiDelete } from '../api'

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

export default function ConnectionsViewer({ notify }: ConnectionsViewerProps) {
  const [connections, setConnections] = useState<ConnectionItem[]>([])
  const [closedConnections, setClosedConnections] = useState<ConnectionItem[]>([])
  const [downloadTotal, setDownloadTotal] = useState(0)
  const [uploadTotal, setUploadTotal] = useState(0)
  const [loading, setLoading] = useState(true)
  const [autoRefresh, setAutoRefresh] = useState(true)
  const [search, setSearch] = useState('')
  const [sortBy, setSortBy] = useState<'download' | 'upload' | 'time'>('download')
  const [filterTab, setFilterTab] = useState<'all' | 'active' | 'proxy' | 'direct' | 'closed'>('all')

  // Имена устройств локальной сети (IP -> имя)
  const [deviceNames, setDeviceNames] = useState<Record<string, string>>({})
  // Список принудительно проксируемых доменов (для индикации «✓ В прокси»)
  const [forceDomains, setForceDomains] = useState<string[]>([])

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
        apiGet<{ force: string[] }>('domains'),
      ])
      if (devRes.status === 'fulfilled' && devRes.value.devices) {
        const map: Record<string, string> = {}
        for (const d of devRes.value.devices) {
          if (d.ip && d.name) map[d.ip] = d.name
        }
        setDeviceNames(map)
      }
      if (domRes.status === 'fulfilled' && domRes.value.force) {
        setForceDomains(domRes.value.force.map((d) => d.toLowerCase()))
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
      interval = setInterval(() => loadData(false), 2500)
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
          const res = await apiGet<{ rules: Record<string, Array<{ domain: string; target: string }>> }>('devices/domain-rules')
          const curRules = res.rules?.[modal.clientIp] || []
          if (!curRules.some((r) => r.domain === domain)) {
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
      if (sortBy === 'download') return b.download - a.download
      if (sortBy === 'upload') return b.upload - a.upload
      const timeA = a.closedAt || new Date(a.start).getTime()
      const timeB = b.closedAt || new Date(b.start).getTime()
      return timeB - timeA
    })
    return copy
  }, [filtered, sortBy])

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
      <div className="section-header">
        <div>
          <h2>🌐 Все соединения</h2>
          <p className="muted">
            Мониторинг всех сетевых сессий ядра Mihomo в реальном времени: активные сокеты, история завершённых подключений,
            передача трафика и мгновенная отправка в прокси.
          </p>
        </div>
      </div>

      {/* Метрики (плитки в 1/4 ширины) */}
      <div className="stats-grid four-col">
        <div className="stat-card">
          <div className="stat-label">Активных сессий</div>
          <div className="stat-value">{connections.length}</div>
          <div className="muted small">Открытых сетевых сокетов</div>
        </div>
        <div className="stat-card">
          <div className="stat-label">Всего скачано (Down)</div>
          <div className="stat-value down-color">↓ {formatBytes(downloadTotal)}</div>
          <div className="muted small">Входящий трафик сессий</div>
        </div>
        <div className="stat-card">
          <div className="stat-label">Всего отдано (Up)</div>
          <div className="stat-value up-color">↑ {formatBytes(uploadTotal)}</div>
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
              <option value="download">Сортировка: По скачиванию (↓)</option>
              <option value="upload">Сортировка: По отдаче (↑)</option>
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
                  <th>Скачано (↓)</th>
                  <th>Отдано (↑)</th>
                  <th style={{ width: 140, textAlign: 'center' }}>Действие</th>
                </tr>
              </thead>
              <tbody>
                {sorted.map((c) => {
                  const host = c.metadata.host || c.metadata.destinationIP || 'Неизвестно'
                  const client = c.metadata.sourceIP
                  const clientName = deviceNames[client]
                  const chain = c.chains && c.chains.length > 0 ? c.chains.join(' → ') : 'DIRECT'
                  const isForced = isHostAlreadyForced(host)

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
                      <td className="traffic-cell down-color">↓ {formatBytes(c.download)}</td>
                      <td className="traffic-cell up-color">↑ {formatBytes(c.upload)}</td>
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
    </div>
  )
}
