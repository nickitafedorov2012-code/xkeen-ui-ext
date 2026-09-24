import { useCallback, useEffect, useMemo, useState } from 'react'
import { apiGet, apiPost } from '../api'
import DeviceRow from './DeviceRow'
import DeviceRoutingModal from './DeviceRoutingModal'
import DeviceScheduleModal from './DeviceScheduleModal'
import PoliciesMap from './PoliciesMap'
import {
  type DeviceInfo,
  type DeviceRoutingEntry,
  type DeviceTraffic,
  type DeviceTrafficResponse,
  type PolicyInfo,
  type RoutingAssignmentInfo,
  type ServerInfo,
} from '../types'

interface Props {
  notify: (msg: string, isError?: boolean) => void
}

type SortColumn = 'device' | 'ip' | 'policy' | 'speed' | 'traffic' | 'server'

function fmtBytes(bytes: number): string {
  if (!bytes || bytes <= 0) return '0 B'
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`
}

export default function Devices({ notify }: Props) {
  const [devices, setDevices] = useState<DeviceInfo[]>([])
  const [policies, setPolicies] = useState<PolicyInfo[]>([])
  const [servers, setServers] = useState<ServerInfo[]>([])
  const [routing, setRouting] = useState<RoutingAssignmentInfo[]>([])
  const [drMap, setDrMap] = useState<Record<string, DeviceRoutingEntry>>({})
  const [devFailover, setDevFailover] = useState(false)
  const [trafficMap, setTrafficMap] = useState<Record<string, DeviceTraffic>>({})
  const [totalDownBytes, setTotalDownBytes] = useState(0)
  const [totalUpBytes, setTotalUpBytes] = useState(0)
  const [filter, setFilter] = useState('')
  const [statusFilter, setStatusFilter] = useState<'all' | 'online' | 'offline'>('all')
  const [offlineExpanded, setOfflineExpanded] = useState(false)
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState(false)

  // Периодический опрос трафика (каждые 2 сек)
  useEffect(() => {
    let active = true
    const pollTraffic = async () => {
      if (document.hidden) return
      try {
        const res = await apiGet<DeviceTrafficResponse>('devices/traffic')
        if (active && res) {
          setTrafficMap(res.devices || {})
          setTotalDownBytes(res.download_total || 0)
          setTotalUpBytes(res.upload_total || 0)
        }
      } catch {
        /* опрос трафика не критичен */
      }
    }

    pollTraffic()
    const timer = setInterval(pollTraffic, 3000)
    return () => {
      active = false
      clearInterval(timer)
    }
  }, [])

  // Сортировка колонок
  const [sortCol, setSortCol] = useState<SortColumn | null>(null)
  const [sortAsc, setSortAsc] = useState(true)

  // Модальные окна массовых действий
  const [batchPolicyOpen, setBatchPolicyOpen] = useState(false)
  const [batchServerOpen, setBatchServerOpen] = useState(false)

  // Переключение вида (Таблица / Карта)
  const [activeView, setActiveView] = useState<'table' | 'map'>('table')

  // Модальное окно расписания (⏰)
  const [scheduleTarget, setScheduleTarget] = useState<{ ip: string; name: string } | null>(null)

  // Модальное окно резервирования (⚙ Edit)
  const [drModal, setDrModal] = useState<{
    ip: string
    name: string
    servers: string[]
    threshold: number
    autoRestore: boolean
  } | null>(null)

  const load = useCallback(async () => {
    try {
      const [d, p, s, r, dr] = await Promise.all([
        apiGet<{ devices: DeviceInfo[] }>('devices'),
        apiGet<{ policies: PolicyInfo[] }>('policies'),
        apiGet<{ servers: ServerInfo[] }>('servers'),
        apiGet<{ assignments: RoutingAssignmentInfo[] }>('routing'),
        apiGet<{ routing: Record<string, DeviceRoutingEntry>; device_failover_enabled: boolean }>('device-routing').catch(() => ({
          routing: {} as Record<string, DeviceRoutingEntry>,
          device_failover_enabled: false,
        })),
      ])
      setDevices(d.devices)
      setPolicies(p.policies)
      setServers(s.servers)
      setRouting(r.assignments)
      setDrMap(dr.routing)
      setDevFailover(dr.device_failover_enabled)
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка загрузки устройств', true)
    } finally {
      setLoading(false)
    }
  }, [notify])

  useEffect(() => {
    load()
  }, [load])

  const serverByIp = useMemo(() => {
    const m = new Map<string, string>()
    for (const a of routing) m.set(a.ip, a.current_server)
    return m
  }, [routing])

  // Поиск по имени, IP, MAC
  const q = filter.trim().toLowerCase()
  const matchesSearch = useCallback(
    (d: DeviceInfo) =>
      !q ||
      d.name.toLowerCase().includes(q) ||
      d.ip.includes(q) ||
      d.mac.toLowerCase().includes(q),
    [q],
  )

  const onlineTotal = useMemo(() => devices.filter((d) => d.online || d.is_current_device).length, [devices])
  const offlineTotal = useMemo(() => devices.filter((d) => !d.online && !d.is_current_device).length, [devices])

  const onlineDevices = useMemo(
    () => devices.filter((d) => (d.online || d.is_current_device) && matchesSearch(d)),
    [devices, matchesSearch],
  )
  const offlineDevices = useMemo(
    () => devices.filter((d) => !d.online && !d.is_current_device && matchesSearch(d)),
    [devices, matchesSearch],
  )

  // Функция сортировки
  const sortDevices = useCallback(
    (list: DeviceInfo[]) => {
      if (!sortCol) return list
      return [...list].sort((a, b) => {
        if (sortCol === 'device') {
          return sortAsc ? a.name.localeCompare(b.name) : b.name.localeCompare(a.name)
        }
        if (sortCol === 'ip') {
          const pa = a.ip.split('.').map(Number)
          const pb = b.ip.split('.').map(Number)
          for (let i = 0; i < 4; i++) {
            if ((pa[i] || 0) !== (pb[i] || 0)) {
              return sortAsc ? (pa[i] || 0) - (pb[i] || 0) : (pb[i] || 0) - (pa[i] || 0)
            }
          }
          return 0
        }
        if (sortCol === 'policy') {
          const pa = (a.policy_name || a.policy).toLowerCase()
          const pb = (b.policy_name || b.policy).toLowerCase()
          return sortAsc ? pa.localeCompare(pb) : pb.localeCompare(pa)
        }
        if (sortCol === 'speed') {
          return sortAsc
            ? a.speed_limit_kbps - b.speed_limit_kbps
            : b.speed_limit_kbps - a.speed_limit_kbps
        }
        if (sortCol === 'traffic') {
          const ta = (trafficMap[a.ip]?.download_bytes || 0) + (trafficMap[a.ip]?.upload_bytes || 0)
          const tb = (trafficMap[b.ip]?.download_bytes || 0) + (trafficMap[b.ip]?.upload_bytes || 0)
          return sortAsc ? ta - tb : tb - ta
        }
        if (sortCol === 'server') {
          const sa = (serverByIp.get(a.ip) || '').toLowerCase()
          const sb = (serverByIp.get(b.ip) || '').toLowerCase()
          return sortAsc ? sa.localeCompare(sb) : sb.localeCompare(sa)
        }
        return 0
      })
    },
    [sortCol, sortAsc, serverByIp],
  )

  const sortedOnline = useMemo(() => sortDevices(onlineDevices), [onlineDevices, sortDevices])
  const sortedOffline = useMemo(() => sortDevices(offlineDevices), [offlineDevices, sortDevices])

  // Превью имён в аккордеоне офлайн-устройств
  const offlinePreview = useMemo(() => {
    if (offlineDevices.length === 0) return 'No offline devices'
    const names = offlineDevices.map((d) => d.name)
    if (names.length <= 3) return names.join(', ')
    return `${names.slice(0, 3).join(', ')} and ${names.length - 3} more...`
  }, [offlineDevices])

  // Выбор всех устройств в текущем виде
  const visibleDevices = useMemo(() => {
    if (statusFilter === 'online') return sortedOnline
    if (statusFilter === 'offline') return sortedOffline
    return offlineExpanded ? [...sortedOnline, ...sortedOffline] : sortedOnline
  }, [statusFilter, sortedOnline, sortedOffline, offlineExpanded])

  const allVisibleSelected =
    visibleDevices.length > 0 && visibleDevices.every((d) => selected.has(d.mac))

  const toggleSelectAll = (on: boolean) => {
    setSelected((prev) => {
      const next = new Set(prev)
      for (const d of visibleDevices) {
        if (on) next.add(d.mac)
        else next.delete(d.mac)
      }
      return next
    })
  }

  const toggleSelect = (mac: string, on: boolean) => {
    setSelected((prev) => {
      const next = new Set(prev)
      if (on) next.add(mac)
      else next.delete(mac)
      return next
    })
  }

  const handleSort = (col: SortColumn) => {
    if (sortCol === col) {
      if (sortAsc) setSortAsc(false)
      else {
        setSortCol(null)
        setSortAsc(true)
      }
    } else {
      setSortCol(col)
      setSortAsc(true)
    }
  }

  const applyPolicy = async (macs: string[], policy_id: string) => {
    if (!macs.length || !policy_id) return
    setBusy(true)
    try {
      const data = await apiPost<{ applied: number; errors: string[] }>('devices/policy', { macs, policy_id })
      notify(`Policy applied to ${data.applied} device(s)${data.errors.length ? `, errors: ${data.errors.length}` : ''}`, data.errors.length > 0)
      load()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Error applying policy', true)
    } finally {
      setBusy(false)
    }
  }

  const applySpeed = async (macs: string[], kbps: number) => {
    if (!macs.length) return
    setBusy(true)
    try {
      const data = await apiPost<{ applied: number }>('devices/speed', { macs, kbps })
      notify(`Speed limit applied to ${data.applied} device(s)`)
      load()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Error applying speed', true)
    } finally {
      setBusy(false)
    }
  }

  const applyServer = async (ip: string, name: string, server: string) => {
    setBusy(true)
    try {
      const data = await apiPost<{ applied: number }>('routing', {
        assignments: [{ ip, name, server: server === 'default' ? null : server }],
      })
      notify(`Routing updated (${data.applied})`)
      load()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Error updating routing', true)
    } finally {
      setBusy(false)
    }
  }

  const serverLabel = (id: string) => {
    const s = servers.find((x) => x.id === id)
    if (!s) return id
    return s.ping_ms > 0 ? `${s.name} · ${s.ping_ms} ms` : `${s.name} · —`
  }

  const openDrModal = (d: DeviceInfo, assigned?: string) => {
    const entry = drMap[d.ip]
    setDrModal({
      ip: d.ip,
      name: d.name,
      servers: entry?.servers?.length ? [...entry.servers] : assigned ? [assigned] : [],
      threshold: entry?.ping_threshold_ms || 300,
      autoRestore: entry?.auto_restore ?? true,
    })
  }

  const saveDr = async () => {
    if (!drModal) return
    setBusy(true)
    try {
      await apiPost('device-routing', {
        ip: drModal.ip,
        name: drModal.name,
        servers: drModal.servers,
        ping_threshold_ms: drModal.threshold,
        auto_restore: drModal.autoRestore,
      })
      notify(drModal.servers.length ? `Failover chain saved: ${drModal.servers.length} server(s)` : 'Routing cleared')
      setDrModal(null)
      load()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Error saving failover chain', true)
    } finally {
      setBusy(false)
    }
  }

  // Пакетные действия
  const handleBatchPolicyApply = async (policyId: string) => {
    await applyPolicy([...selected], policyId)
    setBatchPolicyOpen(false)
    setSelected(new Set())
  }

  const handleBatchServerApply = async (serverId: string) => {
    setBusy(true)
    try {
      const assignments = [...selected]
        .map((mac) => {
          const d = devices.find((x) => x.mac === mac)
          return {
            ip: d ? d.ip : '',
            name: d ? d.name : '',
            server: serverId === 'default' ? null : serverId,
          }
        })
        .filter((a) => !!a.ip)
      await apiPost('routing', { assignments })
      notify(`Server updated for ${assignments.length} device(s)`)
      setBatchServerOpen(false)
      setSelected(new Set())
      load()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Error changing server', true)
    } finally {
      setBusy(false)
    }
  }

  const handleBatchDelete = async () => {
    if (!confirm(`Reset custom server routing for ${selected.size} selected device(s)?`)) return
    setBusy(true)
    try {
      const assignments = [...selected]
        .map((mac) => {
          const d = devices.find((x) => x.mac === mac)
          return {
            ip: d ? d.ip : '',
            name: d ? d.name : '',
            server: null,
          }
        })
        .filter((a) => !!a.ip)
      await apiPost('routing', { assignments })
      notify(`Routing reset for ${assignments.length} device(s)`)
      setSelected(new Set())
      load()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Error resetting routing', true)
    } finally {
      setBusy(false)
    }
  }

  const sortIndicator = (col: SortColumn) => {
    if (sortCol !== col) return ' ⇅'
    return sortAsc ? ' ↑' : ' ↓'
  }

  return (
    <section className="devices-card">
      <div className="devices-toolbar-wrap">
        {/* Верхняя строка управления */}
        <div className="devices-toolbar">
          <div className="devices-search-box">
            <span className="devices-search-icon">🔍</span>
            <input
              type="text"
              className="devices-search-input"
              placeholder="Search devices..."
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
            />
          </div>

          <div className="devices-filter-pills">
            <button
              type="button"
              className={`devices-pill-btn ${statusFilter === 'online' ? 'active' : ''}`}
              onClick={() => setStatusFilter('online')}
            >
              <span className="devices-pill-dot online" />
              <span>Online</span>
              <span className="devices-pill-count">{onlineTotal}</span>
            </button>
            <button
              type="button"
              className={`devices-pill-btn ${statusFilter === 'offline' ? 'active' : ''}`}
              onClick={() => setStatusFilter('offline')}
            >
              <span className="devices-pill-dot offline" />
              <span>Offline</span>
              <span className="devices-pill-count">{offlineTotal}</span>
            </button>
            <button
              type="button"
              className={`devices-pill-btn ${statusFilter === 'all' ? 'active' : ''}`}
              onClick={() => setStatusFilter('all')}
            >
              <span>All</span>
              <span className="devices-pill-count">{devices.length}</span>
            </button>
          </div>

          <button
            type="button"
            className="devices-refresh-btn"
            onClick={load}
            disabled={busy}
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <path d="M21.5 2v6h-6M21.34 15.57a10 10 0 1 1-.57-8.38l5.67-5.67" />
            </svg>
            <span>Refresh</span>
          </button>

          <div className="devices-view-switcher" style={{ display: 'inline-flex', gap: 4, background: 'rgba(0,0,0,0.25)', padding: 3, borderRadius: 8 }}>
            <button
              type="button"
              className={`btn btn-xs ${activeView === 'table' ? 'btn-primary' : 'btn-secondary'}`}
              onClick={() => setActiveView('table')}
              title="Таблица устройств"
            >
              📋 Таблица
            </button>
            <button
              type="button"
              className={`btn btn-xs ${activeView === 'map' ? 'btn-primary' : 'btn-secondary'}`}
              onClick={() => setActiveView('map')}
              title="Интерактивная карта политик Keenetic"
            >
              🗺️ Карта политик
            </button>
          </div>
        </div>

        {/* Сводка количества и трафика */}
        <div className="devices-summary-text" style={{ display: 'flex', gap: 16, alignItems: 'center', flexWrap: 'wrap' }}>
          <span>{onlineTotal} online / {offlineTotal} offline out of {devices.length} devices</span>
          {(totalDownBytes > 0 || totalUpBytes > 0) && (
            <span style={{ fontFamily: 'Consolas, monospace', fontSize: 12, color: 'var(--accent)' }}>
              ⚡ Трафик: ↓ {fmtBytes(totalDownBytes)} · ↑ {fmtBytes(totalUpBytes)}
            </span>
          )}
        </div>
      </div>

      {loading ? (
        <p className="muted" style={{ padding: '20px 18px' }}>Загрузка устройств…</p>
      ) : activeView === 'map' ? (
        <PoliciesMap notify={notify} />
      ) : (
        <div className="devices-table-wrap">
          <table className="devices-table">
            <thead>
              <tr>
                <th>
                  <input
                    type="checkbox"
                    checked={allVisibleSelected}
                    onChange={(e) => toggleSelectAll(e.target.checked)}
                  />
                </th>
                <th className="sortable" onClick={() => handleSort('device')}>
                  Device{sortIndicator('device')}
                </th>
                <th className="sortable" onClick={() => handleSort('ip')}>
                  IP{sortIndicator('ip')}
                </th>
                <th className="sortable" onClick={() => handleSort('policy')}>
                  Policy{sortIndicator('policy')}
                </th>
                <th className="sortable" onClick={() => handleSort('speed')}>
                  Speed{sortIndicator('speed')}
                </th>
                <th className="sortable" onClick={() => handleSort('traffic')}>
                  Traffic{sortIndicator('traffic')}
                </th>
                <th className="sortable" onClick={() => handleSort('server')}>
                  Server{sortIndicator('server')}
                </th>
                <th />
              </tr>
            </thead>
            <tbody>
              {/* Секция Online */}
              {(statusFilter === 'all' || statusFilter === 'online') && (
                <>
                  <tr>
                    <td colSpan={8} style={{ padding: '12px 14px 4px', borderBottom: 'none' }}>
                      <span className="devices-group-header online">
                        Online ({sortedOnline.length})
                      </span>
                    </td>
                  </tr>
                  {sortedOnline.length === 0 && (
                    <tr>
                      <td colSpan={8} className="muted small" style={{ padding: '10px 14px' }}>
                        Нет устройств в сети
                      </td>
                    </tr>
                  )}
                  {sortedOnline.map((d) => (
                    <DeviceRow
                      key={d.mac}
                      d={d}
                      policies={policies}
                      servers={servers}
                      assigned={serverByIp.get(d.ip)}
                      drEntry={drMap[d.ip]}
                      devFailover={devFailover}
                      traffic={trafficMap[d.ip]}
                      busy={busy}
                      selected={selected.has(d.mac)}
                      onToggleSelect={toggleSelect}
                      applyPolicy={applyPolicy}
                      applySpeed={applySpeed}
                      applyServer={applyServer}
                      serverLabel={serverLabel}
                      openDrModal={openDrModal}
                      onOpenSchedule={(d) => setScheduleTarget({ ip: d.ip, name: d.name })}
                    />
                  ))}
                </>
              )}

              {/* Секция Offline */}
              {statusFilter === 'all' && (
                <>
                  <tr>
                    <td colSpan={8} style={{ padding: '16px 14px 4px', borderBottom: 'none' }}>
                      <div className="offline-section-title">OFFLINE SECTION</div>
                      <div
                        className="offline-accordion-row"
                        onClick={() => setOfflineExpanded((prev) => !prev)}
                      >
                        <span className={`offline-accordion-chevron ${offlineExpanded ? 'expanded' : ''}`}>
                          {offlineExpanded ? '⌄' : '›'}
                        </span>
                        <div className="offline-accordion-info">
                          <span className="offline-accordion-heading">
                            Offline ({sortedOffline.length})
                          </span>
                          <span className="offline-accordion-preview">
                            {offlinePreview}
                          </span>
                        </div>
                      </div>
                    </td>
                  </tr>
                  {offlineExpanded &&
                    sortedOffline.map((d) => (
                      <DeviceRow
                        key={d.mac}
                        d={d}
                        policies={policies}
                        servers={servers}
                        assigned={serverByIp.get(d.ip)}
                        drEntry={drMap[d.ip]}
                        devFailover={devFailover}
                        traffic={trafficMap[d.ip]}
                        busy={busy}
                        selected={selected.has(d.mac)}
                        onToggleSelect={toggleSelect}
                        applyPolicy={applyPolicy}
                        applySpeed={applySpeed}
                        applyServer={applyServer}
                        serverLabel={serverLabel}
                        openDrModal={openDrModal}
                        onOpenSchedule={(d) => setScheduleTarget({ ip: d.ip, name: d.name })}
                      />
                    ))}
                </>
              )}

              {statusFilter === 'offline' && (
                <>
                  <tr>
                    <td colSpan={8} style={{ padding: '12px 14px 4px', borderBottom: 'none' }}>
                      <span className="devices-group-header">
                        Offline ({sortedOffline.length})
                      </span>
                    </td>
                  </tr>
                  {sortedOffline.length === 0 && (
                    <tr>
                      <td colSpan={8} className="muted small" style={{ padding: '10px 14px' }}>
                        Нет офлайн-устройств
                      </td>
                    </tr>
                  )}
                  {sortedOffline.map((d) => (
                    <DeviceRow
                      key={d.mac}
                      d={d}
                      policies={policies}
                      servers={servers}
                      assigned={serverByIp.get(d.ip)}
                      drEntry={drMap[d.ip]}
                      devFailover={devFailover}
                      traffic={trafficMap[d.ip]}
                      busy={busy}
                      selected={selected.has(d.mac)}
                      onToggleSelect={toggleSelect}
                      applyPolicy={applyPolicy}
                      applySpeed={applySpeed}
                      applyServer={applyServer}
                      serverLabel={serverLabel}
                      openDrModal={openDrModal}
                      onOpenSchedule={(d) => setScheduleTarget({ ip: d.ip, name: d.name })}
                    />
                  ))}
                </>
              )}
            </tbody>
          </table>
        </div>
      )}

      {/* Плавающая нижняя панель действий при выборе устройств */}
      {selected.size > 0 && (
        <div className="floating-batch-bar">
          <span className="floating-batch-text">{selected.size} selected</span>
          <button
            type="button"
            className="floating-batch-btn"
            onClick={() => setBatchPolicyOpen(true)}
          >
            Change Policy
          </button>
          <button
            type="button"
            className="floating-batch-btn"
            onClick={() => setBatchServerOpen(true)}
          >
            Change Server
          </button>
          <button
            type="button"
            className="floating-batch-btn danger"
            onClick={handleBatchDelete}
          >
            Delete
          </button>
        </div>
      )}

      {/* Модальное окно смены политики для выбранных */}
      {batchPolicyOpen && (
        <div className="modal-overlay" onClick={() => setBatchPolicyOpen(false)}>
          <div className="modal sm" onClick={(e) => e.stopPropagation()}>
            <h2>Change Policy ({selected.size} devices)</h2>
            <div className="modal-list">
              {policies.map((p) => (
                <button
                  key={p.id}
                  className="btn wide"
                  style={{ textAlign: 'left', marginBottom: 4 }}
                  onClick={() => handleBatchPolicyApply(p.id)}
                >
                  {p.name}
                </button>
              ))}
            </div>
            <div className="modal-actions">
              <button className="btn" onClick={() => setBatchPolicyOpen(false)}>Cancel</button>
            </div>
          </div>
        </div>
      )}

      {/* Модальное окно смены сервера для выбранных */}
      {batchServerOpen && (
        <div className="modal-overlay" onClick={() => setBatchServerOpen(false)}>
          <div className="modal sm" onClick={(e) => e.stopPropagation()}>
            <h2>Change Server ({selected.size} devices)</h2>
            <div className="modal-list">
              <button
                className="btn wide"
                style={{ textAlign: 'left', marginBottom: 4 }}
                onClick={() => handleBatchServerApply('default')}
              >
                Default (PROXY)
              </button>
              {servers.map((s) => (
                <button
                  key={s.id}
                  className="btn wide"
                  style={{ textAlign: 'left', marginBottom: 4 }}
                  onClick={() => handleBatchServerApply(s.id)}
                >
                  {s.name} · {s.ping_ms > 0 ? `${s.ping_ms} ms` : '—'}
                </button>
              ))}
            </div>
            <div className="modal-actions">
              <button className="btn" onClick={() => setBatchServerOpen(false)}>Cancel</button>
            </div>
          </div>
        </div>
      )}

      {/* Модальное окно резервирования и failover устройства */}
      {drModal && (
        <DeviceRoutingModal
          modal={drModal}
          servers={servers}
          busy={busy}
          hasExisting={!!drMap[drModal.ip]}
          onChange={setDrModal}
          onClose={() => setDrModal(null)}
          onSave={saveDr}
          serverLabel={serverLabel}
        />
      )}

      {/* Модальное окно расписания устройства */}
      {scheduleTarget && (
        <DeviceScheduleModal
          isOpen={true}
          deviceIp={scheduleTarget.ip}
          deviceName={scheduleTarget.name}
          availableServers={servers.map((s) => s.name)}
          onClose={() => setScheduleTarget(null)}
          notify={notify}
        />
      )}
    </section>
  )
}
