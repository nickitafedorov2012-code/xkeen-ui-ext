import { useCallback, useEffect, useState } from 'react'
import Dashboard from './components/Dashboard'
import Devices from './components/Devices'
import Servers from './components/Servers'
import Settings from './components/Settings'
import { apiGet } from './api'
import type { StatusInfo } from './types'

type TabId = 'dashboard' | 'servers' | 'devices' | 'settings'

const TABS: { id: TabId; label: string }[] = [
  { id: 'dashboard', label: '📊 Дашборд' },
  { id: 'servers', label: '🛰 Серверы' },
  { id: 'devices', label: '📱 Устройства' },
  { id: 'settings', label: '⚙️ Настройки' },
]

interface Toast {
  id: number
  msg: string
  error: boolean
}

let toastSeq = 1

export default function App() {
  const initial = (() => {
    const h = window.location.hash.replace('#', '')
    return (TABS.some((t) => t.id === h) ? h : 'dashboard') as TabId
  })()
  const [tab, setTab] = useState<TabId>(initial)
  const [status, setStatus] = useState<StatusInfo | null>(null)
  const [toasts, setToasts] = useState<Toast[]>([])

  const switchTab = (t: TabId) => {
    setTab(t)
    history.replaceState(null, '', '#' + t)
  }

  const notify = useCallback((msg: string, error = false) => {
    const id = toastSeq++
    setToasts((prev) => [...prev, { id, msg, error }])
    setTimeout(() => setToasts((prev) => prev.filter((t) => t.id !== id)), 5000)
  }, [])

  const refresh = useCallback(async () => {
    try {
      const data = await apiGet<StatusInfo>('status')
      setStatus(data)
    } catch {
      /* роутер недоступен — оставляем прошлые данные */
    }
  }, [])

  useEffect(() => {
    refresh()
    const interval = setInterval(refresh, (status?.refresh_interval_sec ?? 10) * 1000)
    return () => clearInterval(interval)
  }, [refresh, status?.refresh_interval_sec])

  return (
    <div className="app">
      <header className="header">
        <div className="header-title">
          <span className="logo">🛣️</span>
          <div>
            <h1>XKeen Route</h1>
            <p className="subtitle">
              {status
                ? `панель ${status.version}${status.router?.model ? ` · ${status.router.model}` : ''}`
                : 'подключение…'}
            </p>
          </div>
        </div>
        {status?.active_server && (
          <div className="active-pill">
            {status.active_server.name}
            <span className="muted"> · {status.active_server.ping_ms > 0 ? `${status.active_server.ping_ms} мс` : '—'}</span>
          </div>
        )}
      </header>

      <nav className="tabs">
        {TABS.map((t) => (
          <button key={t.id} className={'tab' + (tab === t.id ? ' active' : '')} onClick={() => switchTab(t.id)}>
            {t.label}
          </button>
        ))}
      </nav>

      <main className="content">
        {tab === 'dashboard' && <Dashboard status={status} notify={notify} />}
        {tab === 'servers' && <Servers notify={notify} />}
        {tab === 'devices' && <Devices notify={notify} />}
        {tab === 'settings' && <Settings notify={notify} />}
      </main>

      <div className="toasts">
        {toasts.map((t) => (
          <div key={t.id} className={'toast' + (t.error ? ' err' : '')}>{t.msg}</div>
        ))}
      </div>
    </div>
  )
}

