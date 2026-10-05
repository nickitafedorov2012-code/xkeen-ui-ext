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
  RecentGamingConn,
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

function getDeviceIcon(name?: string): string {
  const n = (name || '').toLowerCase()
  if (
    n.includes('ps5') ||
    n.includes('playstation') ||
    n.includes('ps4') ||
    n.includes('xbox') ||
    n.includes('switch') ||
    n.includes('nintendo') ||
    n.includes('deck') ||
    n.includes('console')
  ) {
    return '🎮'
  }
  if (
    n.includes('pc') ||
    n.includes('комп') ||
    n.includes('desktop') ||
    n.includes('rig') ||
    n.includes('laptop') ||
    n.includes('ноут') ||
    n.includes('mac') ||
    n.includes('win')
  ) {
    return '🖥️'
  }
  if (n.includes('phone') || n.includes('iphone') || n.includes('android') || n.includes('тел') || n.includes('mobile')) {
    return '📱'
  }
  return '💻'
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
  const [deviceDropdownOpen, setDeviceDropdownOpen] = useState(false)
  const [deviceSearch, setDeviceSearch] = useState('')
  const [connFilterDevice, setConnFilterDevice] = useState<string>('all')
  const [status, setStatus] = useState<GamingStatus | null>(null)

  const [cfg, setCfg] = useState<GamingConfig>({
    enabled: false,
    mode: 'bypass_ru',
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
        apiGet<{ servers?: ServerInfo[]; proxies?: ServerInfo[]; all?: ServerInfo[] }>('servers').catch(() => null),
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
        const list = serversRes?.servers || serversRes?.proxies || serversRes?.all || []
        setServers(list.filter((s) => s.id && s.id !== 'REJECT' && s.id !== 'DIRECT'))
      }

      if (devicesRes?.devices) {
        setDevices(devicesRes.devices)
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

  // Главная кнопка: Включение/отключение игрового режима
  const handleToggle = async (val: boolean, overrideMode?: GamingMode) => {
    setToggling(true)
    const targetMode = overrideMode || cfg.mode || 'bypass_ru'
    let currentDevices = [...(cfg.devices || [])]
    if (val && !currentDevices.some((d) => d.enabled)) {
      const preferred = devices.find((d) => d.is_current_device) || devices[0]
      if (preferred) {
        currentDevices = [
          {
            mac: preferred.mac,
            ip: preferred.ip,
            ipv6: preferred.ipv6 || [],
            name: preferred.name || preferred.ip,
            enabled: true,
            server: undefined,
          },
        ]
      }
    }
    const updatedCfg: GamingConfig = { ...cfg, enabled: val, mode: targetMode, devices: currentDevices }
    setCfg(updatedCfg)
    try {
      const preferredMac = currentDevices.find((d) => d.enabled)?.mac || (val && devices[0]?.mac) || undefined
      const payload: any = { enabled: val }
      if (val) {
        payload.mode = targetMode
        if (preferredMac) {
          payload.device_mac = preferredMac
        }
        if (cfg.target_server) {
          payload.target_server = cfg.target_server
        }
      }
      await apiPost('gaming/toggle', payload)
      notify(
        val
          ? targetMode === 'bypass_ru'
            ? '🟢 Игровой режим включен («Все в прокси кроме RU»)'
            : targetMode === 'compatibility'
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

  const handleAddDevice = (d: DeviceInfo) => {
    setCfg((prev) => {
      const exists = prev.devices.some((item) => item.mac.toLowerCase() === d.mac.toLowerCase())
      if (exists) {
        return {
          ...prev,
          devices: prev.devices.map((item) =>
            item.mac.toLowerCase() === d.mac.toLowerCase() ? { ...item, enabled: true } : item
          ),
        }
      }
      const newDev: GamingDevice = {
        mac: d.mac,
        ip: d.ip,
        ipv6: d.ipv6 || [],
        name: d.name || d.ip,
        enabled: true,
        server: undefined,
      }
      return {
        ...prev,
        devices: [...prev.devices, newDev],
      }
    })
  }

  const handleRemoveDevice = (mac: string) => {
    setCfg((prev) => ({
      ...prev,
      devices: prev.devices.filter((d) => d.mac.toLowerCase() !== mac.toLowerCase()),
    }))
  }

  const handleToggleDeviceEnabled = (mac: string) => {
    setCfg((prev) => ({
      ...prev,
      devices: prev.devices.map((d) =>
        d.mac.toLowerCase() === mac.toLowerCase() ? { ...d, enabled: !d.enabled } : d
      ),
    }))
  }

  const handleDeviceServerChange = (mac: string, server: string) => {
    setCfg((prev) => ({
      ...prev,
      devices: prev.devices.map((d) =>
        d.mac.toLowerCase() === mac.toLowerCase() ? { ...d, server: server || undefined } : d
      ),
    }))
  }

  const handleSelectAllDevices = (selectAll: boolean) => {
    if (!selectAll) {
      setCfg((prev) => ({ ...prev, devices: [] }))
      return
    }
    const currentMacs = new Set(cfg.devices.map((d) => d.mac.toLowerCase()))
    const newItems: GamingDevice[] = [...cfg.devices]
    for (const d of devices) {
      if (!currentMacs.has(d.mac.toLowerCase())) {
        newItems.push({
          mac: d.mac,
          ip: d.ip,
          ipv6: d.ipv6 || [],
          name: d.name || d.ip,
          enabled: true,
          server: undefined,
        })
      }
    }
    setCfg((prev) => ({
      ...prev,
      devices: newItems.map((d) => ({ ...d, enabled: true })),
    }))
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

      const newCfg: GamingConfig = {
        ...cfg,
        devices: cfg.devices,
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

  const handleToggleIgnoreConn = async (target: string, currentlyIgnored: boolean) => {
    try {
      const res = await apiPost<{ saved: boolean; ignored_game_conns: string[]; recent_gaming_conns: RecentGamingConn[] }>(
        'gaming/ignore-conn',
        { target, remove: currentlyIgnored }
      )
      notify(currentlyIgnored ? `✓ Хост '${target}' удален из игнор-листа` : `✓ Хост '${target}' добавлен в игнор-лист`)
      if (res?.recent_gaming_conns) {
        setStatus((prev) => (prev ? { ...prev, recent_gaming_conns: res.recent_gaming_conns } : prev))
      } else {
        await loadData()
      }
    } catch (e: any) {
      notify(e.message || 'Ошибка обновления игнор-листа', true)
    }
  }

  const handleToggleSmartMode = async (enabled: boolean) => {
    const updated = { ...cfg, smart_mode: enabled }
    setCfg(updated)
    try {
      await apiPost('gaming/save', { gaming: updated })
      notify(enabled ? '✓ Умный игровой режим включен' : 'Умный игровой режим выключен')
    } catch (e: any) {
      notify(e.message || 'Ошибка сохранения настроек', true)
    }
  }

  const enabledCount = Object.values(cfg.platforms).filter(Boolean).length
  const activeDevices = (cfg.devices || []).filter((d) => d.enabled)
  const isActuallyActive = status ? (status.is_active !== undefined ? status.is_active : cfg.enabled) : cfg.enabled

  const filteredRealConns = (status?.real_connections || []).filter((c) => {
    if (connFilterDevice === 'all') return true
    return (
      (c.device_ip && c.device_ip === connFilterDevice) ||
      (c.device_name && c.device_name.toLowerCase().includes(connFilterDevice.toLowerCase()))
    )
  })

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
      {/* 1. Главная карточка статуса, узла и действия */}
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
                    background:
                      cfg.mode === 'bypass_ru'
                        ? 'rgba(56, 189, 248, 0.15)'
                        : cfg.mode === 'compatibility'
                        ? 'rgba(245, 158, 11, 0.15)'
                        : 'rgba(168, 85, 247, 0.15)',
                    color:
                      cfg.mode === 'bypass_ru'
                        ? '#38bdf8'
                        : cfg.mode === 'compatibility'
                        ? '#fbbf24'
                        : '#c084fc',
                    fontSize: 11,
                  }}
                >
                  {cfg.mode === 'bypass_ru'
                    ? '🌐 Все в прокси кроме RU'
                    : cfg.mode === 'compatibility'
                    ? '🔒 100% Все в прокси'
                    : '🕹️ Только игры (GeoSite)'}
                </span>
                {activeDevices.length > 0 && (
                  <span
                    className="badge"
                    style={{ background: 'rgba(34, 197, 94, 0.15)', color: '#4ade80', fontSize: 11 }}
                  >
                    {activeDevices.length} устр. онлайн
                  </span>
                )}
              </div>
              <p className="muted small" style={{ margin: '4px 0 0' }}>
                {cfg.mode === 'bypass_ru'
                  ? 'Весь интернет-трафик и игры идут через туннель, кроме российских сервисов и IP (RU DIRECT).'
                  : cfg.mode === 'compatibility'
                  ? 'Строгий туннель: 100% интернет-трафика устройств без исключений направляется через туннель.'
                  : 'Экономный режим: направляет через туннель только известные игровые платформы и category-games.'}
              </p>
            </div>
          </div>

          <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
            <button
              type="button"
              className={`btn sm ${cfg.enabled ? 'btn-danger' : 'btn-primary'}`}
              onClick={() => handleToggle(!cfg.enabled)}
              disabled={toggling}
              style={{ minWidth: 160, fontWeight: 600 }}
            >
              {toggling
                ? '⏳ Применение...'
                : cfg.enabled
                ? '⏹ Выключить'
                : cfg.mode === 'compatibility'
                ? '▶ Включить совместимость'
                : '▶ Включить игровой режим'}
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

        {/* Статус в UI: Устройства, активный туннель, TCP/UDP, IPv6 */}
        <div
          style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))',
            gap: 10,
            marginTop: 16,
            padding: '12px 14px',
            background: 'rgba(0, 0, 0, 0.25)',
            borderRadius: 8,
            border: '1px solid rgba(255, 255, 255, 0.05)',
          }}
        >
          <div>
            <div className="muted small">📱 Игровые устройства</div>
            <div style={{ fontWeight: 600, fontSize: 13, marginTop: 2 }}>
              {activeDevices.length > 0 ? `${activeDevices.length} активных` : 'Не выбраны'}
            </div>
            <div className="muted small" style={{ fontSize: 11, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
              {activeDevices.length > 0
                ? activeDevices.map((d) => d.name || d.ip).join(', ')
                : 'Добавьте устройства ниже'}
            </div>
          </div>

          <div>
            <div className="muted small">🎯 Основной туннель</div>
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
              {activeDevices.some((d) => d.ipv6 && d.ipv6.length > 0)
                ? 'IPv6 поддерживается'
                : 'Только IPv4'}
            </div>
          </div>
        </div>
      </div>

      {/* 2. Пресеты маршрутизации трафика (как в XKeen UI) */}
      <div className="card" style={{ padding: '16px 18px' }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 12, flexWrap: 'wrap', gap: 8 }}>
          <div>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <span style={{ fontSize: 20 }}>📋</span>
              <h3 style={{ margin: 0, fontSize: 15, fontWeight: 700 }}>Пресет маршрутизации трафика</h3>
            </div>
            <p className="muted small" style={{ margin: '4px 0 0', fontSize: 12 }}>
              Выберите поведение игрового режима для выбранных устройств (механизм аналогичен XKeen UI)
            </p>
          </div>
          <span className="badge" style={{ background: 'rgba(56, 189, 248, 0.15)', color: '#38bdf8', fontSize: 11 }}>
            {cfg.mode === 'bypass_ru' ? 'Обход RU' : cfg.mode === 'compatibility' ? '100% Туннель' : 'GeoSite игры'}
          </span>
        </div>

        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(280px, 1fr))', gap: 12 }}>
          {/* Пресет 1: Все в прокси кроме RU */}
          <div
            onClick={() => handleModeChange('bypass_ru')}
            style={{
              padding: '14px 16px',
              borderRadius: 8,
              cursor: 'pointer',
              background: cfg.mode === 'bypass_ru' ? 'rgba(56, 189, 248, 0.12)' : 'rgba(255, 255, 255, 0.02)',
              border: cfg.mode === 'bypass_ru' ? '1px solid rgba(56, 189, 248, 0.5)' : '1px solid rgba(255, 255, 255, 0.08)',
              display: 'flex',
              flexDirection: 'column',
              justifyContent: 'space-between',
              transition: 'all 0.15s ease',
            }}
          >
            <div>
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 6 }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                  <input type="radio" checked={cfg.mode === 'bypass_ru'} onChange={() => {}} />
                  <b style={{ fontSize: 13.5, color: '#e2e8f0' }}>Все в прокси кроме RU</b>
                </div>
                <span className="badge" style={{ background: 'rgba(34, 197, 94, 0.15)', color: '#4ade80', fontSize: 10 }}>
                  ⭐ Рекомендуется
                </span>
              </div>
              <p className="muted small" style={{ margin: 0, lineHeight: 1.45, fontSize: 11.5 }}>
                Весь интернет-трафик и игры идут через туннель, а российские домены (.RU, госуслуги, банки) и GeoIP RU идут напрямую (DIRECT).
              </p>
            </div>
            <div style={{ marginTop: 10, display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: 6 }}>
              <span style={{ fontSize: 11, color: cfg.mode === 'bypass_ru' ? '#38bdf8' : '#94a3b8', fontWeight: 600 }}>
                {cfg.mode === 'bypass_ru' ? '✓ Выбранный пресет' : 'Выбрать пресет'}
              </span>
              <button
                type="button"
                className={`btn sm ${cfg.enabled && cfg.mode === 'bypass_ru' ? 'btn-danger' : 'btn-primary'}`}
                onClick={(e) => {
                  e.stopPropagation()
                  if (cfg.enabled && cfg.mode === 'bypass_ru') {
                    handleToggle(false)
                  } else {
                    handleToggle(true, 'bypass_ru')
                  }
                }}
                disabled={toggling}
                style={{ fontSize: 11 }}
              >
                {cfg.enabled && cfg.mode === 'bypass_ru' ? '⏹ Отключить режим' : '▶ Включить обход RU'}
              </button>
            </div>
          </div>

          {/* Пресет 2: Все напрямую кроме игр */}
          <div
            onClick={() => handleModeChange('known_services')}
            style={{
              padding: '14px 16px',
              borderRadius: 8,
              cursor: 'pointer',
              background: cfg.mode === 'known_services' ? 'rgba(168, 85, 247, 0.12)' : 'rgba(255, 255, 255, 0.02)',
              border: cfg.mode === 'known_services' ? '1px solid rgba(168, 85, 247, 0.5)' : '1px solid rgba(255, 255, 255, 0.08)',
              display: 'flex',
              flexDirection: 'column',
              justifyContent: 'space-between',
              transition: 'all 0.15s ease',
            }}
          >
            <div>
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 6 }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                  <input type="radio" checked={cfg.mode === 'known_services'} onChange={() => {}} />
                  <b style={{ fontSize: 13.5, color: '#e2e8f0' }}>Все напрямую кроме игр</b>
                </div>
                <span className="badge" style={{ background: 'rgba(168, 85, 247, 0.15)', color: '#c084fc', fontSize: 10 }}>
                  🕹️ GeoSite
                </span>
              </div>
              <p className="muted small" style={{ margin: 0, lineHeight: 1.45, fontSize: 11.5 }}>
                Обычный трафик идет напрямую. В туннель направляются только известные игровые платформы (Steam, Discord, Xbox, PSN) и <code>category-games</code>.
              </p>
              <div
                style={{
                  marginTop: 6,
                  padding: '4px 6px',
                  borderRadius: 4,
                  background: 'rgba(234, 179, 8, 0.1)',
                  color: '#fde047',
                  fontSize: 10.5,
                  lineHeight: 1.35,
                }}
              >
                ⚠️ <b>Важно:</b> этот режим <b>не охватывает неизвестные игры</b>. Роутер не может классифицировать закрытые игровые протоколы без доменов.
              </div>
            </div>
            <div style={{ marginTop: 10, display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: 6 }}>
              <span style={{ fontSize: 11, color: cfg.mode === 'known_services' ? '#c084fc' : '#94a3b8', fontWeight: 600 }}>
                {cfg.mode === 'known_services' ? '✓ Выбранный пресет' : 'Выбрать пресет'}
              </span>
              <button
                type="button"
                className={`btn sm ${cfg.enabled && cfg.mode === 'known_services' ? 'btn-danger' : 'btn-secondary'}`}
                onClick={(e) => {
                  e.stopPropagation()
                  if (cfg.enabled && cfg.mode === 'known_services') {
                    handleToggle(false)
                  } else {
                    handleToggle(true, 'known_services')
                  }
                }}
                disabled={toggling}
                style={{ fontSize: 11 }}
              >
                {cfg.enabled && cfg.mode === 'known_services' ? '⏹ Отключить известные сервисы' : '▶ Включить режим известных сервисов'}
              </button>
            </div>
          </div>

          {/* Пресет 3: 100% Всё в прокси */}
          <div
            onClick={() => handleModeChange('compatibility')}
            style={{
              padding: '14px 16px',
              borderRadius: 8,
              cursor: 'pointer',
              background: cfg.mode === 'compatibility' ? 'rgba(245, 158, 11, 0.12)' : 'rgba(255, 255, 255, 0.02)',
              border: cfg.mode === 'compatibility' ? '1px solid rgba(245, 158, 11, 0.5)' : '1px solid rgba(255, 255, 255, 0.08)',
              display: 'flex',
              flexDirection: 'column',
              justifyContent: 'space-between',
              transition: 'all 0.15s ease',
            }}
          >
            <div>
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 6 }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                  <input type="radio" checked={cfg.mode === 'compatibility'} onChange={() => {}} />
                  <b style={{ fontSize: 13.5, color: '#e2e8f0' }}>100% Всё в прокси</b>
                </div>
                <span className="badge" style={{ background: 'rgba(245, 158, 11, 0.15)', color: '#fbbf24', fontSize: 10 }}>
                  🔒 Full Proxy
                </span>
              </div>
              <p className="muted small" style={{ margin: 0, lineHeight: 1.45, fontSize: 11.5 }}>
                100% трафика устройства без исключений идет через игровой узел. Решает проблемы со Strict NAT (Type 3) и блокировками серверов консолей.
              </p>
            </div>
            <div style={{ marginTop: 10, display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: 6 }}>
              <span style={{ fontSize: 11, color: cfg.mode === 'compatibility' ? '#fbbf24' : '#94a3b8', fontWeight: 600 }}>
                {cfg.mode === 'compatibility' ? '✓ Выбранный пресет' : 'Выбрать пресет'}
              </span>
              <button
                type="button"
                className={`btn sm ${cfg.enabled && cfg.mode === 'compatibility' ? 'btn-danger' : 'btn-primary'}`}
                onClick={(e) => {
                  e.stopPropagation()
                  if (cfg.enabled && cfg.mode === 'compatibility') {
                    handleToggle(false)
                  } else {
                    handleToggle(true, 'compatibility')
                  }
                }}
                disabled={toggling}
                style={{ fontSize: 11 }}
              >
                {cfg.enabled && cfg.mode === 'compatibility' ? '⏹ Отключить совместимость' : '▶ Включить полную совместимость'}
              </button>
            </div>
          </div>
        </div>
      </div>

      {/* 3. Управление устройствами и серверами */}
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
              <option value="PROXY">🔒 PROXY (Основной туннель)</option>
              <option value="DIRECT">⏭ DIRECT (Прямой выход)</option>
              {servers.map((s) => (
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
                <div style={{ display: 'flex', alignItems: 'center', gap: 10, flex: 1, minWidth: 260, justifyContent: 'flex-end' }}>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
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
                      <option value="PROXY">🔒 PROXY (Основной туннель)</option>
                      <option value="DIRECT">⏭ DIRECT (Прямой выход)</option>
                      {servers.map((s) => (
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


      {/* Умный игровой режим (Smart Auto-Gaming) и детектор игр */}
      <div className="card" style={{ padding: '16px 18px', border: cfg.smart_mode ? '1px solid rgba(56, 189, 248, 0.4)' : '1px solid var(--border)' }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: 12 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
            <span style={{ fontSize: 24 }}>🧠</span>
            <div>
              <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                <b style={{ fontSize: 14 }}>Умный игровой режим (Smart Auto-Gaming)</b>
                <span className="badge" style={{ background: cfg.smart_mode ? 'rgba(34, 197, 94, 0.2)' : 'rgba(255, 255, 255, 0.08)', color: cfg.smart_mode ? '#4ade80' : 'var(--muted)' }}>
                  {cfg.smart_mode ? '🟢 Активен' : '⚪ Выключен'}
                </span>
                {status?.recent_gaming_conns && status.recent_gaming_conns.length > 0 && (
                  <span className="badge" style={{ background: 'rgba(56, 189, 248, 0.15)', color: '#38bdf8', fontSize: 11 }}>
                    {status.recent_gaming_conns.length} хостов в детекторе
                  </span>
                )}
              </div>
              <p className="muted small" style={{ margin: '3px 0 0', fontSize: 12, lineHeight: 1.4 }}>
                Автоматически включает игровой туннель при обнаружении сетевой активности игр на выбранном устройстве и отключает после таймаута бездействия для экономии VPS.
              </p>
            </div>
          </div>

          <div style={{ display: 'flex', alignItems: 'center', gap: 16 }}>
            <label className="switch" title={cfg.smart_mode ? 'Отключить умный режим' : 'Включить умный режим'}>
              <input
                type="checkbox"
                checked={cfg.smart_mode ?? false}
                onChange={(e) => handleToggleSmartMode(e.target.checked)}
              />
              <span className="slider" />
            </label>
          </div>
        </div>

        {cfg.smart_mode && (
          <div style={{ marginTop: 14, paddingTop: 14, borderTop: '1px solid rgba(255, 255, 255, 0.06)', display: 'flex', alignItems: 'center', flexWrap: 'wrap', gap: 16 }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <label htmlFor="smart-idle-timeout" style={{ fontSize: 12.5, color: '#94a3b8' }}>
                ⏱️ Таймаут бездействия (мин):
              </label>
              <input
                id="smart-idle-timeout"
                type="number"
                min={1}
                max={180}
                value={cfg.smart_idle_timeout_mins ?? 15}
                onChange={(e) => setCfg({ ...cfg, smart_idle_timeout_mins: Math.max(1, parseInt(e.target.value, 10) || 15) })}
                style={{ width: 75, padding: '4px 8px', borderRadius: 6, border: '1px solid var(--border)', background: 'rgba(0,0,0,0.3)', color: '#fff', fontSize: 12 }}
              />
            </div>
            <span className="muted small" style={{ fontSize: 11.5 }}>
              Если в течение {cfg.smart_idle_timeout_mins ?? 15} мин нет сетевых игровых пакетов, туннель перейдет в режим ожидания.
            </span>
          </div>
        )}

        {/* Недавняя игровая активность и игнор-лист детектора внутри единой карточки */}
        <div style={{ marginTop: 16, paddingTop: 14, borderTop: '1px solid rgba(255, 255, 255, 0.06)' }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 10, flexWrap: 'wrap', gap: 8 }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <span style={{ fontSize: 16 }}>🕹️</span>
              <b style={{ fontSize: 13.5 }}>Недавняя игровая активность (детектор игр)</b>
            </div>
            <button
              type="button"
              className="btn sm"
              onClick={loadData}
              style={{ fontSize: 12 }}
            >
              🔄 Обновить
            </button>
          </div>

          {status?.recent_gaming_conns && status.recent_gaming_conns.length > 0 ? (
            <div style={{ overflowX: 'auto', maxHeight: 220, overflowY: 'auto' }}>
              <table style={{ width: '100%', fontSize: 12, borderCollapse: 'collapse' }}>
                <thead>
                  <tr style={{ textAlign: 'left', borderBottom: '1px solid rgba(255, 255, 255, 0.08)', color: '#94a3b8' }}>
                    <th style={{ padding: '6px 8px' }}>Хост / Назначение</th>
                    <th style={{ padding: '6px 8px' }}>Протокол</th>
                    <th style={{ padding: '6px 8px' }}>Время</th>
                    <th style={{ padding: '6px 8px' }}>Статус</th>
                    <th style={{ padding: '6px 8px', textAlign: 'right' }}>Действие</th>
                  </tr>
                </thead>
                <tbody>
                  {status.recent_gaming_conns.map((conn, idx) => {
                    const target = conn.destination || conn.host
                    return (
                      <tr key={idx} style={{ borderBottom: '1px solid rgba(255, 255, 255, 0.04)', height: 32 }}>
                        <td style={{ padding: '6px 8px', fontFamily: 'Consolas, monospace', fontSize: 11.5 }}>
                          <span title={conn.destination}>{conn.host || conn.destination}</span>
                        </td>
                        <td style={{ padding: '6px 8px' }}>
                          <span
                            className="badge"
                            style={{
                              background: conn.network === 'UDP' ? 'rgba(168, 85, 247, 0.2)' : 'rgba(56, 189, 248, 0.2)',
                              color: conn.network === 'UDP' ? '#c084fc' : '#38bdf8',
                              fontSize: 10,
                              padding: '1px 5px',
                            }}
                          >
                            {conn.network}
                          </span>
                        </td>
                        <td style={{ padding: '6px 8px', color: '#94a3b8', fontSize: 11 }}>
                          {conn.last_seen || 'Недавно'}
                        </td>
                        <td style={{ padding: '6px 8px' }}>
                          {conn.ignored ? (
                            <span className="badge" style={{ background: 'rgba(239, 68, 68, 0.15)', color: '#f87171', fontSize: 10 }}>
                              В игноре
                            </span>
                          ) : (
                            <span className="badge" style={{ background: 'rgba(34, 197, 94, 0.15)', color: '#4ade80', fontSize: 10 }}>
                              Отслеживается
                            </span>
                          )}
                        </td>
                        <td style={{ padding: '6px 8px', textAlign: 'right' }}>
                          <button
                            type="button"
                            className="btn sm"
                            onClick={() => handleToggleIgnoreConn(target, !!conn.ignored)}
                            style={{
                              fontSize: 11,
                              padding: '2px 8px',
                              background: conn.ignored ? 'rgba(56, 189, 248, 0.15)' : 'rgba(239, 68, 68, 0.12)',
                              color: conn.ignored ? '#38bdf8' : '#f87171',
                              border: conn.ignored ? '1px solid rgba(56, 189, 248, 0.3)' : '1px solid rgba(239, 68, 68, 0.3)',
                            }}
                          >
                            {conn.ignored ? 'Следить' : 'Не реагировать'}
                          </button>
                        </td>
                      </tr>
                    )
                  })}
                </tbody>
              </table>
            </div>
          ) : (
            <div className="muted small" style={{ padding: '12px 0', textAlign: 'center' }}>
              Пока нет зафиксированных игровых соединений. При сетевой активности игр они появятся здесь.
            </div>
          )}
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

      {/* 4. Реальные соединения устройств через Mihomo */}
      <div className="card" style={{ padding: '14px 16px' }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 10, flexWrap: 'wrap', gap: 8 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <span style={{ fontSize: 16 }}>📊</span>
            <b style={{ fontSize: 13.5 }}>Реальные соединения устройств через Mihomo</b>
            {status?.real_connections && (
              <span className="badge" style={{ background: 'rgba(56, 189, 248, 0.15)', color: '#38bdf8', fontSize: 11 }}>
                {filteredRealConns.length} активных
              </span>
            )}
          </div>
          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            {cfg.devices.length > 1 && (
              <select
                className="input sm"
                value={connFilterDevice}
                onChange={(e) => setConnFilterDevice(e.target.value)}
                style={{ fontSize: 11.5 }}
              >
                <option value="all">Все устройства</option>
                {cfg.devices.map((d) => (
                  <option key={d.mac} value={d.ip || d.mac}>
                    {d.name || d.ip || d.mac}
                  </option>
                ))}
              </select>
            )}
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
        </div>

        {filteredRealConns.length > 0 ? (
          <div style={{ overflowX: 'auto', maxHeight: 240, overflowY: 'auto' }}>
            <table style={{ width: '100%', fontSize: 12, borderCollapse: 'collapse' }}>
              <thead>
                <tr style={{ textAlign: 'left', borderBottom: '1px solid rgba(255, 255, 255, 0.08)', color: '#94a3b8' }}>
                  <th style={{ padding: '6px 8px' }}>Устройство</th>
                  <th style={{ padding: '6px 8px' }}>Хост / Назначение</th>
                  <th style={{ padding: '6px 8px' }}>Протокол</th>
                  <th style={{ padding: '6px 8px' }}>Правило</th>
                  <th style={{ padding: '6px 8px' }}>Выходной узел</th>
                  <th style={{ padding: '6px 8px', textAlign: 'right' }}>Трафик</th>
                </tr>
              </thead>
              <tbody>
                {filteredRealConns.map((c: GamingRealConnection, idx: number) => (
                  <tr
                    key={c.id || idx}
                    style={{ borderBottom: '1px solid rgba(255, 255, 255, 0.04)', height: 32 }}
                  >
                    <td style={{ padding: '6px 8px', fontSize: 11 }}>
                      <span style={{ color: '#38bdf8', fontWeight: 600 }}>
                        {c.device_name || c.device_ip || '—'}
                      </span>
                    </td>
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
              ? 'Соединения от выбранных устройств пока не зафиксированы в ядре Mihomo. Запустите игру или откройте страницу на устройстве.'
              : 'Включите игровой режим или режим совместимости, чтобы отслеживать соединения.'}
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
