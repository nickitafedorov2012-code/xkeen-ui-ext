import { useCallback, useEffect, useRef, useState } from 'react'
import { apiGet, apiPost, apiPut, runWhitelistDetectorCheck, detectWhitelistServers, applyDetectedWhitelistServers } from '../api'
import { pingClass, type FailoverEventInfo, type StatusInfo, type AghStatus, type SwapStatus, type DomainCheckResult, type ProviderInfo, type ServerInfo } from '../types'
import { formatQuotaBytes } from './Servers'
import TrafficGraph from './TrafficGraph'

interface Props {
  status: StatusInfo | null
  notify: (msg: string, isError?: boolean) => void
  refresh?: () => void
  onSwitchTab?: (tab: 'dashboard' | 'servers' | 'devices' | 'settings' | 'google-ai') => void
}

export function SwapRecommendationBanner({
  status,
  onSwitchTab,
}: {
  status: SwapStatus | null
  onSwitchTab?: (tab: 'dashboard' | 'servers' | 'devices' | 'settings' | 'google-ai') => void
}) {
  if (!status?.recommend_swap || status?.swap_active) {
    return null
  }

  return (
    <div
      className="notice-banner"
      style={{
        background: 'rgba(234, 179, 8, 0.1)',
        border: '1px solid #eab308',
        padding: 12,
        borderRadius: 8,
        marginBottom: 16,
        display: 'flex',
        justifyContent: 'space-between',
        alignItems: 'center',
        gap: 12,
        flexWrap: 'wrap',
      }}
      data-testid="swap-recommendation-banner"
    >
      <div>
        <b style={{ color: '#eab308' }}>⚠️ Мало оперативной памяти ({status.ram_free_mb} МБ свободно)</b>
        <p style={{ margin: '4px 0 0', fontSize: 12 }} className="muted">
          Для защиты служб Mihomo, Zapret и AdGuard Home от вылетов рекомендуется включить файл подкачки на USB.
        </p>
      </div>
      <button
        className="btn btn-warning"
        onClick={() => onSwitchTab?.('settings')}
        data-testid="swap-recommendation-btn"
      >
        ⚡ Включить Swap в 1 клик
      </button>
    </div>
  )
}

interface GroupedEvent {
  time: string
  message: string
  switched: boolean
  count: number
  type: 'ok' | 'warn' | 'err' | 'switch'
}

function groupEvents(rawEvents: FailoverEventInfo[]): GroupedEvent[] {
  if (!rawEvents || rawEvents.length === 0) return []

  const classify = (e: FailoverEventInfo): 'ok' | 'warn' | 'err' | 'switch' => {
    if (e.switched) return 'switch'
    const m = e.message.toLowerCase()
    if (
      m.includes('ошибка') ||
      m.includes('недоступен') ||
      m.includes('таймаут') ||
      m.includes('failed') ||
      m.includes('timeout')
    )
      return 'err'
    if (m.includes('превышен') || m.includes('высокий') || m.includes('задержк') || m.includes('резерв'))
      return 'warn'
    return 'ok'
  }

  const normMsg = (msg: string) => {
    return msg
      .replace(/пинг\s*\d+\s*мс/gi, '')
      .replace(/\(\d+\s*мс\)/gi, '')
      .replace(/\(в норме[^)]*\)/gi, '')
      .replace(/\s+/g, ' ')
      .trim()
  }

  const grouped: GroupedEvent[] = []
  for (const ev of rawEvents) {
    const t = classify(ev)
    const norm = normMsg(ev.message)
    const last = grouped[grouped.length - 1]
    if (last && normMsg(last.message) === norm && last.type === t) {
      last.count += 1
      last.time = ev.time
      last.message = ev.message
    } else {
      grouped.push({
        time: ev.time,
        message: ev.message,
        switched: ev.switched,
        count: 1,
        type: t,
      })
    }
  }
  return grouped
}

