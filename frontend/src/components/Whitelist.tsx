import { useCallback, useEffect, useMemo, useState } from 'react'
import { apiGet, apiPost } from '../api'
import {
  type DeviceInfo,
  type ServerInfo,
  type StatusInfo,
  type WhitelistCatalogItem,
  type WhitelistCatalogResponse,
  type WhitelistConfig,
  type WhitelistStatusResponse,
  type WhitelistStrategy,
} from '../types'

interface Props {
  notify: (msg: string, isError?: boolean) => void
  status?: StatusInfo | null
  refresh?: () => void
}

interface DomesticCatalogEntryView {
  domain: string
  category: string
  description?: string
}

const CATEGORY_MAP: Record<string, { label: string; filterKey: string }> = {
  all: { label: 'Все (376)', filterKey: 'all' },
  gov: { label: '🏛️ Госуслуги (55)', filterKey: 'category 1' },
  bank: { label: '💳 Банки (65)', filterKey: 'category 2' },
  portal: { label: '🌐 Порталы (45)', filterKey: 'category 3' },
  ecommerce: { label: '🛍️ E-commerce (55)', filterKey: 'category 4' },
  logistics: { label: '📦 Логистика (40)', filterKey: 'category 5' },
  telecom: { label: '📡 Телеком (35)', filterKey: 'category 6' },
  media: { label: '📺 Медиа (45)', filterKey: 'category 7' },
  cloud: { label: '☁️ Облака (36)', filterKey: 'category 8' },
}

