import { useCallback, useEffect, useMemo, useState } from 'react'
import { apiGet, apiPost } from '../api'
import { fmtBytes, fmtSpeed, type DeviceInfo, type PolicyInfo, type RoutingAssignmentInfo, type ServerInfo } from '../types'

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
  const [filter, setFilter] = useState('')
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const [limit, setLimit] = useState(25)
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState(false)
  const [batchPolicy, setBatchPolicy] = useState('')
  const [batchSpeed, setBatchSpeed] = useState(0)
  const [showOffline, setShowOffline] = useState(true)

  const load = useCallback(async () => {
    try {
      const [d, p, s, r] = await Promise.all([
        apiGet<{ devices: DeviceInfo[] }>('devices'),
        apiGet<{ policies: PolicyInfo[] }>('policies'),
        apiGet<{ servers: ServerInfo[] }>('servers'),
        apiGet<{ assignments: RoutingAssignmentInfo[] }>('routing'),
      ])
      setDevices(d.devices)
      setPolicies(p.policies)
      setServers(s.servers)
      setRouting(r.assignments)
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
                  <tr key={d.mac} className={d.is_current_device ? 'me' : ''}>
                    <td>
                      <input
                        type="checkbox"
                        checked={selected.has(d.mac)}
                        onChange={(e) => toggleSelect(d.mac, e.target.checked)}
                      />
                    </td>
                    <td>
                      <span className={'dot ' + (d.online ? 'on' : 'off')} />
                      <b>{d.name}</b>
                      {d.is_current_device && <span className="tag current">ВЫ</span>}
                      <div className="muted small">
                        {d.mac}{d.interface ? ` · ${d.interface}` : ''} · ↓{fmtBytes(d.rxbytes)} ↑{fmtBytes(d.txbytes)}
                      </div>
                    </td>
                    <td className="mono">{d.ip}</td>
                    <td>
                      <select className="select" value={d.policy} disabled={busy} onChange={(e) => applyPolicy([d.mac], e.target.value)}>
                        <option value={d.policy}>{d.policy_name}</option>
                        {policies
                          .filter((p) => p.id !== d.policy)
                          .map((p) => (
                            <option key={p.id} value={p.id}>{p.name}</option>
                          ))}
                      </select>
                    </td>
                    <td>
                      <select className="select" value="" disabled={busy} onChange={(e) => applySpeed([d.mac], Number(e.target.value))}>
                        <option value="">{fmtSpeed(d.speed_limit_kbps)}</option>
                        {SPEED_PRESETS.map((s) => (
                          <option key={s.value} value={s.value}>{s.label}</option>
                        ))}
                      </select>
                    </td>
                    <td>
                      <select
                        className="select"
                        value={assigned ? '__keep__' : 'default'}
                        disabled={busy}
                        onChange={(e) =>
                          applyServer(d.ip, d.name, e.target.value === '__keep__' ? assigned || '' : e.target.value)
                        }
                      >
                        <option value="default">По умолчанию (PROXY)</option>
                        {assigned && <option value="__keep__">{assigned}</option>}
                        {servers
                          .filter((s) => s.id !== assigned)
                          .map((s) => (
                            <option key={s.id} value={s.id}>{s.name}</option>
                          ))}
                      </select>
                    </td>
                  </tr>
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
    </section>
  )
}


