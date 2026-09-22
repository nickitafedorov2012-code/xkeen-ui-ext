import { memo } from 'react'
import {
  fmtSpeed,
  SPEED_PRESETS,
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

/// Строка таблицы устройств по эталонному макету.
const DeviceRow = memo(function DeviceRow({
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
    <tr className={`device-row ${selected ? 'selected' : ''} ${d.is_current_device ? 'me' : ''}`}>
      <td>
        <input
          type="checkbox"
          checked={selected}
          onChange={(e) => onToggleSelect(d.mac, e.target.checked)}
        />
      </td>
      <td>
        <div className="device-name-wrap">
          <span className={`device-dot ${d.online ? 'online' : 'offline'}`} />
          <b className="device-name-text" title={`${d.mac}${d.interface ? ` · ${d.interface}` : ''}`}>{d.name}</b>
          {d.is_current_device && <span className="badge-you">you</span>}
        </div>
      </td>
      <td>
        <span className="device-ip-text">{d.ip}</span>
      </td>
      <td>
        <select
          className="device-select"
          value={d.policy}
          disabled={busy}
          onChange={(e) => applyPolicy([d.mac], e.target.value)}
        >
          <option value={d.policy}>{d.policy_name}</option>
          {policies
            .filter((p) => p.id !== d.policy)
            .map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
        </select>
      </td>
      <td>
        <select
          className="device-select"
          value={d.speed_limit_kbps ? String(d.speed_limit_kbps) : ''}
          disabled={busy}
          onChange={(e) => applySpeed([d.mac], Number(e.target.value))}
        >
          <option value="">{d.speed_limit_kbps ? fmtSpeed(d.speed_limit_kbps) : '-'}</option>
          {SPEED_PRESETS.map((s) => (
            <option key={s.value} value={s.value}>
              {s.label}
            </option>
          ))}
        </select>
      </td>
      <td>
        <select
          className="device-select"
          value={assigned ? '__keep__' : 'default'}
          disabled={busy}
          onChange={(e) =>
            applyServer(d.ip, d.name, e.target.value === '__keep__' ? assigned || '' : e.target.value)
          }
        >
          <option value="default">Default (PROXY)</option>
          {assigned && <option value="__keep__">{serverLabel(assigned)}</option>}
          {servers
            .filter((s) => s.id !== assigned)
            .map((s) => (
              <option key={s.id} value={s.id}>
                {serverLabel(s.id)}
              </option>
            ))}
        </select>
        {(drEntry?.servers?.length ?? 0) > 1 && (
          <div className="device-reserve-hint">
            резерв: {drEntry!.servers.slice(1).map(serverLabel).join(', ')}
            {devFailover ? ` · ${drEntry!.ping_threshold_ms || 300} мс` : ''}
          </div>
        )}
      </td>
      <td style={{ textAlign: 'center' }}>
        <button
          type="button"
          className="device-gear-btn"
          title="Edit"
          disabled={busy}
          onClick={() => openDrModal(d, assigned)}
        >
          <svg
            width="15"
            height="15"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <circle cx="12" cy="12" r="3" />
            <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z" />
          </svg>
        </button>
      </td>
    </tr>
  )
})

export default DeviceRow
