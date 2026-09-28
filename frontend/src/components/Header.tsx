import { useState, useEffect } from 'react'
import { apiGet, apiPost } from '../api'
import type { StatusInfo, SystemStats } from '../types'

export function is5AmCheckDue(lastCheckMs: number, nowMs?: number): boolean {
  const now = nowMs !== undefined ? new Date(nowMs) : new Date()
  if (now.getFullYear() < 2024) return false
  if (!lastCheckMs) return true
  const today5am = new Date(now.getFullYear(), now.getMonth(), now.getDate(), 5, 0, 0, 0).getTime()
  if (now.getTime() >= today5am) {
    return lastCheckMs < today5am
  } else {
    const yesterday5am = today5am - 24 * 60 * 60 * 1000
    return lastCheckMs < yesterday5am
  }
}

interface HeaderProps {
  status: StatusInfo | null
  notify: (msg: string, isError?: boolean) => void
  refresh: () => Promise<void>
  onSwitchTab: (tab: string) => void
  activeTab?: string
  theme?: 'dark' | 'light'
  onToggleTheme?: () => void
  onOpenEditor?: () => void
  onOpenUpdateModal?: () => void
  onOpenMihomoModal?: () => void
  onOpenZapretModal?: () => void
  authStatus?: { enabled: boolean; authenticated: boolean }
  onLogout?: () => void
}


function IconCpu() {
  return (
    <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <rect x="4" y="4" width="16" height="16" rx="2" />
      <rect x="9" y="9" width="6" height="6" />
      <path d="M9 1v3M15 1v3M9 20v3M15 20v3M20 9h3M20 14h3M1 9h3M1 14h3" />
    </svg>
  )
}

function IconBox() {
  return (
    <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16z" />
      <polyline points="3.27 6.96 12 12.01 20.73 6.96" />
      <line x1="12" y1="22.08" x2="12" y2="12" />
    </svg>
  )
}

function IconShield() {
  return (
    <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z" />
    </svg>
  )
}


function IconRefresh({ className = '' }: { className?: string }) {
  return (
    <svg className={className} width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M21.5 2v6h-6M21.34 15.57a10 10 0 1 1-.57-8.38l6.73-1.19" />
    </svg>
  )
}

function IconStop() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="#ef4444" stroke="#ef4444" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <rect x="4" y="4" width="16" height="16" rx="3" />
    </svg>
  )
}

function IconPlay() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="#22c55e" stroke="#22c55e" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <polygon points="5 3 19 12 5 21 5 3" />
    </svg>
  )
}

function IconDisk() {
  return (
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <ellipse cx="12" cy="5" rx="9" ry="3" />
      <path d="M21 12c0 1.66-4 3-9 3s-9-1.34-9-3" />
      <path d="M3 5v14c0 1.66 4 3 9 3s9-1.34 9-3V5" />
    </svg>
  )
}

function BrandLogoIcon() {
  return (
    <svg
      className="header-brand-icon"
      width="28"
      height="28"
      viewBox="0 0 28 28"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
    >
      <defs>
        <linearGradient id="brandIconGrad" x1="2" y1="2" x2="26" y2="26" gradientUnits="userSpaceOnUse">
          <stop offset="0%" stopColor="#00D3F2" />
          <stop offset="50%" stopColor="#2B7FFF" />
          <stop offset="100%" stopColor="#155DFC" />
        </linearGradient>
        <linearGradient id="brandGlowGrad" x1="0" y1="0" x2="28" y2="28" gradientUnits="userSpaceOnUse">
          <stop offset="0%" stopColor="#00D3F2" stopOpacity="0.25" />
          <stop offset="100%" stopColor="#155DFC" stopOpacity="0.08" />
        </linearGradient>
      </defs>
      {/* Мягкая фоновая сфера */}
      <circle cx="14" cy="14" r="13" fill="url(#brandGlowGrad)" />
      <circle cx="14" cy="14" r="12.5" stroke="url(#brandIconGrad)" strokeWidth="1.6" />
      {/* Меридиан и экватор глобуса */}
      <ellipse cx="14" cy="14" rx="5.2" ry="12.5" stroke="#00D3F2" strokeWidth="1.1" strokeOpacity="0.4" />
      <path d="M 2 14 H 26" stroke="#00D3F2" strokeWidth="1.1" strokeOpacity="0.4" />
      {/* Динамическая кривая маршрутизации трафика (route flow) */}
      <path
        d="M 5 19.5 C 8.5 24, 18.5 24, 22.5 18 C 25.5 13, 22 6, 14 5 C 8.5 4.5, 4.5 9, 6.5 15 C 8 19, 14 19.5, 18 16"
        stroke="url(#brandIconGrad)"
        strokeWidth="2.2"
        strokeLinecap="round"
      />
      {/* Узлы сети (active nodes) */}
      <circle cx="18" cy="16" r="2" fill="#00D3F2" />
      <circle cx="6.5" cy="15" r="1.3" fill="#2B7FFF" />
    </svg>
  )
}

