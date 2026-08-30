import {
  fmtBytes,
  fmtSpeed,
  type DeviceInfo,
  type DeviceRoutingEntry,
  type PolicyInfo,
  type ServerInfo,
} from '../types'

interface Props {
  d: DeviceInfo
  policies: PolicyInfo[]
  servers: ServerInfo[]
  assigned?: string
  drEntry?: DeviceRoutingEntry
  devFailover: boolean
  busy: boolean
  selected: boolean
  onToggleSelect: (mac: string, on: boolean) => void
  applyPolicy: (macs: string[], policyId: string) => void
  applySpeed: (macs: string[], kbps: number) => void
  applyServer: (ip: string, name: string, server: string) => void
  serverLabel: (id: string) => string
  openDrModal: (d: DeviceInfo, assigned?: string) => void
}

/// Строка таблицы устройств (вынесено из Devices.tsx для читаемости
/// и ускорения React Reconciliation).
export default function DeviceRow({
  d,
  policies,
  servers,
  assigned,
  drEntry,
  devFailover,
  busy,
  selected,
  onToggleSelect,
  applyPolicy,
  applySpeed,
  applyServer,
  serverLabel,
  openDrModal,
}: Props) {
  return (
    <tr className={d.is_current_device ? 'me' : ''}>
      <td>
        <input
          type="checkbox"
          checked={selected}
          onChange={(e) => onToggleSelect(d.mac, e.target.checked)}
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
        <div style={{ display: 'flex', gap: 4, alignItems: 'center' }}>
          <select
            className="select"
            style={{ flex: 1 }}
            value={assigned ? '__keep__' : 'default'}
            disabled={busy}
            onChange={(e) =>
              applyServer(d.ip, d.name, e.target.value === '__keep__' ? assigned || '' : e.target.value)
            }
          >
            <option value="default">По умолчанию (PROXY)</option>
            {assigned && <option value="__keep__">{serverLabel(assigned)}</option>}
            {servers
              .filter((s) => s.id !== assigned)
              .map((s) => (
                <option key={s.id} value={s.id}>{serverLabel(s.id)}</option>
              ))}
          </select>
          <button
            className="btn sm ghost"
            title="Резервные серверы и порог пинга"
            disabled={busy}
            onClick={() => openDrModal(d, assigned)}
          >
            ⚙
          </button>
        </div>
        {(drEntry?.servers?.length ?? 0) > 1 && (
          <div className="muted small">
            резерв: {drEntry!.servers.slice(1).map(serverLabel).join(', ')}
            {devFailover ? ` · порог ${drEntry!.ping_threshold_ms || 300} мс` : ' · failover выкл'}
          </div>
        )}
      </td>
    </tr>
  )
}

const SPEED_PRESETS: { label: string; value: number }[] = [
  { label: 'Без лимита', value: 0 },
  { label: '10 Мбит/с', value: 10240 },
  { label: '30 Мбит/с', value: 30720 },
  { label: '100 Мбит/с', value: 102400 },
]
