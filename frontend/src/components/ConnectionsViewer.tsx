import { useEffect, useState, useRef, useCallback } from 'react'
import { apiGet, apiDelete } from '../api'

interface ConnectionMetadata {
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

interface ConnectionItem {
  id: string
  metadata: ConnectionMetadata
  upload: number
  download: number
  start: string
  chains: string[]
  rule: string
  rulePayload: string
}

interface ConnectionsResponse {
  downloadTotal: number
  uploadTotal: number
  connections: ConnectionItem[]
}

interface ConnectionsViewerProps {
  notify: (msg: string, error?: boolean) => void
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(2)} MB`
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`
}

export default function ConnectionsViewer({ notify }: ConnectionsViewerProps) {
  const [connections, setConnections] = useState<ConnectionItem[]>([])
  const [downloadTotal, setDownloadTotal] = useState(0)
  const [uploadTotal, setUploadTotal] = useState(0)
  const [loading, setLoading] = useState(true)
  const [autoRefresh, setAutoRefresh] = useState(true)
  const [search, setSearch] = useState('')
  const [sortBy, setSortBy] = useState<'download' | 'upload' | 'time'>('download')
  const activeRef = useRef(true)

  const loadData = useCallback(async (isManual = false) => {
    try {
      if (isManual) setLoading(true)
      const res = await apiGet<ConnectionsResponse>('connections')
      if (!activeRef.current) return
      setConnections(res.connections || [])
      setDownloadTotal(res.downloadTotal || 0)
      setUploadTotal(res.uploadTotal || 0)
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

  // Фильтрация и сортировка
  const filtered = connections.filter((c) => {
    if (!search.trim()) return true
    const q = search.toLowerCase()
    return (
      (c.metadata.host && c.metadata.host.toLowerCase().includes(q)) ||
      (c.metadata.sourceIP && c.metadata.sourceIP.includes(q)) ||
      (c.metadata.destinationIP && c.metadata.destinationIP.includes(q)) ||
      (c.rule && c.rule.toLowerCase().includes(q)) ||
      (c.chains && c.chains.join(' ').toLowerCase().includes(q))
    )
  })

  filtered.sort((a, b) => {
    if (sortBy === 'download') return b.download - a.download
    if (sortBy === 'upload') return b.upload - a.upload
    return new Date(b.start).getTime() - new Date(a.start).getTime()
  })

  return (
    <div className="connections-view">
      <div className="section-header">
        <div>
          <h2>🌐 Активные соединения</h2>
          <p className="muted">
            Мониторинг сетевых сессий ядра Mihomo в реальном времени: кто, куда и сколько трафика передает.
          </p>
        </div>
        <div className="header-actions">
          <button className="btn btn-sm btn-secondary" onClick={() => loadData(true)}>
            🔄 Обновить
          </button>
          <button className="btn btn-sm btn-danger" onClick={handleCloseAll} disabled={connections.length === 0}>
            💥 Закрыть все ({connections.length})
          </button>
        </div>
      </div>

      {/* Метрики */}
      <div className="stats-grid">
        <div className="stat-card">
          <div className="stat-label">Активных сессий</div>
          <div className="stat-value">{connections.length}</div>
        </div>
        <div className="stat-card">
          <div className="stat-label">Всего скачано (Down)</div>
          <div className="stat-value down-color">↓ {formatBytes(downloadTotal)}</div>
        </div>
        <div className="stat-card">
          <div className="stat-label">Всего отдано (Up)</div>
          <div className="stat-value up-color">↑ {formatBytes(uploadTotal)}</div>
        </div>
      </div>

      {/* Панель фильтров */}
      <div className="card toolbar-card">
        <div className="connections-controls">
          <input
            type="text"
            className="input-text connections-search"
            placeholder="🔍 Поиск по хосту, IP клиента (192.168...), назначению или правилу…"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />

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
              className="editor-select"
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
      {loading && connections.length === 0 ? (
        <div className="card loading-placeholder">⏳ Загрузка списка соединений…</div>
      ) : filtered.length === 0 ? (
        <div className="card empty-placeholder">
          {search ? '🔍 Соединений по данному запросу не найдено' : 'Подключения отсутствуют'}
        </div>
      ) : (
        <div className="card table-card">
          <div className="table-responsive">
            <table className="connections-table">
              <thead>
                <tr>
                  <th>Сеть</th>
                  <th>Хост / Назначение</th>
                  <th>Клиент (IP)</th>
                  <th>Маршрут / Цепочка</th>
                  <th>Правило</th>
                  <th>Скачано (↓)</th>
                  <th>Отдано (↑)</th>
                  <th>Действие</th>
                </tr>
              </thead>
              <tbody>
                {filtered.map((c) => {
                  const host = c.metadata.host || c.metadata.destinationIP
                  const client = c.metadata.sourceIP
                  const chain = c.chains && c.chains.length > 0 ? c.chains.join(' → ') : 'DIRECT'
                  return (
                    <tr key={c.id}>
                      <td>
                        <span className={`net-badge ${c.metadata.network.toLowerCase()}`}>
                          {c.metadata.network}
                        </span>
                      </td>
                      <td className="host-cell" title={`${host}:${c.metadata.destinationPort}`}>
                        <div className="host-name">{host || 'Неизвестно'}</div>
                        <div className="host-port muted">:{c.metadata.destinationPort}</div>
                      </td>
                      <td>
                        <span className="client-ip">{client}</span>
                      </td>
                      <td>
                        <span className="chain-badge" title={chain}>
                          {c.chains?.[0] || 'DIRECT'}
                        </span>
                      </td>
                      <td>
                        <span className="rule-badge" title={c.rulePayload}>
                          {c.rule}
                        </span>
                      </td>
                      <td className="traffic-cell down-color">↓ {formatBytes(c.download)}</td>
                      <td className="traffic-cell up-color">↑ {formatBytes(c.upload)}</td>
                      <td>
                        <button
                          className="btn-icon-danger"
                          title="Разорвать соединение"
                          onClick={() => handleCloseOne(c.id)}
                        >
                          ✕
                        </button>
                      </td>
                    </tr>
                  )
                })}
              </tbody>
            </table>
          </div>
        </div>
      )}
    </div>
  )
}
