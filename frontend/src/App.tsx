import { useCallback, useEffect, useRef, useState, lazy, Suspense } from 'react'
import React from 'react'
import Dashboard from './components/Dashboard'
import Header from './components/Header'
import LoginModal from './components/LoginModal'
import { confirmDiscardUnsaved, useHasUnsavedChanges } from './utils/dirtyGuard'

// 12 Secondary Tabs (Code Splitting via React.lazy)
const Servers = lazy(() => import('./components/Servers'))
const Devices = lazy(() => import('./components/Devices'))
const Whitelist = lazy(() => import('./components/Whitelist'))
const Zapret = lazy(() => import('./components/Zapret'))
const VkTurn = lazy(() => import('./components/VkTurn'))
const Gaming = lazy(() => import('./components/Gaming'))
const Antigravity = lazy(() => import('./components/Antigravity'))
const ConnectionsViewer = lazy(() => import('./components/ConnectionsViewer'))
const RulesViewer = lazy(() => import('./components/RulesViewer'))
const Diagnostics = lazy(() => import('./components/Diagnostics'))
const Settings = lazy(() => import('./components/Settings'))
const AdGuard = lazy(() => import('./components/AdGuard'))
const Help = lazy(() => import('./components/Help'))

// 4 Heavy Modals (Code Splitting via React.lazy)
const ConfigEditor = lazy(() => import('./components/ConfigEditor'))
const UpdateModal = lazy(() => import('./components/UpdateModal'))
const MihomoCoreModal = lazy(() => import('./components/MihomoCoreModal'))
const ZapretCoreModal = lazy(() => import('./components/ZapretCoreModal'))

import { apiGet, apiPost } from './api'
import type { AuthStatus, StatusInfo } from './types'
import { useUpdateCheck } from './hooks/useUpdateCheck'
import { tabFromHash, type TabId } from './utils/tabHash'

const TABS: { id: TabId; label: string }[] = [
  { id: 'dashboard', label: '📊 Дашборд' },
  { id: 'servers', label: '🛰 Серверы' },
  { id: 'devices', label: '📱 Устройства' },
  { id: 'whitelist', label: '🛡️ Белые списки' },
  { id: 'zapret', label: '⚡ Zapret' },
  { id: 'gaming', label: '🎮 Игры' },
  { id: 'google-ai', label: '🤖 Google AI' },
  { id: 'adguard', label: '🛡️ AdGuard' },
  { id: 'connections', label: '🌐 Соединения' },
  { id: 'rules', label: '📋 Правила' },
  { id: 'diagnostics', label: '🩺 Диагностика' },
]

interface Toast {
  id: number
  msg: string
  error: boolean
  exiting?: boolean
}

const TOAST_VISIBLE_MS = 4780
const TOAST_EXIT_MS = 220

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

export function TabLoadingFallback() {
  return (
    <div className="tab-skeleton" role="status" aria-label="Загрузка раздела">
      <div className="tab-skeleton-line wide" />
      <div className="tab-skeleton-line" />
      <div className="tab-skeleton-line short" />
    </div>
  )
}