export default function Whitelist({ notify, refresh }: Props) {
  const [config, setConfig] = useState<WhitelistConfig>({
    enabled: false,
    auto_by_failover: true,
    strategy: 'strict_xkeen',
    target_server: null,
    follow_failover: true,
    excluded_devices: [],
    devices: [],
    gaming_suspended: false,
    custom_domains: [],
  })
  const [activeServer, setActiveServer] = useState<string>('')
  const [isBsServer, setIsBsServer] = useState<boolean>(false)
  const [devicesList, setDevicesList] = useState<DeviceInfo[]>([])
  const [, setServersListUnused] = useState<ServerInfo[]>([])
  void setServersListUnused
  const [catalogItems, setCatalogItems] = useState<DomesticCatalogEntryView[]>([])
  const [catalogTotal, setCatalogTotal] = useState<number>(376)
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState(false)
  const [serversList, setServersList] = useState<ServerInfo[]>([])

  // Поиск устройств для исключений
  const [deviceSearch, setDeviceSearch] = useState('')

  // Вкладка менеджера доменов ('catalog' | 'custom')
  const [domainTab, setDomainTab] = useState<'catalog' | 'custom'>('catalog')
  const [catalogSearch, setCatalogSearch] = useState('')
  const [selectedCategory, setSelectedCategory] = useState<string>('all')
  const [catalogPage, setCatalogPage] = useState<number>(1)
  const pageSize = 20

  // Добавление пользовательского домена
  const [newDomainInput, setNewDomainInput] = useState('')

  const loadData = useCallback(async () => {
    try {
      const [wlStatus, devRes, srvRes, catRes] = await Promise.all([
        apiGet<WhitelistStatusResponse>('whitelist').catch(() => null),
        apiGet<{ devices: DeviceInfo[] }>('devices').catch(() => ({ devices: [] })),
        apiGet<{ servers: ServerInfo[] }>('servers').catch(() => ({ servers: [] })),
        apiGet<WhitelistCatalogResponse>('whitelist/catalog').catch(() => null),
      ])

      if (wlStatus) {
        setConfig({
          enabled: Boolean(wlStatus.enabled),
          auto_by_failover: wlStatus.auto_by_failover ?? true,
          strategy: wlStatus.strategy || 'strict_xkeen',
          target_server: wlStatus.target_server || null,
          follow_failover: wlStatus.follow_failover ?? true,
          excluded_devices: wlStatus.excluded_devices || [],
          devices: wlStatus.devices || [],
          gaming_suspended: Boolean(wlStatus.gaming_suspended),
          custom_domains: wlStatus.custom_domains || [],
        })
        if (wlStatus.active_server) {
          setActiveServer(wlStatus.active_server)
        }
        setIsBsServer(Boolean(wlStatus.is_bs_server))
      }

      if (devRes?.devices) {
        setDevicesList(devRes.devices)
      }

      if (srvRes?.servers) {
        setServersList(
          srvRes.servers.filter(
            (s) => !s.is_pool && s.id !== 'PROXY' && s.id !== 'GLOBAL' && s.id !== 'Fastest' && s.id !== 'Fallback'
          )
        )
      }      if (catRes) {
        setCatalogTotal(catRes.total || (catRes.domains ? catRes.domains.length : 376))
        if (catRes.items && catRes.items.length > 0) {
          setCatalogItems(
            catRes.items.map((it: WhitelistCatalogItem) => ({
              domain: it.domain,
              category: it.category,
              description: it.description || it.name || '',
            }))
          )
        } else if (catRes.domains && catRes.domains.length > 0) {
          setCatalogItems(
            catRes.domains.map((d: string) => ({
              domain: d,
              category: 'Каталог РФ',
            }))
          )
        }
      }
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка загрузки данных белых списков', true)
    } finally {
      setLoading(false)
    }
  }, [notify])

  useEffect(() => {
    loadData()
  }, [loadData])

  // Мастер-переключатель режима
  const handleToggleMaster = async () => {
    const nextState = !config.enabled
    setBusy(true)
    try {
      await apiPost('whitelist', {
        enabled: nextState,
      })
      setConfig((prev) => ({ ...prev, enabled: nextState }))
      notify(
        nextState
          ? '🛡️ Режим белых списков успешно включен'
          : '⚪ Режим белых списков выключен'
      )
      window.dispatchEvent(new CustomEvent('xr:refresh-status'))
      if (refresh) refresh()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка переключения режима белых списков', true)
      await loadData()
    } finally {
      setBusy(false)
    }
  }

  // Переключение стратегии (Логика XKeen Route vs Стандартная)
  const isStrictStrategy = config.strategy === 'strict_x_keen' || config.strategy === 'strict_xkeen'
  const handleStrategyChange = async (strategy: WhitelistStrategy) => {
    const isTargetStrict = strategy === 'strict_x_keen' || strategy === 'strict_xkeen'
    if (isStrictStrategy && isTargetStrict) return
    if (!isStrictStrategy && strategy === 'standard') return

    setBusy(true)
    try {
      await apiPost('whitelist/strategy', { strategy })
      setConfig((prev) => ({ ...prev, strategy }))
      notify(
        isTargetStrict
          ? '🛡️ Включена специальная логика XKeen Route (100% туннель, 376 РФ напрямую, без конфликтов игр)'
          : '🌐 Включена стандартная логика маршрутизации (как у обычного сервера)'
      )
      window.dispatchEvent(new CustomEvent('xr:refresh-status'))
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка смены логики работы', true)
      await loadData()
    } finally {
      setBusy(false)
    }
  }

  // Переключение авто-активации при Failover
  const handleAutoFailoverToggle = async () => {
    const nextVal = !(config.auto_by_failover ?? true)
    setBusy(true)
    try {
      await apiPost('whitelist', {
        auto_by_failover: nextVal,
        follow_failover: nextVal,
      })
      setConfig((prev) => ({ ...prev, auto_by_failover: nextVal, follow_failover: nextVal }))
      notify(
        nextVal
          ? '🔄 Автоматическое включение белых списков при переходе Failover на [БС] включено'
          : '⚪ Автоматическое включение при Failover выключено'
      )
      window.dispatchEvent(new CustomEvent('xr:refresh-status'))
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка изменения настройки Failover', true)
      await loadData()
    } finally {
      setBusy(false)
    }
  }

  // Исключение / возврат устройства в белый список (Zero-Touch Exclusions)
  const handleToggleExclude = async (deviceKey: string, currentlyExcluded: boolean) => {
    setBusy(true)
    const nextExcluded = !currentlyExcluded
    try {
      await apiPost('whitelist/exclude', {
        id: deviceKey,
        excluded: nextExcluded,
      })
      setConfig((prev) => {
        const list = [...(prev.excluded_devices || [])]
        if (nextExcluded) {
          if (!list.some((x) => x.toLowerCase() === deviceKey.toLowerCase())) {
            list.push(deviceKey)
          }
        } else {
          return {
            ...prev,
            excluded_devices: list.filter((x) => x.toLowerCase() !== deviceKey.toLowerCase()),
          }
        }
        return { ...prev, excluded_devices: list }
      })
      notify(
        nextExcluded
          ? `⚪ Устройство ${deviceKey} исключено из белых списков (DIRECT)`
          : `🛡️ Устройство ${deviceKey} возвращено в режим белых списков`
      )
      window.dispatchEvent(new CustomEvent('xr:refresh-status'))
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка обновления исключения устройства', true)
      await loadData()
    } finally {
      setBusy(false)
    }
  }

  // BS-PIN: закрепление устройства за конкретной нодой (вне Failover-качелей)
  const handlePinServer = async (device: DeviceInfo, serverId: string) => {
    setBusy(true)
    const prevDevices = config.devices
    const ip = device.ip || ''
    const nextDevices = (() => {
      const others = config.devices.filter((d) => d.ip !== ip && d.mac.toLowerCase() !== device.mac.toLowerCase())
      if (!serverId) {
        return others
      }
      return [
        ...others,
        {
          mac: device.mac,
          ip,
          enabled: true,
          name: device.name,
          server: serverId,
        },
      ]
    })()
    setConfig((prev) => ({ ...prev, devices: nextDevices }))
    try {
      await apiPost('whitelist', { devices: nextDevices })
      notify(
        serverId
          ? `📌 ${device.name || device.ip}: закреплён за узлом ${serverId} — Failover его не трогает`
          : `📌 ${device.name || device.ip}: следует общему серверу БС (авто)`
      )
      window.dispatchEvent(new CustomEvent('xr:refresh-status'))
    } catch (e) {
      setConfig((prev) => ({ ...prev, devices: prevDevices }))
      notify(e instanceof Error ? e.message : 'Ошибка закрепления узла', true)
      await loadData()
    } finally {
      setBusy(false)
    }
  }

  // Добавление пользовательского домена
  const handleAddCustomDomain = async (e: React.FormEvent) => {    e.preventDefault()
    let clean = newDomainInput.trim().toLowerCase()
    clean = clean.replace(/^https?:\/\//i, '').replace(/\/.*$/, '').replace(/:\d+$/, '')

    if (!clean) {
      notify('Введите корректное доменное имя', true)
      return
    }

    if (/\s/.test(clean)) {
      notify('Доменное имя не должно содержать пробелов', true)
      return
    }

    if (!clean.includes('.') || clean.length < 3) {
      notify('Доменное имя должно содержать точку (например, custom-portal.ru)', true)
      return
    }

    if (config.custom_domains.includes(clean)) {
      notify('Данный домен уже добавлен в белый список', true)
      return
    }

    const nextCustomDomains = [...config.custom_domains, clean]
    setBusy(true)
    try {
      await apiPost('whitelist', {
        custom_domains: nextCustomDomains,
      })
      setConfig((prev) => ({ ...prev, custom_domains: nextCustomDomains }))
      setNewDomainInput('')
      notify(`✅ Домен "${clean}" добавлен в прямой доступ (DIRECT)`)
      window.dispatchEvent(new CustomEvent('xr:refresh-status'))
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка сохранения домена', true)
    } finally {
      setBusy(false)
    }
  }

  // Удаление пользовательского домена
  const handleRemoveCustomDomain = async (domain: string) => {
    const nextCustomDomains = config.custom_domains.filter((d) => d !== domain)
    setBusy(true)
    try {
      await apiPost('whitelist', {
        custom_domains: nextCustomDomains,
      })
      setConfig((prev) => ({ ...prev, custom_domains: nextCustomDomains }))
      notify(`🗑️ Домен "${domain}" удален из прямого доступа`)
      window.dispatchEvent(new CustomEvent('xr:refresh-status'))
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка удаления домена', true)
    } finally {
      setBusy(false)
    }
  }

  // Фильтрация устройств в таблице исключений
  const filteredDevices = useMemo(() => {
    const q = deviceSearch.trim().toLowerCase()
    return devicesList.filter((d) => {
      if (!q) return true
      return (
        d.name.toLowerCase().includes(q) ||
        d.ip.includes(q) ||
        d.mac.toLowerCase().includes(q)
      )
    })
  }, [devicesList, deviceSearch])

  // Фильтрация и пагинация базового каталога (376)
  const filteredCatalog = useMemo(() => {
    let list = catalogItems
    const filterKey = CATEGORY_MAP[selectedCategory]?.filterKey || 'all'

    if (filterKey !== 'all') {
      list = list.filter((item) => item.category.toLowerCase().includes(filterKey))
    }

    const q = catalogSearch.trim().toLowerCase()
    if (q) {
      list = list.filter(
        (item) =>
          item.domain.toLowerCase().includes(q) ||
          (item.description && item.description.toLowerCase().includes(q))
      )
    }

    return list
  }, [catalogItems, selectedCategory, catalogSearch])

  const totalPages = Math.ceil(filteredCatalog.length / pageSize) || 1
  const paginatedCatalog = useMemo(() => {
    const start = (catalogPage - 1) * pageSize
    return filteredCatalog.slice(start, start + pageSize)
  }, [filteredCatalog, catalogPage])

  const isDeviceExcluded = (d: DeviceInfo) => {
    const exList = config.excluded_devices || []
    return exList.some(
      (item) => item.toLowerCase() === d.mac.toLowerCase() || (d.ip && item === d.ip)
    )
  }

  const excludedCount = useMemo(() => {
    return (config.excluded_devices || []).length
  }, [config.excluded_devices])

  if (loading) {
    return (
      <section className="card" data-testid="whitelist-loading">
        <h2>🛡️ Режим белых списков (Whitelist Mode)</h2>
        <p className="muted">Загрузка конфигурации и каталога сервисов…</p>
        <div className="skeleton-shimmer" style={{ width: '100%', height: 160, borderRadius: 12, marginTop: 16 }} />
      </section>
    )
  }

  return (
    <section className="whitelist-page" data-testid="whitelist-component">
      {/* 1. Hero Card: Master Switch & Status */}
      <div className="whitelist-hero-card">
        <div className="whitelist-hero-header">
          <div className="whitelist-hero-title-group">
            <h2 style={{ margin: 0, fontSize: 20, display: 'flex', alignItems: 'center', gap: 8 }}>
              <span>🛡️</span>
              <span>Режим белых списков (Whitelist)</span>
            </h2>
            <span
              className={`whitelist-badge ${config.enabled ? 'active' : 'inactive'}`}
              data-testid="whitelist-status-badge"
            >
              {config.enabled ? '🟢 АКТИВЕН' : '⚪ ОТКЛЮЧЕН'}
            </span>
            {config.gaming_suspended && (
              <span
                className="badge badge-warning"
                data-testid="gaming-suspended-badge"
                style={{
                  fontSize: 11,
                  padding: '3px 8px',
                  borderRadius: 6,
                  background: 'rgba(234, 179, 8, 0.15)',
                  color: '#eab308',
                  border: '1px solid rgba(234, 179, 8, 0.3)',
                }}
                title="Игровой режим временно приостановлен для предотвращения конфликтов маршрутизации"
              >
                🎮 Игровой режим на паузе
              </span>
            )}
          </div>

          <button
            type="button"
            className={`whitelist-master-toggle-btn ${config.enabled ? 'btn-active' : 'btn-inactive'}`}
            data-testid="whitelist-master-toggle"
            disabled={busy}
            onClick={handleToggleMaster}
          >
            {config.enabled ? '⏹ Выключить режим белых списков' : '▶ Включить режим белых списков'}
          </button>
        </div>

        <p className="muted" style={{ fontSize: 13, lineHeight: 1.5, margin: 0 }}>
          Автоматический режим для работы через сотовых операторов и провайдеров в условиях мобильных блокировок. 
          Все устройства домашней сети автоматически направляются в защищенный туннель, исключая сбои интернета. 
          При этом <strong>376 сервисов РФ</strong> (Госуслуги, Банки, СБП, Маркетплейсы, Медиа) 
          и ваши персональные домены идут строго <strong>напрямую (DIRECT)</strong> через локальный DNS без ошибок VPN.
        </p>

        {activeServer && (
          <div style={{ marginTop: 10, fontSize: 12, color: '#94a3b8', display: 'flex', alignItems: 'center', gap: 6 }}>
            <span>Активный сервер:</span>
            <strong style={{ color: isBsServer ? '#34d399' : '#f1f5f9' }}>{activeServer}</strong>
            {isBsServer && (
              <span style={{ color: '#34d399', background: 'rgba(52, 211, 153, 0.1)', padding: '1px 6px', borderRadius: 4, fontSize: 11 }}>
                🛡️ Нода белых списков [БС]
              </span>
            )}
          </div>
        )}

        <div className="whitelist-kpis">
          <div className="whitelist-kpi-card">
            <div className="whitelist-kpi-value" data-testid="kpi-active-devices">
              {excludedCount > 0 ? `Все (искл: ${excludedCount})` : 'Все (100%)'}
            </div>
            <div className="whitelist-kpi-label">Охват сети (Zero-Touch)</div>
          </div>
          <div className="whitelist-kpi-card">
            <div className="whitelist-kpi-value" data-testid="kpi-strategy" style={{ fontSize: 15 }}>
              {isStrictStrategy ? '🛡️ XKeen Route' : '🌐 Стандартная'}
            </div>
            <div className="whitelist-kpi-label">Логика маршрутизации</div>
          </div>
          <div className="whitelist-kpi-card">
            <div className="whitelist-kpi-value" data-testid="kpi-catalog-count">{catalogTotal}</div>
            <div className="whitelist-kpi-label">Доменов в каталоге РФ</div>
          </div>
          <div className="whitelist-kpi-card">
            <div className="whitelist-kpi-value" data-testid="kpi-custom-count">{config.custom_domains.length}</div>
            <div className="whitelist-kpi-label">Моих доменов (DIRECT)</div>
          </div>
        </div>
      </div>

      {/* 2. Strategy Selector Card: XKeen Route vs Standard */}
      <div className="card" style={{ marginBottom: 20 }}>
        <h3 style={{ margin: '0 0 6px', fontSize: 16 }}>⚙️ Логика работы режима белых списков</h3>
        <p className="muted small" style={{ marginBottom: 14 }}>
          Определяет, как именно маршрутизируется трафик клиентов при активном режиме белых списков.
        </p>

        <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
          <label
            style={{
              display: 'flex',
              alignItems: 'flex-start',
              gap: 12,
              padding: '12px 16px',
              borderRadius: 8,
              background: isStrictStrategy ? 'rgba(16, 185, 129, 0.12)' : 'rgba(255,255,255,0.03)',
              border: `1px solid ${isStrictStrategy ? '#10b981' : 'rgba(255,255,255,0.1)'}`,
              cursor: 'pointer',
            }}
          >
            <input
              type="radio"
              name="whitelist-strategy"
              checked={isStrictStrategy}
              disabled={busy}
              onChange={() => handleStrategyChange('strict_x_keen')}
              data-testid="radio-strategy-strict"
              style={{ marginTop: 3 }}
            />
            <div>
              <div style={{ fontWeight: 600, color: config.strategy === 'strict_xkeen' ? '#34d399' : '#f1f5f9', fontSize: 14 }}>
                🛡️ Специальная логика XKeen Route (Рекомендуется)
              </div>
              <div className="muted small" style={{ marginTop: 4, lineHeight: 1.5 }}>
                • <strong>Весь трафик сети</strong> направляется в защищенный туннель [БС], предотвращая сбои интернета.<br />
                • <strong>376 отечественных сервисов</strong> (Госуслуги, Банки, Ozon, WB) идут напрямую (DIRECT) без замедления и ошибок VPN.<br />
                • <strong>Игровой режим временно приостанавливается</strong> для исключения конфликтов правил и утечек сессий.
              </div>
            </div>
          </label>

          <label
            style={{
              display: 'flex',
              alignItems: 'flex-start',
              gap: 12,
              padding: '12px 16px',
              borderRadius: 8,
              background: config.strategy === 'standard' ? 'rgba(56, 189, 248, 0.12)' : 'rgba(255,255,255,0.03)',
              border: `1px solid ${config.strategy === 'standard' ? '#38bdf8' : 'rgba(255,255,255,0.1)'}`,
              cursor: 'pointer',
            }}
          >
            <input
              type="radio"
              name="whitelist-strategy"
              checked={config.strategy === 'standard'}
              disabled={busy}
              onChange={() => handleStrategyChange('standard')}
              data-testid="radio-strategy-standard"
              style={{ marginTop: 3 }}
            />
            <div>
              <div style={{ fontWeight: 600, color: config.strategy === 'standard' ? '#38bdf8' : '#f1f5f9', fontSize: 14 }}>
                🌐 Стандартная логика (как у обычного сервера)
              </div>
              <div className="muted small" style={{ marginTop: 4, lineHeight: 1.5 }}>
                Используются обычные правила провайдеров и подписок без тотального перехвата локальных подсетей.
              </div>
            </div>
          </label>
        </div>
      </div>

      {/* 3. Failover Auto-Activation Card */}
      <div className="card" style={{ marginBottom: 20 }}>
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', flexWrap: 'wrap', gap: 12 }}>
          <div>
            <h3 style={{ margin: 0, fontSize: 16 }}>🔄 Автоматическое включение при Failover</h3>
            <p className="muted small" style={{ margin: '4px 0 0', maxWidth: 640 }}>
              Когда основной сервер (Германия, Нидерланды) падает и модуль Резервирования переключается на ноду 
              белых списков (помеченную [БС] или из провайдера БС) — режим белых списков включается автоматически. 
              При возврате на обычный сервер режим отключается, а игровой режим восстанавливается.
            </p>
          </div>

          <label
            style={{
              display: 'inline-flex',
              alignItems: 'center',
              gap: 8,
              cursor: 'pointer',
              userSelect: 'none',
              background: config.auto_by_failover ? 'rgba(16, 185, 129, 0.15)' : 'rgba(255,255,255,0.05)',
              border: `1px solid ${config.auto_by_failover ? '#10b981' : 'rgba(255,255,255,0.15)'}`,
              borderRadius: 20,
              padding: '6px 14px',
            }}
          >
            <input
              type="checkbox"
              checked={Boolean(config.auto_by_failover)}
              disabled={busy}
              onChange={handleAutoFailoverToggle}
              data-testid="auto-failover-toggle"
            />
            <span style={{ fontWeight: 600, fontSize: 13, color: config.auto_by_failover ? '#34d399' : '#94a3b8' }}>
              {config.auto_by_failover ? 'Авто: ВКЛ 🟢' : 'Авто: ВЫКЛ ⚪'}
            </span>
          </label>
        </div>
      </div>

      {/* 4. Zero-Touch Device Coverage & Exclusions */}
      <div className="card" style={{ marginBottom: 20 }}>
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', flexWrap: 'wrap', gap: 12, marginBottom: 12 }}>
          <div>
            <h3 style={{ margin: 0, fontSize: 16, display: 'flex', alignItems: 'center', gap: 8 }}>
              <span>📱</span>
              <span>Охват устройств (Zero-Touch)</span>
            </h3>
            <p className="muted small" style={{ margin: '4px 0 0' }}>
              Все устройства домашней сети защищены автоматически. Если какому-то устройству НЕ нужен режим БС, 
              исключите его кнопкой ниже (оно будет направляться напрямую мимо туннеля).
            </p>
          </div>

          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <input
              type="text"
              className="input"
              placeholder="🔍 Поиск устройства..."
              style={{ width: 220, height: 32, fontSize: 12 }}
              value={deviceSearch}
              onChange={(e) => setDeviceSearch(e.target.value)}
              data-testid="device-search-input"
            />
          </div>
        </div>

        <div className="devices-table-wrap" style={{ maxHeight: 380, overflowY: 'auto' }}>
          <table className="devices-table">
            <thead>
              <tr>
                <th style={{ width: 40 }}>Статус</th>
                <th>Устройство</th>
                <th style={{ width: 130 }}>IP-адрес</th>
                <th style={{ width: 150 }}>MAC-адрес</th>
                <th style={{ width: 180, textAlign: 'center' }}>Маршрутизация в БС</th>
                <th style={{ width: 200, textAlign: 'center' }}>📌 Фиксированный узел</th>
              </tr>
            </thead>
            <tbody>
              {filteredDevices.length === 0 ? (
                <tr>
                  <td colSpan={6} className="muted small" style={{ textAlign: 'center', padding: 20 }}>
                    Устройства не найдены
                  </td>
                </tr>
              ) : (
                filteredDevices.map((d) => {
                  const excluded = isDeviceExcluded(d)
                  const pinned = config.devices.find(
                    (pd) => pd.enabled && (pd.ip === d.ip || pd.mac.toLowerCase() === d.mac.toLowerCase())
                  )
                  const pinnedServer = pinned?.server || ''
                  return (
                    <tr key={d.mac} className="device-row">
                      <td style={{ textAlign: 'center' }}>
                        <span
                          className={`devices-pill-dot ${d.online || d.is_current_device ? 'online' : 'offline'}`}
                          title={d.online || d.is_current_device ? 'В сети' : 'Не в сети'}
                        />
                      </td>
                      <td>
                        <div style={{ fontWeight: 600, color: '#f1f5f9' }}>{d.name}</div>
                        {d.is_current_device && (
                          <span className="badge badge-accent" style={{ fontSize: 9 }}>Текущее</span>
                        )}
                      </td>
                      <td style={{ fontFamily: 'Consolas, monospace', fontSize: 12 }}>{d.ip || '—'}</td>
                      <td style={{ fontFamily: 'Consolas, monospace', fontSize: 11, color: '#94a3b8' }}>{d.mac}</td>
                      <td style={{ textAlign: 'center' }}>
                        {excluded ? (
                          <button
                            type="button"
                            className="btn btn-xs btn-secondary"
                            data-testid={`btn-include-${d.mac}`}
                            disabled={busy}
                            onClick={() => handleToggleExclude(d.mac, true)}
                            title="Вернуть устройство в режим белых списков"
                            style={{ minWidth: 120 }}
                          >
                            <span>⚪ Исключено (DIRECT)</span>
                          </button>
                        ) : (
                          <button
                            type="button"
                            className="btn btn-xs btn-primary"
                            data-testid={`btn-exclude-${d.mac}`}
                            disabled={busy}
                            onClick={() => handleToggleExclude(d.mac, false)}
                            title="Исключить устройство из режима белых списков"
                            style={{ minWidth: 120, background: 'rgba(16, 185, 129, 0.2)', border: '1px solid #10b981', color: '#34d399' }}
                          >
                            <span>🛡️ В БС (Защищено)</span>
                          </button>
                        )}
                      </td>
                      <td style={{ textAlign: 'center' }}>
                        {excluded ? (
                          <span className="muted small">—</span>
                        ) : (
                          <select
                            data-testid={`pin-server-${d.mac}`}
                            value={pinnedServer}
                            disabled={busy}
                            onChange={(e) => handlePinServer(d, e.target.value)}
                            title="Закрепить устройство за стабильным узлом: Failover не будет его переключать"
                            style={{
                              width: '100%',
                              maxWidth: 190,
                              fontSize: 11,
                              padding: '4px 6px',
                              borderRadius: 6,
                              border: '1px solid rgba(148,163,184,0.35)',
                              background: pinnedServer ? 'rgba(59,130,246,0.15)' : 'rgba(15,23,42,0.6)',
                              color: '#e2e8f0',
                            }}
                          >
                            <option value="">↔ Авто (сервер БС)</option>
                            {serversList.map((s) => (
                              <option key={s.id} value={s.id}>
                                {s.name}
                              </option>
                            ))}
                          </select>
                        )}
                      </td>
                    </tr>
                  )
                })
              )}
            </tbody>
          </table>
        </div>
      </div>

      {/* 5. Domain Manager: Built-in 376 Catalog & Custom Domains */}
      <div className="card">
        <div style={{ display: 'flex', alignItems: 'center', gap: 12, borderBottom: '1px solid #1e2638', paddingBottom: 12, marginBottom: 14 }}>
          <button
            type="button"
            className={`btn btn-sm ${domainTab === 'catalog' ? 'btn-primary' : 'btn-secondary'}`}
            onClick={() => {
              setDomainTab('catalog')
              setCatalogPage(1)
            }}
            data-testid="tab-catalog"
          >
            🇷🇺 Базовый каталог РФ ({catalogTotal})
          </button>
          <button
            type="button"
            className={`btn btn-sm ${domainTab === 'custom' ? 'btn-primary' : 'btn-secondary'}`}
            onClick={() => setDomainTab('custom')}
            data-testid="tab-custom"
          >
            ➕ Мои домены ({config.custom_domains.length})
          </button>
        </div>

        {domainTab === 'catalog' ? (
          <div>
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', flexWrap: 'wrap', gap: 10, marginBottom: 12 }}>
              <input
                type="text"
                className="input"
                placeholder="🔍 Поиск домена (gosuslugi, sber, ozon...)"
                style={{ width: 280, height: 34 }}
                value={catalogSearch}
                onChange={(e) => {
                  setCatalogSearch(e.target.value)
                  setCatalogPage(1)
                }}
                data-testid="catalog-search-input"
              />

              <div className="muted small">
                Показано: {filteredCatalog.length} из {catalogTotal} сервисов
              </div>
            </div>

            {/* Category Filter Pills */}
            <div className="whitelist-pills">
              {Object.entries(CATEGORY_MAP).map(([key, info]) => (
                <button
                  key={key}
                  type="button"
                  className={`whitelist-pill ${selectedCategory === key ? 'active' : ''}`}
                  onClick={() => {
                    setSelectedCategory(key)
                    setCatalogPage(1)
                  }}
                  data-testid={`category-pill-${key}`}
                >
                  {info.label}
                </button>
              ))}
            </div>

            {/* Catalog Grid */}
            <div className="whitelist-catalog-grid" data-testid="catalog-grid">
              {paginatedCatalog.length === 0 ? (
                <div className="muted small" style={{ textAlign: 'center', gridColumn: '1 / -1', padding: '30px 0' }}>
                  {catalogSearch ? `Сервисы по запросу «${catalogSearch}» не найдены` : 'Сервисы не найдены'}
                </div>
              ) : (
                paginatedCatalog.map((item) => (
                  <div key={item.domain} className="whitelist-domain-card" data-testid={`catalog-card-${item.domain}`}>
                    <div>
                      <div className="whitelist-domain-name">{item.domain}</div>
                      {item.description && (
                        <div className="muted small" style={{ fontSize: 11, marginTop: 2 }}>{item.description}</div>
                      )}
                    </div>
                    <div className="whitelist-domain-meta">
                      <span className="whitelist-tag-direct">DIRECT</span>
                    </div>
                  </div>
                ))
              )}
            </div>

            {/* Pagination Controls */}
            {totalPages > 1 && (
              <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', gap: 12, marginTop: 16 }}>
                <button
                  type="button"
                  className="btn btn-xs btn-secondary"
                  disabled={catalogPage <= 1}
                  onClick={() => setCatalogPage((p) => Math.max(1, p - 1))}
                  data-testid="catalog-prev-page"
                >
                  ← Назад
                </button>
                <span className="muted small">
                  Страница <strong>{catalogPage}</strong> из <strong>{totalPages}</strong>
                </span>
                <button
                  type="button"
                  className="btn btn-xs btn-secondary"
                  disabled={catalogPage >= totalPages}
                  onClick={() => setCatalogPage((p) => Math.min(totalPages, p + 1))}
                  data-testid="catalog-next-page"
                >
                  Вперед →
                </button>
              </div>
            )}
          </div>
        ) : (
          /* Custom Domains Manager */
          <div data-testid="custom-domains-manager">
            <p className="muted small" style={{ marginBottom: 14 }}>
              Добавьте доменные имена для прямого доступа (DIRECT) в обход прокси. 
              Все поддомены добавленного домена также будут направляться напрямую.
            </p>

            <form onSubmit={handleAddCustomDomain} style={{ display: 'flex', gap: 10, maxWidth: 520, marginBottom: 16 }}>
              <input
                type="text"
                className="input"
                placeholder="Например: my-corporate-portal.ru"
                style={{ flex: 1 }}
                value={newDomainInput}
                disabled={busy}
                onChange={(e) => setNewDomainInput(e.target.value)}
                data-testid="custom-domain-input"
              />
              <button
                type="submit"
                className="btn btn-primary"
                disabled={busy || !newDomainInput.trim()}
                data-testid="btn-add-custom-domain"
              >
                ➕ Добавить
              </button>
            </form>

            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8, minHeight: 60 }}>
              {config.custom_domains.length === 0 ? (
                <div className="muted small" style={{ fontStyle: 'italic', padding: '10px 0' }}>
                  Пользовательские домены пока не добавлены.
                </div>
              ) : (
                config.custom_domains.map((d) => (
                  <span key={d} className="whitelist-chip" data-testid={`custom-chip-${d}`}>
                    <span>{d}</span>
                    <button
                      type="button"
                      className="whitelist-chip-remove"
                      disabled={busy}
                      onClick={() => handleRemoveCustomDomain(d)}
                      title={`Удалить ${d} из белого списка`}
                      data-testid={`btn-remove-custom-${d}`}
                    >
                      ✕
                    </button>
                  </span>
                ))
              )}
            </div>
          </div>
        )}
      </div>
    </section>
  )
}
