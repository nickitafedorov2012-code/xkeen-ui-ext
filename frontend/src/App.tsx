import { useCallback, useEffect, useState } from 'react'
import React from 'react'
import Dashboard from './components/Dashboard'
import Devices from './components/Devices'
import Servers from './components/Servers'
import Settings from './components/Settings'
import Help from './components/Help'
import Header from './components/Header'
import Antigravity from './components/Antigravity'
import LoginModal from './components/LoginModal'
import ConfigEditor from './components/ConfigEditor'
import ConnectionsViewer from './components/ConnectionsViewer'
import RulesViewer from './components/RulesViewer'
import Diagnostics from './components/Diagnostics'
import UpdateModal from './components/UpdateModal'
import { apiGet, apiPost } from './api'
import type { AuthStatus, StatusInfo } from './types'

type TabId =
  | 'dashboard'
  | 'servers'
  | 'devices'
  | 'connections'
  | 'rules'
  | 'diagnostics'
  | 'google-ai'
  | 'settings'
  | 'help'
  | 'antigravity'

const TABS: { id: TabId; label: string }[] = [
  { id: 'dashboard', label: '📊 Дашборд' },
  { id: 'servers', label: '🛰 Серверы' },
  { id: 'devices', label: '📱 Устройства' },
  { id: 'connections', label: '🌐 Соединения' },
  { id: 'rules', label: '📋 Правила' },
  { id: 'diagnostics', label: '🩺 Диагностика' },
  { id: 'google-ai', label: '🤖 Google AI' },
  { id: 'settings', label: '⚙️ Настройки' },
  { id: 'help', label: '📖 Справка' },
]

interface Toast {
  id: number
  msg: string
  error: boolean
}

let toastSeq = 1

class ErrorBoundary extends React.Component<
  { children: React.ReactNode },
  { error: Error | null }
> {
  state = { error: null as Error | null }
  static getDerivedStateFromError(error: Error) {
    return { error }
  }
  render() {
    if (this.state.error) {
      return (
        <section className="card">
          <h2>⚠️ Ошибка интерфейса</h2>
          <p className="muted">{this.state.error.message}</p>
          <button className="btn" onClick={() => this.setState({ error: null })}>Перезагрузить</button>
        </section>
      )
    }
    return this.props.children
  }
}

