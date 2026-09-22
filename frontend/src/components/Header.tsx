import { useState, useEffect } from 'react'
import { apiGet, apiPost } from '../api'
import type { StatusInfo, SystemStats } from '../types'

interface HeaderProps {
  status: StatusInfo | null
  notify: (msg: string, isError?: boolean) => void
  refresh: () => Promise<void>
  onSwitchTab: (tab: 'dashboard' | 'servers' | 'devices' | 'settings' | 'help') => void
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

export default function Header({ status, notify, refresh, onSwitchTab }: HeaderProps) {
  const [pending, setPending] = useState(false)
  const [liveMetrics, setLiveMetrics] = useState<SystemStats | null>(null)

  const isRunning = status ? Boolean(status.failover?.enabled) : true

  // Периодическое обновление метрик каждую секунду (1 сек)
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
    const timer = setInterval(fetchMetrics, 1000)
    return () => {
      active = false
      clearInterval(timer)
    }
  }, [])

  // Память и CPU (живые из 1-секундного таймера либо из статуса)
  const currentMetrics = liveMetrics || status?.system
  const memUsed = currentMetrics?.memory_used_mb ?? 348
  const memTotal = currentMetrics?.memory_total_mb ?? 512
  const cpuPercent = currentMetrics?.cpu_percent ?? 0
  const appMemMb = currentMetrics?.app_memory_mb ?? 0
  const appCpu = currentMetrics?.app_cpu_percent ?? 0

  const mihomoVersion = status?.mihomo_version || 'v1.19.29'
  const appVersion = status?.version ? status.version.replace(/^v/, '') : '1.0.17'

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
              <span className="status-stat" title="Оперативная память роутера">
                <IconDisk />
                <span>{memUsed}/{memTotal} МБ</span>
              </span>
              <span className="status-stat-sep">|</span>
              <span className="status-stat" title="Нагрузка на процессор роутера">
                <IconGauge />
                <span>{cpuPercent}%</span>
              </span>
              {appMemMb > 0 && (
                <>
                  <span className="status-stat-sep">|</span>
                  <span
                    className="status-stat"
                    title={`Потребление процесса XKeen Route: ${appMemMb} МБ RAM (RSS), CPU: ${appCpu}%`}
                  >
                    <span className="status-xr-label">XR:</span>
                    <span>{appMemMb} МБ</span>
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
          <span className="header-brand-text">XKeen Route</span>
        </a>
      </div>

      {/* ПРАВАЯ ЧАСТЬ: Чип Mihomo + Чип Версии + Кнопка Настроек */}
      <div className="header-right">
        <button
          type="button"
          className="header-pill-btn"
          onClick={() => onSwitchTab('servers')}
          title="Управление ядром / серверы"
        >
          <IconCpu />
          <span className="header-pill-title">Mihomo</span>
          <span className="header-pill-subtitle">{mihomoVersion}</span>
        </button>

        <button
          type="button"
          className="header-pill-btn"
          onClick={() => onSwitchTab('dashboard')}
          title="Версия XKeen Route"
        >
          <IconBox />
          <span className="header-pill-title">{appVersion}</span>
        </button>

        <button
          type="button"
          className="header-action-btn"
          onClick={() => onSwitchTab('settings')}
          title="Настройки"
        >
          <IconSettings />
        </button>
      </div>
    </header>
  )
}