export default function App() {
  const initial = tabFromHash(window.location.hash) ?? 'dashboard'
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
  const { updateInfo } = useUpdateCheck()

  // Управление ядром Mihomo (релизы и обновление)
  const [mihomoModalOpen, setMihomoModalOpen] = useState(false)
  // Управление ядром Zapret (DPI)
  const [zapretModalOpen, setZapretModalOpen] = useState(false)

  useEffect(() => {
    const handleOpenMihomo = () => setMihomoModalOpen(true)
    const handleOpenUpdate = () => setUpdateModalOpen(true)
    const handleOpenZapret = () => setZapretModalOpen(true)
    window.addEventListener('xr:open-mihomo-modal', handleOpenMihomo)
    window.addEventListener('xr:open-update-modal', handleOpenUpdate)
    window.addEventListener('xr:open-zapret-modal', handleOpenZapret)
    return () => {
      window.removeEventListener('xr:open-mihomo-modal', handleOpenMihomo)
      window.removeEventListener('xr:open-update-modal', handleOpenUpdate)
      window.removeEventListener('xr:open-zapret-modal', handleOpenZapret)
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

  // Предупреждение браузера при закрытии/обновлении вкладки с несохранёнными правками
  const hasUnsaved = useHasUnsavedChanges()
  useEffect(() => {
    if (!hasUnsaved) return
    const handleBeforeUnload = (e: BeforeUnloadEvent) => {
      e.preventDefault()
      e.returnValue = ''
    }
    window.addEventListener('beforeunload', handleBeforeUnload)
    return () => window.removeEventListener('beforeunload', handleBeforeUnload)
  }, [hasUnsaved])

  // Синхронизация активной вкладки при навигации назад/вперед в браузере
  useEffect(() => {
    const handleLocationChange = () => {
      const targetTab = tabFromHash(window.location.hash)
      if (targetTab && targetTab !== tab) {
        if (!confirmDiscardUnsaved()) {
          window.history.replaceState(null, '', '#' + tab)
          return
        }
        setTab(targetTab)
      }
    }
    window.addEventListener('popstate', handleLocationChange)
    window.addEventListener('hashchange', handleLocationChange)
    return () => {
      window.removeEventListener('popstate', handleLocationChange)
      window.removeEventListener('hashchange', handleLocationChange)
    }
  }, [tab])

  const handleLogout = async () => {
    try {
      await apiPost('auth/logout')
    } catch (err) {
      console.warn('Ошибка при вызове logout API:', err)
    }
    setAuthStatus((prev) => ({ ...prev, authenticated: false }))
    notify('Вы вышли из веб-панели')
  }

  const notify = useCallback((msg: string, error = false) => {
    const id = toastSeq++
    setToasts((prev) => [...prev, { id, msg, error }])
    setTimeout(() => {
      setToasts((prev) => prev.map((t) => (t.id === id ? { ...t, exiting: true } : t)))
      setTimeout(() => {
        setToasts((prev) => prev.filter((t) => t.id !== id))
      }, TOAST_EXIT_MS)
    }, TOAST_VISIBLE_MS)
  }, [])

  // FE-01: защита поллинга — не допускаем наложение запросов и опрос в фоновой вкладке
  const refreshInFlight = useRef(false)
  const pendingRefresh = useRef(false)
  const refresh = useCallback(async (force = false) => {
    if (refreshInFlight.current) {
      if (force) {
        pendingRefresh.current = true
      }
      return
    }
    if (!force && document.hidden) return
    refreshInFlight.current = true
    try {
      const data = await apiGet<StatusInfo>('status')
      setStatus(data)
      setConnected(true)
    } catch {
      /* роутер недоступен — оставляем прошлые данные, но показываем плашку */
      setConnected(false)
    } finally {
      refreshInFlight.current = false
      if (pendingRefresh.current) {
        pendingRefresh.current = false
        void refresh(true)
      }
    }
  }, [])

  const switchTab = useCallback((t: TabId) => {
    if (t !== tab && !confirmDiscardUnsaved()) return
    setTab(t)
    if (window.location.hash !== '#' + t) {
      window.location.hash = t
    }
    if (t === 'dashboard' || t === 'settings') {
      refresh(true)
    }
  }, [refresh, tab])

  useEffect(() => {
    refresh()
    const intervalMs = Math.max(3, status?.refresh_interval_sec ?? 3) * 1000
    const interval = setInterval(() => refresh(false), intervalMs)
    return () => clearInterval(interval)
  }, [refresh, status?.refresh_interval_sec])

  useEffect(() => {
    const handleRefreshStatus = () => {
      refresh(true)
    }
    window.addEventListener('xr:refresh-status', handleRefreshStatus)
    return () => window.removeEventListener('xr:refresh-status', handleRefreshStatus)
  }, [refresh])

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
      // Ctrl+, (или Cmd+,): быстрый переход в настройки
      else if ((e.ctrlKey || e.metaKey) && e.key === ',') {
        e.preventDefault()
        switchTab('settings')
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
  }, [refresh, notify, switchTab])

  // Слушатель смены вкладки по событию xr:switch-tab
  useEffect(() => {
    const handleSwitchTab = (e: Event) => {
      const ce = e as CustomEvent<TabId>
      const target = ce.detail as TabId
      if (target && (TABS.some((t) => t.id === target) || target === 'settings' || target === 'help' || target === 'vkturn')) {
        switchTab(target)
      }
    }
    window.addEventListener('xr:switch-tab', handleSwitchTab)
    return () => window.removeEventListener('xr:switch-tab', handleSwitchTab)
  }, [switchTab])

  return (
    <div className="app">
      <ErrorBoundary>
        <Header
          status={status}
          notify={notify}
          refresh={refresh}
          onSwitchTab={(t) => switchTab(t as TabId)}
          activeTab={tab}
          theme={theme}
          onToggleTheme={() => setTheme((prev) => (prev === 'dark' ? 'light' : 'dark'))}
          onOpenEditor={() => setGlobalEditorOpen(true)}
          onOpenUpdateModal={() => setUpdateModalOpen(true)}
          onOpenMihomoModal={() => setMihomoModalOpen(true)}
          onOpenZapretModal={() => setZapretModalOpen(true)}
          authStatus={authStatus}
          onLogout={handleLogout}
        />
      </ErrorBoundary>

      <nav className="tabs">
        {TABS.map((t) => (
          <button
            key={t.id}
            className={'tab' + (tab === t.id || (tab === 'vkturn' && t.id === 'whitelist') ? ' active' : '')}
            onClick={() => switchTab(t.id)}
          >
            {t.label}
          </button>
        ))}
      </nav>

      {/* FE-03: бета-баннер убран — вводил в заблуждение в релизных версиях */}

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
        <ErrorBoundary key={tab}>
          <Suspense fallback={<TabLoadingFallback />}>
            {tab === 'dashboard' && <Dashboard status={status} notify={notify} refresh={refresh} onSwitchTab={switchTab} />}
            {tab === 'servers' && <Servers notify={notify} status={status} refresh={refresh} />}
            {tab === 'devices' && <Devices notify={notify} />}
            {(tab === 'whitelist' || tab === 'vkturn') && (
              <div className="whitelist-workspace">
                <section className="whitelist-workspace-header" aria-label="Разделы белых списков">
                  <div className="whitelist-workspace-intro">
                    <h2>🛡️ Белые списки</h2>
                    <p>Настройте работу сети и подключение через звонки ВКонтакте.</p>
                  </div>
                  <nav className="whitelist-workspace-nav" aria-label="Настройки белых списков">
                    <button type="button" className="whitelist-workspace-tab" aria-current={tab === 'whitelist' ? 'page' : undefined} onClick={() => switchTab('whitelist')}>
                      <span>🛡️ Режим и устройства</span><small>Маршруты, исключения и домены</small>
                    </button>
                    <button type="button" className="whitelist-workspace-tab" aria-current={tab === 'vkturn' ? 'page' : undefined} onClick={() => switchTab('vkturn')}>
                      <span>📞 Звонки ВКонтакте</span><small>Подключение и состояние туннеля</small>
                    </button>
                  </nav>
                </section>
                {tab === 'whitelist' ? <Whitelist notify={notify} status={status} refresh={refresh} /> : <VkTurn notify={notify} />}
              </div>
            )}
            {tab === 'zapret' && <Zapret notify={notify} />}
            {(tab === 'gaming' || tab === 'xbox-dns') && <Gaming notify={notify} />}
            {(tab === 'google-ai' || tab === 'antigravity') && <Antigravity notify={notify} />}
            {tab === 'connections' && <ConnectionsViewer notify={notify} />}
            {tab === 'rules' && <RulesViewer notify={notify} />}
            {tab === 'diagnostics' && <Diagnostics notify={notify} />}
            {tab === 'settings' && <Settings notify={notify} status={status} refresh={refresh} />}
            {tab === 'adguard' && <AdGuard notify={notify} />}
            {tab === 'help' && <Help status={status} />}
          </Suspense>
        </ErrorBoundary>
      </main>

      <div className="toasts">
        {toasts.map((t) => (
          <div key={t.id} className={'toast' + (t.error ? ' err' : '') + (t.exiting ? ' toast-exit' : '')}>
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
      {globalEditorOpen && (
        <Suspense fallback={null}>
          <ConfigEditor
            isOpen={globalEditorOpen}
            onClose={() => setGlobalEditorOpen(false)}
            notify={notify}
          />
        </Suspense>
      )}

      {/* Модальное окно 1-клик обновления XKeen Route */}
      {updateModalOpen && (
        <Suspense fallback={null}>
          <UpdateModal
            isOpen={updateModalOpen}
            onClose={() => setUpdateModalOpen(false)}
            currentVersion={updateInfo?.current || status?.version || ''}
            latestVersion={updateInfo?.latest || ''}
            notes={updateInfo?.notes || []}
            notify={notify}
          />
        </Suspense>
      )}

      {/* Модальное окно управления и обновления ядра Mihomo */}
      {mihomoModalOpen && (
        <Suspense fallback={null}>
          <MihomoCoreModal
            isOpen={mihomoModalOpen}
            onClose={() => setMihomoModalOpen(false)}
            notify={notify}
            onUpdated={refresh}
          />
        </Suspense>
      )}

      {/* Модальное окно управления и выбора ядра Zapret (DPI) */}
      {zapretModalOpen && (
        <Suspense fallback={null}>
          <ZapretCoreModal
            isOpen={zapretModalOpen}
            onClose={() => setZapretModalOpen(false)}
            notify={notify}
            onUpdated={refresh}
          />
        </Suspense>
      )}
    </div>
  )
}