export default function App() {
  const initial = (() => {
    const h = window.location.hash.replace('#', '')
    if (h === 'antigravity') return 'google-ai'
    return (TABS.some((t) => t.id === h) ? h : 'dashboard') as TabId
  })()
  const [tab, setTab] = useState<TabId>(initial)
  const [status, setStatus] = useState<StatusInfo | null>(null)
  const [connected, setConnected] = useState(true)
  const [toasts, setToasts] = useState<Toast[]>([])

  // Авторизация
  const [authStatus, setAuthStatus] = useState<AuthStatus>({ enabled: false, authenticated: true })

  // Тема (Dark / Light)
  const [theme, setTheme] = useState<'dark' | 'light'>(() => {
    return (localStorage.getItem('xr_theme') as 'dark' | 'light') || 'dark'
  })

  // Глобальный редактор конфигов
  const [globalEditorOpen, setGlobalEditorOpen] = useState(false)

  // Обновление XKeen Route
  const [updateModalOpen, setUpdateModalOpen] = useState(false)
  const [updateInfo, setUpdateInfo] = useState<{ current: string; latest: string; notes: string[]; update_available: boolean } | null>(null)

  useEffect(() => {
    let active = true
    const checkUpd = async () => {
      try {
        const res = await apiGet<{ current: string; latest: string; notes?: string[]; update_available: boolean }>('update/check')
        if (active && res) {
          setUpdateInfo({
            current: res.current,
            latest: res.latest,
            notes: res.notes || [],
            update_available: Boolean(res.update_available),
          })
        }
      } catch {
        /* игнорируем ошибку сети */
      }
    }
    checkUpd()
    const timer = setInterval(checkUpd, 60_000)
    return () => {
      active = false
      clearInterval(timer)
    }
  }, [])

  // Применение темы
  useEffect(() => {
    document.documentElement.setAttribute('data-theme', theme)
    localStorage.setItem('xr_theme', theme)
  }, [theme])

  const checkAuth = useCallback(async () => {
    try {
      const res = await apiGet<AuthStatus>('auth/status')
      setAuthStatus(res)
    } catch {
      /* при ошибке проверки не блокируем */
    }
  }, [])

  useEffect(() => {
    checkAuth()
    const handleAuthRequired = () => {
      setAuthStatus((prev) => ({ ...prev, enabled: true, authenticated: false }))
    }
    window.addEventListener('xr:auth-required', handleAuthRequired)
    return () => window.removeEventListener('xr:auth-required', handleAuthRequired)
  }, [checkAuth])

  // Синхронизация активной вкладки при навигации назад/вперед в браузере
  useEffect(() => {
    const handleLocationChange = () => {
      const h = window.location.hash.replace('#', '')
      const targetTab = (h === 'antigravity' ? 'google-ai' : h) as TabId
      if (TABS.some((t) => t.id === targetTab)) {
        setTab(targetTab)
      }
    }
    window.addEventListener('popstate', handleLocationChange)
    window.addEventListener('hashchange', handleLocationChange)
    return () => {
      window.removeEventListener('popstate', handleLocationChange)
      window.removeEventListener('hashchange', handleLocationChange)
    }
  }, [])

  const handleLogout = async () => {
    try {
      await apiPost('auth/logout')
    } catch (err) {
      console.warn('Ошибка при вызове logout API:', err)
    }
    setAuthStatus((prev) => ({ ...prev, authenticated: false }))
    notify('Вы вышли из веб-панели')
  }

  const switchTab = (t: TabId) => {
    setTab(t)
    if (window.location.hash !== '#' + t) {
      window.location.hash = t
    }
    if (t === 'dashboard' || t === 'settings') {
      refresh()
    }
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
      setConnected(true)
    } catch {
      /* роутер недоступен — оставляем прошлые данные, но показываем плашку */
      setConnected(false)
    }
  }, [])

  useEffect(() => {
    refresh()
    const interval = setInterval(refresh, (status?.refresh_interval_sec ?? 10) * 1000)
    return () => clearInterval(interval)
  }, [refresh, status?.refresh_interval_sec])

  // Глобальные горячие клавиши (Ctrl+K, Ctrl+P, Alt+R)
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement
      const isInput = target && (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable)

      // Ctrl+K (или Cmd+K): мгновенный переход к поиску серверов
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault()
        switchTab('servers')
        setTimeout(() => {
          window.dispatchEvent(new CustomEvent('xr:focus-search'))
        }, 50)
      }
      // Ctrl+P (или Cmd+P): пинг всех серверов
      else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'p') {
        e.preventDefault()
        switchTab('servers')
        setTimeout(() => {
          window.dispatchEvent(new CustomEvent('xr:ping-all'))
        }, 50)
      }
      // Alt+R или Shift+R: быстрое обновление панели (когда не в инпуте)
      else if (!isInput && ((e.altKey && e.key.toLowerCase() === 'r') || (e.shiftKey && e.key === 'R'))) {
        e.preventDefault()
        refresh()
        notify('Данные обновлены')
      }
    }
    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [refresh, notify])

  return (
    <div className="app">
      <ErrorBoundary>
        <Header
          status={status}
          notify={notify}
          refresh={refresh}
          onSwitchTab={switchTab}
          theme={theme}
          onToggleTheme={() => setTheme((prev) => (prev === 'dark' ? 'light' : 'dark'))}
          onOpenEditor={() => setGlobalEditorOpen(true)}
          onOpenUpdateModal={() => setUpdateModalOpen(true)}
          authStatus={authStatus}
          onLogout={handleLogout}
        />
      </ErrorBoundary>

      <nav className="tabs">
        {TABS.map((t) => (
          <button key={t.id} className={'tab' + (tab === t.id ? ' active' : '')} onClick={() => switchTab(t.id)}>
            {t.label}
          </button>
        ))}
      </nav>

      {!connected && (
        <div
          style={{
            background: 'var(--warn-bg, #4a3b12)',
            color: 'var(--warn-fg, #ffd54f)',
            padding: '8px 16px',
            textAlign: 'center',
            fontWeight: 600,
          }}
        >
          ⚠ Нет связи с роутером — переподключение…
        </div>
      )}

      <main className="content">
        <ErrorBoundary>
          {tab === 'dashboard' && <Dashboard status={status} notify={notify} refresh={refresh} onSwitchTab={switchTab} />}
          {tab === 'servers' && <Servers notify={notify} />}
          {tab === 'devices' && <Devices notify={notify} />}
          {tab === 'connections' && <ConnectionsViewer notify={notify} />}
          {tab === 'rules' && <RulesViewer notify={notify} />}
          {tab === 'diagnostics' && <Diagnostics notify={notify} />}
          {(tab === 'google-ai' || tab === 'antigravity') && <Antigravity notify={notify} />}
          {tab === 'settings' && <Settings notify={notify} status={status} refresh={refresh} />}
          {tab === 'help' && <Help status={status} />}
        </ErrorBoundary>
      </main>

      <div className="toasts">
        {toasts.map((t) => (
          <div key={t.id} className={'toast' + (t.error ? ' err' : '')}>
            <span className="toast-icon">{t.error ? '❌' : '✅'}</span>
            <span>{t.msg}</span>
          </div>
        ))}
      </div>

      {/* Модальное окно авторизации */}
      <LoginModal
        isOpen={authStatus.enabled && !authStatus.authenticated}
        onSuccess={() => {
          checkAuth()
          refresh()
          notify('Успешная авторизация')
        }}
      />

      {/* Глобальное модальное окно редактора конфигов */}
      <ConfigEditor
        isOpen={globalEditorOpen}
        onClose={() => setGlobalEditorOpen(false)}
        notify={notify}
      />

      {/* Модальное окно 1-клик обновления XKeen Route */}
      <UpdateModal
        isOpen={updateModalOpen}
        onClose={() => setUpdateModalOpen(false)}
        currentVersion={updateInfo?.current || status?.version || ''}
        latestVersion={updateInfo?.latest || ''}
        notes={updateInfo?.notes || []}
        notify={notify}
      />
    </div>
  )
}


