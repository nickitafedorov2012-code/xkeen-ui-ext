import { useState, useEffect, useCallback } from 'react'
import { apiGet, apiPost } from '../api'
import type {
  DeviceInfo,
  GamingConfig,
  GamingDevice,
  GamingMode,
  GamingPingResult,
  GamingRealConnection,
  GamingStatus,
  ServerInfo,
} from '../types'

interface GamingProps {
  notify: (msg: string, error?: boolean) => void
}

interface PlatformDef {
  key: keyof GamingConfig['platforms']
  name: string
  icon: string
  badge?: string
  desc: string
  defaultChecked?: boolean
}

const PLATFORMS: PlatformDef[] = [
  {
    key: 'discord',
    name: 'Discord',
    icon: '💬',
    desc: 'Голосовые каналы RTC WebRTC (UDP 50000:65535), текст, медиа и шлюзы',
  },
  {
    key: 'steam',
    name: 'Steam',
    icon: '🚂',
    desc: 'Сообщество Steam, магазин, друзья, инвентарь и торговая площадка',
  },
  {
    key: 'xbox',
    name: 'Xbox Live & Game Pass',
    icon: '🟢',
    badge: 'Fix 0x80a40401',
    desc: 'Сетевые сервисы Xbox, Game Pass и обход блокировки авторизации Microsoft в РФ',
  },
  {
    key: 'supercell',
    name: 'Supercell / Brawl Stars',
    icon: '👑',
    badge: 'Обход блокировки по IP',
    desc: 'Brawl Stars, Clash of Clans, Clash Royale — устранение ошибки региона при входе',
  },
  {
    key: 'playstation',
    name: 'PlayStation Network (PSN)',
    icon: '🎮',
    desc: 'PlayStation Store, сетевые сервисы PS5/PS4, облачные сохранения и подписка PS Plus',
  },
  {
    key: 'battlenet',
    name: 'Battle.net / Blizzard',
    icon: '❄️',
    desc: 'Лаунчер Battle.net, World of Warcraft, Diablo IV, Overwatch и магазин',
  },
  {
    key: 'epicgames',
    name: 'Epic Games Store',
    icon: '⚡',
    desc: 'Магазин Epic Games Store, Fortnite, сервисы обновлений и Unreal Engine',
  },
  {
    key: 'ea',
    name: 'EA App / Origin',
    icon: '🏆',
    desc: 'EA Sports FC (FIFA), Battlefield, Apex Legends, Origin и авторизация EA',
  },
  {
    key: 'riot',
    name: 'Riot Games',
    icon: '⚔️',
    desc: 'Valorant, League of Legends, сетевые шлюзы Riot Client и античит Vanguard',
  },
  {
    key: 'nintendo',
    name: 'Nintendo Online',
    icon: '🔴',
    desc: 'Магазин Nintendo Switch eShop, облачные сервисы и мультиплеер',
  },
  {
    key: 'roblox',
    name: 'Roblox',
    icon: '🧱',
    desc: 'Игровая метавселенная Roblox, загрузка плейсов и CDN игровых ассетов',
  },
  {
    key: 'category_games',
    name: 'Все онлайн-игры (Geosite)',
    icon: '🌐',
    badge: 'Geosite',
    desc: 'Глобальная база онлайн-игр Mihomo (category-games) для сотен других тайтлов',
  },
]

