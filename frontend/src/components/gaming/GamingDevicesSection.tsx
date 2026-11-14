import React from 'react'
import type { DeviceInfo, GamingConfig, ServerInfo } from '../../types'

export interface GamingDevicesSectionProps {
  cfg: GamingConfig
  setCfg: React.Dispatch<React.SetStateAction<GamingConfig>>
  devices: DeviceInfo[]
  servers: ServerInfo[]
  activeServer: string
  deviceDropdownOpen: boolean
  setDeviceDropdownOpen: React.Dispatch<React.SetStateAction<boolean>>
  deviceSearch: string
  setDeviceSearch: React.Dispatch<React.SetStateAction<string>>
  pinging: boolean
  getDeviceIcon: (name?: string) => string
  handleAddDevice: (dev: DeviceInfo) => void
  handleSelectAllDevices: (selectAll: boolean) => void
  handleToggleDeviceEnabled: (mac: string) => void
  handleDeviceServerChange: (mac: string, server: string) => void
  handleRemoveDevice: (mac: string) => void
  handlePingTest: () => void
}

export function GamingDevicesSection({
  cfg,
  setCfg,
  devices,
  servers,
  activeServer,
  deviceDropdownOpen,
  setDeviceDropdownOpen,
  deviceSearch,
  setDeviceSearch,
  pinging,
  getDeviceIcon,
  handleAddDevice,
  handleSelectAllDevices,
  handleToggleDeviceEnabled,
  handleDeviceServerChange,
  handleRemoveDevice,
  handlePingTest,
}: GamingDevicesSectionProps) {
  return (
  <div className="card" style={{ padding: '16px 18px' }}>
    <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 14, flexWrap: 'wrap', gap: 10 }}>
      <div>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <span style={{ fontSize: 20 }}>📱</span>
          <h3 style={{ margin: 0, fontSize: 15, fontWeight: 700 }}>
            Игровые устройства ({cfg.devices.length})
          </h3>
          {cfg.devices.filter((d) => d.enabled).length > 0 && (
            <span className="badge" style={{ background: 'rgba(34, 197, 94, 0.15)', color: '#4ade80', fontSize: 11 }}>
              {cfg.devices.filter((d) => d.enabled).length} активно
            </span>
          )}
        </div>
        <p className="muted small" style={{ margin: '3px 0 0', fontSize: 12 }}>
          Выберите устройства для игрового режима. Каждому устройству можно задать собственный выходной сервер.
        </p>
      </div>

      <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap', position: 'relative' }}>
        {/* Кнопка добавления устройства */}
        <div style={{ position: 'relative' }}>
          <button
            type="button"
            className="btn btn-primary sm"
            onClick={() => setDeviceDropdownOpen(!deviceDropdownOpen)}
            style={{ fontSize: 12, fontWeight: 600 }}
          >
            ＋ Добавить устройство
          </button>

          {deviceDropdownOpen && (
            <div
              style={{
                position: 'absolute',
                top: '100%',
                right: 0,
                marginTop: 6,
                width: 320,
                background: '#1e293b',
                border: '1px solid rgba(255, 255, 255, 0.15)',
                borderRadius: 8,
                boxShadow: '0 8px 24px rgba(0,0,0,0.5)',
                padding: 8,
                zIndex: 50,
              }}
            >
              <input
                type="text"
                className="input sm"
                placeholder="Поиск по имени, IP или MAC..."
                value={deviceSearch}
                onChange={(e) => setDeviceSearch(e.target.value)}
                style={{ width: '100%', boxSizing: 'border-box', marginBottom: 6, fontSize: 12 }}
                autoFocus
              />
              <div style={{ maxHeight: 200, overflowY: 'auto', display: 'flex', flexDirection: 'column', gap: 4 }}>
                {devices
                  .filter((d) => {
                    if (!deviceSearch) return true
                    const q = deviceSearch.toLowerCase()
                    return (
                      (d.name || '').toLowerCase().includes(q) ||
                      (d.ip || '').includes(q) ||
                      (d.mac || '').toLowerCase().includes(q)
                    )
                  })
                  .map((d) => {
                    const isAdded = cfg.devices.some((item) => item.mac.toLowerCase() === d.mac.toLowerCase())
                    return (
                      <div
                        key={d.mac}
                        onClick={() => {
                          handleAddDevice(d)
                          setDeviceDropdownOpen(false)
                          setDeviceSearch('')
                        }}
                        style={{
                          padding: '6px 10px',
                          borderRadius: 6,
                          cursor: isAdded ? 'default' : 'pointer',
                          background: isAdded ? 'rgba(255,255,255,0.03)' : 'rgba(255,255,255,0.07)',
                          opacity: isAdded ? 0.5 : 1,
                          display: 'flex',
                          alignItems: 'center',
                          justifyContent: 'space-between',
                          fontSize: 12,
                        }}
                      >
                        <div style={{ display: 'flex', alignItems: 'center', gap: 8, overflow: 'hidden' }}>
                          <span>{getDeviceIcon(d.name)}</span>
                          <div style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                            <div style={{ fontWeight: 600, color: '#f1f5f9' }}>
                              {d.name || d.ip || d.mac} {d.is_current_device ? '★' : ''}
                            </div>
                            <div className="muted small" style={{ fontSize: 10.5 }}>
                              {d.ip || 'DHCP'} • {d.mac}
                            </div>
                          </div>
                        </div>
                        <span style={{ fontSize: 11, color: isAdded ? '#94a3b8' : '#38bdf8' }}>
                          {isAdded ? 'Добавлено' : '+ Добавить'}
                        </span>
                      </div>
                    )
                  })}
                {devices.length === 0 && (
                  <div className="muted small" style={{ padding: 8, textAlign: 'center' }}>
                    Устройства не найдены
                  </div>
                )}
              </div>
            </div>
          )}
        </div>

        <button
          type="button"
          className="btn sm"
          onClick={() => handleSelectAllDevices(true)}
          style={{ fontSize: 11.5 }}
        >
          ✓ Все
        </button>
        <button
          type="button"
          className="btn sm"
          onClick={() => handleSelectAllDevices(false)}
          style={{ fontSize: 11.5 }}
        >
          ✕ Очистить
        </button>
      </div>
    </div>

    {/* Настройка глобального сервера по умолчанию */}
    <div
      style={{
        display: 'grid',
        gridTemplateColumns: 'repeat(auto-fit, minmax(240px, 1fr))',
        gap: 12,
        marginBottom: 16,
        padding: '12px 14px',
        borderRadius: 8,
        background: 'rgba(0, 0, 0, 0.2)',
        border: '1px solid rgba(255, 255, 255, 0.05)',
      }}
    >
      <div>
        <label className="label-sm" style={{ display: 'block', marginBottom: 4, color: '#94a3b8' }}>
          🎯 Общий выходной узел (Gaming Group)
        </label>
        <select
          className="input sm"
          value={cfg.target_server}
          onChange={(e) => setCfg({ ...cfg, target_server: e.target.value })}
          style={{ width: '100%', boxSizing: 'border-box' }}
        >
          <option value="Fastest">⚡ Fastest (Авто-выбор ноды с мин. пингом)</option>
          <option value="PROXY">🌐 Общий активный узел</option>
          <option value="DIRECT">⏭ DIRECT (Прямой выход)</option>
          {servers
            .filter((s) => !s.is_pool && s.id !== 'PROXY' && s.id !== 'GLOBAL')
            .map((s) => (
              <option key={s.id} value={s.id}>
                {s.name || s.id} {s.ping_ms ? `(${s.ping_ms} ms)` : ''}
              </option>
            ))}
        </select>
      </div>

      <div>
        <label className="label-sm" style={{ display: 'block', marginBottom: 4, color: '#94a3b8' }}>
          ⚡ Текущий узел в ядре Mihomo
        </label>
        <div
          className="input sm"
          style={{
            background: 'rgba(0, 0, 0, 0.25)',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
          }}
        >
          <span style={{ fontWeight: 600, color: '#38bdf8' }}>{activeServer || 'Fastest'}</span>
          <span className="badge" style={{ background: 'rgba(56, 189, 248, 0.15)', color: '#38bdf8', fontSize: 10 }}>
            🎮 Gaming Group
          </span>
        </div>
      </div>

      <div>
        <label className="label-sm" style={{ display: 'block', marginBottom: 4, color: '#94a3b8' }}>
          📊 Диагностика задержки
        </label>
        <button
          type="button"
          className="btn btn-secondary sm"
          onClick={handlePingTest}
          disabled={pinging}
          style={{ width: '100%', height: 32, fontSize: 11.5 }}
        >
          {pinging ? '⏳ Замер задержки...' : '⚡ Замерить пинг до серверов игр'}
        </button>
      </div>
    </div>

    {/* Список добавленных устройств с персональным выбором сервера */}
    {cfg.devices.length === 0 ? (
      <div
        style={{
          padding: '24px 16px',
          textAlign: 'center',
          borderRadius: 8,
          background: 'rgba(255, 255, 255, 0.02)',
          border: '1px dashed rgba(255, 255, 255, 0.1)',
        }}
      >
        <p className="muted small" style={{ margin: '0 0 10px' }}>
          Устройства не добавлены. Добавьте устройство из локальной сети, чтобы направить его через игровой туннель.
        </p>
        {devices.find((d) => d.is_current_device) && (
          <button
            type="button"
            className="btn sm btn-primary"
            onClick={() => {
              const curr = devices.find((d) => d.is_current_device)
              if (curr) handleAddDevice(curr)
            }}
          >
            ★ Добавить текущее устройство ({devices.find((d) => d.is_current_device)?.name || 'Этот ПК'})
          </button>
        )}
      </div>
    ) : (
      <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
        {cfg.devices.map((d) => (
          <div
            key={d.mac}
            style={{
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'space-between',
              padding: '10px 14px',
              borderRadius: 8,
              background: d.enabled ? 'rgba(56, 189, 248, 0.04)' : 'rgba(255, 255, 255, 0.01)',
              border: d.enabled ? '1px solid rgba(56, 189, 248, 0.25)' : '1px solid rgba(255, 255, 255, 0.06)',
              flexWrap: 'wrap',
              gap: 12,
            }}
          >
            {/* Чекбокс и информация об устройстве */}
            <div style={{ display: 'flex', alignItems: 'center', gap: 12, minWidth: 220 }}>
              <input
                type="checkbox"
                checked={d.enabled}
                onChange={() => handleToggleDeviceEnabled(d.mac)}
                style={{ cursor: 'pointer' }}
                title={d.enabled ? 'Отключить игровой режим для устройства' : 'Включить'}
              />
              <span style={{ fontSize: 22 }}>{getDeviceIcon(d.name)}</span>
              <div>
                <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                  <b style={{ fontSize: 13, color: d.enabled ? '#f1f5f9' : '#94a3b8' }}>
                    {d.name || d.ip || d.mac}
                  </b>
                  {devices.find((item) => item.mac.toLowerCase() === d.mac.toLowerCase())?.is_current_device && (
                    <span className="badge" style={{ background: 'rgba(234, 179, 8, 0.15)', color: '#facc15', fontSize: 9.5 }}>
                      ★ Текущее
                    </span>
                  )}
                </div>
                <div className="muted small" style={{ fontSize: 11 }}>
                  {d.ip ? `IP: ${d.ip}` : 'DHCP'} • <span style={{ fontFamily: 'Consolas, monospace' }}>{d.mac}</span>
                </div>
              </div>
            </div>

            {/* Персональный сервер для устройства */}
            {/* FE-MOBILE: класс вместо inline-стилей — на мобильном блок занимает
                отдельную строку, селект растягивается, подпись не обрезается */}
            <div className="gaming-device-server-block">
              <div style={{ display: 'flex', alignItems: 'center', gap: 6, flexWrap: 'wrap' }}>
                <span className="muted small" style={{ fontSize: 11.5, whiteSpace: 'nowrap' }}>
                  Выходной узел:
                </span>
                <select
                  className="input sm"
                  value={d.server || ''}
                  onChange={(e) => handleDeviceServerChange(d.mac, e.target.value)}
                  style={{ fontSize: 11.5, minWidth: 180, maxWidth: 240 }}
                >
                  <option value="">⚡ Как в группе ({cfg.target_server})</option>
                  <option value="Fastest">⚡ Fastest (Мин. пинг)</option>
                  <option value="PROXY">🌐 Общий активный узел</option>
                  <option value="DIRECT">⏭ DIRECT (Прямой выход)</option>
                  {servers
                    .filter((s) => !s.is_pool && s.id !== 'PROXY' && s.id !== 'GLOBAL')
                    .map((s) => (
                      <option key={s.id} value={s.id}>
                        {s.name || s.id} {s.ping_ms ? `(${s.ping_ms} ms)` : ''}
                      </option>
                    ))}
                </select>
              </div>

              <button
                type="button"
                className="btn sm"
                onClick={() => handleRemoveDevice(d.mac)}
                style={{
                  background: 'rgba(239, 68, 68, 0.12)',
                  color: '#f87171',
                  border: '1px solid rgba(239, 68, 68, 0.25)',
                  padding: '3px 8px',
                  fontSize: 12,
                }}
                title="Удалить устройство из игрового режима"
              >
                ✕
              </button>
            </div>
          </div>
        ))}
      </div>
    )}
  </div>
  )
}