export default function Header({
  status,
  notify,
  refresh,
  onSwitchTab,
  activeTab,
  theme = 'dark',
  onToggleTheme,
  onOpenEditor,
  onOpenUpdateModal,
  onOpenMihomoModal,
  onOpenZapretModal,
  authStatus,
  onLogout,
}: HeaderProps) {
  const [pending, setPending] = useState(false)
  const [liveMetrics, setLiveMetrics] = useState<SystemStats | null>(null)
  const [updateAvailable, setUpdateAvailable] = useState(false)
  const [latestVersion, setLatestVersion] = useState('')

  const isRunning = status?.service_stopped
    ? false
    : (status?.service_running !== undefined ? status.service_running : (status ? Boolean(status.failover?.enabled) : true))

  // Периодическое обновление метрик (раз в 3 сек для минимизации нагрузки на CPU роутера)
  useEffect(() => {
    let active = true
    const fetchMetrics = async () => {
      if (document.hidden) return
      try {
        const data = await apiGet<SystemStats>('system/metrics')
        if (active && data) {
          setLiveMetrics(data)
        }
      } catch {
        /* временный сбой соединения — не прерываем таймер */
      }
    }

    fetchMetrics()
    const timer = setInterval(fetchMetrics, 3000)
    return () => {
      active = false
      clearInterval(timer)
    }
  }, [])

  // Периодическая проверка наличия новой версии (при старте и раз в 60 сек)
  useEffect(() => {
    let active = true
    const checkUpdate = async () => {
      if (document.hidden) return
      try {
        const res = await apiGet<{ current: string; latest: string; update_available: boolean }>('update/check')
        if (active && res) {
          setUpdateAvailable(Boolean(res.update_available))
          setLatestVersion(res.latest || '')
        }
      } catch {
        /* игнорируем ошибку сети */
      }
    }

    checkUpdate()
    const timer = setInterval(checkUpdate, 60_000)
    return () => {
      active = false
      clearInterval(timer)
    }
  }, [])

  // Периодическая проверка обновления ядра Mihomo
  const [mihomoUpdateAvailable, setMihomoUpdateAvailable] = useState(false)
  const [mihomoLatestVersion, setMihomoLatestVersion] = useState('')

  useEffect(() => {
    let active = true
    const checkMihomoUpdate = async () => {
      if (document.hidden) return
      try {
        const res = await apiGet<{ current_version: string; latest_version: string }>('mihomo/releases')
        if (active && res) {
          const cur = res.current_version || ''
          const lat = res.latest_version || ''
          const hasUpd = Boolean(lat && cur && lat !== cur && !cur.includes(lat))
          setMihomoUpdateAvailable(hasUpd)
          setMihomoLatestVersion(lat)
        }
      } catch {
        /* игнорируем ошибку сети */
      }
    }

    checkMihomoUpdate()
    const timer = setInterval(checkMihomoUpdate, 90_000)
    return () => {
      active = false
      clearInterval(timer)
    }
  }, [])

  // Запрет статус и обновление (проверка в 5:00 утра раз в сутки)
  const [zapretUpdateAvailable, setZapretUpdateAvailable] = useState<boolean | null>(null)
  const [zapretLatestVersion, setZapretLatestVersion] = useState<string | null>(null)
  const [zapretVersion, setZapretVersion] = useState('')
  const [zapretEngine, setZapretEngine] = useState<'v1' | 'v2'>('v1')

  const effectiveZapretUpdate =
    zapretUpdateAvailable !== null
      ? zapretUpdateAvailable
      : Boolean(status?.zapret?.update_available)

  const effectiveZapretLatest =
    zapretLatestVersion !== null
      ? zapretLatestVersion
      : (status?.zapret?.latest_version || '')

  useEffect(() => {
    const handleZapretUpdated = (
      e: Event
    ) => {
      const customEvent = e as CustomEvent<{
        update_available?: boolean
        engine?: 'v1' | 'v2'
        version?: string
        latest_version?: string
      }>
      if (customEvent.detail) {
        if (customEvent.detail.update_available !== undefined) {
          setZapretUpdateAvailable(Boolean(customEvent.detail.update_available))
        }
        if (customEvent.detail.engine) setZapretEngine(customEvent.detail.engine)
        if (customEvent.detail.version) setZapretVersion(customEvent.detail.version)
        if (customEvent.detail.latest_version) setZapretLatestVersion(customEvent.detail.latest_version)
      } else {
        setZapretUpdateAvailable(false)
      }
    }
    window.addEventListener('xr:zapret-updated', handleZapretUpdated)
    return () => window.removeEventListener('xr:zapret-updated', handleZapretUpdated)
  }, [])

  useEffect(() => {
    if (status?.zapret) {
      if (status.zapret.engine) {
        setZapretEngine(status.zapret.engine)
      }
      if (status.zapret.version) {
        setZapretVersion(status.zapret.version)
      }
      if (status.zapret.update_available !== undefined) {
        setZapretUpdateAvailable(Boolean(status.zapret.update_available))
      }
      if (status.zapret.latest_version) {
        setZapretLatestVersion(status.zapret.latest_version)
      }
    }
  }, [status?.zapret])

  useEffect(() => {
    let active = true
    const checkZapretDaily = async (force = false) => {
      if (document.hidden) return
      const lastCheckStr = localStorage.getItem('xr_zapret_last_check')
      const lastCheckMs = lastCheckStr ? parseInt(lastCheckStr, 10) : 0
      if (!force && !is5AmCheckDue(lastCheckMs)) {
        return
      }

      try {
        const res = await apiGet<{
          current_engine?: 'v1' | 'v2'
          current_version?: string
          label?: string
          latest_version?: string
          update_available?: boolean
        }>('zapret/update/check')
        if (active && res) {
          localStorage.setItem('xr_zapret_last_check', Date.now().toString())
          if (res.update_available !== undefined) {
            setZapretUpdateAvailable(Boolean(res.update_available))
          }
          if (res.latest_version) setZapretLatestVersion(res.latest_version)
          if (res.current_version) setZapretVersion(res.current_version)
          if (res.current_engine) setZapretEngine(res.current_engine)
        }
      } catch {
        /* игнорируем ошибку сети */
      }
    }

    checkZapretDaily()
    const timer = setInterval(() => checkZapretDaily(false), 60_000)
    return () => {
      active = false
      clearInterval(timer)
    }
  }, [])

  // Память и CPU (живые из 1-секундного таймера либо из статуса)
  const currentMetrics = liveMetrics || status?.system
  const memUsed = currentMetrics?.memory_used_mb ?? 0
  const memTotal = currentMetrics?.memory_total_mb ?? 0
  const cpuPercent = currentMetrics?.cpu_percent ?? 0
  const appMemMb = currentMetrics?.app_memory_mb ?? 0
  const appCpu = currentMetrics?.app_cpu_percent ?? 0
  const coreMemMb = currentMetrics?.core_memory_mb ?? 0
  const totalXkeenMem = currentMetrics?.total_xkeen_memory_mb ?? (appMemMb + coreMemMb)

  const mihomoVersion = status?.mihomo_version || '—'
  const appVersion = status?.version ? status.version.replace(/^v/, '') : '—'
  const zapretEngineLabel = zapretEngine === 'v2' ? 'Запрет 2' : 'Запрет 1'
  const rawZapretVersion = zapretVersion || status?.zapret?.version || ''
  const zapretVersionDisplay = rawZapretVersion
    ? (rawZapretVersion.startsWith('v') ? rawZapretVersion : `v${rawZapretVersion}`)
    : '—'

  const handleRestart = async () => {
    if (pending) return
    setPending(true)
    try {
      const res = await apiPost<{ message?: string }>('xkeen/service', { action: 'restart_all' })
      notify(res?.message || 'Полный перезапуск всех компонентов (XKeen, Mihomo, Zapret, Панель)…')
      await refresh()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка перезапуска сервисов', true)
    } finally {
      setPending(false)
    }
  }

  const handleToggle = async () => {
    if (pending) return
    setPending(true)
    const nextAction = isRunning ? 'stop' : 'start'
    try {
      await apiPost<{ message?: string; service_running?: boolean; service_stopped?: boolean }>('xkeen/service', { action: nextAction })
      notify(nextAction === 'stop' ? 'Все службы (XKeen, Mihomo, Zapret) остановлены. Трафик идёт напрямую.' : 'Службы успешно запущены.')
      await refresh()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка переключения сервиса', true)
    } finally {
      setPending(false)
    }
  }

  const cpuTemp = currentMetrics?.cpu_temp_c

  return (
    <header className="header-bar">
      {/* ЛЕВАЯ ЧАСТЬ: Статус сервиса + RAM/CPU + Температура + Потребление XKeen Route + Кнопки */}
      <div className="header-left">
        <div className={`status-badge-custom ${isRunning ? 'status-badge-running' : 'status-badge-stopped'}`}>
          <div className="status-badge-content">
            <div className="status-badge-row1">
              <span className={`status-dot ${isRunning ? 'status-dot-running' : 'status-dot-stopped'}`} />
              <span className="status-label">{isRunning ? 'Сервис запущен' : 'Сервис остановлен'}</span>
            </div>
            <div className="status-badge-row2">
              <span
                className="status-stat"
                title={memTotal > 0
                  ? `Оперативная память роутера: ${memUsed} из ${memTotal} МБ (${Math.round((memUsed / memTotal) * 100)}%)\nПотребление XKeen: всего ${totalXkeenMem > 0 ? totalXkeenMem : appMemMb} МБ (XR: ${appMemMb} МБ, Mihomo: ${coreMemMb > 0 ? coreMemMb + ' МБ' : '—'})`
                  : 'Оперативная память роутера'}
              >
                <IconDisk />
                <span>{memTotal > 0 ? `${memUsed}/${memTotal} МБ` : '—'}</span>
              </span>
              <span className="status-stat-sep">|</span>
              <span className="status-stat" title={`Нагрузка на процессор роутера: ${cpuPercent}%`}>
                <IconCpu />
                <span>{cpuPercent}%</span>
              </span>
              {(cpuTemp !== undefined && cpuTemp !== null) && (
                <>
                  <span className="status-stat-sep">|</span>
                  <span
                    className="status-stat"
                    title={`Температура процессора: ${Math.round(cpuTemp)}°C${cpuTemp > 90 ? ' (ВНИМАНИЕ: Критический нагрев выше 90°C!)' : ''}`}
                  >
                    <span>🌡️</span>
                    <span style={cpuTemp > 90 ? { color: '#ef4444', fontWeight: 'bold' } : {}}>
                      {Math.round(cpuTemp)}°C
                    </span>
                  </span>
                </>
              )}
              {(totalXkeenMem > 0 || appMemMb > 0) && (
                <>
                  <span className="status-stat-sep">|</span>
                  <span
                    className="status-stat"
                    title={`Потребление XKeen: всего ${Math.round(totalXkeenMem > 0 ? totalXkeenMem : appMemMb)} МБ RAM (Панель XR: ${Math.round(appMemMb)} МБ, Ядро Mihomo: ${coreMemMb > 0 ? Math.round(coreMemMb) + ' МБ' : '—'}), CPU: ${Math.round(appCpu)}%`}
                  >
                    <span className="status-xr-label">XKeen:</span>
                    <span>{Math.round(totalXkeenMem > 0 ? totalXkeenMem : appMemMb)} МБ</span>
                  </span>
                </>
              )}
            </div>
          </div>
        </div>

        <div className="header-actions">
          <button
            type="button"
            className="header-action-btn"
            onClick={handleRestart}
            disabled={pending}
            title="Перезапустить все службы (XKeen, Mihomo, Zapret, Панель)"
          >
            <IconRefresh className={pending ? 'spin-icon' : ''} />
          </button>
          <button
            type="button"
            className="header-action-btn"
            onClick={handleToggle}
            disabled={pending}
            title={isRunning ? 'Остановить сервис (все службы остановятся, прямой выход для всех устройств)' : 'Запустить сервис'}
          >
            {isRunning ? <IconStop /> : <IconPlay />}
          </button>
        </div>
      </div>

      {/* ЦЕНТР: Логотип XKeen UI / Route с градиентом */}
      <div className="header-center">
        <a
          href="#dashboard"
          className="header-brand-link"
          onClick={(e) => {
            e.preventDefault()
            onSwitchTab('dashboard')
          }}
        >
          <BrandLogoIcon />
          <span className="header-brand-text">XKeen Route</span>
        </a>
      </div>

      {/* ПРАВАЯ ЧАСТЬ: Чип Mihomo + Чип Версии + Кнопка Настроек */}
      <div className="header-right">
        <button
          type="button"
          data-testid="header-mihomo-pill"
          className={`header-pill-btn ${mihomoUpdateAvailable ? 'header-pill-update-green' : ''}`}
          onClick={() => {
            if (onOpenMihomoModal) {
              onOpenMihomoModal()
            } else {
              onSwitchTab('servers')
            }
          }}
          title={
            mihomoUpdateAvailable
              ? `Доступно обновление ядра Mihomo до ${mihomoLatestVersion}! Нажмите для установки`
              : `Управление ядром Mihomo (версия: ${mihomoVersion})`
          }
        >
          <IconCpu />
          <span className="header-pill-title">Mihomo</span>
          <span className="header-pill-subtitle">{mihomoVersion}</span>
          {mihomoUpdateAvailable && (
            <span
              className="update-pill-badge update-pill-badge-green"
              data-testid="mihomo-update-badge"
              title={`Доступна новая версия ${mihomoLatestVersion}`}
            >
              ↑ {mihomoLatestVersion.replace(/^v/, '')}
            </span>
          )}
        </button>

        <button
          type="button"
          data-testid="header-app-pill"
          className={`header-pill-btn ${updateAvailable ? 'header-pill-update-blue' : ''}`}
          onClick={() => {
            if (onOpenUpdateModal) {
              onOpenUpdateModal()
            }
          }}
          title={
            updateAvailable
              ? `Доступно обновление XKeen Route до ${latestVersion}! Нажмите для установки`
              : `Версия XKeen Route: ${appVersion}. Нажмите для управления версией`
          }
        >
          <IconBox />
          <span className="header-pill-title">{appVersion}</span>
          {updateAvailable && (
            <span
              className="update-pill-badge update-pill-badge-blue"
              data-testid="app-update-badge"
              title={`Доступна новая версия ${latestVersion}`}
            >
              ↑ {latestVersion.replace(/^v/, '')}
            </span>
          )}
        </button>

        {/* Чип Запрет */}
        <button
          type="button"
          data-testid="header-zapret-pill"
          className={`header-pill-btn ${effectiveZapretUpdate ? 'header-pill-update-green' : ''}`}
          onClick={() => {
            if (onOpenZapretModal) {
              onOpenZapretModal()
            } else {
              window.dispatchEvent(new CustomEvent('xr:open-zapret-modal'))
            }
            onSwitchTab('zapret')
          }}
          title={
            effectiveZapretUpdate
              ? `Доступно обновление Запрет до ${effectiveZapretLatest || 'новой версии'}! Нажмите для перехода`
              : `${zapretEngineLabel} ${zapretVersionDisplay}. Нажмите для управления Запретом`
          }
        >
          <IconShield />
          <span className="header-pill-title">{zapretEngineLabel}</span>
          <span className="header-pill-subtitle">{zapretVersionDisplay}</span>
          {effectiveZapretUpdate && (
            <span
              className="update-pill-badge update-pill-badge-green"
              data-testid="zapret-update-badge"
              title={`Доступна новая версия ${effectiveZapretLatest}`}
            >
              ↑ {(effectiveZapretLatest || '').replace(/^v/, '')}
            </span>
          )}
        </button>

        {/* Кнопка справки и API документации */}
        <button
          type="button"
          className={`header-action-btn ${activeTab === 'help' ? 'active' : ''}`}
          onClick={() => onSwitchTab('help')}
          title="Справка, руководство пользователя и API документация"
          data-testid="header-help-btn"
        >
          <span style={{ fontSize: '15px' }}>❓</span>
        </button>

        <button
          type="button"
          className={`header-action-btn ${activeTab === 'settings' ? 'active' : ''}`}
          onClick={() => onSwitchTab('settings')}
          title="Настройки"
          data-testid="header-settings-btn"
        >
          <span style={{ fontSize: '15px' }}>⚙️</span>
        </button>

        {onOpenEditor && (
          <button
            type="button"
            className="header-action-btn"
            onClick={onOpenEditor}
            title="Редактор конфигов"
          >
            <span style={{ fontSize: '15px' }}>📝</span>
          </button>
        )}

        {onToggleTheme && (
          <button
            type="button"
            className="header-action-btn"
            onClick={onToggleTheme}
            title={theme === 'dark' ? 'Переключить на светлую тему' : 'Переключить на тёмную тему'}
          >
            <span style={{ fontSize: '15px' }}>{theme === 'dark' ? '🌙' : '☀️'}</span>
          </button>
        )}


        {authStatus?.enabled && authStatus?.authenticated && onLogout && (
          <button
            type="button"
            className="header-action-btn"
            onClick={onLogout}
            title="Выйти из панели"
          >
            <span style={{ fontSize: '15px' }}>🚪</span>
          </button>
        )}
      </div>
    </header>
  )
}
