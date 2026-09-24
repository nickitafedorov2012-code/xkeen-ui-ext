import { memo, useEffect, useState } from 'react'
import {
  type DeviceInfo,
  type DeviceRoutingEntry,
  type DeviceTraffic,
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
  traffic?: DeviceTraffic
  busy: boolean
  selected: boolean
  onToggleSelect: (mac: string, on: boolean) => void
  applyPolicy: (macs: string[], policyId: string) => void
  applySpeed: (macs: string[], kbps: number) => void
  applyServer: (ip: string, name: string, server: string) => void
  serverLabel: (id: string) => string
  openDrModal: (d: DeviceInfo, assigned?: string) => void
  onOpenSchedule?: (d: DeviceInfo) => void
}

function fmtBytes(bytes: number): string {
  if (!bytes || bytes <= 0) return '0 B'
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`
}

function SpeedCell({
  mac,
  kbps,
  busy,
  applySpeed,
}: {
  mac: string
  kbps?: number
  busy: boolean
  applySpeed: (macs: string[], kbps: number) => void
}) {
  const formatInitial = (k?: number) => {
    if (!k || k <= 0) return ''
    const mbps = k / 1024
    return k % 1024 === 0 ? String(mbps) : mbps.toFixed(1)
  }

  const [val, setVal] = useState(() => formatInitial(kbps))
  const [editing, setEditing] = useState(false)

  useEffect(() => {
    if (!editing) {
      setVal(formatInitial(kbps))
    }
  }, [kbps, editing])

  const commit = () => {
    setEditing(false)
    const trimmed = val.trim()
    if (!trimmed || trimmed === '0') {
      if (kbps && kbps > 0) {
        applySpeed([mac], 0)
      }
      return
    }
    const num = parseFloat(trimmed.replace(',', '.'))
    if (!isNaN(num) && num > 0) {
      const nextKbps = Math.round(num * 1024)
      if (nextKbps !== kbps) {
        applySpeed([mac], nextKbps)
      }
    } else {
      setVal(formatInitial(kbps))
    }
  }

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') {
      e.currentTarget.blur()
    } else if (e.key === 'Escape') {
      setVal(formatInitial(kbps))
      setEditing(false)
    }
  }

  const clearLimit = (e: React.MouseEvent) => {
    e.stopPropagation()
    setVal('')
    applySpeed([mac], 0)
  }

  return (
    <div className="device-speed-input-wrap">
      <input
        type="text"
        inputMode="decimal"
        className="device-speed-input"
        placeholder="—"
        value={val}
        disabled={busy}
        onFocus={() => setEditing(true)}
        onChange={(e) => setVal(e.target.value)}
        onBlur={commit}
        onKeyDown={handleKeyDown}
        title="Ограничение скорости в Мбит/с. Введите число (например, 10, 50, 100) и нажмите Enter или кликните вне поля. 0 или пусто = без лимита."
      />
      <span className="device-speed-unit">Мб/с</span>
      {kbps && kbps > 0 && !busy ? (
        <button
          type="button"
          className="device-speed-clear-btn"
          title="Снять ограничение скорости (без лимита)"
          onClick={clearLimit}
        >
          ✕
        </button>
      ) : null}
    </div>
  )
}

/// Строка таблицы устройств по эталонному макету.
const DeviceRow = memo(function DeviceRow({
  d,
  policies,
  servers,
  assigned,
  drEntry,
  devFailover,
  traffic,
  busy,
  selected,
  onToggleSelect,
  applyPolicy,
  applySpeed,
  applyServer,
  serverLabel,
  openDrModal,
  onOpenSchedule,
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
        <SpeedCell
          mac={d.mac}
          kbps={d.speed_limit_kbps}
          busy={busy}
          applySpeed={applySpeed}
        />
      </td>
      <td>
        {traffic && (traffic.download_bytes > 0 || traffic.upload_bytes > 0 || traffic.active_connections > 0) ? (
          <div
            className="device-traffic-cell"
            title={`Всего скачано: ${fmtBytes(traffic.download_bytes)}\nВсего отдано: ${fmtBytes(traffic.upload_bytes)}\nАктивных соединений: ${traffic.active_connections}${traffic.recent_hosts?.length ? '\nХосты: ' + traffic.recent_hosts.slice(0, 5).join(', ') : ''}`}
            style={{ display: 'flex', flexDirection: 'column', gap: 2, fontSize: 11, fontFamily: 'Consolas, monospace' }}
          >
            <span style={{ color: traffic.download_bytes > 0 ? '#38bdf8' : 'var(--muted)' }}>
              ↓ {fmtBytes(traffic.download_bytes)}
            </span>
            <span style={{ color: traffic.upload_bytes > 0 ? '#a855f7' : 'var(--muted)' }}>
              ↑ {fmtBytes(traffic.upload_bytes)}
            </span>
          </div>
        ) : (
          <span className="muted small" style={{ fontSize: 11 }}>—</span>
        )}
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
      <td style={{ textAlign: 'center', whiteSpace: 'nowrap' }}>
        <button
          type="button"
          className="device-gear-btn"
          title="Настройка резервирования (Failover)"
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
        <button
          type="button"
          className="device-gear-btn"
          title="Расписание (автоблокировка/переключение по времени)"
          disabled={busy}
          onClick={() => onOpenSchedule?.(d)}
          style={{ marginLeft: 6 }}
        >
          ⏰
        </button>
      </td>
    </tr>
  )
})

export default DeviceRow
