import { useState, useEffect, useCallback } from 'react'
import { apiGet, apiPost } from '../api'
import XboxDns from './XboxDns'
import { useDirtyGuard } from '../utils/dirtyGuard'
import { GamingPresetsSection } from './gaming/GamingPresetsSection'
import { GamingDevicesSection } from './gaming/GamingDevicesSection'
import { GamingSmartSection } from './gaming/GamingSmartSection'
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
  // FE-MOBILE/UX: при сбое загрузки gaming/status раньше показывался ПУСТОЙ конфиг
  // («Не выбраны» без устройств) — выглядело как будто устройства стёрты
  const [loadFailed, setLoadFailed] = useState(false)
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
  useDirtyGuard(
    'gaming',
    customText.trim() !== (Array.isArray(cfg?.custom_domains) ? cfg.custom_domains : []).join('\n').trim()
  )

  const loadData = useCallback(async () => {
    let statusOk = false
    try {
      const [statusRes, serversRes, devicesRes] = await Promise.all([
        apiGet<GamingStatus>('gaming/status').catch(() => null),
        apiGet<{ servers?: ServerInfo[]; proxies?: ServerInfo[]; all?: ServerInfo[] }>('servers').catch(() => null),
        apiGet<{ devices?: DeviceInfo[] }>('devices').catch(() => null),
      ])

      if (statusRes) {
        statusOk = true
        setStatus(statusRes)
        if (statusRes.config) {
          setCfg(statusRes.config)
          setActiveServer(statusRes.active_server || statusRes.config.target_server || 'Fastest')
          setCustomText((Array.isArray(statusRes.config.custom_domains) ? statusRes.config.custom_domains : []).join('\n'))
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
      setLoadFailed(!statusOk)
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
      const preferredMac =
        currentDevices.find((d) => d.enabled)?.mac ||
        devices.find((d) => d.is_current_device)?.mac ||
        devices[0]?.mac ||
        undefined
      const payload: any = {
        enabled: val,
        mode: targetMode,
      }
      if (preferredMac) {
        payload.device_mac = preferredMac
      }
      if (cfg.target_server) {
        payload.target_server = cfg.target_server
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

      const { ignored_game_conns, ...patchFields } = newCfg
      await apiPost('gaming/save', { gaming: patchFields })
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
      if (res?.ignored_game_conns) {
        setCfg((prev: GamingConfig) => ({ ...prev, ignored_game_conns: res.ignored_game_conns }))
      }
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
    setCfg((prev: GamingConfig) => ({ ...prev, smart_mode: enabled }))
    try {
      await apiPost('gaming/save', { smart_mode: enabled })
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

  if (loadFailed) {
    return (
      <div className="card" style={{ textAlign: 'center', padding: '2.5rem 1rem' }}>
        <div style={{ fontSize: 30, marginBottom: 10 }}>📡</div>
        <h2 style={{ marginBottom: 6 }}>Нет связи с панелью</h2>
        <p className="muted" style={{ marginBottom: 14 }}>
          Не удалось загрузить конфигурацию игрового режима. Ваши устройства и настройки в безопасности — повторите попытку.
        </p>
        <button
          type="button"
          className="btn primary"
          onClick={() => {
            setLoadFailed(false)
            setLoading(true)
            loadData()
          }}
        >
          🔄 Повторить загрузку
        </button>
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
                    background: loading
                      ? 'rgba(148, 163, 184, 0.2)'
                      : isActuallyActive
                      ? 'rgba(34, 197, 94, 0.2)'
                      : cfg.enabled
                      ? 'rgba(234, 179, 8, 0.2)'
                      : 'rgba(148, 163, 184, 0.15)',
                    color: loading ? '#94a3b8' : isActuallyActive ? '#4ade80' : cfg.enabled ? '#facc15' : '#94a3b8',
                    fontWeight: 600,
                  }}
                >
                  {loading ? '⏳ ПРОВЕРКА...' : isActuallyActive ? '🟢 АКТИВЕН' : cfg.enabled ? '⚠️ МАРШРУТ НЕ ПОДТВЕРЖДЕН' : '⚪ ОТКЛЮЧЕН'}
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
              disabled={loading || toggling}
              style={{ minWidth: 160, fontWeight: 600 }}
            >
              {loading
                ? '⏳ Проверка...'
                : toggling
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

        {/* Статус в UI: Устройства, активный туннель, TCP/UDP, IPv6, NAT */}
        <div
          style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fit, minmax(170px, 1fr))',
            gap: 10,
            marginTop: 16,
            padding: '12px 14px',
            background: 'linear-gradient(135deg, rgba(15, 23, 42, 0.75) 0%, rgba(30, 41, 59, 0.6) 100%)',
            borderRadius: 8,
            border: '1px solid rgba(56, 189, 248, 0.15)',
            boxShadow: 'inset 0 1px 0 rgba(255, 255, 255, 0.05)',
          }}
        >
          <div>
            <div className="muted small" style={{ fontSize: 11, letterSpacing: '0.3px', textTransform: 'uppercase' }}>📱 Устройства</div>
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
            <div className="muted small" style={{ fontSize: 11, letterSpacing: '0.3px', textTransform: 'uppercase' }}>🎯 Основной туннель</div>
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
            <div className="muted small" style={{ fontSize: 11, letterSpacing: '0.3px', textTransform: 'uppercase' }}>🔌 Перехват TCP / UDP</div>
            <div style={{ display: 'flex', gap: 6, marginTop: 4, flexWrap: 'wrap' }}>
              <span
                className="badge"
                style={{
                  background: status?.tcp_interception ? 'rgba(34, 197, 94, 0.2)' : 'rgba(239, 68, 68, 0.2)',
                  color: status?.tcp_interception ? '#4ade80' : '#f87171',
                  fontSize: 10.5,
                  border: status?.tcp_interception ? '1px solid rgba(34, 197, 94, 0.4)' : '1px solid rgba(239, 68, 68, 0.4)',
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
                  border: status?.udp_interception ? '1px solid rgba(34, 197, 94, 0.4)' : '1px solid rgba(239, 68, 68, 0.4)',
                }}
              >
                UDP: {status?.udp_interception ? '✓ OK' : 'Сбой'}
              </span>
            </div>
          </div>

          <div>
            <div className="muted small" style={{ fontSize: 11, letterSpacing: '0.3px', textTransform: 'uppercase' }}>⚡ TProxy / Full-Cone</div>
            <div style={{ fontWeight: 600, fontSize: 13, marginTop: 3, display: 'flex', alignItems: 'center', gap: 6 }}>
              <span className="badge" style={{ background: 'rgba(56, 189, 248, 0.15)', color: '#38bdf8', fontSize: 10.5, border: '1px solid rgba(56, 189, 248, 0.35)' }}>
                TProxy + Full-Cone NAT
              </span>
            </div>
            <div className="muted small" style={{ fontSize: 11 }}>
              Type 1 / Open NAT
            </div>
          </div>

          <div>
            <div className="muted small" style={{ fontSize: 11, letterSpacing: '0.3px', textTransform: 'uppercase' }}>🌐 IPv6 маршрут</div>
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
      <GamingPresetsSection
        cfg={cfg}
        toggling={toggling}
        handleModeChange={handleModeChange}
        handleToggle={handleToggle}
      />

      {/* 3. Управление устройствами и серверами */}
      <GamingDevicesSection
        cfg={cfg}
        setCfg={setCfg}
        devices={devices}
        servers={servers}
        activeServer={activeServer}
        deviceDropdownOpen={deviceDropdownOpen}
        setDeviceDropdownOpen={setDeviceDropdownOpen}
        deviceSearch={deviceSearch}
        setDeviceSearch={setDeviceSearch}
        pinging={pinging}
        getDeviceIcon={getDeviceIcon}
        handleAddDevice={handleAddDevice}
        handleSelectAllDevices={handleSelectAllDevices}
        handleToggleDeviceEnabled={handleToggleDeviceEnabled}
        handleDeviceServerChange={handleDeviceServerChange}
        handleRemoveDevice={handleRemoveDevice}
        handlePingTest={handlePingTest}
      />


      {/* Умный игровой режим (Smart Auto-Gaming) и детектор игр */}
      <GamingSmartSection
        cfg={cfg}
        setCfg={setCfg}
        status={status}
        handleToggleSmartMode={handleToggleSmartMode}
        handleToggleIgnoreConn={handleToggleIgnoreConn}
        loadData={loadData}
      />

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
                      <span title={(Array.isArray(c.chains) ? c.chains : []).join(' → ') || '🎮 Gaming'}>
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
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 14, flexWrap: 'wrap', gap: 8 }}>
          <div>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <span style={{ fontSize: 18 }}>🎮</span>
              <h3 style={{ margin: 0, fontSize: 15, fontWeight: 700, letterSpacing: '-0.2px' }}>🎯 Целевые игровые платформы</h3>
            </div>
            <span className="muted small" style={{ fontSize: 11.5 }}>
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
                  borderRadius: 10,
                  cursor: 'pointer',
                  userSelect: 'none',
                  transition: 'all 0.2s cubic-bezier(0.4, 0, 0.2, 1)',
                  background: isChecked
                    ? 'linear-gradient(135deg, rgba(56, 189, 248, 0.12) 0%, rgba(14, 165, 233, 0.05) 100%)'
                    : 'rgba(255, 255, 255, 0.02)',
                  border: isChecked ? '1px solid rgba(56, 189, 248, 0.45)' : '1px solid rgba(255, 255, 255, 0.06)',
                  boxShadow: isChecked ? '0 0 16px rgba(56, 189, 248, 0.18), inset 0 0 10px rgba(56, 189, 248, 0.08)' : 'none',
                  display: 'flex',
                  alignItems: 'flex-start',
                  gap: 12,
                }}
              >
                <input
                  type="checkbox"
                  checked={isChecked}
                  onChange={() => {}}
                  style={{ marginTop: 3, cursor: 'pointer', accentColor: '#38bdf8' }}
                />
                <div style={{ flex: 1 }}>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                    <span style={{ fontSize: 18, filter: isChecked ? 'drop-shadow(0 0 6px rgba(56, 189, 248, 0.6))' : 'none' }}>{p.icon}</span>
                    <span style={{ fontWeight: 600, fontSize: 13.5, color: isChecked ? '#38bdf8' : '#e2e8f0' }}>{p.name}</span>
                    {p.badge && (
                      <span
                        className="badge"
                        style={{
                          background: 'rgba(244, 63, 94, 0.15)',
                          color: '#fb7185',
                          fontSize: 10,
                          padding: '1px 5px',
                          border: '1px solid rgba(244, 63, 94, 0.3)',
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
          <div style={{ display: 'flex', alignItems: 'center', gap: 8, margin: '0 0 10px' }}>
            <h3 style={{ margin: 0, fontSize: 14.5, fontWeight: 700 }}>⚡ Оптимизации задержки и NAT</h3>
            <span className="badge badge-subtle" style={{ fontSize: 11 }}>Встроено в правила</span>
          </div>

          <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
            <div style={{ opacity: 0.9 }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                <span style={{ color: 'var(--color-success, #10b981)' }}>✓</span>
                <b style={{ fontSize: 13 }}>Smart Split-Tunneling (Активно)</b>
              </div>
              <p className="muted small" style={{ margin: '2px 0 0 18px' }}>
                Проксирование только авторизации, сервисов обновлений и заблокированных API при сохранении прямого матча на минимальном пинге.
              </p>
            </div>

            <div style={{ opacity: 0.9 }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                <span style={{ color: 'var(--color-success, #10b981)' }}>✓</span>
                <b style={{ fontSize: 13 }}>Anti-Cheat & Strict NAT Fix (Fake-IP Bypass)</b>
              </div>
              <p className="muted small" style={{ margin: '2px 0 0 18px' }}>
                Предотвращает предупреждения «Strict NAT» (Type 3) на PlayStation/Xbox и конфликты античитов (Vanguard, BattlEye, EasyAntiCheat).
              </p>
            </div>
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

      {/* 7. Интеграция Xbox DNS & SmartDNS Bypass */}
      <div style={{ marginTop: 20, marginBottom: 16 }}>
        <XboxDns notify={notify} />
      </div>

      {/* 8. Плавающая нижняя панель сохранения */}
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