function formatBytes(bytes: number): string {
  if (!bytes || bytes <= 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB']
  const i = Math.floor(Math.log(bytes) / Math.log(1024))
  return `${(bytes / Math.pow(1024, i)).toFixed(i === 0 ? 0 : 1)} ${units[i] || 'B'}`
}

export default function Gaming({ notify }: GamingProps) {
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [toggling, setToggling] = useState(false)
  const [pinging, setPinging] = useState(false)
  const [refreshingConns, setRefreshingConns] = useState(false)
  const [pingResults, setPingResults] = useState<GamingPingResult[] | null>(null)
  const [activeServer, setActiveServer] = useState<string>('')
  const [servers, setServers] = useState<ServerInfo[]>([])
  const [devices, setDevices] = useState<DeviceInfo[]>([])
  const [selectedDeviceMac, setSelectedDeviceMac] = useState<string>('')
  const [status, setStatus] = useState<GamingStatus | null>(null)
  const [showAdvanced, setShowAdvanced] = useState(true)

  const [cfg, setCfg] = useState<GamingConfig>({
    enabled: false,
    mode: 'compatibility',
    target_server: 'Fastest',
    devices: [],
    smart_split: true,
    fix_nat_fake_ip: true,
    platforms: {
      discord: true,
      steam: true,
      playstation: true,
      xbox: true,
      battlenet: true,
      epicgames: true,
      ea: true,
      riot: true,
      supercell: true,
      nintendo: true,
      roblox: true,
      category_games: false,
    },
    custom_domains: [],
  })

  const [customText, setCustomText] = useState('')

  const loadData = useCallback(async () => {
    try {
      const [statusRes, serversRes, devicesRes] = await Promise.all([
        apiGet<GamingStatus>('gaming/status').catch(() => null),
        apiGet<{ proxies?: ServerInfo[]; all?: ServerInfo[] }>('servers').catch(() => null),
        apiGet<{ devices?: DeviceInfo[] }>('devices').catch(() => null),
      ])

      if (statusRes) {
        setStatus(statusRes)
        if (statusRes.config) {
          setCfg(statusRes.config)
          setActiveServer(statusRes.active_server || statusRes.config.target_server || 'Fastest')
          setCustomText((statusRes.config.custom_domains || []).join('\n'))
        }
      }

      if (serversRes) {
        const list = serversRes.proxies || serversRes.all || []
        setServers(list.filter((s) => s.id && s.id !== 'REJECT' && s.id !== 'DIRECT'))
      }

      if (devicesRes?.devices) {
        setDevices(devicesRes.devices)
        setSelectedDeviceMac((prev) => {
          if (prev) return prev
          if (statusRes?.active_device?.mac) return statusRes.active_device.mac
          const enabledDev = statusRes?.config?.devices?.find((d) => d.enabled)
          if (enabledDev?.mac) return enabledDev.mac
          const currentDev = devicesRes.devices?.find((d) => d.is_current_device)
          if (currentDev?.mac) return currentDev.mac
          return devicesRes.devices?.[0]?.mac || ''
        })
      }
    } catch (e: any) {
      notify(e.message || 'Ошибка загрузки настроек игрового режима', true)
    } finally {
      setLoading(false)
    }
  }, [notify])

  useEffect(() => {
    loadData()
  }, [loadData])

  const handleRefreshConnections = async () => {
    setRefreshingConns(true)
    try {
      const statusRes = await apiGet<GamingStatus>('gaming/status')
      if (statusRes) {
        setStatus(statusRes)
      }
    } catch {
      // Игнорируем
    } finally {
      setRefreshingConns(false)
    }
  }

  // Главная кнопка: Включение/отключение игрового режима / режима совместимости
  const handleToggle = async (val: boolean, overrideMode?: GamingMode) => {
    setToggling(true)
    const targetMode = overrideMode || cfg.mode || 'compatibility'
    setCfg((prev) => ({ ...prev, enabled: val, mode: targetMode }))
    try {
      const payload: {
        enabled: boolean
        mode?: GamingMode
        device_mac?: string
        target_server?: string
      } = { enabled: val }

      if (val) {
        payload.mode = targetMode
        const devMac = selectedDeviceMac || devices?.[0]?.mac || ''
        if (devMac) payload.device_mac = devMac
        if (cfg.target_server) payload.target_server = cfg.target_server
      }

      await apiPost('gaming/toggle', payload)
      notify(
        val
          ? targetMode === 'compatibility'
            ? '🟢 Режим полной совместимости включен'
            : '🟢 Режим известных игровых сервисов включен'
          : '⚪ Игровой режим выключен'
      )
      await loadData()
    } catch (e: any) {
      setCfg((prev) => ({ ...prev, enabled: !val }))
      notify(e.message || 'Ошибка переключения игрового режима', true)
      await loadData()
    } finally {
      setToggling(false)
    }
  }

  const handleDeviceChange = (mac: string) => {
    setSelectedDeviceMac(mac)
    const found = devices.find((d) => d.mac.toLowerCase() === mac.toLowerCase())
    if (found) {
      const updatedDevices: GamingDevice[] = [
        {
          mac: found.mac,
          ip: found.ip,
          ipv6: found.ipv6 || [],
          name: found.name,
          enabled: true,
        },
      ]
      setCfg((prev) => ({ ...prev, devices: updatedDevices }))
    }
  }

  const handleModeChange = (mode: GamingMode) => {
    setCfg((prev) => ({ ...prev, mode }))
  }

  const handlePlatformToggle = (key: keyof GamingConfig['platforms']) => {
    setCfg((prev) => ({
      ...prev,
      platforms: {
        ...prev.platforms,
        [key]: !prev.platforms[key],
      },
    }))
  }

  const handleSelectAll = (select: boolean) => {
    setCfg((prev) => {
      const updated = { ...prev.platforms }
      for (const k of Object.keys(updated) as (keyof GamingConfig['platforms'])[]) {
        updated[k] = select
      }
      return { ...prev, platforms: updated }
    })
  }

  const handleSave = async () => {
    setSaving(true)
    try {
      const lines = customText
        .split('\n')
        .map((s) => s.trim().toLowerCase())
        .filter((s) => s.length > 0 && s.includes('.'))
      const uniqueDomains = Array.from(new Set(lines))

      let currentDevices = [...(cfg.devices || [])]
      if (selectedDeviceMac) {
        const found = devices.find((d) => d.mac.toLowerCase() === selectedDeviceMac.toLowerCase())
        if (found) {
          currentDevices = [
            {
              mac: found.mac,
              ip: found.ip,
              ipv6: found.ipv6 || [],
              name: found.name,
              enabled: true,
            },
          ]
        }
      }

      const newCfg: GamingConfig = {
        ...cfg,
        devices: currentDevices,
        custom_domains: uniqueDomains,
      }

      await apiPost('gaming/save', { gaming: newCfg })
      setCfg(newCfg)
      notify('✓ Настройки игрового режима успешно сохранены и применены')
      await loadData()
    } catch (e: any) {
      notify(e.message || 'Ошибка сохранения настроек игрового режима', true)
    } finally {
      setSaving(false)
    }
  }

  const handlePingTest = async () => {
    setPinging(true)
    try {
      const res = await apiPost<{ results: GamingPingResult[] }>('gaming/ping', {})
      if (res?.results) {
        setPingResults(res.results)
        notify('✓ Замер задержки до игровых серверов завершен')
      }
    } catch (e: any) {
      notify(e.message || 'Ошибка замера задержки', true)
    } finally {
      setPinging(false)
    }
  }

  const enabledCount = Object.values(cfg.platforms).filter(Boolean).length
  const activeDeviceObj = status?.active_device || devices.find((d) => d.mac.toLowerCase() === selectedDeviceMac.toLowerCase())
  const isActuallyActive = status ? (status.is_active !== undefined ? status.is_active : cfg.enabled) : cfg.enabled
  const isCompatMode = cfg.mode === 'compatibility'

  if (loading) {
    return (
      <div className="card" style={{ textAlign: 'center', padding: '3rem 1rem' }}>
        <div className="spinner" style={{ margin: '0 auto 12px' }} />
        <p className="muted">Загрузка конфигурации игрового режима...</p>
      </div>
    )
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
      {/* 1. Главная карточка статуса, узла и устройства */}
      <div
        className="card"
        style={{
          background: isActuallyActive
            ? 'linear-gradient(135deg, rgba(34, 197, 94, 0.08) 0%, rgba(14, 165, 233, 0.05) 100%)'
            : cfg.enabled
            ? 'linear-gradient(135deg, rgba(234, 179, 8, 0.08) 0%, rgba(239, 68, 68, 0.05) 100%)'
            : 'rgba(255, 255, 255, 0.02)',
          border: isActuallyActive
            ? '1px solid rgba(34, 197, 94, 0.35)'
            : cfg.enabled
            ? '1px solid rgba(234, 179, 8, 0.35)'
            : '1px solid rgba(255, 255, 255, 0.08)',
          boxShadow: isActuallyActive ? '0 0 24px rgba(34, 197, 94, 0.12)' : 'none',
          position: 'relative',
          overflow: 'hidden',
        }}
      >
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: 14 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 14 }}>
            <span style={{ fontSize: 32 }}>🎮</span>
            <div>
              <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
                <h2 style={{ margin: 0, fontSize: 18, fontWeight: 700 }}>Игровой режим (Gaming Mode)</h2>
                {/* Не считать режим включённым только потому, что кнопка нажата */}
                <span
                  className="badge"
                  style={{
                    background: isActuallyActive
                      ? 'rgba(34, 197, 94, 0.2)'
                      : cfg.enabled
                      ? 'rgba(234, 179, 8, 0.2)'
                      : 'rgba(148, 163, 184, 0.15)',
                    color: isActuallyActive ? '#4ade80' : cfg.enabled ? '#facc15' : '#94a3b8',
                    fontWeight: 600,
                  }}
                >
                  {isActuallyActive ? '🟢 АКТИВЕН' : cfg.enabled ? '⚠️ МАРШРУТ НЕ ПОДТВЕРЖДЕН' : '⚪ ОТКЛЮЧЕН'}
                </span>
                <span
                  className="badge"
                  style={{
                    background: isCompatMode ? 'rgba(56, 189, 248, 0.15)' : 'rgba(168, 85, 247, 0.15)',
                    color: isCompatMode ? '#38bdf8' : '#c084fc',
                    fontSize: 11,
                  }}
                >
                  {isCompatMode ? 'Полный маршрут устройства' : 'Экономный GeoSite'}
                </span>
              </div>
              <p className="muted small" style={{ margin: '4px 0 0' }}>
                {isCompatMode
                  ? 'Основной режим: направляет весь интернет-трафик устройства через игровой туннель.'
                  : 'Дополнительный режим: направляет только известные игровые платформы и category-games.'}
              </p>
            </div>
          </div>

          <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
            <button
              type="button"
              className={`btn sm ${cfg.enabled ? 'btn-danger' : 'btn-primary'}`}
              onClick={() => handleToggle(!cfg.enabled, !cfg.enabled ? 'compatibility' : undefined)}
              disabled={toggling}
              style={{ minWidth: 150, fontWeight: 600 }}
            >
              {toggling
                ? '⏳ Применение...'
                : cfg.enabled
                ? '⏹ Выключить'
                : '▶ Включить совместимость'}
            </button>
          </div>
        </div>

        {/* Предупреждение о неподтвержденном маршруте */}
        {cfg.enabled && !isActuallyActive && status?.verification_error && (
          <div
            style={{
              marginTop: 14,
              padding: '10px 14px',
              borderRadius: 6,
              background: 'rgba(239, 68, 68, 0.15)',
              border: '1px solid rgba(239, 68, 68, 0.35)',
              color: '#fca5a5',
              fontSize: 12.5,
              display: 'flex',
              alignItems: 'center',
              gap: 10,
            }}
          >
            <span>⚠️</span>
            <div>
              <b>Внимание:</b> {status.verification_error}
            </div>
          </div>
        )}

        {/* Выбор устройства и выходного узла */}
        <div
          style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fit, minmax(240px, 1fr))',
            gap: 12,
            marginTop: 18,
            paddingTop: 16,
            borderTop: '1px solid rgba(255, 255, 255, 0.08)',
          }}
        >
          {/* Выбор устройства */}
          <div>
            <label className="label-sm" style={{ display: 'block', marginBottom: 6, color: '#94a3b8' }}>
              📱 Целевое устройство
            </label>
            <select
              className="input"
              value={selectedDeviceMac}
              onChange={(e) => handleDeviceChange(e.target.value)}
              style={{ width: '100%', boxSizing: 'border-box' }}
            >
              {devices.length === 0 && <option value="">Устройства не найдены</option>}
              {devices.map((d) => (
                <option key={d.mac} value={d.mac}>
                  {d.name || d.ip || d.mac} ({d.ip || 'DHCP'}) {d.is_current_device ? '★' : ''}
                </option>
              ))}
            </select>
          </div>

          {/* Выбор туннеля */}
          <div>
            <label className="label-sm" style={{ display: 'block', marginBottom: 6, color: '#94a3b8' }}>
              🎯 Выходной игровой узел (Proxy)
            </label>
            <select
              className="input"
              value={cfg.target_server}
              onChange={(e) => setCfg({ ...cfg, target_server: e.target.value })}
              style={{ width: '100%', boxSizing: 'border-box' }}
            >
              <option value="Fastest">⚡ Fastest (Авто-выбор ноды с мин. пингом)</option>
              <option value="PROXY">🔒 PROXY (Основной туннель)</option>
              <option value="DIRECT">⏭ DIRECT (Прямой выход)</option>
              {servers.map((s) => (
                <option key={s.id} value={s.id}>
                  {s.name || s.id} {s.ping_ms ? `(${s.ping_ms} ms)` : ''}
                </option>
              ))}
            </select>
          </div>

          {/* Текущий узел в ядре Mihomo */}
          <div>
            <label className="label-sm" style={{ display: 'block', marginBottom: 6, color: '#94a3b8' }}>
              ⚡ Текущий узел в ядре Mihomo
            </label>
            <div
              className="input"
              style={{
                background: 'rgba(0, 0, 0, 0.25)',
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'space-between',
              }}
            >
              <span style={{ fontWeight: 600, color: '#38bdf8' }}>{activeServer || 'Fastest'}</span>
              <span className="badge" style={{ background: 'rgba(56, 189, 248, 0.15)', color: '#38bdf8' }}>
                🎮 Gaming Group
              </span>
            </div>
          </div>

          <div style={{ display: 'flex', alignItems: 'flex-end' }}>
            <button
              type="button"
              className="btn btn-secondary sm"
              onClick={handlePingTest}
              disabled={pinging}
              style={{ width: '100%' }}
              title="Замерить задержку подключения до Steam, Discord, Xbox, PSN, Supercell"
            >
              {pinging ? '⏳ Замер задержки...' : '⚡ Замерить пинг до серверов игр'}
            </button>
          </div>
        </div>

        {/* Статус в UI: Устройство, активный туннель, TCP/UDP, IPv6 */}
        <div
          style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))',
            gap: 10,
            marginTop: 14,
            padding: '12px 14px',
            background: 'rgba(0, 0, 0, 0.25)',
            borderRadius: 8,
            border: '1px solid rgba(255, 255, 255, 0.05)',
          }}
        >
          <div>
            <div className="muted small">📱 Устройство</div>
            <div style={{ fontWeight: 600, fontSize: 13, marginTop: 2 }}>
              {activeDeviceObj?.name || 'Не выбрано'}
            </div>
            <div className="muted small" style={{ fontSize: 11 }}>
              {activeDeviceObj?.ip ? `IP: ${activeDeviceObj.ip}` : activeDeviceObj?.mac || '—'}
            </div>
          </div>

          <div>
            <div className="muted small">🎯 Активный туннель</div>
            <div style={{ fontWeight: 600, fontSize: 13, marginTop: 2, color: '#38bdf8' }}>
              {activeServer || cfg.target_server || 'Fastest'}
            </div>
            <div className="muted small" style={{ fontSize: 11 }}>
              {status?.tunnel_status?.reachable ? (
                <span style={{ color: '#4ade80' }}>
                  ✓ Доступен {status.tunnel_status.latency_ms ? `(${status.tunnel_status.latency_ms} ms)` : ''}
                </span>
              ) : (
                <span style={{ color: '#f87171' }}>✕ Проверка...</span>
              )}
            </div>
          </div>

          <div>
            <div className="muted small">🔌 Перехват TCP / UDP</div>
            <div style={{ display: 'flex', gap: 6, marginTop: 4, flexWrap: 'wrap' }}>
              <span
                className="badge"
                style={{
                  background: status?.tcp_interception ? 'rgba(34, 197, 94, 0.2)' : 'rgba(239, 68, 68, 0.2)',
                  color: status?.tcp_interception ? '#4ade80' : '#f87171',
                  fontSize: 10.5,
                }}
              >
                TCP: {status?.tcp_interception ? '✓ OK' : 'Сбой'}
              </span>
              <span
                className="badge"
                style={{
                  background: status?.udp_interception ? 'rgba(34, 197, 94, 0.2)' : 'rgba(239, 68, 68, 0.2)',
                  color: status?.udp_interception ? '#4ade80' : '#f87171',
                  fontSize: 10.5,
                }}
              >
                UDP: {status?.udp_interception ? '✓ OK' : 'Сбой'}
              </span>
            </div>
          </div>

          <div>
            <div className="muted small">🌐 IPv6 маршрут</div>
            <div style={{ fontWeight: 600, fontSize: 13, marginTop: 2 }}>
              {status?.ipv6_status?.active ? (
                <span style={{ color: '#4ade80' }}>🟢 Активен</span>
              ) : (
                <span className="muted">⚪ Не используется</span>
              )}
            </div>
            <div className="muted small" style={{ fontSize: 11 }}>
              {activeDeviceObj?.ipv6 && activeDeviceObj.ipv6.length > 0
                ? `${activeDeviceObj.ipv6.length} v6 адресов`
                : 'Только IPv4'}
            </div>
          </div>
        </div>
      </div>

      {/* 2. Результаты пинга, если замерены */}
      {pingResults && pingResults.length > 0 && (
        <div className="card" style={{ padding: '14px 16px' }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 10 }}>
            <b style={{ fontSize: 13, color: '#38bdf8' }}>📊 Задержка подключения до игровых шлюзов:</b>
            <button className="btn sm" onClick={() => setPingResults(null)}>✕ Скрыть</button>
          </div>
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))', gap: 8 }}>
            {pingResults.map((r) => (
              <div
                key={r.name}
                style={{
                  padding: '8px 10px',
                  borderRadius: 6,
                  background: 'rgba(255, 255, 255, 0.03)',
                  border: '1px solid rgba(255, 255, 255, 0.06)',
                  display: 'flex',
                  justifyContent: 'space-between',
                  alignItems: 'center',
                }}
              >
                <div>
                  <div style={{ fontWeight: 600, fontSize: 12.5 }}>{r.name}</div>
                  <div className="muted small" style={{ fontSize: 11 }}>{r.host}</div>
                </div>
                <span
                  className="badge"
                  style={{
                    background: r.available ? (r.ping_ms < 100 ? 'rgba(34, 197, 94, 0.2)' : 'rgba(234, 179, 8, 0.2)') : 'rgba(239, 68, 68, 0.2)',
                    color: r.available ? (r.ping_ms < 100 ? '#4ade80' : '#facc15') : '#f87171',
                    fontWeight: 700,
                  }}
                >
                  {r.available ? `${r.ping_ms} ms` : 'Сбой'}
                </span>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* 3. Реальные соединения устройства через Mihomo */}
      <div className="card" style={{ padding: '14px 16px' }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 10, flexWrap: 'wrap', gap: 8 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <span style={{ fontSize: 16 }}>📊</span>
            <b style={{ fontSize: 13.5 }}>Реальные соединения устройства через Mihomo</b>
            {status?.real_connections && (
              <span className="badge" style={{ background: 'rgba(56, 189, 248, 0.15)', color: '#38bdf8', fontSize: 11 }}>
                {status.real_connections.length} активных
              </span>
            )}
          </div>
          <button
            type="button"
            className="btn sm"
            onClick={handleRefreshConnections}
            disabled={refreshingConns}
            style={{ fontSize: 12 }}
          >
            {refreshingConns ? '⏳ Опрос...' : '🔄 Обновить соединения'}
          </button>
        </div>

        {status?.real_connections && status.real_connections.length > 0 ? (
          <div style={{ overflowX: 'auto', maxHeight: 240, overflowY: 'auto' }}>
            <table style={{ width: '100%', fontSize: 12, borderCollapse: 'collapse' }}>
              <thead>
                <tr style={{ textAlign: 'left', borderBottom: '1px solid rgba(255, 255, 255, 0.08)', color: '#94a3b8' }}>
                  <th style={{ padding: '6px 8px' }}>Хост / Назначение</th>
                  <th style={{ padding: '6px 8px' }}>Протокол</th>
                  <th style={{ padding: '6px 8px' }}>Правило</th>
                  <th style={{ padding: '6px 8px' }}>Выходной узел</th>
                  <th style={{ padding: '6px 8px', textAlign: 'right' }}>Трафик</th>
                </tr>
              </thead>
              <tbody>
                {status.real_connections.map((c: GamingRealConnection, idx: number) => (
                  <tr
                    key={c.id || idx}
                    style={{ borderBottom: '1px solid rgba(255, 255, 255, 0.04)', height: 32 }}
                  >
                    <td style={{ padding: '6px 8px', fontFamily: 'Consolas, monospace', fontSize: 11.5 }}>
                      <span title={c.destination}>{c.host || c.destination}</span>
                    </td>
                    <td style={{ padding: '6px 8px' }}>
                      <span
                        className="badge"
                        style={{
                          background: c.network === 'UDP' ? 'rgba(168, 85, 247, 0.2)' : 'rgba(56, 189, 248, 0.2)',
                          color: c.network === 'UDP' ? '#c084fc' : '#38bdf8',
                          fontSize: 10,
                          padding: '1px 5px',
                        }}
                      >
                        {c.network}
                      </span>
                    </td>
                    <td style={{ padding: '6px 8px', color: '#94a3b8', fontSize: 11 }}>
                      {c.rule || 'SRC-IP-CIDR'}
                    </td>
                    <td style={{ padding: '6px 8px', color: '#4ade80', fontWeight: 600 }}>
                      <span title={c.chains?.join(' → ') || '🎮 Gaming'}>
                        {c.chains && c.chains.length > 1
                          ? `${c.chains[0]} → ${c.chains[c.chains.length - 1]}`
                          : (c.chains?.[0] || '🎮 Gaming')}
                      </span>
                    </td>
                    <td style={{ padding: '6px 8px', textAlign: 'right', fontFamily: 'Consolas, monospace', fontSize: 11 }}>
                      ↓ {formatBytes(c.download)} / ↑ {formatBytes(c.upload)}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <div className="muted small" style={{ padding: '12px 0', textAlign: 'center' }}>
            {cfg.enabled
              ? 'Соединения от выбранного устройства пока не зафиксированы в ядре Mihomo. Запустите игру или откройте страницу на устройстве.'
              : 'Включите игровой режим или режим совместимости, чтобы отслеживать соединения.'}
          </div>
        )}
      </div>

      {/* 4. Два честно разделенных режима в дополнительных настройках */}
      <div className="card" style={{ padding: '14px 16px' }}>
        <div
          onClick={() => setShowAdvanced(!showAdvanced)}
          style={{
            display: 'flex',
            justifyContent: 'space-between',
            alignItems: 'center',
            cursor: 'pointer',
            userSelect: 'none',
          }}
        >
          <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
            <span style={{ fontSize: 18 }}>⚙️</span>
            <div>
              <b style={{ fontSize: 14 }}>Дополнительные настройки: Режимы маршрутизации</b>
              <p className="muted small" style={{ margin: '2px 0 0', fontSize: 11.5 }}>
                Выбор между полной совместимостью устройства и режимом «Только известные игровые сервисы» (GeoSite)
              </p>
            </div>
          </div>
          <span style={{ fontSize: 14, color: '#94a3b8' }}>{showAdvanced ? '▲ Свернуть' : '▼ Развернуть'}</span>
        </div>

        {showAdvanced && (
          <div style={{ marginTop: 14, display: 'flex', flexDirection: 'column', gap: 14 }}>
            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(300px, 1fr))', gap: 12 }}>
              {/* Режим 1: Полная совместимость */}
              <div
                onClick={() => handleModeChange('compatibility')}
                style={{
                  padding: '12px 14px',
                  borderRadius: 8,
                  cursor: 'pointer',
                  background: isCompatMode ? 'rgba(56, 189, 248, 0.12)' : 'rgba(255, 255, 255, 0.02)',
                  border: isCompatMode ? '1px solid rgba(56, 189, 248, 0.45)' : '1px solid rgba(255, 255, 255, 0.06)',
                  display: 'flex',
                  flexDirection: 'column',
                  justifyContent: 'space-between',
                }}
              >
                <div>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                    <input type="radio" checked={isCompatMode} onChange={() => {}} />
                    <b style={{ fontSize: 13.5 }}>Полная совместимость устройства (Рекомендуется)</b>
                  </div>
                  <p className="muted small" style={{ margin: '6px 0 0', lineHeight: 1.4, fontSize: 11.5 }}>
                    Направляет <b>весь интернет-трафик устройства через туннель</b>. Гарантирует работу любых неизвестных игр, приватных серверов и сетевых протоколов.
                  </p>
                </div>
                {cfg.enabled && !isCompatMode && (
                  <div style={{ marginTop: 10, display: 'flex', justifyContent: 'flex-end' }}>
                    <button
                      type="button"
                      className="btn btn-primary sm"
                      onClick={(e) => {
                        e.stopPropagation()
                        handleToggle(true, 'compatibility')
                      }}
                      disabled={toggling}
                      style={{ fontSize: 11.5 }}
                    >
                      ▶ Включить полную совместимость
                    </button>
                  </div>
                )}
              </div>

              {/* Режим 2: Только известные сервисы */}
              <div
                onClick={() => handleModeChange('known_services')}
                style={{
                  padding: '12px 14px',
                  borderRadius: 8,
                  cursor: 'pointer',
                  background: !isCompatMode ? 'rgba(168, 85, 247, 0.12)' : 'rgba(255, 255, 255, 0.02)',
                  border: !isCompatMode ? '1px solid rgba(168, 85, 247, 0.45)' : '1px solid rgba(255, 255, 255, 0.06)',
                  display: 'flex',
                  flexDirection: 'column',
                  justifyContent: 'space-between',
                }}
              >
                <div>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                    <input type="radio" checked={!isCompatMode} onChange={() => {}} />
                    <b style={{ fontSize: 13.5 }}>Только известные игровые сервисы</b>
                  </div>
                  <p className="muted small" style={{ margin: '6px 0 0', lineHeight: 1.4, fontSize: 11.5 }}>
                    Экономный вариант на базе <code>category-games</code> и списков доменов платформ.
                  </p>
                  {/* Обязательное указание честного ограничения */}
                  <div
                    style={{
                      marginTop: 8,
                      padding: '6px 8px',
                      borderRadius: 4,
                      background: 'rgba(234, 179, 8, 0.12)',
                      border: '1px solid rgba(234, 179, 8, 0.25)',
                      color: '#fde047',
                      fontSize: 11,
                      lineHeight: 1.35,
                    }}
                  >
                    ⚠️ <b>Важно:</b> этот режим <b>не охватывает неизвестные игры</b>. Роутер не может знать, что неизвестное соединение принадлежит игре.
                  </div>
                </div>
                <div style={{ marginTop: 10, display: 'flex', justifyContent: 'flex-end' }}>
                  <button
                    type="button"
                    className={`btn sm ${cfg.enabled && !isCompatMode ? 'btn-danger' : 'btn-secondary'}`}
                    onClick={(e) => {
                      e.stopPropagation()
                      if (cfg.enabled && !isCompatMode) {
                        handleToggle(false)
                      } else {
                        handleToggle(true, 'known_services')
                      }
                    }}
                    disabled={toggling}
                    style={{ fontSize: 11.5 }}
                  >
                    {cfg.enabled && !isCompatMode ? '⏹ Отключить известные сервисы' : '▶ Включить режим известных сервисов'}
                  </button>
                </div>
              </div>
            </div>
          </div>
        )}
      </div>

      {/* 5. Сетка платформ и сервисов */}
      <div className="card">
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 12, flexWrap: 'wrap', gap: 8 }}>
          <div>
            <h3 style={{ margin: 0, fontSize: 15, fontWeight: 700 }}>🎯 Целевые игровые платформы</h3>
            <span className="muted small">
              Выбрано: {enabledCount} из {PLATFORMS.length} платформ
            </span>
          </div>
          <div style={{ display: 'flex', gap: 8 }}>
            <button type="button" className="btn sm" onClick={() => handleSelectAll(true)}>
              ✓ Выбрать все
            </button>
            <button type="button" className="btn sm" onClick={() => handleSelectAll(false)}>
              ✕ Снять все
            </button>
          </div>
        </div>

        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(280px, 1fr))', gap: 10 }}>
          {PLATFORMS.map((p) => {
            const isChecked = Boolean(cfg.platforms[p.key])
            return (
              <div
                key={p.key}
                onClick={() => handlePlatformToggle(p.key)}
                style={{
                  padding: '12px 14px',
                  borderRadius: 8,
                  cursor: 'pointer',
                  userSelect: 'none',
                  transition: 'all 0.15s ease',
                  background: isChecked ? 'rgba(56, 189, 248, 0.08)' : 'rgba(255, 255, 255, 0.02)',
                  border: isChecked ? '1px solid rgba(56, 189, 248, 0.35)' : '1px solid rgba(255, 255, 255, 0.06)',
                  display: 'flex',
                  alignItems: 'flex-start',
                  gap: 12,
                }}
              >
                <input
                  type="checkbox"
                  checked={isChecked}
                  onChange={() => {}}
                  style={{ marginTop: 3, cursor: 'pointer' }}
                />
                <div style={{ flex: 1 }}>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                    <span style={{ fontSize: 16 }}>{p.icon}</span>
                    <span style={{ fontWeight: 600, fontSize: 13.5 }}>{p.name}</span>
                    {p.badge && (
                      <span
                        className="badge"
                        style={{
                          background: 'rgba(244, 63, 94, 0.15)',
                          color: '#fb7185',
                          fontSize: 10,
                          padding: '1px 5px',
                        }}
                      >
                        {p.badge}
                      </span>
                    )}
                  </div>
                  <p className="muted small" style={{ margin: '4px 0 0', lineHeight: 1.35, fontSize: 11.5 }}>
                    {p.desc}
                  </p>
                </div>
              </div>
            )
          })}
        </div>
      </div>

      {/* 6. Опции оптимизации и пользовательские домены */}
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(320px, 1fr))', gap: 14 }}>
        <div className="card">
          <h3 style={{ margin: '0 0 10px', fontSize: 14.5, fontWeight: 700 }}>⚡ Оптимизации задержки и NAT</h3>

          <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
            <label style={{ display: 'flex', gap: 10, alignItems: 'flex-start', cursor: 'pointer' }}>
              <input
                type="checkbox"
                checked={cfg.smart_split}
                onChange={(e) => setCfg({ ...cfg, smart_split: e.target.checked })}
                style={{ marginTop: 2 }}
              />
              <div>
                <b style={{ fontSize: 13 }}>Smart Split-Tunneling (Рекомендуется)</b>
                <p className="muted small" style={{ margin: '2px 0 0' }}>
                  Проксировать только авторизацию, сервисы обновлений и заблокированные API, сохраняя прямой матч на минимальном пинге.
                </p>
              </div>
            </label>

            <label style={{ display: 'flex', gap: 10, alignItems: 'flex-start', cursor: 'pointer' }}>
              <input
                type="checkbox"
                checked={cfg.fix_nat_fake_ip}
                onChange={(e) => setCfg({ ...cfg, fix_nat_fake_ip: e.target.checked })}
                style={{ marginTop: 2 }}
              />
              <div>
                <b style={{ fontSize: 13 }}>Anti-Cheat & Strict NAT Fix (Fake-IP Bypass)</b>
                <p className="muted small" style={{ margin: '2px 0 0' }}>
                  Предотвращает предупреждения «Strict NAT» (Type 3) на PlayStation/Xbox и конфликты античитов (Vanguard, BattlEye, EasyAntiCheat) с диапазоном 198.18.0.0/16.
                </p>
              </div>
            </label>
          </div>
        </div>

        <div className="card">
          <h3 style={{ margin: '0 0 6px', fontSize: 14.5, fontWeight: 700 }}>✏️ Дополнительные игровые домены</h3>
          <p className="muted small" style={{ margin: '0 0 8px' }}>
            По одному домену на строку. Автоматически направляются в группу <code>🎮 Gaming</code> и синхронизируются в <code>geo_override</code>.
          </p>
          <textarea
            className="input"
            rows={4}
            placeholder={'custom-gameserver.com\nprivateserver.org'}
            value={customText}
            onChange={(e) => setCustomText(e.target.value)}
            style={{ width: '100%', boxSizing: 'border-box', fontFamily: 'Consolas, monospace', fontSize: 12 }}
          />
        </div>
      </div>

      {/* 7. Плавающая нижняя панель сохранения */}
      <div
        className="card"
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'center',
          padding: '12px 18px',
          background: 'rgba(15, 23, 42, 0.85)',
          backdropFilter: 'blur(12px)',
          border: '1px solid rgba(56, 189, 248, 0.25)',
          position: 'sticky',
          bottom: 12,
          zIndex: 10,
        }}
      >
        <div className="muted small">
          💡 После сохранения настройки обновят <code>config.yaml</code> ядра Mihomo и правила маршрутизации роутера.
        </div>
        <button
          type="button"
          className="btn btn-primary"
          onClick={handleSave}
          disabled={saving}
          style={{ minWidth: 160, fontWeight: 700 }}
        >
          {saving ? '⏳ Применение...' : '💾 Сохранить и применить'}
        </button>
      </div>
    </div>
  )
}
