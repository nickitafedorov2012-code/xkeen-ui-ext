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
    target_server: null,
    follow_failover: true,
    devices: [],
    custom_domains: [],
  })
  const [activeServer, setActiveServer] = useState<string>('Follow Failover')
  const [devicesList, setDevicesList] = useState<DeviceInfo[]>([])
  const [serversList, setServersList] = useState<ServerInfo[]>([])
  const [catalogItems, setCatalogItems] = useState<DomesticCatalogEntryView[]>([])
  const [catalogTotal, setCatalogTotal] = useState<number>(376)
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState(false)

  // Поиск и фильтрация устройств
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
          target_server: wlStatus.target_server || null,
          follow_failover: wlStatus.follow_failover ?? true,
          devices: wlStatus.devices || [],
          custom_domains: wlStatus.custom_domains || [],
        })
        if (wlStatus.active_server) {
          setActiveServer(wlStatus.active_server)
        }
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
      }

      if (catRes) {
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

  // Переключение выбора сервера
  const handleServerModeChange = async (followFailover: boolean, targetServerId?: string | null) => {
    setBusy(true)
    try {
      const payload: Partial<WhitelistConfig> = {
        follow_failover: followFailover,
        target_server: followFailover ? 'follow_failover' : (targetServerId || null),
      }
      await apiPost('whitelist', payload)
      setConfig((prev) => ({
        ...prev,
        follow_failover: followFailover,
        target_server: followFailover ? null : (targetServerId || null),
      }))
      notify(
        followFailover
          ? '🔄 Включен динамический выбор: следование за Failover'
          : `🎯 Назначен фиксированный прокси-сервер: ${targetServerId || 'Основной'}`
      )
      window.dispatchEvent(new CustomEvent('xr:refresh-status'))
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка смены сервера белых списков', true)
      await loadData()
    } finally {
      setBusy(false)
    }
  }

  // Переключение одного устройства
  const handleToggleDevice = async (ip: string, mac: string, enabled: boolean) => {
    setBusy(true)
    const macLower = mac.toLowerCase()
    try {
      await apiPost('whitelist/device/toggle', {
        ip,
        mac,
        enabled,
      })
      setConfig((prev) => {
        const nextDevices = [...prev.devices]
        const idx = nextDevices.findIndex((d) => d.mac.toLowerCase() === macLower || (ip && d.ip === ip))
        if (idx >= 0) {
          nextDevices[idx] = { ...nextDevices[idx], enabled }
        } else if (enabled) {
          const foundInfo = devicesList.find((d) => d.mac.toLowerCase() === macLower)
          nextDevices.push({
            mac,
            ip,
            name: foundInfo?.name || '',
            enabled: true,
          })
        }
        return {
          ...prev,
          enabled: enabled ? true : prev.enabled,
          devices: nextDevices,
        }
      })
      notify(
        enabled
          ? `🛡️ Белые списки включены для ${ip || mac}`
          : `⚪ Белые списки выключены для ${ip || mac}`
      )
      window.dispatchEvent(new CustomEvent('xr:refresh-status'))
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка изменения белого списка для устройства', true)
      await loadData()
    } finally {
      setBusy(false)
    }
  }

  // Массовое включение/выключение устройств
  const handleBatchToggleAllOnline = async (enable: boolean) => {
    setBusy(true)
    try {
      const onlineDevs = devicesList.filter((d) => d.online || d.is_current_device)
      const currentDevicesMap = new Map(config.devices.map((d) => [d.mac.toLowerCase(), d]))

      for (const od of onlineDevs) {
        currentDevicesMap.set(od.mac.toLowerCase(), {
          mac: od.mac,
          ip: od.ip,
          name: od.name,
          enabled: enable,
        })
      }

      const updatedDevices = Array.from(currentDevicesMap.values())
      await apiPost('whitelist', {
        devices: updatedDevices,
        enabled: enable ? true : config.enabled,
      })
      setConfig((prev) => ({
        ...prev,
        enabled: enable ? true : prev.enabled,
        devices: updatedDevices,
      }))
      notify(
        enable
          ? `🛡️ Включен режим белых списков для всех онлайн-устройств (${onlineDevs.length})`
          : '⚪ Отключен режим белых списков для всех устройств'
      )
      window.dispatchEvent(new CustomEvent('xr:refresh-status'))
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка массового переключения', true)
      await loadData()
    } finally {
      setBusy(false)
    }
  }

  // Добавление пользовательского домена
  const handleAddCustomDomain = async (e: React.FormEvent) => {
    e.preventDefault()
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
      notify(`✅ Домен "${clean}" добавлен в белый список`)
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
      notify(`🗑️ Домен "${domain}" удален из белого списка`)
      window.dispatchEvent(new CustomEvent('xr:refresh-status'))
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка удаления домена', true)
    } finally {
      setBusy(false)
    }
  }

  // Фильтрация устройств в таблице
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

  const activeDevicesCount = useMemo(() => {
    return config.devices.filter((d) => d.enabled).length
  }, [config.devices])

  const isDeviceActiveInWl = (d: DeviceInfo) => {
    const match = config.devices.find(
      (wd) => wd.mac.toLowerCase() === d.mac.toLowerCase() || (d.ip && wd.ip === d.ip)
    )
    return Boolean(match?.enabled)
  }

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
          Все исходящие соединения выбранных устройств направляются в защищенный туннель, исключая утечки и блокировки. 
          При этом <strong>376 базовых отечественных сервисов</strong> (Госуслуги, Банки, СБП, Маркетплейсы, Медиа) 
          и ваши персональные домены направляются строго <strong>напрямую (DIRECT)</strong> через локальный DNS без GeoIP-ошибок.
        </p>

        <div className="whitelist-kpis">
          <div className="whitelist-kpi-card">
            <div className="whitelist-kpi-value" data-testid="kpi-active-devices">{activeDevicesCount}</div>
            <div className="whitelist-kpi-label">Активных устройств</div>
          </div>
          <div className="whitelist-kpi-card">
            <div className="whitelist-kpi-value" style={{ fontSize: 14, overflow: 'hidden', textOverflow: 'ellipsis' }}>
              {config.follow_failover ? '🔄 Failover Auto' : (config.target_server || 'Default')}
            </div>
            <div className="whitelist-kpi-label">Целевой узел туннеля</div>
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

      {/* 2. Target Server Selector (Failover Awareness) */}
      <div className="card" style={{ marginBottom: 20 }}>
        <h3 style={{ margin: '0 0 10px', fontSize: 16 }}>🎯 Настройка прокси-туннеля для режима</h3>
        <p className="muted small" style={{ marginBottom: 14 }}>
          Определяет, через какой прокси-сервер направлять не-российский трафик выбранных клиентов.
        </p>

        <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
          <label
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: 10,
              padding: '10px 14px',
              borderRadius: 8,
              background: config.follow_failover ? 'rgba(16, 185, 129, 0.12)' : 'rgba(255,255,255,0.03)',
              border: `1px solid ${config.follow_failover ? '#10b981' : 'rgba(255,255,255,0.1)'}`,
              cursor: 'pointer',
            }}
          >
            <input
              type="radio"
              name="server-mode"
              checked={config.follow_failover}
              disabled={busy}
              onChange={() => handleServerModeChange(true)}
              data-testid="radio-follow-failover"
            />
            <div>
              <div style={{ fontWeight: 600, color: config.follow_failover ? '#34d399' : '#f1f5f9' }}>
                🔄 Следовать за Failover (Рекомендуется)
              </div>
              <div className="muted small">
                Автоматически переключает туннель на активный сервер из модуля Резервирования (Failover). 
                Приоритет отдается серверам с тегом [БС].
                {activeServer && <span style={{ marginLeft: 6, color: '#94a3b8' }}>[Текущий узел: {activeServer}]</span>}
              </div>
            </div>
          </label>

          <label
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: 10,
              padding: '10px 14px',
              borderRadius: 8,
              background: !config.follow_failover ? 'rgba(56, 189, 248, 0.12)' : 'rgba(255,255,255,0.03)',
              border: `1px solid ${!config.follow_failover ? '#38bdf8' : 'rgba(255,255,255,0.1)'}`,
              cursor: 'pointer',
            }}
          >
            <input
              type="radio"
              name="server-mode"
              checked={!config.follow_failover}
              disabled={busy}
              onChange={() => {
                const firstSrv = serversList[0]?.id || null
                handleServerModeChange(false, config.target_server || firstSrv)
              }}
              data-testid="radio-fixed-server"
            />
            <div style={{ flex: 1 }}>
              <div style={{ fontWeight: 600, color: !config.follow_failover ? '#38bdf8' : '#f1f5f9' }}>
                🎯 Фиксированный сервер
              </div>
              <div className="muted small" style={{ marginBottom: 6 }}>
                Весь трафик устройств будет направляться строго через выбранный узел.
              </div>

              {!config.follow_failover && (
                <div style={{ marginTop: 8 }} onClick={(e) => e.stopPropagation()}>
                  <select
                    className="input"
                    style={{ maxWidth: 380, width: '100%' }}
                    value={config.target_server || ''}
                    disabled={busy}
                    data-testid="target-server-select"
                    onChange={(e) => handleServerModeChange(false, e.target.value)}
                  >
                    <option value="">— Выберите сервер —</option>
                    {serversList.map((s) => (
                      <option key={s.id} value={s.id}>
                        {s.name} {s.ping_ms > 0 ? `(${s.ping_ms} ms)` : ''} {s.is_whitelist ? '🛡️ [БС]' : ''}
                      </option>
                    ))}
                  </select>
                </div>
              )}
            </div>
          </label>
        </div>
      </div>

      {/* 3. Per-Device Whitelist Table */}
      <div className="card" style={{ marginBottom: 20 }}>
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', flexWrap: 'wrap', gap: 12, marginBottom: 12 }}>
          <div>
            <h3 style={{ margin: 0, fontSize: 16 }}>📱 Устройства в режиме белых списков</h3>
            <p className="muted small" style={{ margin: '4px 0 0' }}>
              Включите режим для необходимых клиентов в один клик.
            </p>
          </div>

          <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
            <input
              type="text"
              className="input"
              placeholder="🔍 Поиск по имени, IP, MAC..."
              style={{ width: 220, height: 32, fontSize: 12 }}
              value={deviceSearch}
              onChange={(e) => setDeviceSearch(e.target.value)}
              data-testid="device-search-input"
            />
            <button
              type="button"
              className="btn btn-xs btn-primary"
              disabled={busy}
              onClick={() => handleBatchToggleAllOnline(true)}
              data-testid="btn-enable-all-online"
            >
              Включить все онлайн
            </button>
            <button
              type="button"
              className="btn btn-xs btn-secondary"
              disabled={busy}
              onClick={() => handleBatchToggleAllOnline(false)}
              data-testid="btn-disable-all"
            >
              Отключить все
            </button>
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
                <th style={{ width: 120, textAlign: 'center' }}>Режим БС</th>
              </tr>
            </thead>
            <tbody>
              {filteredDevices.length === 0 ? (
                <tr>
                  <td colSpan={5} className="muted small" style={{ textAlign: 'center', padding: 20 }}>
                    Устройства не найдены
                  </td>
                </tr>
              ) : (
                filteredDevices.map((d) => {
                  const active = isDeviceActiveInWl(d)
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
                        <button
                          type="button"
                          className={`device-whitelist-btn ${active ? 'active' : 'inactive'}`}
                          data-testid={`whitelist-device-btn-${d.mac}`}
                          disabled={busy}
                          onClick={() => handleToggleDevice(d.ip, d.mac, !active)}
                          title={active ? 'Отключить режим белых списков' : 'Включить режим белых списков'}
                        >
                          <span className="btn-icon device-whitelist-icon">🛡️</span>
                          <span className="btn-label device-whitelist-label">{active ? 'БС ON' : 'БС OFF'}</span>
                        </button>
                      </td>
                    </tr>
                  )
                })
              )}
            </tbody>
          </table>
        </div>
      </div>

      {/* 4. Domain Manager: Built-in 376 Catalog & Custom Domains */}
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
