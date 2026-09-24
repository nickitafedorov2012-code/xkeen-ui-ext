import { useState, useEffect } from 'react'
import { apiGet, apiPost } from '../api'
import type { StatusInfo, SystemStats } from '../types'

interface HeaderProps {
  status: StatusInfo | null
  notify: (msg: string, isError?: boolean) => void
  refresh: () => Promise<void>
  onSwitchTab: (tab: 'dashboard' | 'servers' | 'devices' | 'settings' | 'help' | 'google-ai') => void
  theme?: 'dark' | 'light'
  onToggleTheme?: () => void
  onOpenEditor?: () => void
  onOpenUpdateModal?: () => void
  onOpenMihomoModal?: () => void
  authStatus?: { enabled: boolean; authenticated: boolean }
  onLogout?: () => void
}

function StatusWaveform({ isRunning }: { isRunning: boolean }) {
  const color = isRunning
    ? 'rgba(34, 197, 94, 0.28)'
    : 'rgba(239, 68, 68, 0.25)'

  return (
    <svg
      aria-hidden="true"
      className="status-badge-wave"
      viewBox="0 0 200 36"
      preserveAspectRatio="none"
      fill="none"
    >
      {!isRunning ? (
        <line x1="0" y1="18" x2="200" y2="18" stroke={color} strokeWidth="1.5" vectorEffect="non-scaling-stroke" />
      ) : (
        <path
          d="M 0,18 L 22,18 L 26,14.5 L 30,18 L 36,18 L 39,21 L 43,4 L 47,30 L 51,18 L 57,13.5 L 63,18 L 112,18 L 116,14.5 L 120,18 L 126,18 L 129,21 L 133,4 L 137,30 L 141,18 L 147,13.5 L 153,18 L 200,18"
          stroke={color}
          strokeWidth="1.5"
          strokeLinecap="round"
          strokeLinejoin="round"
          vectorEffect="non-scaling-stroke"
        />
      )}
    </svg>
  )
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

function IconSettings() {
  return (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" />
      <circle cx="12" cy="12" r="3" />
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

function IconGauge() {
  return (
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M6.34 17.66l-1.41 1.41M19.07 4.93l-1.41 1.41" />
      <circle cx="12" cy="12" r="3.5" />
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
  theme = 'dark',
  onToggleTheme,
  onOpenEditor,
  onOpenUpdateModal,
  onOpenMihomoModal,
  authStatus,
  onLogout,
}: HeaderProps) {
  const [pending, setPending] = useState(false)
  const [liveMetrics, setLiveMetrics] = useState<SystemStats | null>(null)
  const [updateAvailable, setUpdateAvailable] = useState(false)
  const [latestVersion, setLatestVersion] = useState('')

  const isRunning = status ? Boolean(status.failover?.enabled) : true

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

  const handleRestart = async () => {
    if (pending) return
    setPending(true)
    try {
      const res = await apiPost<{ message: string }>('failover/check')
      notify(res?.message || 'Проверка выполнена')
      await refresh()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка проверки', true)
    } finally {
      setPending(false)
    }
  }

  const handleToggle = async () => {
    if (pending) return
    setPending(true)
    const nextState = !isRunning
    try {
      await apiPost<{ enabled: boolean }>('failover/toggle', { enabled: nextState })
      notify(nextState ? 'Failover запущен' : 'Failover остановлен')
      await refresh()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка переключения', true)
    } finally {
      setPending(false)
    }
  }

  return (
    <header className="header-bar">
      {/* ЛЕВАЯ ЧАСТЬ: Статус сервиса + RAM/CPU + Потребление XKeen Route + Кнопки */}
      <div className="header-left">
        <div className={`status-badge-custom ${isRunning ? 'status-badge-running' : 'status-badge-stopped'}`}>
          <StatusWaveform isRunning={isRunning} />
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
                {memTotal > 0 && (
                  <span className="mini-progress-bar">
                    <span
                      className={`mini-progress-fill ${
                        (memUsed / memTotal) > 0.85
                          ? 'fill-red'
                          : (memUsed / memTotal) > 0.65
                          ? 'fill-yellow'
                          : 'fill-green'
                      }`}
                      style={{ width: `${Math.min(100, Math.round((memUsed / memTotal) * 100))}%` }}
                    />
                  </span>
                )}
              </span>
              <span className="status-stat-sep">|</span>
              <span className="status-stat" title={`Нагрузка на процессор роутера: ${cpuPercent}%`}>
                <IconGauge />
                <span>{cpuPercent}%</span>
                <span className="mini-progress-bar">
                  <span
                    className={`mini-progress-fill ${
                      cpuPercent > 80 ? 'fill-red' : cpuPercent > 45 ? 'fill-yellow' : 'fill-blue'
                    }`}
                    style={{ width: `${Math.min(100, cpuPercent)}%` }}
                  />
                </span>
              </span>
              {(totalXkeenMem > 0 || appMemMb > 0) && (
                <>
                  <span className="status-stat-sep">|</span>
                  <span
                    className="status-stat"
                    title={`Потребление XKeen: всего ${totalXkeenMem > 0 ? totalXkeenMem : appMemMb} МБ RAM (Панель XR: ${appMemMb} МБ, Ядро Mihomo: ${coreMemMb > 0 ? coreMemMb + ' МБ' : '—'}), CPU: ${appCpu}%`}
                  >
                    <span className="status-xr-label">XKeen:</span>
                    <span>{totalXkeenMem > 0 ? totalXkeenMem : appMemMb} МБ</span>
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
            title="Перезапустить failover / обновить"
          >
            <IconRefresh className={pending ? 'spin-icon' : ''} />
          </button>
          <button
            type="button"
            className="header-action-btn"
            onClick={handleToggle}
            disabled={pending}
            title={isRunning ? 'Остановить сервис' : 'Запустить сервис'}
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
          className="header-pill-btn"
          onClick={() => {
            if (onOpenMihomoModal) {
              onOpenMihomoModal()
            } else {
              onSwitchTab('servers')
            }
          }}
          title="Управление ядром Mihomo (версии, релизы, обновление)"
        >
          <IconCpu />
          <span className="header-pill-title">Mihomo</span>
          <span className="header-pill-subtitle">{mihomoVersion}</span>
        </button>

        <button
          type="button"
          className={`header-pill-btn ${updateAvailable ? 'header-pill-update' : ''}`}
          onClick={() => {
            if (updateAvailable && onOpenUpdateModal) {
              onOpenUpdateModal()
            } else {
              onSwitchTab('settings')
            }
          }}
          title={
            updateAvailable
              ? `Доступно обновление до ${latestVersion}! Нажмите для быстрой установки`
              : `Версия XKeen Route: ${appVersion}`
          }
        >
          <IconBox />
          <span className="header-pill-title">{appVersion}</span>
          {updateAvailable && (
            <span className="update-pill-badge" title={`Доступна новая версия ${latestVersion}`}>
              ↑ {latestVersion.replace(/^v/, '')}
            </span>
          )}
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

        <button
          type="button"
          className="header-action-btn"
          onClick={() => onSwitchTab('settings')}
          title="Настройки"
        >
          <IconSettings />
        </button>

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