function formatBytesRu(bytes: number): string {
  if (!bytes || bytes <= 0) return '0 Б'
  if (bytes < 1024) return `${bytes} Б`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} КБ`
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} МБ`
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} ГБ`
}

export default function Dashboard({ status, notify, refresh, onSwitchTab }: Props) {
  const [events, setEvents] = useState<FailoverEventInfo[]>([])
  const [checking, setChecking] = useState(false)
  const [togglingFailover, setTogglingFailover] = useState(false)
  const [togglingSmartFailover, setTogglingSmartFailover] = useState(false)
  const [togglingDetector, setTogglingDetector] = useState(false)
  const [checkingDetector, setCheckingDetector] = useState(false)
  const [detectorCheckResult, setDetectorCheckResult] = useState<string | null>(null)
  const [detectorModalOpen, setDetectorModalOpen] = useState(false)
  const [detectedClassifier, setDetectedClassifier] = useState<{
    detectedServerIds: string[]
    detectedProviderIds: string[]
    details: Array<{ id: string; name: string; reason: string; type: 'server' | 'provider' }>
  } | null>(null)
  const [classifierApplying, setClassifierApplying] = useState(false)
  const [allServers, setAllServers] = useState<ServerInfo[]>([])
  const [providers, setProviders] = useState<ProviderInfo[]>([])

  useEffect(() => {
    const fetchProviders = () => {
      apiGet<{ providers: ProviderInfo[] }>('providers')
        .then((d) => setProviders(d.providers || []))
        .catch(() => {})
      apiGet<{ servers: ServerInfo[] }>('servers')
        .then((d) => setAllServers(d.servers || []))
        .catch(() => {})
    }
    fetchProviders()
    const interval = setInterval(fetchProviders, 15000)
    return () => clearInterval(interval)
  }, [])
  // История пинга активного сервера (для sparkline): {значение, было ли измерение}
  const [pingHistory, setPingHistory] = useState<{ ms: number; ok: boolean }[]>([])
  const lastActiveServerIdRef = useRef<string | undefined>(undefined)

  useEffect(() => {
    const sId = status?.active_server?.id || status?.active_server?.name
    if (!sId) {
      setPingHistory([])
      lastActiveServerIdRef.current = undefined
      return
    }

    const ping = status?.active_server?.ping_ms
    const isValidPing = typeof ping === 'number' && ping > 0 && ping < 9999

    if (lastActiveServerIdRef.current !== sId) {
      lastActiveServerIdRef.current = sId
      let stored: { ms: number; ok: boolean }[] = []
      try {
        const raw = sessionStorage.getItem(`xr_dash_pings_${sId}`)
        if (raw) stored = JSON.parse(raw)
      } catch {}

      if (isValidPing) {
        if (stored.length === 0 || stored[stored.length - 1].ms !== ping) {
          stored = [...stored, { ms: ping, ok: true }].slice(-40)
          try {
            sessionStorage.setItem(`xr_dash_pings_${sId}`, JSON.stringify(stored))
          } catch {}
        }
      }
      setPingHistory(stored)
    } else if (isValidPing) {
      setPingHistory((prev) => {
        if (prev.length > 0 && prev[prev.length - 1].ms === ping) {
          return prev
        }
        const next = [...prev, { ms: ping, ok: true }].slice(-40)
        try {
          sessionStorage.setItem(`xr_dash_pings_${sId}`, JSON.stringify(next))
        } catch {}
        return next
      })
    }
  }, [status?.active_server?.id, status?.active_server?.name, status?.active_server?.ping_ms])

  const [adblockEnabled, setAdblockEnabled] = useState<boolean>(() => !!status?.adblock_enabled)
  const [adguardStatus, setAdguardStatus] = useState<AghStatus | null>(null)
  const [adguardToggling, setAdguardToggling] = useState(false)

  const loadAdguardStatus = useCallback(async () => {
    try {
      const res = await apiGet<AghStatus>('/adguard/status')
      if (res) {
        setAdguardStatus(res)
        if (res.running) {
          setAdblockEnabled(res.protection_enabled)
        }
      }
    } catch {
      /* ignore */
    }
  }, [])

  useEffect(() => {
    loadAdguardStatus()
    const timer = setInterval(loadAdguardStatus, 8000)
    return () => clearInterval(timer)
  }, [loadAdguardStatus])

  // Слушаем синхронизацию с вкладки AdGuard
  useEffect(() => {
    const handleSync = (e: Event) => {
      const detail = (e as CustomEvent<{ enabled: boolean }>).detail
      if (detail && typeof detail.enabled === 'boolean') {
        setAdblockEnabled(detail.enabled)
        setAdguardStatus((prev) => (prev ? { ...prev, protection_enabled: detail.enabled } : null))
      }
    }
    window.addEventListener('xr:adguard-protection-changed', handleSync)
    return () => window.removeEventListener('xr:adguard-protection-changed', handleSync)
  }, [])

  const [swapStatus, setSwapStatus] = useState<SwapStatus | null>(null)

  useEffect(() => {
    apiGet<SwapStatus>('system/swap/status')
      .then(setSwapStatus)
      .catch((err) => console.warn('Не удалось получить статус Swap:', err))
  }, [])

  useEffect(() => {
    if (status?.adblock_enabled !== undefined) {
      setAdblockEnabled(status.adblock_enabled)
    }
  }, [status?.adblock_enabled])

  const isAghActive = adguardStatus?.running ? adguardStatus.protection_enabled : adblockEnabled

  const handleToggleAdblock = async () => {
    if (adguardToggling) return
    const nextState = !isAghActive
    setAdguardToggling(true)
    try {
      // 1. Переключаем защиту в AdGuard Home через REST API
      const aghRes = await apiPost<{ enabled: boolean }>('/adguard/protection', { enabled: nextState }).catch(() => null)
      // 2. Синхронизируем флаг adblock в Mihomo
      await apiPost<{ enabled: boolean }>('adblock/toggle', { enabled: nextState }).catch(() => null)

      const effectiveState = aghRes?.enabled !== undefined ? aghRes.enabled : nextState
      setAdblockEnabled(effectiveState)
      setAdguardStatus((prev) => (prev ? { ...prev, protection_enabled: effectiveState } : null))

      // Синхронизируем вкладку AdGuard через глобальное событие
      window.dispatchEvent(new CustomEvent('xr:adguard-protection-changed', { detail: { enabled: effectiveState } }))

      notify(effectiveState ? 'Защита AdGuard Home включена' : 'Защита AdGuard Home отключена')
      refresh?.()
    } catch (e: any) {
      notify('Ошибка переключения AdGuard: ' + (e.message || e), true)
    } finally {
      setAdguardToggling(false)
    }
  }

  const [domainQuery, setDomainQuery] = useState('')
  const [domainChecking, setDomainChecking] = useState(false)
  const [domainResult, setDomainResult] = useState<DomainCheckResult | null>(null)
  const [unblocking, setUnblocking] = useState(false)

  const handleCheckDomain = async (e?: React.FormEvent) => {
    if (e) e.preventDefault()
    const d = domainQuery.trim()
    if (!d) return
    setDomainChecking(true)
    setDomainResult(null)
    try {
      const res = await apiGet<DomainCheckResult>(`diagnostics/check-domain?domain=${encodeURIComponent(d)}`)
      setDomainResult(res)
    } catch (err: any) {
      notify(`Ошибка проверки домена: ${err?.message || err}`, true)
    } finally {
      setDomainChecking(false)
    }
  }

  const handleUnblockDomain = async (domain: string) => {
    setUnblocking(true)
    try {
      const res = await apiPost<{ message: string; rule: string }>('diagnostics/unblock-domain', { domain })
      notify(res?.message || `Домен ${domain} успешно разблокирован`)
      if (domainResult) {
        setDomainResult({ ...domainResult, is_blocked: false, block_reason: 'Разблокирован правилом исключения' })
      }
    } catch (err: any) {
      notify(`Ошибка разблокировки: ${err?.message || err}`, true)
    } finally {
      setUnblocking(false)
    }
  }

  const [devicesCount, setDevicesCount] = useState<number | null>(null)
  const [devicesTotal, setDevicesTotal] = useState<number | null>(null)

  useEffect(() => {
    const loadDevices = () => {
      apiGet<{ devices: any[] }>('devices')
        .then((d) => {
          if (d?.devices) {
            const online = d.devices.filter((dev) => dev.online || dev.is_current_device).length
            setDevicesCount(online)
            setDevicesTotal(d.devices.length)
          }
        })
        .catch(() => {})
    }
    loadDevices()
    const timer = setInterval(loadDevices, 10000)
    const onSync = () => loadDevices()
    window.addEventListener('xr:data-changed', onSync)
    window.addEventListener('xr:refresh-status', onSync)
    return () => {
      clearInterval(timer)
      window.removeEventListener('xr:data-changed', onSync)
      window.removeEventListener('xr:refresh-status', onSync)
    }
  }, [])

  const loadEvents = useCallback(async () => {
    try {
      const data = await apiGet<{ events: FailoverEventInfo[] }>('failover/events')
      setEvents(data.events)
    } catch {
      /* ignore */
    }
  }, [])

  useEffect(() => {
    loadEvents()
    const t = setInterval(loadEvents, 10000)
    return () => clearInterval(t)
  }, [loadEvents])

  const runCheck = async () => {
    setChecking(true)
    try {
      const data = await apiPost<{ message: string }>('failover/check')
      notify(data.message)
      loadEvents()
      refresh?.()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка проверки', true)
    } finally {
      setChecking(false)
    }
  }

  const toggleFailover = async (enabled: boolean) => {
    setTogglingFailover(true)
    try {
      await apiPut('settings', { failover: { enabled } })
      notify(enabled ? 'Failover включён' : 'Failover выключен')
      refresh?.()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка переключения failover', true)
    } finally {
      setTogglingFailover(false)
    }
  }

  const toggleSmartFailover = async (enabled: boolean) => {
    setTogglingSmartFailover(true)
    try {
      await apiPut('settings', { failover: { smart_failover_enabled: enabled } })
      notify(enabled ? 'Умный фейловер включён' : 'Умный фейловер выключен')
      refresh?.()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка переключения умного фейловера', true)
    } finally {
      setTogglingSmartFailover(false)
    }
  }

  const f = status?.failover
  const groupedEvents = groupEvents(events)

  const okPings = pingHistory.filter((p) => p.ok).map((p) => p.ms)
  const minPing = okPings.length > 0 ? okPings.reduce((a, b) => Math.min(a, b)) : 0
  const maxPing = okPings.length > 0 ? okPings.reduce((a, b) => Math.max(a, b)) : 0

  return (
    <div>
      {/* 1.1 Компактная статусная полоска роутера и ядра вместо двух статичных карточек */}
      <div className="dash-info-strip">
        <div className="dash-info-group">
          <span className="dash-info-badge" title="Информация об интернет-центре Keenetic">
            <span>🌐 Роутер:</span>
            <b>{status?.router?.model || 'Keenetic'}</b>
            <span className="dash-info-sep">·</span>
            <span>{status?.router?.version ? (status.router.version.startsWith('KeeneticOS') ? status.router.version : `KeeneticOS ${status.router.version}`) : 'KeeneticOS'}</span>
            <span className="dash-info-sep">·</span>
            <span className="muted">RCI {status?.rci ? `${status.rci.host}:${status.rci.port}` : '127.0.0.1:79'}</span>
          </span>
        </div>

        <div className="dash-info-group">
          <button
            type="button"
            className="dash-info-badge"
            onClick={handleToggleAdblock}
            disabled={adguardToggling}
            data-testid="dashboard-adguard-btn"
            title="Быстрое включение/отключение защиты AdGuard Home (синхронизировано с вкладкой AdGuard)"
            style={{
              cursor: 'pointer',
              background: isAghActive ? 'rgba(16, 185, 129, 0.15)' : 'rgba(255, 255, 255, 0.05)',
              border: isAghActive ? '1px solid #10b981' : '1px solid var(--border)',
            }}
          >
            <span>🛡️ AdGuard:</span>
            <b style={{ color: isAghActive ? '#10b981' : 'var(--text-secondary)' }}>
              {isAghActive ? 'ВКЛ 🟢' : 'ВЫКЛ ⚪'}
            </b>
            {adguardToggling && <span style={{ fontSize: 11 }}>⏳</span>}
          </button>
        </div>
      </div>

      {/* Умный баннер рекомендации Swap при нехватке ОЗУ */}
      <SwapRecommendationBanner status={swapStatus} onSwitchTab={onSwitchTab} />

      {/* График трафика в реальном времени */}
      <TrafficGraph notify={notify} />

      {/* Оперативные виджеты */}
      <div className="grid2">
        {/* КАРТОЧКА 1: Активный сервер */}
        <section className="card active-server-card" style={{ height: '100%', display: 'flex', flexDirection: 'column', justifyContent: 'space-between' }}>
          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 12 }}>
            <h2 style={{ margin: 0 }}>🛰 Активный сервер</h2>
            <span className="badge" style={{ color: '#38bdf8', borderColor: 'rgba(56, 189, 248, 0.3)', background: 'rgba(56, 189, 248, 0.1)' }}>
              🔵 В сети
            </span>
          </div>

          {status?.active_server ? (
            <div className="active-server-hero" style={{ display: 'flex', flexDirection: 'column', height: '100%', justifyContent: 'space-between' }}>
              <div>
                {/* Верхняя идентичность: флаг, имя, протокол, подписка */}
                <div className="active-server-top">
                  <div className="active-server-identity">
                    <div className="active-server-title">
                      <span className="active-server-name">{status.active_server.name}</span>
                      <div className="active-server-badges">
                        {status.active_server.protocol && (
                          <span className="badge" style={{ fontSize: 10, padding: '1px 6px' }}>
                            {status.active_server.protocol}
                          </span>
                        )}
                        {status.active_server.provider && (
                          <span className="tag-provider" style={{ cursor: 'default', fontSize: 11, padding: '1px 7px' }}>
                            📁 {status.active_server.provider_name || status.active_server.provider}
                          </span>
                        )}
                        {status?.whitelist_mode_active && (
                          <span className="badge" style={{ background: 'rgba(168, 85, 247, 0.25)', color: '#c084fc', border: '1px solid rgba(168, 85, 247, 0.5)', fontSize: 10, fontWeight: 700 }}>
                            🟣 ОБХОД БЕЛЫХ СПИСКОВ
                          </span>
                        )}
                      </div>
                    </div>
                  </div>

                  <div style={{ textAlign: 'right' }}>
                    <div className={'ping ' + pingClass(status.active_server.ping_ms)} style={{ fontSize: 15, fontWeight: 700 }}>
                      {status.active_server.ping_ms > 0 ? `${status.active_server.ping_ms} мс` : '—'}
                    </div>
                    <div className="muted small" style={{ fontSize: 11 }}>
                      {status.active_server.ping_ms > 0 && status.active_server.ping_ms <= 100
                        ? 'отличный отклик'
                        : status.active_server.ping_ms > 100 && status.active_server.ping_ms <= 250
                        ? 'хороший отклик'
                        : status.active_server.ping_ms > 250
                        ? 'высокая задержка'
                        : 'проверка…'}
                    </div>
                  </div>
                </div>

                {/* Таблица параметров в едином стиле с соседней карточкой */}
                <ul className="kv" style={{ margin: '4px 0 8px' }}>
                  <li>
                    <span>Хост и порт</span>
                    <b className="mono" style={{ fontSize: 12 }}>
                      {status.active_server.host ? `${status.active_server.host}${status.active_server.port ? `:${status.active_server.port}` : ''}` : '—'}
                    </b>
                  </li>
                  <li>
                    <span>Роль в Failover</span>
                    <b>
                      {f?.priority_server && f.priority_server === status.active_server.name ? (
                        <span style={{ color: '#38bdf8' }}>★ Основной (приоритетный)</span>
                      ) : f?.priority_chain && f.priority_chain.indexOf(status.active_server.id) === 0 ? (
                        <span style={{ color: '#38bdf8' }}>★ Основной (в цепочке)</span>
                      ) : f?.priority_chain && f.priority_chain.indexOf(status.active_server.id) > 0 ? (
                        <span style={{ color: '#eab308' }}>⚡ Резервный (РЕЗ{f.priority_chain.indexOf(status.active_server.id)})</span>
                      ) : (
                        <span style={{ color: 'var(--muted)' }}>Обычный сервер</span>
                      )}
                    </b>
                  </li>
                  <li>
                    <span>Режим трафика</span>
                    <b>Rule (авто-маршрутизация)</b>
                  </li>
                  <li>
                    <span>Mixed Port</span>
                    <b className="mono" style={{ fontSize: 12 }}>7890 (HTTP/SOCKS5)</b>
                  </li>
                  <li>
                    <span>Перехват / NAT</span>
                    <b>
                      <span style={{ color: '#38bdf8' }}>TProxy / Redir</span>
                      <span className="muted small" style={{ marginLeft: 6, fontWeight: 'normal' }}>(Full-Cone NAT)</span>
                    </b>
                  </li>
                  <li>
                    <span>Подключено устройств</span>
                    <b>{devicesCount !== null ? (devicesTotal !== null ? `${devicesCount} онлайн из ${devicesTotal}` : `${devicesCount} онлайн`) : '—'}</b>
                  </li>
                </ul>

                {/* График стабильности задержки (отображается при наличии хотя бы 2 замеров) */}
                {pingHistory.length >= 2 && (
                  <div className="active-server-sparkline-box">
                    <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 2 }}>
                      <span className="muted small" style={{ fontSize: 11 }}>
                        Стабильность пинга ({pingHistory.length} точек):
                      </span>
                      <span className="muted small" style={{ fontSize: 11, fontFamily: 'monospace' }}>
                        мин: {minPing} мс / макс: {maxPing} мс
                      </span>
                    </div>
                    <PingSparkline data={pingHistory} />
                  </div>
                )}
              </div>

              {/* Кнопки перехода и действий */}
              <div style={{ display: 'grid', gridTemplateColumns: onSwitchTab ? '1fr 1fr' : '1fr', gap: 10, marginTop: 8 }}>
                {onSwitchTab && (
                  <button
                    type="button"
                    className="btn ghost sm"
                    onClick={() => onSwitchTab('servers')}
                    style={{ justifyContent: 'center', width: '100%', padding: '7px 12px' }}
                    title="Выбрать другой сервер из списка"
                  >
                    🔌 Сменить сервер →
                  </button>
                )}
                <button
                  type="button"
                  className="btn ghost sm"
                  onClick={runCheck}
                  disabled={checking}
                  style={{ justifyContent: 'center', width: '100%', padding: '7px 12px' }}
                  title="Измерить текущую задержку"
                >
                  {checking ? '⏳ Проверка…' : '⚡ Проверить пинг'}
                </button>
              </div>
            </div>
          ) : (
            <p className="muted">Сервер не выбран или Mihomo не запущен.</p>
          )}
        </section>

        {/* КАРТОЧКА 2: Failover статус и управление */}
        <section className="card" style={{ height: '100%', display: 'flex', flexDirection: 'column', justifyContent: 'space-between' }}>
          <div>
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 12 }}>
              <h2 style={{ margin: 0 }}>⚡ Failover контроль</h2>
              {f?.active_tier === 2 ? (
                <span
                  className="badge badge-warning"
                  data-testid="failover-tier-badge"
                  style={{
                    background: 'rgba(234, 179, 8, 0.15)',
                    color: '#eab308',
                    border: '1px solid rgba(234, 179, 8, 0.3)',
                    fontWeight: 600,
                  }}
                  title="Активен резервный узел белых списков (Tier 2)"
                >
                  🛡️ Tier 2 (БС)
                </span>
              ) : (
                <span
                  className="badge badge-info"
                  data-testid="failover-tier-badge"
                  style={{
                    background: 'rgba(56, 189, 248, 0.15)',
                    color: '#38bdf8',
                    border: '1px solid rgba(56, 189, 248, 0.3)',
                    fontWeight: 600,
                  }}
                  title="Активен основной VPS-сервер (Tier 1)"
                >
                  ⚡ Tier 1 (VPS)
                </span>
              )}
            </div>
          <ul className="kv">
            <li>
              <span>Автоматический мониторинг</span>
              <div style={{ display: 'inline-flex', alignItems: 'center', gap: 10 }}>
                <label className="switch" title={f?.enabled ? 'Выключить failover' : 'Включить failover'}>
                  <input
                    type="checkbox"
                    checked={f?.enabled ?? false}
                    disabled={togglingFailover}
                    onChange={(e) => toggleFailover(e.target.checked)}
                  />
                  <span className="slider" />
                </label>
                <b style={{ color: f?.enabled ? '#22c55e' : 'var(--muted)', fontSize: 13 }}>
                  {togglingFailover ? 'Сохранение…' : f?.enabled ? 'Включён' : 'Выключен'}
                </b>
              </div>
            </li>
            <li>
              <span>🧠 Умный фейловер</span>
              <div style={{ display: 'inline-flex', alignItems: 'center', gap: 10 }}>
                <label className="switch" title="Приоритет VPS-серверов над белыми списками">
                  <input
                    type="checkbox"
                    data-testid="dashboard-smart-failover-toggle"
                    checked={f?.smart_failover_enabled ?? false}
                    disabled={togglingSmartFailover}
                    onChange={(e) => toggleSmartFailover(e.target.checked)}
                  />
                  <span className="slider" />
                </label>
                <b style={{ color: f?.smart_failover_enabled ? '#38bdf8' : 'var(--muted)', fontSize: 13 }}>
                  {togglingSmartFailover ? 'Сохранение…' : f?.smart_failover_enabled ? 'Включён' : 'Выключен'}
                </b>
              </div>
            </li>
            <li><span>Порог пинга</span><b>{f ? `${f.ping_threshold_ms} мс` : '—'}</b></li>
            <li>
              <span>Приоритетный</span>
              <b>
                {f?.priority_server || 'не задан'}
                {f?.priority_chain &&
                f.priority_chain.length > 0 &&
                status?.active_server?.id &&
                f.priority_chain.indexOf(status.active_server.id) > 0 ? (
                  <span className="muted small" style={{ marginLeft: 6, fontWeight: 'normal' }}>
                    (ожидает восстановления)
                  </span>
                ) : null}
              </b>
            </li>
            {(f?.priority_chain?.length ?? 0) > 0 &&
              status?.active_server?.id &&
              f!.priority_chain!.indexOf(status.active_server.id) >= 0 && (
                <li>
                  <span>Позиция в цепочке</span>
                  <b>
                    {f!.priority_chain!.indexOf(status.active_server.id) === 0 ? (
                      <span style={{ color: '#38bdf8' }}>ОСН (основной)</span>
                    ) : (
                      <span style={{ color: '#eab308' }}>
                        РЕЗ{f!.priority_chain!.indexOf(status.active_server.id)} (резервный)
                      </span>
                    )}
                  </b>
                </li>
              )}
            <li><span>Интервал проверки</span><b>{f ? `${f.interval_secs} с` : '—'}</b></li>
            {f?.canary?.active && (
              <li>
                <span>🧪 Тест узла</span>
                <b style={{ color: '#c084fc' }}>
                  {f.canary.target_name} ({f.canary.remaining_secs}с)
                </b>
              </li>
            )}
            {f?.last_test && (
              <li>
                <span>Последний тест</span>
                <b>
                  <span style={{ color: f.last_test.success ? '#4ade80' : '#f87171' }}>
                     {f.last_test.success ? '✓' : '✗'} {f.last_test.target_name}
                  </span>
                  <span className="muted small" style={{ marginLeft: 6, fontWeight: 'normal' }}>
                    ({typeof f.last_test.timestamp === 'string'
                      ? f.last_test.timestamp
                      : new Date(f.last_test.timestamp > 1e11 ? f.last_test.timestamp : f.last_test.timestamp * 1000).toLocaleTimeString()})
                  </span>
                </b>
              </li>
            )}
            {f?.zombie_servers && f.zombie_servers.length > 0 && (
              <li>
                <span>Зомби-серверы</span>
                <b style={{ color: '#f87171' }} data-testid="dashboard-zombie-count">
                  🧟 {f.zombie_servers.length} в карантине
                </b>
              </li>
            )}
            {(f?.active_tier === 2 || (f?.whitelist_episode_bytes ?? 0) > 0) && (
              <li>
                <span>🛡️ Трафик БС (эпизод)</span>
                <b style={{ color: f?.active_tier === 2 ? '#eab308' : '#94a3b8' }} data-testid="dashboard-whitelist-traffic">
                  {formatBytesRu(f?.whitelist_episode_bytes ?? 0)} за сессию
                </b>
              </li>
            )}
            {f?.best_ping_enabled && (
              <li>
                <span>Режим Best-Ping</span>
                <b style={{ color: '#00D3F2' }} data-testid="dashboard-best-ping-status">
                  🚀 Активен (пул {f.best_ping_pool_size ?? 15} узлов, ±{f.best_ping_hysteresis_ms ?? 40} мс)
                </b>
              </li>
            )}
            {/* Приоритет провайдеров (Tier 1 vs Tier 2) в дашборде */}
            {providers.length > 0 && (
              <li style={{ flexDirection: 'column', alignItems: 'flex-start', gap: 4 }}>
                <div style={{ display: 'flex', justifyContent: 'space-between', width: '100%', alignItems: 'center' }}>
                  <span>Приоритет провайдеров</span>
                  {onSwitchTab && (
                    <button
                      type="button"
                      className="btn sm ghost"
                      style={{ padding: '0 4px', fontSize: 11, height: 18 }}
                      onClick={() => onSwitchTab('settings')}
                      title="Настроить приоритет провайдеров в настройках Failover"
                    >
                      изменить →
                    </button>
                  )}
                </div>
                <div style={{ display: 'flex', flexWrap: 'wrap', gap: 4, marginTop: 2, width: '100%' }}>
                  {(() => {
                    const pChain = f?.provider_chain ?? []
                    const ordered = pChain
                      .map((id) => providers.find((p) => p.id === id))
                      .filter((p): p is ProviderInfo => Boolean(p))
                    const remaining = providers.filter((p) => !pChain.includes(p.id))
                    const list = ordered.length > 0 ? ordered : remaining

                    return list.slice(0, 4).map((p, idx) => {
                      const isTier2 = Boolean(p.is_whitelist)
                      return (
                        <span
                          key={p.id}
                          className="badge"
                          style={{
                            fontSize: 10,
                            padding: '1px 6px',
                            background: isTier2 ? 'rgba(234, 179, 8, 0.12)' : 'rgba(56, 189, 248, 0.12)',
                            color: isTier2 ? '#eab308' : '#38bdf8',
                            border: `1px solid ${isTier2 ? 'rgba(234, 179, 8, 0.3)' : 'rgba(56, 189, 248, 0.3)'}`,
                            fontWeight: 600,
                            display: 'inline-flex',
                            alignItems: 'center',
                            gap: 3,
                          }}
                          title={`${p.name || p.id} (${isTier2 ? 'Tier 2 Whitelist' : 'Tier 1 VPS'}) — ${p.count} серверов`}
                        >
                          <span style={{ opacity: 0.7 }}>#{idx + 1}</span>
                          <span>{p.name || p.id}</span>
                          <span style={{ fontSize: 9 }}>{isTier2 ? '🛡️' : '⚡'}</span>
                        </span>
                      )
                    })
                  })()}
                  {providers.length > 4 && (
                    <span className="muted" style={{ fontSize: 10, alignSelf: 'center' }}>
                      +{providers.length - 4}
                    </span>
                  )}
                </div>
              </li>
            )}
          </ul>
          </div>
          <div style={{ display: 'flex', gap: 8, marginTop: 12 }}>
            <button className="btn primary" onClick={runCheck} disabled={checking} style={{ flex: 1 }}>
              {checking ? 'Проверка…' : '🔍 Проверить сейчас'}
            </button>
            {onSwitchTab && (
              <button
                type="button"
                className="btn ghost"
                onClick={() => onSwitchTab('settings')}
                title="Настроить порог пинга и интервал"
              >
                ⚙️
              </button>
            )}
          </div>
        </section>
      </div>

      {/* ВИДЖЕТ «🛡️ Детектор белых списков» (Задача 4) */}
      {(() => {
        const detCfg = f?.whitelist_detector
        const detStatus = f?.whitelist_detector_status
        const isDetectorEnabled = Boolean(detCfg?.enabled)

        const isBsActive = Boolean(
          status?.whitelist_mode_active ||
          f?.active_tier === 2 ||
          detStatus?.state === 'WhitelistDetected' ||
          (status?.active_server && (
            status?.whitelist_servers?.includes(status.active_server.id) ||
            (status.active_server.provider && status?.whitelist_providers?.includes(status.active_server.provider))
          ))
        )

        const monProv = detCfg?.monitored_provider || detStatus?.monitored_provider || 'Все зарубежные узлы'
        const totalNodes = detStatus?.total_nodes ?? 0
        const onlineNodes = detStatus?.online_nodes ?? 0
        const offlineNodes = detStatus?.offline_nodes ?? 0
        const offlinePct = detStatus?.offline_percent ?? 0
        const triggerPct = detCfg?.trigger_offline_percent ?? 80
        const checkIntervalMins = detCfg?.check_interval_mins ?? detStatus?.check_interval_mins ?? 10

        const activeWlProv = providers.find((p) => p.is_whitelist && (p.total_bytes ?? 0) > 0) || providers.find((p) => (p.total_bytes ?? 0) > 0)
        const total = activeWlProv?.total_bytes ?? 0
        const used = (activeWlProv?.upload_bytes ?? 0) + (activeWlProv?.download_bytes ?? 0)
        const remaining = total > 0 ? Math.max(0, total - used) : 0

        const handleToggleDetector = async () => {
          if (togglingDetector) return
          setTogglingDetector(true)
          try {
            const newEnabled = !isDetectorEnabled
            const curFailover = status?.failover || ({} as any)
            const curDetector = curFailover.whitelist_detector || {}
            await apiPut('settings', {
              failover: {
                ...curFailover,
                whitelist_detector: {
                  ...curDetector,
                  enabled: newEnabled,
                },
              },
            })
            notify(`Авто-детектор белых списков: ${newEnabled ? 'ВКЛ 🟢' : 'ВЫКЛ ⚪'}`)
            refresh?.()
          } catch (e: any) {
            notify(e?.message || 'Ошибка переключения детектора', true)
          } finally {
            setTogglingDetector(false)
          }
        }

        const handleRunDetectorCheck = async () => {
          if (checkingDetector) return
          setCheckingDetector(true)
          try {
            notify('Запуск точной проверки зарубежных узлов детектора БС...')
            const res = await runWhitelistDetectorCheck()
            setDetectorCheckResult(res.message || 'Проверка выполнена')
            notify(res.message || 'Проверка детектора завершена')
            refresh?.()
          } catch (e: any) {
            notify(e?.message || 'Ошибка проверки детектора', true)
          } finally {
            setCheckingDetector(false)
          }
        }

        const handleOpenClassifierModal = () => {
          const detected = detectWhitelistServers(allServers, providers)
          setDetectedClassifier(detected)
          setDetectorModalOpen(true)
        }

        const handleApplyClassifier = async () => {
          if (!detectedClassifier || classifierApplying) return
          setClassifierApplying(true)
          try {
            const res = await applyDetectedWhitelistServers(
              detectedClassifier.detectedServerIds,
              detectedClassifier.detectedProviderIds
            )
            notify(
              `Применено: серверов БС — ${res.whitelist_servers.length}, подписок БС — ${res.whitelist_providers.length}`
            )
            setDetectorModalOpen(false)
            refresh?.()
          } catch (e: any) {
            notify(e?.message || 'Ошибка применения белых списков', true)
          } finally {
            setClassifierApplying(false)
          }
        }

        return (
          <section className="card" style={{ marginTop: 14 }} data-testid="dashboard-whitelist-detector-widget">
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 12, flexWrap: 'wrap', gap: 10 }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
                <h2 style={{ margin: 0, fontSize: 16, display: 'flex', alignItems: 'center', gap: 6 }}>
                  <span>🛡️</span> Детектор белых списков
                </h2>
                <span
                  className="badge"
                  data-testid="detector-state-badge"
                  style={{
                    background: isBsActive ? 'rgba(168, 85, 247, 0.2)' : 'rgba(34, 197, 94, 0.15)',
                    color: isBsActive ? '#c084fc' : '#4ade80',
                    border: `1px solid ${isBsActive ? 'rgba(168, 85, 247, 0.4)' : 'rgba(34, 197, 94, 0.3)'}`,
                    fontWeight: 600,
                  }}
                >
                  {isBsActive ? '🟣 Обнаружены БС (Туннель активен)' : '🟢 Штатный режим (VPS)'}
                </span>
                {detStatus?.warning && (
                  <span className="badge" style={{ background: 'rgba(239, 68, 68, 0.15)', color: '#f87171', border: '1px solid rgba(239, 68, 68, 0.3)' }}>
                    ⚠️ {detStatus.warning}
                  </span>
                )}
              </div>

              <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
                {/* 1-клик тумблер авто-детектора в шапке */}
                <button
                  type="button"
                  className={`btn sm ${isDetectorEnabled ? 'primary' : 'ghost'}`}
                  onClick={handleToggleDetector}
                  disabled={togglingDetector}
                  data-testid="detector-toggle-btn"
                  title="Включение/выключение автодетектора белых списков"
                  style={{ minWidth: 100 }}
                >
                  {togglingDetector ? 'Сохранение…' : isDetectorEnabled ? 'Авто: ВКЛ 🟢' : 'Авто: ВЫКЛ ⚪'}
                </button>

                {/* Проверить сейчас */}
                <button
                  type="button"
                  className="btn sm"
                  onClick={handleRunDetectorCheck}
                  disabled={checkingDetector}
                  data-testid="detector-check-now-btn"
                  title="Принудительно опросить контролируемые зарубежные узлы прямо сейчас"
                >
                  {checkingDetector ? '⏳ Проверка…' : '🔍 Проверить сейчас'}
                </button>

                {/* Определить БС */}
                <button
                  type="button"
                  className="btn sm"
                  onClick={handleOpenClassifierModal}
                  data-testid="detector-classify-modal-btn"
                  title="Автоматически классифицировать серверы и подписки по тегам БС и применить в 1 клик"
                >
                  ✨ Определить БС
                </button>
              </div>
            </div>

            {detectorCheckResult && (
              <div
                style={{
                  marginBottom: 12,
                  padding: '8px 12px',
                  borderRadius: 6,
                  fontSize: 12,
                  background: 'rgba(56, 189, 248, 0.08)',
                  border: '1px solid rgba(56, 189, 248, 0.3)',
                  color: '#38bdf8',
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'space-between',
                  gap: 8,
                }}
                data-testid="detector-check-result-banner"
              >
                <span>📊 {detectorCheckResult}</span>
                <button
                  type="button"
                  className="btn ghost sm"
                  style={{ padding: '0 6px', height: 20, fontSize: 11 }}
                  onClick={() => setDetectorCheckResult(null)}
                >
                  ✕
                </button>
              </div>
            )}

            {/* Честные карточки метрик */}
            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(210px, 1fr))', gap: 12, marginTop: 4 }}>
              {/* Карточка 1: Статус ТСПУ / Туннеля */}
              <div style={{ background: 'rgba(255, 255, 255, 0.02)', border: '1px solid var(--border)', borderRadius: 8, padding: '10px 12px' }} data-testid="card-tspu-tunnel-status">
                <div className="muted small" style={{ fontSize: 11, marginBottom: 4 }}>Статус ТСПУ / Туннеля</div>
                <div style={{ fontSize: 13, fontWeight: 600, color: isBsActive ? '#c084fc' : '#4ade80' }}>
                  {isBsActive ? '🟣 БС активны (ТСПУ блокирует)' : '🟢 Штатный режим (Normal VPS)'}
                </div>
                <div className="muted small" style={{ fontSize: 10, marginTop: 2 }}>
                  Контроль: <b style={{ color: 'var(--text-bright)' }}>{monProv}</b>
                </div>
              </div>

              {/* Карточка 2: Здоровье зарубежных серверов */}
              <div style={{ background: 'rgba(255, 255, 255, 0.02)', border: '1px solid var(--border)', borderRadius: 8, padding: '10px 12px' }} data-testid="card-foreign-servers-health">
                <div className="muted small" style={{ fontSize: 11, marginBottom: 4 }}>Здоровье зарубежных узлов</div>
                <div style={{ fontSize: 13, fontWeight: 600, color: totalNodes > 0 ? (offlinePct >= triggerPct ? '#ef4444' : '#4ade80') : 'var(--muted)' }}>
                  {totalNodes > 0
                    ? `${onlineNodes} из ${totalNodes} онлайн (${offlinePct}% оффлайн, ${offlineNodes} недоступно)`
                    : 'Ожидание первой проверки'}
                </div>
                <div className="muted small" style={{ fontSize: 10, marginTop: 2 }}>
                  Порог срабатывания: <b style={{ color: '#f87171' }}>≥{triggerPct}%</b> {detStatus?.last_checked ? `• ${detStatus.last_checked}` : ''}
                </div>
              </div>

              {/* Карточка 3: Интервал проверки в минутах */}
              <div style={{ background: 'rgba(255, 255, 255, 0.02)', border: '1px solid var(--border)', borderRadius: 8, padding: '10px 12px' }} data-testid="card-detector-interval">
                <div className="muted small" style={{ fontSize: 11, marginBottom: 4 }}>Интервал сканирования</div>
                <div style={{ fontSize: 13, fontWeight: 600, color: 'var(--text-bright)' }}>
                  каждые {checkIntervalMins} мин
                </div>
                <div className="muted small" style={{ fontSize: 10, marginTop: 2 }}>
                  Фоновый опрос ТСПУ ({isDetectorEnabled ? 'активен' : 'на паузе'})
                </div>
              </div>

              {/* Карточка 4: Остаток трафика подписки БС */}
              <div style={{ background: 'rgba(255, 255, 255, 0.02)', border: '1px solid var(--border)', borderRadius: 8, padding: '10px 12px' }} data-testid="card-bs-quota-remaining">
                <div className="muted small" style={{ fontSize: 11, marginBottom: 4 }}>Остаток трафика подписки БС</div>
                <div style={{ fontSize: 13, fontWeight: 600, color: total > 0 ? (remaining === 0 ? '#ef4444' : '#38bdf8') : 'var(--muted)' }}>
                  {total > 0 ? `${formatQuotaBytes(remaining)} (из ${formatQuotaBytes(total)})` : 'Не лимитирован / без квоты'}
                </div>
                <div className="muted small" style={{ fontSize: 10, marginTop: 2 }}>
                  {activeWlProv ? `Подписка: ${activeWlProv.name || activeWlProv.id}` : 'Провайдер БС не привязан'}
                </div>
              </div>
            </div>

            {/* Модальное окно классификатора БС */}
            {detectorModalOpen && detectedClassifier && (
              <div
                className="modal-backdrop"
                style={{
                  position: 'fixed',
                  top: 0,
                  left: 0,
                  right: 0,
                  bottom: 0,
                  background: 'rgba(0, 0, 0, 0.7)',
                  backdropFilter: 'blur(3px)',
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'center',
                  zIndex: 9999,
                  padding: 16,
                }}
                onClick={() => setDetectorModalOpen(false)}
              >
                <div
                  className="modal-content card"
                  style={{
                    maxWidth: 580,
                    width: '100%',
                    background: 'var(--card-bg, #1e293b)',
                    border: '1px solid var(--border, rgba(255, 255, 255, 0.1))',
                    borderRadius: 12,
                    padding: 20,
                  }}
                  onClick={(e) => e.stopPropagation()}
                  data-testid="detector-classifier-modal"
                >
                  <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 16 }}>
                    <h3 style={{ margin: 0, fontSize: 16, display: 'flex', alignItems: 'center', gap: 8 }}>
                      <span>✨</span> Автоопределение серверов и подписок БС
                    </h3>
                    <button
                      type="button"
                      className="btn ghost sm"
                      onClick={() => setDetectorModalOpen(false)}
                    >
                      ✕
                    </button>
                  </div>

                  <p className="muted small" style={{ margin: '0 0 14px' }}>
                    Система сканирует конфигурацию на наличие тегов <code>[БС]</code>, <code>[WL]</code>, <code>whitelist</code> в названиях серверов и подписок.
                  </p>

                  {detectedClassifier.details.length === 0 ? (
                    <div
                      style={{
                        padding: 16,
                        borderRadius: 8,
                        background: 'rgba(255, 255, 255, 0.02)',
                        border: '1px solid var(--border)',
                        textAlign: 'center',
                        color: 'var(--muted)',
                        fontSize: 13,
                      }}
                    >
                      ℹ️ Теги белых списков не обнаружены среди доступных узлов и подписок.
                    </div>
                  ) : (
                    <div style={{ maxHeight: 280, overflowY: 'auto', display: 'flex', flexDirection: 'column', gap: 6, marginBottom: 16 }}>
                      {detectedClassifier.details.map((d) => (
                        <div
                          key={`${d.type}-${d.id}`}
                          style={{
                            display: 'flex',
                            alignItems: 'center',
                            justifyContent: 'space-between',
                            gap: 10,
                            padding: '8px 12px',
                            background: 'rgba(255, 255, 255, 0.03)',
                            border: '1px solid rgba(255, 255, 255, 0.08)',
                            borderRadius: 6,
                            fontSize: 12,
                          }}
                        >
                          <div style={{ display: 'flex', alignItems: 'center', gap: 8, overflow: 'hidden' }}>
                            <span className="badge" style={{ background: d.type === 'provider' ? 'rgba(56, 189, 248, 0.2)' : 'rgba(168, 85, 247, 0.2)', color: d.type === 'provider' ? '#38bdf8' : '#c084fc', fontSize: 10 }}>
                              {d.type === 'provider' ? 'Подписка' : 'Сервер' }
                            </span>
                            <b style={{ color: 'var(--text-bright)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                              {d.name || d.id}
                            </b>
                          </div>
                          <span className="muted small" style={{ fontSize: 10.5, whiteSpace: 'nowrap' }}>
                            {d.reason}
                          </span>
                        </div>
                      ))}
                    </div>
                  )}

                  <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8, marginTop: 16 }}>
                    <button
                      type="button"
                      className="btn ghost"
                      onClick={() => setDetectorModalOpen(false)}
                    >
                      Отмена
                    </button>
                    <button
                      type="button"
                      className="btn primary"
                      disabled={detectedClassifier.details.length === 0 || classifierApplying}
                      onClick={handleApplyClassifier}
                      data-testid="detector-apply-classifier-btn"
                    >
                      {classifierApplying ? 'Применение…' : `Применить в 1 клик (${detectedClassifier.details.length})`}
                    </button>
                  </div>
                </div>
              </div>
            )}
          </section>
        )
      })()}

      {/* КАРТОЧКА: Быстрая проверка домена («Почему не открывается?») */}
      <section className="card" style={{ marginTop: 14 }}>
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 10 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <h2 style={{ margin: 0, fontSize: 16 }}>🔍 Быстрая проверка домена («Почему не открывается?»)</h2>
          </div>
          <span className="muted small">AdGuard Home & Маршруты</span>
        </div>

        <form onSubmit={handleCheckDomain} style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
          <input
            type="text"
            className="input"
            placeholder="Введите домен или URL (например: adsrv.hh.ru или rutracker.org)"
            value={domainQuery}
            onChange={(e) => setDomainQuery(e.target.value)}
            style={{ flex: 1, padding: '8px 12px', fontSize: 13 }}
          />
          <button
            type="submit"
            className="btn primary"
            disabled={domainChecking || !domainQuery.trim()}
            style={{ whiteSpace: 'nowrap' }}
          >
            {domainChecking ? 'Опрос…' : 'Проверить в 1 клик'}
          </button>
        </form>

        {domainResult && (
          <div
            style={{
              marginTop: 12,
              padding: '12px 16px',
              borderRadius: 8,
              background: domainResult.is_blocked ? 'rgba(239, 68, 68, 0.08)' : 'rgba(34, 197, 94, 0.08)',
              border: `1px solid ${domainResult.is_blocked ? 'rgba(239, 68, 68, 0.3)' : 'rgba(34, 197, 94, 0.3)'}`,
            }}
          >
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start', flexWrap: 'wrap', gap: 10 }}>
              <div>
                <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 4 }}>
                  <span style={{ fontSize: 18 }}>{domainResult.is_blocked ? '🛡️' : '🟢'}</span>
                  <b style={{ fontSize: 14, color: domainResult.is_blocked ? '#f87171' : '#4ade80' }}>
                    {domainResult.is_blocked
                      ? `Заблокирован AdGuard Home`
                      : `Домен доступен (не блокируется AdGuard)`}
                  </b>
                  <span className="mono" style={{ fontSize: 13, background: 'rgba(255,255,255,0.06)', padding: '2px 6px', borderRadius: 4 }}>
                    {domainResult.domain}
                  </span>
                </div>
                {domainResult.is_blocked && (
                  <div className="muted small" style={{ fontSize: 12, marginTop: 4 }}>
                    {domainResult.rule && <span>Правило: <code className="mono">{domainResult.rule}</code></span>}
                    {domainResult.filter_name && <span style={{ marginLeft: 8 }}>в фильтре: <b>{domainResult.filter_name}</b></span>}
                  </div>
                )}
                <div style={{ fontSize: 12, marginTop: 6, color: 'var(--text)' }}>
                  <span>🚀 Маршрутизация: <b>{domainResult.routing_target}</b></span>
                </div>
              </div>

              {domainResult.is_blocked && (
                <button
                  type="button"
                  className="btn success sm"
                  disabled={unblocking}
                  onClick={() => handleUnblockDomain(domainResult.domain)}
                  style={{
                    background: '#22c55e',
                    color: '#fff',
                    fontWeight: 600,
                    padding: '6px 14px',
                    borderRadius: 6,
                    border: 'none',
                    cursor: 'pointer',
                  }}
                >
                  {unblocking ? 'Разблокировка…' : '🔓 Разблокировать в 1 клик'}
                </button>
              )}
            </div>
          </div>
        )}
      </section>

      {/* КАРТОЧКА 3: Блок событий с группировкой и цветовой разметкой */}
      <section className="card" style={{ marginTop: 14 }}>
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 10 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <h2 style={{ margin: 0 }}>📋 Журнал событий Failover</h2>
            {groupedEvents.length > 0 && (
              <span className="badge" style={{ fontSize: 11, background: 'rgba(255,255,255,0.06)' }}>
                {groupedEvents.length} {groupedEvents.length === 1 ? 'запись' : 'записей'}
              </span>
            )}
          </div>
          {onSwitchTab && (
            <button
              type="button"
              className="btn sm ghost"
              onClick={() => onSwitchTab('settings')}
              title="Открыть системный лог и настройки"
            >
              Системный журнал →
            </button>
          )}
        </div>

        {groupedEvents.length === 0 ? (
          <div style={{ padding: '16px 20px', background: 'rgba(34, 197, 94, 0.04)', border: '1px solid rgba(34, 197, 94, 0.15)', borderRadius: 8, display: 'flex', alignItems: 'center', gap: 12 }}>
            <span style={{ fontSize: 22 }}>🟢</span>
            <div>
              <div style={{ fontWeight: 600, color: 'var(--text)', fontSize: 13 }}>Сбоев и переключений не зафиксировано</div>
              <div className="muted small">Автоматический мониторинг активен. Все серверы работают в штатном режиме.</div>
            </div>
          </div>
        ) : (
          <div className="events-container">
            {groupedEvents.map((e, i) => (
              <div key={i} className={`event-entry ev-${e.type}`}>
                <span className="ev-type-icon">
                  {e.type === 'switch' ? '🔵' : e.type === 'err' ? '🔴' : e.type === 'warn' ? '🟡' : '🟢'}
                </span>
                <span className="ev-time-pill">{e.time}</span>
                <span className="ev-msg-text" title={e.message}>{e.message}</span>
                {e.count > 1 && (
                  <span className="ev-repeat-badge" title={`Повторено ${e.count} раз(а) подряд`}>
                    ×{e.count}
                  </span>
                )}
              </div>
            ))}
          </div>
        )}
      </section>
    </div>
  )
}

/// Мини-график пинга (SVG sparkline): последняя точка справа, провалы — красные.
function PingSparkline({ data }: { data: { ms: number; ok: boolean }[] }) {
  const W = 280
  const H = 40
  if (!data || data.length === 0) return null
  const okVals = data.filter((d) => d.ok).map((d) => d.ms)
  const maxVal = okVals.length > 0 ? okVals.reduce((a, b) => Math.max(a, b)) : 100
  const minVal = okVals.length > 0 ? okVals.reduce((a, b) => Math.min(a, b)) : 0
  const max = Math.max(100, maxVal) * 1.15
  const min = Math.max(0, minVal * 0.85)
  const range = max - min || 1
  const step = data.length > 1 ? W / (data.length - 1) : W
  const y = (d: { ms: number; ok: boolean }) =>
    d.ok ? H - 4 - ((d.ms - min) / range) * (H - 8) : H - 2
  const last = data[data.length - 1]

  if (data.length === 1) {
    const singleY = y(last)
    return (
      <svg width="100%" height={H} viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" style={{ display: 'block' }}>
        <line x1="0" y1={singleY} x2={W} y2={singleY} stroke="#38bdf8" strokeWidth="1.5" strokeDasharray="4 4" opacity="0.6" />
        <circle cx={W / 2} cy={singleY} r="3.5" fill="#38bdf8" />
      </svg>
    )
  }

  const pts = data.map((d, i) => `${(i * step).toFixed(1)},${y(d).toFixed(1)}`).join(' ')
  const areaPts = `0,${H} ` + pts + ` ${W},${H}`
  return (
    <svg width="100%" height={H} viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" style={{ display: 'block' }}>
      <defs>
        <linearGradient id="dashSparkGrad" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor="#38bdf8" stopOpacity="0.25" />
          <stop offset="100%" stopColor="#38bdf8" stopOpacity="0.0" />
        </linearGradient>
      </defs>
      <polygon points={areaPts} fill="url(#dashSparkGrad)" />
      <polyline points={pts} fill="none" stroke="#38bdf8" strokeWidth="1.8" />
      {data.map((d, i) =>
        d.ok ? null : (
          <circle key={i} cx={i * step} cy={y(d)} r="2.6" fill="var(--red, #e5484d)" />
        ),
      )}
      <circle cx={(data.length - 1) * step} cy={y(last)} r="3" fill="#38bdf8" />
    </svg>
  )
}
