import { useState, useEffect, useCallback } from 'react'
import { apiGet, apiPost } from '../api'
import type { GamingConfig, GamingPingResult, GamingStatus, ServerInfo } from '../types'

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

export default function Gaming({ notify }: GamingProps) {
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [pinging, setPinging] = useState(false)
  const [pingResults, setPingResults] = useState<GamingPingResult[] | null>(null)
  const [activeServer, setActiveServer] = useState<string>('')
  const [servers, setServers] = useState<ServerInfo[]>([])

  const [cfg, setCfg] = useState<GamingConfig>({
    enabled: false,
    target_server: 'Fastest',
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
      const [statusRes, serversRes] = await Promise.all([
        apiGet<GamingStatus>('gaming/status').catch(() => null),
        apiGet<{ proxies?: ServerInfo[]; all?: ServerInfo[] }>('servers').catch(() => null),
      ])

      if (statusRes?.config) {
        setCfg(statusRes.config)
        setActiveServer(statusRes.active_server || statusRes.config.target_server || 'Fastest')
        setCustomText((statusRes.config.custom_domains || []).join('\n'))
      }

      if (serversRes) {
        const list = serversRes.proxies || serversRes.all || []
        setServers(list.filter((s) => s.id && s.id !== 'REJECT' && s.id !== 'DIRECT'))
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

  const handleToggle = async (val: boolean) => {
    setCfg((prev) => ({ ...prev, enabled: val }))
    try {
      await apiPost('gaming/toggle', { enabled: val })
      notify(val ? '🟢 Игровой режим включен' : '⚪ Игровой режим выключен')
      loadData()
    } catch (e: any) {
      setCfg((prev) => ({ ...prev, enabled: !val }))
      notify(e.message || 'Ошибка переключения игрового режима', true)
    }
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
        custom_domains: uniqueDomains,
      }

      await apiPost('gaming/save', { gaming: newCfg })
      setCfg(newCfg)
      notify('✓ Настройки игрового режима успешно сохранены и применены')
      loadData()
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
      {/* Главная карточка статуса и шлюза */}
      <div
        className="card"
        style={{
          background: cfg.enabled
            ? 'linear-gradient(135deg, rgba(34, 197, 94, 0.08) 0%, rgba(14, 165, 233, 0.05) 100%)'
            : 'rgba(255, 255, 255, 0.02)',
          border: cfg.enabled ? '1px solid rgba(34, 197, 94, 0.35)' : '1px solid rgba(255, 255, 255, 0.08)',
          boxShadow: cfg.enabled ? '0 0 24px rgba(34, 197, 94, 0.12)' : 'none',
          position: 'relative',
          overflow: 'hidden',
        }}
      >
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: 14 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 14 }}>
            <span style={{ fontSize: 32 }}>🎮</span>
            <div>
              <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                <h2 style={{ margin: 0, fontSize: 18, fontWeight: 700 }}>Игровой режим (Gaming Mode)</h2>
                <span
                  className="badge"
                  style={{
                    background: cfg.enabled ? 'rgba(34, 197, 94, 0.2)' : 'rgba(148, 163, 184, 0.15)',
                    color: cfg.enabled ? '#4ade80' : '#94a3b8',
                    fontWeight: 600,
                  }}
                >
                  {cfg.enabled ? '🟢 АКТИВЕН' : '⚪ ОТКЛЮЧЕН'}
                </span>
              </div>
              <p className="muted small" style={{ margin: '4px 0 0' }}>
                Обход региональных санкций зарубежных издателей (Brawl Stars, EA, Blizzard, PSN) и блокировок РКН (Discord, Steam, Xbox 0x80a40401).
              </p>
            </div>
          </div>

          <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
            <button
              type="button"
              className={`btn sm ${cfg.enabled ? 'btn-danger' : 'btn-primary'}`}
              onClick={() => handleToggle(!cfg.enabled)}
              style={{ minWidth: 130, fontWeight: 600 }}
            >
              {cfg.enabled ? '⏹ Выключить' : '▶ Включить'}
            </button>
          </div>
        </div>

        {/* Настройки выходного узла и быстрые метрики */}
        <div
          style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fit, minmax(220px, 1fr))',
            gap: 12,
            marginTop: 18,
            paddingTop: 16,
            borderTop: '1px solid rgba(255, 255, 255, 0.08)',
          }}
        >
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
      </div>

      {/* Результаты пинга, если замерены */}
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

      {/* Сетка платформ и сервисов */}
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
                  onChange={() => {}} // Обработка в onClick контейнера
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

      {/* Опции оптимизации и пользовательские домены */}
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(320px, 1fr))', gap: 14 }}>
        {/* Режимы оптимизации */}
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
                  Проксировать только авторизацию, сервисы обновлений и заблокированные API, сохраняя игровой матч на минимальном прямом пинге (DIRECT).
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

        {/* Пользовательские домены */}
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

      {/* Плавающая нижняя панель сохранения */}
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
