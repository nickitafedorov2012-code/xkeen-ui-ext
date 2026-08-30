import { useCallback, useEffect, useMemo, useState } from 'react'
import { apiGet, apiPost, apiPut } from '../api'
import DeviceRow from './DeviceRow'
import DeviceRoutingModal from './DeviceRoutingModal'
import {
  type DeviceInfo,
  type DeviceRoutingEntry,
  type PolicyInfo,
  type RoutingAssignmentInfo,
  type ServerInfo,
} from '../types'

interface Props {
  notify: (msg: string, isError?: boolean) => void
}

const SPEED_PRESETS: { label: string; value: number }[] = [
  { label: 'Без лимита', value: 0 },
  { label: '10 Мбит/с', value: 10240 },
  { label: '30 Мбит/с', value: 30720 },
  { label: '100 Мбит/с', value: 102400 },
]

export default function Devices({ notify }: Props) {
  const [devices, setDevices] = useState<DeviceInfo[]>([])
  const [policies, setPolicies] = useState<PolicyInfo[]>([])
  const [servers, setServers] = useState<ServerInfo[]>([])
  const [routing, setRouting] = useState<RoutingAssignmentInfo[]>([])
  const [drMap, setDrMap] = useState<Record<string, DeviceRoutingEntry>>({})
  const [devFailover, setDevFailover] = useState(false)
  const [filter, setFilter] = useState('')
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const [limit, setLimit] = useState(25)
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState(false)
  const [batchPolicy, setBatchPolicy] = useState('')
  const [batchSpeed, setBatchSpeed] = useState(0)
  const [showOffline, setShowOffline] = useState(true)
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

  const filtered = useMemo(() => {
    const q = filter.trim().toLowerCase()
    return devices.filter(
      (d) =>
        (showOffline || d.online || d.is_current_device) &&
        (!q || d.name.toLowerCase().includes(q) || d.ip.includes(q) || d.mac.includes(q)),
    )
  }, [devices, filter, showOffline])

  const toggleSelect = (mac: string, on: boolean) => {
    setSelected((prev) => {
      const next = new Set(prev)
      if (on) next.add(mac)
      else next.delete(mac)
      return next
    })
  }

  const applyPolicy = async (macs: string[], policy_id: string) => {
    if (!macs.length || !policy_id) return
    setBusy(true)
    try {
      const data = await apiPost<{ applied: number; errors: string[] }>('devices/policy', { macs, policy_id })
      notify(`Политика применена к ${data.applied} устр.${data.errors.length ? `, ошибок: ${data.errors.length}` : ''}`, data.errors.length > 0)
      load()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка', true)
    } finally {
      setBusy(false)
    }
  }

  const applySpeed = async (macs: string[], kbps: number) => {
    if (!macs.length) return
    setBusy(true)
    try {
      const data = await apiPost<{ applied: number }>('devices/speed', { macs, kbps })
      notify(`Скорость применена к ${data.applied} устр.`)
      load()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка', true)
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
      notify(`Маршрутизация обновлена (${data.applied})`)
      load()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка', true)
    } finally {
      setBusy(false)
    }
  }

  // Подпись сервера с пингом для дропдаунов.
  const serverLabel = (id: string) => {
    const s = servers.find((x) => x.id === id)
    if (!s) return id
    return s.ping_ms > 0 ? `${s.name} · ${s.ping_ms} мс` : `${s.name} · —`
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
      notify(drModal.servers.length ? `Цепочка сохранена: ${drModal.servers.length} сервер(ов)` : 'Маршрутизация снята')
      setDrModal(null)
      load()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка сохранения', true)
    } finally {
      setBusy(false)
    }
  }

  const toggleDevFailover = async (on: boolean) => {
    setDevFailover(on)
    try {
      await apiPut('settings', { failover: { device_failover_enabled: on } })
      notify(on ? 'Per-device failover включён' : 'Per-device failover выключен')
    } catch (e) {
      setDevFailover(!on)
      notify(e instanceof Error ? e.message : 'Ошибка', true)
    }
  }

  const selectedMacs = [...selected]

  return (
    <section className="card">
      <div className="toolbar">
        <input
          className="input"
          placeholder="Поиск: имя / IP / MAC…"
          value={filter}
          onChange={(e) => {
            setFilter(e.target.value)
            setLimit(25)
          }}
        />
        <label className="check">
          <input type="checkbox" checked={showOffline} onChange={(e) => setShowOffline(e.target.checked)} />
          показывать офлайн
        </label>
        <label className="check" title="Мониторинг цепочек сервер+резерв: смена сервера при отвале или пинге выше порога">
          <input type="checkbox" checked={devFailover} onChange={(e) => toggleDevFailover(e.target.checked)} />
          ⚡ failover устройств
        </label>
        <button className="btn" onClick={load} disabled={busy}>🔄 Обновить</button>
        <span className="muted">{filtered.length} шт.</span>
      </div>

      {selectedMacs.length > 0 && (
        <div className="batch-bar">
          <b>Выбрано: {selectedMacs.length}</b>
          <select className="select" value={batchPolicy} onChange={(e) => setBatchPolicy(e.target.value)}>
            <option value="">— политика —</option>
            {policies.map((p) => (
              <option key={p.id} value={p.id}>{p.name}</option>
            ))}
          </select>
          <button className="btn sm" disabled={busy || !batchPolicy} onClick={() => applyPolicy(selectedMacs, batchPolicy)}>
            Применить политику
          </button>
          <select className="select" value={batchSpeed} onChange={(e) => setBatchSpeed(Number(e.target.value))}>
            {SPEED_PRESETS.map((s) => (
              <option key={s.value} value={s.value}>{s.label}</option>
            ))}
          </select>
          <button className="btn sm" disabled={busy} onClick={() => applySpeed(selectedMacs, batchSpeed)}>
            Применить скорость
          </button>
          <button className="btn sm ghost" onClick={() => setSelected(new Set())}>Снять выбор</button>
        </div>
      )}

      {loading ? (
        <p className="muted">Загрузка…</p>
      ) : (
        <>
          <table className="devices">
            <thead>
              <tr>
                <th></th>
                <th>Устройство</th>
                <th>IP</th>
                <th>Политика</th>
                <th>Скорость</th>
                <th>Сервер (раздельная)</th>
              </tr>
            </thead>
            <tbody>
              {filtered.slice(0, limit).map((d) => {
                const assigned = serverByIp.get(d.ip)
                return (
                  <DeviceRow
                    key={d.mac}
                    d={d}
                    policies={policies}
                    servers={servers}
                    assigned={assigned}
                    drEntry={drMap[d.ip]}
                    devFailover={devFailover}
                    busy={busy}
                    selected={selected.has(d.mac)}
                    onToggleSelect={toggleSelect}
                    applyPolicy={applyPolicy}
                    applySpeed={applySpeed}
                    applyServer={applyServer}
                    serverLabel={serverLabel}
                    openDrModal={openDrModal}
                  />
                )
              })}
            </tbody>
          </table>
          {filtered.length > limit && (
            <button className="btn wide" onClick={() => setLimit(limit + 25)}>
              Показать ещё ({filtered.length - limit})
            </button>
          )}
        </>
      )}

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
    </section>
  )
}


