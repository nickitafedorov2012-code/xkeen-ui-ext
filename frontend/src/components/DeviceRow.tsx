import { memo, useEffect, useRef, useState } from 'react'
import {
  type DeviceInfo,
  type DeviceRoutingEntry,
  type DeviceTraffic,
  type PolicyInfo,
  type ServerInfo,
} from '../types'

interface CustomSelectOption {
  value: string
  label: string
}

function CustomSelect({
  value,
  options,
  disabled,
  onChange,
  className,
}: {
  value: string
  options: CustomSelectOption[]
  disabled?: boolean
  onChange: (val: string) => void
  className?: string
}) {
  const [open, setOpen] = useState(false)
  const containerRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!open) return
    const handleClick = (e: MouseEvent) => {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        setOpen(false)
      }
    }
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpen(false)
    }
    document.addEventListener('mousedown', handleClick)
    document.addEventListener('keydown', handleKey)
    return () => {
      document.removeEventListener('mousedown', handleClick)
      document.removeEventListener('keydown', handleKey)
    }
  }, [open])

  const selectedOpt = options.find((o) => o.value === value) || options[0]

  return (
    <div ref={containerRef} className="custom-select-container" style={{ position: 'relative', width: '100%' }}>
      {/* Скрытый нативный select для совместимости с тестами и формами */}
      <select
        className="device-select"
        value={value}
        disabled={disabled}
        onChange={(e) => onChange(e.target.value)}
        style={{
          position: 'absolute',
          opacity: 0,
          pointerEvents: 'none',
          width: '100%',
          height: '100%',
          left: 0,
          top: 0,
          zIndex: -1,
        }}
        tabIndex={-1}
        aria-hidden="true"
      >
        {options.map((o) => (
          <option key={o.value} value={o.value}>
            {o.label}
          </option>
        ))}
      </select>

      {/* Стилизованная темная кнопка-триггер без системных всплывающих меню */}
      <button
        type="button"
        disabled={disabled}
        onClick={() => !disabled && setOpen((prev) => !prev)}
        className={`custom-select-trigger ${open ? 'open' : ''} ${className || ''}`}
        style={{
          width: '100%',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          height: 32,
          padding: '0 10px',
          background: open ? '#171e2c' : '#141923',
          border: open ? '1px solid #38bdf8' : '1px solid #232c3f',
          borderRadius: 6,
          color: disabled ? 'var(--muted)' : '#cbd5e1',
          fontSize: '12.5px',
          cursor: disabled ? 'not-allowed' : 'pointer',
          outline: 'none',
          boxShadow: open ? '0 0 0 2px rgba(56, 189, 248, 0.2)' : 'none',
          transition: 'all 0.15s ease',
          userSelect: 'none',
        }}
      >
        <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', textAlign: 'left' }}>
          {selectedOpt?.label || value}
        </span>
        <svg
          width="12"
          height="12"
          viewBox="0 0 24 24"
          fill="none"
          stroke="#94a3b8"
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round"
          style={{
            flexShrink: 0,
            marginLeft: 6,
            transform: open ? 'rotate(180deg)' : 'rotate(0deg)',
            transition: 'transform 0.15s ease',
          }}
        >
          <polyline points="6 9 12 15 18 9" />
        </svg>
      </button>

      {/* Выпадающее меню в стиле темного стекла */}
      {open && (
        <div
          className="custom-select-menu"
          style={{
            position: 'absolute',
            top: 'calc(100% + 4px)',
            left: 0,
            width: '100%',
            minWidth: 180,
            maxHeight: 220,
            overflowY: 'auto',
            background: 'rgba(20, 25, 35, 0.98)',
            backdropFilter: 'blur(16px)',
            border: '1px solid rgba(255, 255, 255, 0.12)',
            borderRadius: 8,
            boxShadow: '0 10px 30px rgba(0, 0, 0, 0.75)',
            zIndex: 100,
            padding: 4,
          }}
        >
          {options.map((opt) => {
            const isSelected = opt.value === value
            return (
              <div
                key={opt.value}
                onClick={() => {
                  onChange(opt.value)
                  setOpen(false)
                }}
                className={`custom-select-option ${isSelected ? 'selected' : ''}`}
                style={{
                  padding: '7px 10px',
                  borderRadius: 6,
                  fontSize: '12px',
                  color: isSelected ? '#38bdf8' : '#e2e8f0',
                  background: isSelected ? 'rgba(56, 189, 248, 0.12)' : 'transparent',
                  fontWeight: isSelected ? 600 : 400,
                  cursor: 'pointer',
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'space-between',
                  transition: 'background 0.12s ease',
                }}
                onMouseEnter={(e) => {
                  if (!isSelected) (e.currentTarget as HTMLElement).style.background = 'rgba(255, 255, 255, 0.05)'
                }}
                onMouseLeave={(e) => {
                  if (!isSelected) (e.currentTarget as HTMLElement).style.background = 'transparent'
                }}
              >
                <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                  {opt.label}
                </span>
                {isSelected && (
                  <svg
                    width="12"
                    height="12"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="#38bdf8"
                    strokeWidth="2.5"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    style={{ flexShrink: 0, marginLeft: 6 }}
                  >
                    <polyline points="20 6 9 17 4 12" />
                  </svg>
                )}
              </div>
            )
          })}
        </div>
      )}
    </div>
  )
}

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
  zapretActive?: boolean
  onToggleZapret?: (ip: string, mac: string, enabled: boolean) => void
  gamingActive?: boolean
  onToggleGaming?: (mac: string, ip: string, enabled: boolean) => void
  isGamingCompat?: boolean
  whitelistActive?: boolean
  onToggleWhitelist?: (ip: string, mac: string, enabled: boolean) => void
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
  zapretActive = true,
  onToggleZapret,
  gamingActive = false,
  onToggleGaming,
  isGamingCompat = false,
  whitelistActive = false,
  onToggleWhitelist,
}: Props) {
  const cleanName = d.name.replace(/\s*-\s*Home network.*$/i, '').trim()
  const isOfflineOrNoIp = !d.online || !d.ip || d.ip === '0.0.0.0' || d.ip === '—'
  const cleanIp = isOfflineOrNoIp ? '—' : d.ip

  return (
    <tr className={`device-row ${selected ? 'selected' : ''} ${d.is_current_device ? 'me' : ''} ${isOfflineOrNoIp ? 'device-row-offline' : ''}`}>
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
          <b className="device-name-text" title={`${d.mac}${d.interface ? ` · ${d.interface}` : ''}`}>{cleanName || d.name}</b>
          {d.is_current_device && (
            <span className="badge-you" title="Текущее устройство (с которого открыта панель)">
              <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" style={{ flexShrink: 0 }}>
                <rect x="2" y="3" width="20" height="14" rx="2" ry="2" />
                <line x1="8" y1="21" x2="16" y2="21" />
                <line x1="12" y1="17" x2="12" y2="21" />
              </svg>
              Это устройство
            </span>
          )}
        </div>
      </td>
      <td>
        <span className="device-ip-text">{cleanIp}</span>
      </td>
      <td>
        <CustomSelect
          value={d.policy}
          disabled={busy}
          onChange={(newVal) => applyPolicy([d.mac], newVal)}
          options={[
            { value: d.policy, label: d.policy_name },
            ...policies
              .filter((p) => p.id !== d.policy)
              .map((p) => ({ value: p.id, label: p.name })),
          ]}
        />
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
        <div style={{ display: 'flex', alignItems: 'center', gap: 6, flexWrap: 'wrap', minWidth: 160 }}>
          <CustomSelect
            value={assigned || 'default'}
            disabled={busy || isOfflineOrNoIp}
            onChange={(newVal) => {
              if (isOfflineOrNoIp) return
              applyServer(d.ip, d.name, newVal)
            }}
            options={[
              { value: 'default', label: '🌐 Общий активный узел' },
              ...(assigned && assigned !== 'default' && assigned !== 'PROXY' && !servers.some((s) => s.id === assigned)
                ? [{ value: assigned, label: serverLabel(assigned) }]
                : []),
              ...servers
                .filter((s) => !s.is_pool && s.id !== 'PROXY' && s.id !== 'GLOBAL' && s.id !== 'Fastest' && s.id !== 'Fallback')
                .map((s) => ({ value: s.id, label: serverLabel(s.id) })),
            ]}
          />
          <select
            className="device-select"
            style={{ display: 'none' }}
            tabIndex={-1}
            aria-hidden="true"
            value={assigned || 'default'}
            onChange={(e) => {
              if (isOfflineOrNoIp) return
              applyServer(d.ip, d.name, e.target.value)
            }}
          >
            <option value="default">🌐 Общий активный узел</option>
            {assigned && assigned !== 'default' && assigned !== 'PROXY' && !servers.some((s) => s.id === assigned) && (
              <option value={assigned}>{serverLabel(assigned)}</option>
            )}
            {servers
              .filter((s) => !s.is_pool && s.id !== 'PROXY' && s.id !== 'GLOBAL' && s.id !== 'Fastest' && s.id !== 'Fallback')
              .map((s) => (
                <option key={s.id} value={s.id}>
                  {serverLabel(s.id)}
                </option>
              ))}
          </select>
          {isGamingCompat && (
            <span
              className="badge badge-gaming-compat"
              data-testid="badge-gaming-compat"
              title="100% Gaming Proxy: весь TCP/UDP трафик устройства направляется через игровой туннель"
              style={{
                fontSize: 11,
                fontWeight: 600,
                color: '#ec4899',
                background: 'rgba(236, 72, 153, 0.12)',
                border: '1px solid rgba(236, 72, 153, 0.3)',
                borderRadius: 6,
                padding: '2px 6px',
                whiteSpace: 'nowrap',
              }}
            >
              🎮 100% Gaming
            </span>
          )}
        </div>
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
          className={`device-zapret-btn ${zapretActive ? 'active' : 'inactive'}`}
          disabled={busy}
          onClick={() => onToggleZapret?.(d.ip, d.mac, !zapretActive)}
          title={
            zapretActive
              ? `Zapret DPI включен для ${d.name} (${d.ip || d.mac}). Трафик обходит блокировки напрямую. Нажмите для отключения.`
              : `Zapret DPI выключен для ${d.name} (${d.ip || d.mac}). Трафик исключен из NFQUEUE zapret. Нажмите для включения.`
          }
        >
          <span className="device-zapret-icon">
            <svg
              width="13"
              height="13"
              viewBox="0 0 24 24"
              fill={zapretActive ? 'currentColor' : 'none'}
              stroke="currentColor"
              strokeWidth="2.2"
              strokeLinecap="round"
              strokeLinejoin="round"
            >
              <polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2" />
            </svg>
          </span>
          <span className="device-zapret-label">{zapretActive ? 'Zapret ON' : 'Zapret OFF'}</span>
        </button>
      </td>
      <td style={{ textAlign: 'center', whiteSpace: 'nowrap' }}>
        <button
          type="button"
          className={`device-gaming-btn ${gamingActive ? 'active' : 'inactive'}`}
          data-testid={`device-gaming-btn-${d.mac}`}
          disabled={busy}
          onClick={() => onToggleGaming?.(d.mac, d.ip, !gamingActive)}
          title={
            gamingActive
              ? `Игровой режим включен для ${d.name} (${d.ip || d.mac}). Нажмите для отключения.`
              : `Игровой режим выключен для ${d.name} (${d.ip || d.mac}). Нажмите для включения.`
          }
        >
          <span className="device-gaming-icon">🎮</span>
          <span className="device-gaming-label">{gamingActive ? 'Gaming ON' : 'Gaming OFF'}</span>
        </button>
      </td>
      <td style={{ textAlign: 'center', whiteSpace: 'nowrap' }}>
        <button
          type="button"
          className={`device-whitelist-btn ${whitelistActive ? 'active' : 'inactive'}`}
          data-testid={`device-whitelist-btn-${d.mac}`}
          disabled={busy}
          onClick={() => onToggleWhitelist?.(d.ip, d.mac, !whitelistActive)}
          title={
            whitelistActive
              ? `Белые списки (БС) включены для ${d.name} (${d.ip || d.mac}). Нажмите для отключения.`
              : `Белые списки (БС) выключены для ${d.name} (${d.ip || d.mac}). Нажмите для включения.`
          }
        >
          <span className="btn-icon device-whitelist-icon">🛡️</span>
          <span className="btn-label device-whitelist-label">{whitelistActive ? 'БС ON' : 'БС OFF'}</span>
        </button>
      </td>
      <td style={{ textAlign: 'center', whiteSpace: 'nowrap' }}>
        <button
          type="button"
          className="device-gear-btn"
          title={isOfflineOrNoIp ? 'Устройство оффлайн' : 'Настройка резервирования (Failover)'}
          disabled={busy || isOfflineOrNoIp}
          onClick={() => {
            if (isOfflineOrNoIp) return
            openDrModal(d, assigned)
          }}
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
          title={isOfflineOrNoIp ? 'Устройство оффлайн' : 'Расписание (автоблокировка/переключение по времени)'}
          disabled={busy || isOfflineOrNoIp}
          onClick={() => {
            if (isOfflineOrNoIp) return
            onOpenSchedule?.(d)
          }}
          style={{ marginLeft: 6 }}
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
            <circle cx="12" cy="12" r="10" />
            <polyline points="12 6 12 12 16 14" />
          </svg>
        </button>
      </td>
    </tr>
  )
})

export default DeviceRow
