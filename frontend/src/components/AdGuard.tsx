import { useState, useEffect, useCallback, useMemo } from 'react'
import { apiGet, apiPost } from '../api'
import type {
  AdGuardConfig,
  AghStatus,
  AghHealth,
  AghOverview,
  AghQueryLogItem,
  AghFilterSubscription,
  AghRewriteEntry,
  AghDiagnostics,
} from '../types'

interface AdGuardProps {
  notify: (msg: string, error?: boolean) => void
}

type SubTab = 'overview' | 'querylog' | 'filtering' | 'rewrites' | 'diagnostics' | 'settings'

export default function AdGuard({ notify }: AdGuardProps) {
  const [activeSubTab, setActiveSubTab] = useState<SubTab>('overview')
  const [loading, setLoading] = useState(true)
  const [actionPending, setActionPending] = useState(false)

  // Данные AdGuard Home
  const [status, setStatus] = useState<AghStatus | null>(null)
  const [health, setHealth] = useState<AghHealth | null>(null)
  const [overview, setOverview] = useState<AghOverview | null>(null)
  const [config, setConfig] = useState<AdGuardConfig | null>(null)
  const [diagnostics, setDiagnostics] = useState<AghDiagnostics | null>(null)

  // Журнал запросов
  const [queryLog, setQueryLog] = useState<AghQueryLogItem[]>([])
  const [logSearch, setLogSearch] = useState('')
  const [logStatusFilter, setLogStatusFilter] = useState('')
  const [logLoading, setLogLoading] = useState(false)

  // Фильтры и правила
  const [filters, setFilters] = useState<AghFilterSubscription[]>([])
  const [userRulesText, setUserRulesText] = useState('')
  const [rulesSaving, setRulesSaving] = useState(false)

  // DNS Rewrites
  const [rewrites, setRewrites] = useState<AghRewriteEntry[]>([])
  const [newRewriteDomain, setNewRewriteDomain] = useState('')
  const [newRewriteAnswer, setNewRewriteAnswer] = useState('')
  const [rewriteAdding, setRewriteAdding] = useState(false)

  // Форма настроек
  const [settingsForm, setSettingsForm] = useState<AdGuardConfig>({
    enabled: true,
    host: '127.0.0.1',
    http_port: 3000,
    dns_port: 53,
    username: '',
    password: '',
    integration_mode: 'managed',
    upstream_dns: ['tls://1.1.1.1', 'tls://8.8.8.8'],
    failsafe_rollback: true,
    config_path: '/opt/etc/AdGuardHome.yaml',
  })

  // Загрузка первичных данных
  const loadData = useCallback(async () => {
    setLoading(true)
    try {
      const [st, hl, ov, cfg, diag] = await Promise.all([
        apiGet<AghStatus>('/adguard/status').catch(() => null),
        apiGet<AghHealth>('/adguard/health').catch(() => null),
        apiGet<AghOverview>('/adguard/overview').catch(() => null),
        apiGet<AdGuardConfig>('/adguard/config').catch(() => null),
        apiGet<AghDiagnostics>('/adguard/diagnostics').catch(() => null),
      ])

      if (st) setStatus(st)
      if (hl) setHealth(hl)
      if (ov) setOverview(ov)
      if (cfg) {
        setConfig(cfg)
        setSettingsForm(cfg)
      }
      if (diag) setDiagnostics(diag)
    } catch (e: any) {
      notify(`Ошибка загрузки данных AdGuard Home: ${e.message || e}`, true)
    } finally {
      setLoading(false)
    }
  }, [notify])

  useEffect(() => {
    loadData()
    const timer = setInterval(loadData, 10000)
    return () => clearInterval(timer)
  }, [loadData])

  // Загрузка журнала запросов
  const loadQueryLog = useCallback(async () => {
    setLogLoading(true)
    try {
      const params = new URLSearchParams()
      params.set('limit', '50')
      if (logSearch.trim()) params.set('search', logSearch.trim())
      if (logStatusFilter) params.set('response_status', logStatusFilter)

      const res = await apiGet<{ data: AghQueryLogItem[] }>(`/adguard/query-log?${params.toString()}`)
      setQueryLog(res?.data || [])
    } catch (e: any) {
      notify(`Не удалось загрузить журнал запросов: ${e.message || e}`, true)
    } finally {
      setLogLoading(false)
    }
  }, [logSearch, logStatusFilter, notify])

  useEffect(() => {
    if (activeSubTab === 'querylog' && status?.running) {
      loadQueryLog()
    }
  }, [activeSubTab, loadQueryLog, status?.running])

  // Загрузка фильтрации
  const loadFiltering = useCallback(async () => {
    try {
      const res = await apiGet<{ filters: AghFilterSubscription[]; user_rules: string[] }>('/adguard/filtering')
      if (res?.filters) setFilters(res.filters)
      if (res?.user_rules) setUserRulesText(res.user_rules.join('\n'))
    } catch (e: any) {
      notify(`Ошибка загрузки фильтров: ${e.message || e}`, true)
    }
  }, [notify])

  // Загрузка DNS Rewrites
  const loadRewrites = useCallback(async () => {
    try {
      const res = await apiGet<AghRewriteEntry[]>('/adguard/rewrites')
      if (Array.isArray(res)) setRewrites(res)
    } catch (e: any) {
      notify(`Ошибка загрузки DNS переопределений: ${e.message || e}`, true)
    }
  }, [notify])

  useEffect(() => {
    if (activeSubTab === 'filtering' && status?.running) loadFiltering()
    if (activeSubTab === 'rewrites' && status?.running) loadRewrites()
  }, [activeSubTab, loadFiltering, loadRewrites, status?.running])

  // Переключение защиты (Protection Toggle)
  const handleToggleProtection = async () => {
    if (!status || actionPending) return
    const nextState = !status.protection_enabled
    setActionPending(true)
    try {
      await apiPost<{ enabled: boolean }>('/adguard/protection', { enabled: nextState })
      setStatus({ ...status, protection_enabled: nextState })
      notify(nextState ? 'Защита AdGuard Home включена' : 'Защита AdGuard Home временно отключена')
      await loadData()
    } catch (e: any) {
      notify(`Не удалось изменить статус защиты: ${e.message || e}`, true)
    } finally {
      setActionPending(false)
    }
  }

  // Управление службой (Restart / Start / Stop)
  const handleServiceAction = async (action: 'start' | 'stop' | 'restart') => {
    setActionPending(true)
    try {
      const res = await apiPost<{ output: string }>('/adguard/service', { action })
      notify(`Служба AdGuard Home: ${action} (${res.output || 'успешно'})`)
      await new Promise((r) => setTimeout(r, 2000))
      await loadData()
    } catch (e: any) {
      notify(`Ошибка управления службой AdGuard Home: ${e.message || e}`, true)
    } finally {
      setActionPending(false)
    }
  }

  // Сохранение пользовательских правил
  const handleSaveRules = async () => {
    setRulesSaving(true)
    try {
      const rules = userRulesText
        .split('\n')
        .map((s) => s.trim())
        .filter((s) => s.length > 0)

      await apiPost('/adguard/filtering/rules', { rules })
      notify(`Пользовательские правила сохранены (правил: ${rules.length})`)
      await loadFiltering()
    } catch (e: any) {
      notify(`Ошибка сохранения правил: ${e.message || e}`, true)
    } finally {
      setRulesSaving(false)
    }
  }

  // Переключение подписки фильтрации
  const handleToggleFilter = async (url: string, currentEnabled: boolean) => {
    try {
      await apiPost('/adguard/filtering/toggle-filter', {
        url,
        enabled: !currentEnabled,
        whitelist: false,
      })
      notify(`Список фильтров ${!currentEnabled ? 'включен' : 'отключен'}`)
      await loadFiltering()
    } catch (e: any) {
      notify(`Ошибка переключения фильтра: ${e.message || e}`, true)
    }
  }

  // Добавление DNS Rewrite
  const handleAddRewrite = async (e: React.FormEvent) => {
    e.preventDefault()
    if (!newRewriteDomain.trim() || !newRewriteAnswer.trim()) {
      notify('Укажите домен и ответ для переопределения', true)
      return
    }

    setRewriteAdding(true)
    try {
      await apiPost('/adguard/rewrites/add', {
        domain: newRewriteDomain.trim(),
        answer: newRewriteAnswer.trim(),
      })
      notify(`DNS Rewrite добавлен: ${newRewriteDomain} -> ${newRewriteAnswer}`)
      setNewRewriteDomain('')
      setNewRewriteAnswer('')
      await loadRewrites()
    } catch (e: any) {
      notify(`Ошибка добавления DNS Rewrite: ${e.message || e}`, true)
    } finally {
      setRewriteAdding(false)
    }
  }

  // Удаление DNS Rewrite
  const handleDeleteRewrite = async (entry: AghRewriteEntry) => {
    if (!confirm(`Удалить переопределение для домена ${entry.domain}?`)) return
    try {
      await apiPost('/adguard/rewrites/delete', {
        domain: entry.domain,
        answer: entry.answer,
      })
      notify(`DNS Rewrite удален: ${entry.domain}`)
      await loadRewrites()
    } catch (e: any) {
      notify(`Ошибка удаления DNS Rewrite: ${e.message || e}`, true)
    }
  }

  // Быстрое добавление домена в пользовательские правила (Заблокировать)
  const handleQuickBlockDomain = async (domain: string) => {
    const rule = `||${domain}^`
    if (userRulesText.includes(rule)) {
      notify(`Правило для ${domain} уже присутствует в списке`, true)
      return
    }
    const updated = userRulesText ? `${userRulesText}\n${rule}` : rule
    setUserRulesText(updated)
    try {
      const rules = updated.split('\n').map((s) => s.trim()).filter((s) => s.length > 0)
      await apiPost('/adguard/filtering/rules', { rules })
      notify(`Домен ${domain} заблокирован пользовательским правилом`)
    } catch (e: any) {
      notify(`Ошибка блокировки: ${e.message || e}`, true)
    }
  }

  // Сохранение настроек интеграции
  const handleSaveSettings = async (e: React.FormEvent) => {
    e.preventDefault()
    try {
      await apiPost('/adguard/config', settingsForm)
      notify('Настройки интеграции AdGuard Home успешно сохранены')
      await loadData()
    } catch (e: any) {
      notify(`Ошибка сохранения настроек: ${e.message || e}`, true)
    }
  }

  // Ссылка на родную веб-панель AGH
  const nativePanelUrl = useMemo(() => {
    const host = config?.host || '127.0.0.1'
    const targetHost = host === '127.0.0.1' || host === '0.0.0.0' ? window.location.hostname : host
    const port = config?.http_port || 3000
    return `http://${targetHost}:${port}`
  }, [config?.host, config?.http_port])

  return (
    <div className="space-y-6" data-testid="adguard-view">
      {/* 1. ГЛАВНЫЙ СТАТУСНЫЙ БАННЕР */}
      <div className="card p-6 bg-gradient-to-r from-gray-900/80 via-gray-850/80 to-gray-900/80 border border-emerald-500/20 shadow-xl rounded-2xl backdrop-blur-md">
        <div className="flex flex-col md:flex-row items-start md:items-center justify-between gap-6">
          <div className="flex items-center gap-4">
            <div className="w-14 h-14 rounded-2xl bg-emerald-500/10 border border-emerald-500/30 flex items-center justify-center text-3xl shadow-inner">
              🛡️
            </div>
            <div>
              <div className="flex items-center gap-3">
                <h1 className="text-2xl font-bold text-white tracking-wide">AdGuard Home</h1>
                {loading && (
                  <span
                    className="w-3.5 h-3.5 border-2 border-emerald-400 border-t-transparent rounded-full animate-spin inline-block"
                    title="Обновление данных..."
                  />
                )}
                {status?.running ? (
                  <span className="inline-flex items-center gap-1.5 px-2.5 py-0.5 rounded-full text-xs font-medium bg-emerald-500/20 text-emerald-300 border border-emerald-500/30">
                    <span className="w-2 h-2 rounded-full bg-emerald-400 animate-pulse" />
                    Активен {status.version ? `v${status.version}` : ''}
                  </span>
                ) : (
                  <span className="inline-flex items-center gap-1.5 px-2.5 py-0.5 rounded-full text-xs font-medium bg-rose-500/20 text-rose-300 border border-rose-500/30">
                    <span className="w-2 h-2 rounded-full bg-rose-400" />
                    Не запущен
                  </span>
                )}
                {health?.loop_risk && (
                  <span className="inline-flex items-center gap-1 px-2.5 py-0.5 rounded-full text-xs font-medium bg-amber-500/20 text-amber-300 border border-amber-500/30">
                    ⚠️ Loop Risk
                  </span>
                )}
              </div>
              <p className="text-sm text-gray-400 mt-1">
                Сетевой DNS-сервер: защита от рекламы, трекеров, фишинга и переопределение доменов в роутере
              </p>
            </div>
          </div>

          <div className="flex flex-wrap items-center gap-3 w-full md:w-auto justify-end">
            {/* Тумблер защиты */}
            <button
              type="button"
              onClick={handleToggleProtection}
              disabled={actionPending || !status?.running}
              data-testid="adguard-protection-toggle"
              className={`flex items-center gap-2.5 px-4 py-2.5 rounded-xl font-medium text-sm transition-all duration-200 border shadow-md ${
                status?.protection_enabled
                  ? 'bg-emerald-500/20 border-emerald-500/40 text-emerald-200 hover:bg-emerald-500/30'
                  : 'bg-rose-500/20 border-rose-500/40 text-rose-200 hover:bg-rose-500/30'
              } disabled:opacity-50 disabled:cursor-not-allowed`}
            >
              <span className={`w-3 h-3 rounded-full ${status?.protection_enabled ? 'bg-emerald-400' : 'bg-rose-400'}`} />
              {status?.protection_enabled ? 'Защита включена' : 'Защита отключена'}
              {actionPending && <span className="animate-spin text-xs">⏳</span>}
            </button>

            {/* Ссылка в веб-панель */}
            <a
              href={nativePanelUrl}
              target="_blank"
              rel="noopener noreferrer"
              className="flex items-center gap-1.5 px-3.5 py-2.5 rounded-xl text-sm font-medium bg-gray-800/80 hover:bg-gray-700/80 border border-gray-700 text-gray-200 transition-colors shadow-sm"
              title="Открыть нативную веб-панель AdGuard Home в новой вкладке"
            >
              Веб-панель AGH ↗
            </a>

            {/* Быстрый перезапуск службы */}
            <button
              type="button"
              onClick={() => handleServiceAction('restart')}
              disabled={actionPending}
              className="flex items-center gap-1.5 px-3 py-2.5 rounded-xl text-sm font-medium bg-gray-800/80 hover:bg-gray-700/80 border border-gray-700 text-gray-200 transition-colors"
              title="Перезапустить службу AdGuard Home"
            >
              🔄
            </button>
          </div>
        </div>

        {/* Предупреждение о циклической пересылке (Loop Warning) */}
        {health?.loop_warning && (
          <div className="mt-4 p-3.5 bg-amber-500/10 border border-amber-500/30 rounded-xl text-amber-200 text-xs flex items-center gap-3">
            <span className="text-base">⚠️</span>
            <div className="flex-1">
              <strong>Внимание!</strong> {health.loop_warning}
            </div>
          </div>
        )}
      </div>

      {/* 2. НАВИГАЦИОННЫЕ ВКЛАДКИ РАЗДЕЛА */}
      <div className="flex flex-wrap gap-2 border-b border-gray-800 pb-2">
        <button
          type="button"
          data-testid="subtab-overview"
          onClick={() => setActiveSubTab('overview')}
          className={`px-4 py-2 rounded-xl text-sm font-medium transition-all ${
            activeSubTab === 'overview'
              ? 'bg-emerald-500/20 text-emerald-300 border border-emerald-500/40 shadow-sm'
              : 'text-gray-400 hover:text-gray-200 hover:bg-gray-800/50'
          }`}
        >
          📊 Обзор и статистика
        </button>
        <button
          type="button"
          data-testid="subtab-querylog"
          onClick={() => setActiveSubTab('querylog')}
          className={`px-4 py-2 rounded-xl text-sm font-medium transition-all ${
            activeSubTab === 'querylog'
              ? 'bg-emerald-500/20 text-emerald-300 border border-emerald-500/40 shadow-sm'
              : 'text-gray-400 hover:text-gray-200 hover:bg-gray-800/50'
          }`}
        >
          📜 Журнал запросов (Query Log)
        </button>
        <button
          type="button"
          data-testid="subtab-filtering"
          onClick={() => setActiveSubTab('filtering')}
          className={`px-4 py-2 rounded-xl text-sm font-medium transition-all ${
            activeSubTab === 'filtering'
              ? 'bg-emerald-500/20 text-emerald-300 border border-emerald-500/40 shadow-sm'
              : 'text-gray-400 hover:text-gray-200 hover:bg-gray-800/50'
          }`}
        >
          🛡️ Фильтры и правила
        </button>
        <button
          type="button"
          data-testid="subtab-rewrites"
          onClick={() => setActiveSubTab('rewrites')}
          className={`px-4 py-2 rounded-xl text-sm font-medium transition-all ${
            activeSubTab === 'rewrites'
              ? 'bg-emerald-500/20 text-emerald-300 border border-emerald-500/40 shadow-sm'
              : 'text-gray-400 hover:text-gray-200 hover:bg-gray-800/50'
          }`}
        >
          🔀 DNS Переопределения
        </button>
        <button
          type="button"
          data-testid="subtab-diagnostics"
          onClick={() => setActiveSubTab('diagnostics')}
          className={`px-4 py-2 rounded-xl text-sm font-medium transition-all ${
            activeSubTab === 'diagnostics'
              ? 'bg-emerald-500/20 text-emerald-300 border border-emerald-500/40 shadow-sm'
              : 'text-gray-400 hover:text-gray-200 hover:bg-gray-800/50'
          }`}
        >
          🩺 Диагностика и сеть
        </button>
        <button
          type="button"
          data-testid="subtab-settings"
          onClick={() => setActiveSubTab('settings')}
          className={`px-4 py-2 rounded-xl text-sm font-medium transition-all ${
            activeSubTab === 'settings'
              ? 'bg-emerald-500/20 text-emerald-300 border border-emerald-500/40 shadow-sm'
              : 'text-gray-400 hover:text-gray-200 hover:bg-gray-800/50'
          }`}
        >
          ⚙️ Настройки
        </button>
      </div>

      {/* 3. СОДЕРЖИМОЕ ВКЛАДОК */}

      {/* TAB 1: ОБЗОР И СТАТИСТИКА */}
      {activeSubTab === 'overview' && (
        <div className="space-y-6">
          {/* Метрики */}
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
            <div className="card p-5 bg-gray-900/60 border border-gray-800 rounded-2xl backdrop-blur-sm">
              <div className="flex items-center justify-between text-gray-400 mb-2 text-xs font-medium uppercase tracking-wider">
                <span>DNS Запросов (24ч)</span>
                <span className="text-blue-400">🌐</span>
              </div>
              <div className="text-2xl font-bold text-white tracking-tight">
                {overview?.num_dns_queries?.toLocaleString() || 0}
              </div>
              <p className="text-xs text-gray-500 mt-1">Все входящие DNS-резолвы сети</p>
            </div>

            <div className="card p-5 bg-gray-900/60 border border-gray-800 rounded-2xl backdrop-blur-sm">
              <div className="flex items-center justify-between text-gray-400 mb-2 text-xs font-medium uppercase tracking-wider">
                <span>Заблокировано</span>
                <span className="text-emerald-400">🛑</span>
              </div>
              <div className="text-2xl font-bold text-emerald-400 tracking-tight flex items-baseline gap-2">
                <span>{overview?.num_blocked_filtering?.toLocaleString() || 0}</span>
                <span className="text-sm font-normal text-emerald-500/80">({overview?.block_percentage || 0}%)</span>
              </div>
              <p className="text-xs text-gray-500 mt-1">Реклама, трекеры и угрозы</p>
            </div>

            <div className="card p-5 bg-gray-900/60 border border-gray-800 rounded-2xl backdrop-blur-sm">
              <div className="flex items-center justify-between text-gray-400 mb-2 text-xs font-medium uppercase tracking-wider">
                <span>Активных правил</span>
                <span className="text-indigo-400">📑</span>
              </div>
              <div className="text-2xl font-bold text-white tracking-tight">
                {overview?.active_rules_count?.toLocaleString() || 0}
              </div>
              <p className="text-xs text-gray-500 mt-1">Правил в подписках и вручную</p>
            </div>

            <div className="card p-5 bg-gray-900/60 border border-gray-800 rounded-2xl backdrop-blur-sm">
              <div className="flex items-center justify-between text-gray-400 mb-2 text-xs font-medium uppercase tracking-wider">
                <span>Время обработки</span>
                <span className="text-amber-400">⚡</span>
              </div>
              <div className="text-2xl font-bold text-white tracking-tight">
                {overview?.avg_processing_time_ms ? `${overview.avg_processing_time_ms.toFixed(1)} мс` : '—'}
              </div>
              <p className="text-xs text-gray-500 mt-1">Средняя задержка ответа клиенту</p>
            </div>
          </div>

          {/* Списки топов: Домены и клиенты */}
          <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
            {/* Топ запросов */}
            <div className="card p-5 bg-gray-900/60 border border-gray-800 rounded-2xl">
              <h3 className="text-sm font-semibold text-gray-200 mb-4 flex items-center justify-between">
                <span>📈 Топ запросов</span>
                <span className="text-xs text-gray-500">24ч</span>
              </h3>
              {overview?.top_queried_domains && overview.top_queried_domains.length > 0 ? (
                <div className="space-y-2">
                  {overview.top_queried_domains.map((d, i) => (
                    <div key={i} className="flex items-center justify-between text-xs py-1.5 border-b border-gray-800/50 last:border-0">
                      <span className="font-mono text-gray-300 truncate max-w-[180px]" title={d.domain}>
                        {d.domain}
                      </span>
                      <span className="font-medium text-gray-400 bg-gray-800/80 px-2 py-0.5 rounded-md">
                        {d.count.toLocaleString()}
                      </span>
                    </div>
                  ))}
                </div>
              ) : (
                <p className="text-xs text-gray-500 py-4 text-center">Нет данных о запросах</p>
              )}
            </div>

            {/* Топ заблокированных */}
            <div className="card p-5 bg-gray-900/60 border border-gray-800 rounded-2xl">
              <h3 className="text-sm font-semibold text-gray-200 mb-4 flex items-center justify-between">
                <span>🚫 Топ заблокированных</span>
                <span className="text-xs text-gray-500">24ч</span>
              </h3>
              {overview?.top_blocked_domains && overview.top_blocked_domains.length > 0 ? (
                <div className="space-y-2">
                  {overview.top_blocked_domains.map((d, i) => (
                    <div key={i} className="flex items-center justify-between text-xs py-1.5 border-b border-gray-800/50 last:border-0">
                      <span className="font-mono text-rose-300 truncate max-w-[180px]" title={d.domain}>
                        {d.domain}
                      </span>
                      <span className="font-medium text-rose-400 bg-rose-500/10 px-2 py-0.5 rounded-md border border-rose-500/20">
                        {d.count.toLocaleString()}
                      </span>
                    </div>
                  ))}
                </div>
              ) : (
                <p className="text-xs text-gray-500 py-4 text-center">Блокировок не зафиксировано</p>
              )}
            </div>

            {/* Топ клиентов */}
            <div className="card p-5 bg-gray-900/60 border border-gray-800 rounded-2xl">
              <h3 className="text-sm font-semibold text-gray-200 mb-4 flex items-center justify-between">
                <span>📱 Топ клиентов</span>
                <span className="text-xs text-gray-500">24ч</span>
              </h3>
              {overview?.top_clients && overview.top_clients.length > 0 ? (
                <div className="space-y-2">
                  {overview.top_clients.map((c, i) => (
                    <div key={i} className="flex items-center justify-between text-xs py-1.5 border-b border-gray-800/50 last:border-0">
                      <div className="truncate max-w-[180px]">
                        <span className="font-mono text-gray-300 block">{c.ip}</span>
                        {c.name && <span className="text-[10px] text-gray-500 truncate block">{c.name}</span>}
                      </div>
                      <span className="font-medium text-gray-400 bg-gray-800/80 px-2 py-0.5 rounded-md">
                        {c.count.toLocaleString()}
                      </span>
                    </div>
                  ))}
                </div>
              ) : (
                <p className="text-xs text-gray-500 py-4 text-center">Нет данных о клиентах</p>
              )}
            </div>
          </div>
        </div>
      )}

      {/* TAB 2: ЖУРНАЛ ЗАПРОСОВ (QUERY LOG) */}
      {activeSubTab === 'querylog' && (
        <div className="card p-6 bg-gray-900/60 border border-gray-800 rounded-2xl space-y-4">
          <div className="flex flex-col sm:flex-row items-stretch sm:items-center justify-between gap-4">
            <div className="flex-1 flex items-center gap-3">
              <input
                type="text"
                value={logSearch}
                onChange={(e) => setLogSearch(e.target.value)}
                onKeyDown={(e) => e.key === 'Enter' && loadQueryLog()}
                placeholder="Поиск по домену или IP клиента..."
                className="w-full sm:max-w-md px-3.5 py-2 bg-gray-800/80 border border-gray-700 rounded-xl text-sm text-gray-200 placeholder-gray-500 focus:outline-none focus:border-emerald-500/50"
              />
              <select
                value={logStatusFilter}
                onChange={(e) => setLogStatusFilter(e.target.value)}
                className="px-3 py-2 bg-gray-800/80 border border-gray-700 rounded-xl text-sm text-gray-200 focus:outline-none focus:border-emerald-500/50"
              >
                <option value="">Все статусы</option>
                <option value="filtered">Заблокировано (Blocked)</option>
                <option value="rewritten">Переопределено (Rewrite)</option>
                <option value="ok">Разрешено (OK)</option>
              </select>
            </div>

            <button
              type="button"
              onClick={loadQueryLog}
              disabled={logLoading}
              className="px-4 py-2 bg-gray-800 hover:bg-gray-700 border border-gray-700 text-gray-200 rounded-xl text-sm font-medium flex items-center gap-2 transition-colors"
            >
              <span>{logLoading ? 'Загрузка…' : 'Обновить'}</span>
              <span className={logLoading ? 'animate-spin' : ''}>🔄</span>
            </button>
          </div>

          <div className="overflow-x-auto rounded-xl border border-gray-800">
            <table className="w-full text-left text-xs text-gray-300">
              <thead className="bg-gray-800/60 text-gray-400 uppercase text-[10px] tracking-wider border-b border-gray-800">
                <tr>
                  <th className="px-4 py-3">Время</th>
                  <th className="px-4 py-3">Клиент</th>
                  <th className="px-4 py-3">Домен</th>
                  <th className="px-4 py-3">Тип</th>
                  <th className="px-4 py-3">Статус</th>
                  <th className="px-4 py-3">Время</th>
                  <th className="px-4 py-3 text-right">Действие</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-gray-800/60">
                {queryLog.length > 0 ? (
                  queryLog.map((item, idx) => {
                    const isBlocked = item.status === 'Filtered' || item.status === 'Blocked' || item.reason === 'FilteredBlockedService'
                    const isRewrite = item.status === 'Rewrite' || item.reason === 'Rewrite'
                    const timePart = item.timestamp ? new Date(item.timestamp).toLocaleTimeString() : '—'

                    return (
                      <tr key={idx} className="hover:bg-gray-800/30 transition-colors">
                        <td className="px-4 py-2.5 font-mono text-gray-500 whitespace-nowrap">{timePart}</td>
                        <td className="px-4 py-2.5 whitespace-nowrap">
                          <span className="font-mono text-gray-300 block">{item.client_ip}</span>
                          {item.client_name && <span className="text-[10px] text-gray-500 block truncate max-w-[120px]">{item.client_name}</span>}
                        </td>
                        <td className="px-4 py-2.5 font-mono text-gray-200">
                          <div className="flex items-center gap-2">
                            <span className="truncate max-w-[260px]" title={item.question_name}>
                              {item.question_name}
                            </span>
                            {item.rule && (
                              <span className="text-[10px] text-gray-500 bg-gray-800 px-1.5 py-0.5 rounded truncate max-w-[120px]" title={item.rule}>
                                {item.rule}
                              </span>
                            )}
                          </div>
                        </td>
                        <td className="px-4 py-2.5 font-mono text-gray-400">{item.question_type}</td>
                        <td className="px-4 py-2.5 whitespace-nowrap">
                          {isBlocked ? (
                            <span className="px-2 py-0.5 rounded-full text-[10px] font-medium bg-rose-500/10 text-rose-300 border border-rose-500/20">
                              Блокировка
                            </span>
                          ) : isRewrite ? (
                            <span className="px-2 py-0.5 rounded-full text-[10px] font-medium bg-blue-500/10 text-blue-300 border border-blue-500/20">
                              Rewrite
                            </span>
                          ) : (
                            <span className="px-2 py-0.5 rounded-full text-[10px] font-medium bg-emerald-500/10 text-emerald-300 border border-emerald-500/20">
                              ОК
                            </span>
                          )}
                        </td>
                        <td className="px-4 py-2.5 font-mono text-gray-400 whitespace-nowrap">
                          {item.elapsed_ms ? `${item.elapsed_ms.toFixed(1)} мс` : '<1 мс'}
                        </td>
                        <td className="px-4 py-2.5 text-right whitespace-nowrap">
                          {!isBlocked ? (
                            <button
                              type="button"
                              onClick={() => handleQuickBlockDomain(item.question_name)}
                              className="px-2 py-1 text-[11px] rounded bg-rose-500/20 text-rose-300 hover:bg-rose-500/30 transition-colors"
                              title="Добавить домен в правила блокировки"
                            >
                              Заблокировать
                            </button>
                          ) : (
                            <span className="text-gray-600 text-xs">—</span>
                          )}
                        </td>
                      </tr>
                    )
                  })
                ) : (
                  <tr>
                    <td colSpan={7} className="px-4 py-8 text-center text-gray-500 text-xs">
                      {logLoading ? 'Загрузка записей журнала…' : 'Записей журнала запросов не найдено'}
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
          </div>
        </div>
      )}

      {/* TAB 3: ФИЛЬТРЫ И ПРАВИЛА */}
      {activeSubTab === 'filtering' && (
        <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
          {/* Списки подписок */}
          <div className="card p-6 bg-gray-900/60 border border-gray-800 rounded-2xl space-y-4">
            <h3 className="text-base font-semibold text-gray-200">Подписки фильтрации (Blocklists)</h3>
            <p className="text-xs text-gray-400">
              Подключенные официальные и сторонние базы блокировки рекламы и угроз
            </p>

            <div className="space-y-3 mt-3">
              {filters.length > 0 ? (
                filters.map((flt) => (
                  <div
                    key={flt.id}
                    className="p-3.5 bg-gray-850/70 border border-gray-800 rounded-xl flex items-center justify-between gap-4"
                  >
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-2">
                        <span className="text-sm font-medium text-gray-200 truncate">{flt.name}</span>
                        <span className="text-[10px] bg-gray-800 text-gray-400 px-2 py-0.5 rounded-full font-mono">
                          {flt.rules_count?.toLocaleString()} правил
                        </span>
                      </div>
                      <p className="text-[11px] text-gray-500 truncate mt-0.5">{flt.url}</p>
                    </div>

                    <input
                      type="checkbox"
                      checked={flt.enabled}
                      onChange={() => handleToggleFilter(flt.url, flt.enabled)}
                      className="w-4 h-4 rounded border-gray-700 text-emerald-500 focus:ring-emerald-500/20 bg-gray-800 cursor-pointer"
                    />
                  </div>
                ))
              ) : (
                <p className="text-xs text-gray-500 py-4 text-center">Списки фильтров не настроены</p>
              )}
            </div>
          </div>

          {/* Пользовательские правила */}
          <div className="card p-6 bg-gray-900/60 border border-gray-800 rounded-2xl space-y-4 flex flex-col">
            <div className="flex items-center justify-between">
              <div>
                <h3 className="text-base font-semibold text-gray-200">Пользовательские правила</h3>
                <p className="text-xs text-gray-400 mt-0.5">
                  Собственные правила блокировки (||example.com^) и исключений (@@||safe.com^)
                </p>
              </div>
              <button
                type="button"
                onClick={handleSaveRules}
                disabled={rulesSaving}
                className="px-4 py-2 bg-emerald-600 hover:bg-emerald-500 text-white rounded-xl text-xs font-medium transition-colors shadow-sm disabled:opacity-50"
              >
                {rulesSaving ? 'Сохранение…' : 'Сохранить правила'}
              </button>
            </div>

            <textarea
              value={userRulesText}
              onChange={(e) => setUserRulesText(e.target.value)}
              placeholder="||ad.example.com^&#10;! Комментарий&#10;@@||whitelist.example.com^"
              rows={14}
              className="w-full flex-1 p-3.5 bg-gray-950/80 border border-gray-800 rounded-xl font-mono text-xs text-gray-200 placeholder-gray-600 focus:outline-none focus:border-emerald-500/50 resize-y"
            />
          </div>
        </div>
      )}

      {/* TAB 4: DNS ПЕРЕОПРЕДЕЛЕНИЯ (REWRITES) */}
      {activeSubTab === 'rewrites' && (
        <div className="space-y-6">
          {/* Форма добавления */}
          <div className="card p-6 bg-gray-900/60 border border-gray-800 rounded-2xl">
            <h3 className="text-base font-semibold text-gray-200 mb-1">Добавить DNS Rewrite</h3>
            <p className="text-xs text-gray-400 mb-4">
              Переопределение доменного имени на произвольный IP-адрес или CNAME для локальной сети и сервисов
            </p>

            <form onSubmit={handleAddRewrite} className="flex flex-col sm:flex-row items-stretch sm:items-center gap-3">
              <input
                type="text"
                value={newRewriteDomain}
                onChange={(e) => setNewRewriteDomain(e.target.value)}
                placeholder="Домен (например, router.lan или *.local)"
                className="flex-1 px-3.5 py-2.5 bg-gray-800/80 border border-gray-700 rounded-xl text-sm text-gray-200 placeholder-gray-500 focus:outline-none focus:border-emerald-500/50"
              />
              <input
                type="text"
                value={newRewriteAnswer}
                onChange={(e) => setNewRewriteAnswer(e.target.value)}
                placeholder="IP-адрес или целевой CNAME (192.168.1.1)"
                className="flex-1 px-3.5 py-2.5 bg-gray-800/80 border border-gray-700 rounded-xl text-sm text-gray-200 placeholder-gray-500 focus:outline-none focus:border-emerald-500/50"
              />
              <button
                type="submit"
                disabled={rewriteAdding}
                className="px-5 py-2.5 bg-emerald-600 hover:bg-emerald-500 text-white rounded-xl text-sm font-medium transition-colors shadow-sm disabled:opacity-50 whitespace-nowrap"
              >
                {rewriteAdding ? 'Добавление…' : '+ Добавить'}
              </button>
            </form>
          </div>

          {/* Таблица существующих rewrites */}
          <div className="card p-6 bg-gray-900/60 border border-gray-800 rounded-2xl">
            <h3 className="text-base font-semibold text-gray-200 mb-4">Текущие переопределения ({rewrites.length})</h3>

            <div className="overflow-x-auto rounded-xl border border-gray-800">
              <table className="w-full text-left text-xs text-gray-300">
                <thead className="bg-gray-800/60 text-gray-400 uppercase text-[10px] tracking-wider border-b border-gray-800">
                  <tr>
                    <th className="px-4 py-3">Домен</th>
                    <th className="px-4 py-3">Ответ (IP / CNAME)</th>
                    <th className="px-4 py-3 text-right">Действие</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-gray-800/60">
                  {rewrites.length > 0 ? (
                    rewrites.map((r, i) => (
                      <tr key={i} className="hover:bg-gray-800/30 transition-colors">
                        <td className="px-4 py-3 font-mono text-gray-200">{r.domain}</td>
                        <td className="px-4 py-3 font-mono text-emerald-300">{r.answer}</td>
                        <td className="px-4 py-3 text-right">
                          <button
                            type="button"
                            onClick={() => handleDeleteRewrite(r)}
                            className="px-2.5 py-1 text-xs text-rose-300 bg-rose-500/10 hover:bg-rose-500/20 border border-rose-500/20 rounded-lg transition-colors"
                          >
                            Удалить
                          </button>
                        </td>
                      </tr>
                    ))
                  ) : (
                    <tr>
                      <td colSpan={3} className="px-4 py-8 text-center text-gray-500">
                        Нет настроенных DNS переопределений
                      </td>
                    </tr>
                  )}
                </tbody>
              </table>
            </div>
          </div>
        </div>
      )}

      {/* TAB 5: ДИАГНОСТИКА И СЕТЬ */}
      {activeSubTab === 'diagnostics' && (
        <div className="space-y-6">
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            <div className="card p-5 bg-gray-900/60 border border-gray-800 rounded-2xl">
              <h4 className="text-xs font-semibold text-gray-400 uppercase tracking-wider mb-3">Состояние портов</h4>
              <div className="space-y-2.5 text-xs">
                <div className="flex items-center justify-between py-1 border-b border-gray-800/50">
                  <span className="text-gray-300">Порт 53 (DNS UDP Listener):</span>
                  <span className={`font-mono px-2 py-0.5 rounded ${diagnostics?.port_53_status === 'active' ? 'bg-emerald-500/20 text-emerald-300' : 'bg-rose-500/20 text-rose-300'}`}>
                    {diagnostics?.port_53_status === 'active' ? 'Открыт и отвечает' : 'Не отвечает'}
                  </span>
                </div>
                <div className="flex items-center justify-between py-1 border-b border-gray-800/50">
                  <span className="text-gray-300">Порт 3000 (Веб-интерфейс):</span>
                  <span className={`font-mono px-2 py-0.5 rounded ${diagnostics?.port_3000_status === 'active' ? 'bg-emerald-500/20 text-emerald-300' : 'bg-rose-500/20 text-rose-300'}`}>
                    {diagnostics?.port_3000_status === 'active' ? 'Активен' : 'Не отвечает'}
                  </span>
                </div>
                <div className="flex items-center justify-between py-1 border-b border-gray-800/50">
                  <span className="text-gray-300">Перехват iptables NAT (Port 53):</span>
                  <span className={`font-mono px-2 py-0.5 rounded ${diagnostics?.iptables_redirect_active ? 'bg-emerald-500/20 text-emerald-300' : 'bg-gray-800 text-gray-400'}`}>
                    {diagnostics?.iptables_redirect_active ? 'Активен' : 'Отключен'}
                  </span>
                </div>
              </div>
            </div>

            <div className="card p-5 bg-gray-900/60 border border-gray-800 rounded-2xl">
              <h4 className="text-xs font-semibold text-gray-400 uppercase tracking-wider mb-3">Системные файлы</h4>
              <div className="space-y-2.5 text-xs">
                <div className="flex items-center justify-between py-1 border-b border-gray-800/50">
                  <span className="text-gray-300">Служба Entware:</span>
                  <span className="font-mono text-gray-400 text-right truncate max-w-[200px]" title={diagnostics?.detected_service_path || 'Не найдена'}>
                    {diagnostics?.detected_service_path || 'Не найдена'}
                  </span>
                </div>
                <div className="flex items-center justify-between py-1 border-b border-gray-800/50">
                  <span className="text-gray-300">Файл конфигурации:</span>
                  <span className="font-mono text-gray-400 text-right truncate max-w-[200px]" title={diagnostics?.detected_config_path || 'Не найден'}>
                    {diagnostics?.detected_config_path || 'Не найден'}
                  </span>
                </div>
                <div className="flex items-center justify-between py-1 border-b border-gray-800/50">
                  <span className="text-gray-300">Защитный откат DNS (Watchdog):</span>
                  <span className="font-mono text-emerald-300">Активен</span>
                </div>
              </div>
            </div>
          </div>

          {/* Рекомендации */}
          {diagnostics?.recommendations && diagnostics.recommendations.length > 0 && (
            <div className="card p-5 bg-gray-900/60 border border-gray-800 rounded-2xl">
              <h4 className="text-sm font-semibold text-gray-200 mb-3">Рекомендации системы</h4>
              <ul className="space-y-2 text-xs text-gray-300">
                {diagnostics.recommendations.map((rec, i) => (
                  <li key={i} className="flex items-start gap-2.5">
                    <span className="text-emerald-400 mt-0.5">•</span>
                    <span>{rec}</span>
                  </li>
                ))}
              </ul>
            </div>
          )}
        </div>
      )}

      {/* TAB 6: НАСТРОЙКИ ИНТЕГРАЦИИ */}
      {activeSubTab === 'settings' && (
        <form onSubmit={handleSaveSettings} className="card p-6 bg-gray-900/60 border border-gray-800 rounded-2xl space-y-6">
          <div>
            <h3 className="text-base font-semibold text-gray-200">Параметры подключения к AdGuard Home</h3>
            <p className="text-xs text-gray-400 mt-0.5">
              Настройка хоста, порта и авторизации панели управления
            </p>
          </div>

          <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
            <div>
              <label className="block text-xs font-medium text-gray-400 mb-1.5">Режим интеграции</label>
              <select
                value={settingsForm.integration_mode}
                onChange={(e) => setSettingsForm({ ...settingsForm, integration_mode: e.target.value as any })}
                className="w-full px-3.5 py-2.5 bg-gray-800/80 border border-gray-700 rounded-xl text-sm text-gray-200 focus:outline-none focus:border-emerald-500/50"
              >
                <option value="managed">Managed (на роутере Keenetic / Entware)</option>
                <option value="external">External (внешний сервер AdGuard Home)</option>
              </select>
            </div>

            <div>
              <label className="block text-xs font-medium text-gray-400 mb-1.5">Хост API</label>
              <input
                type="text"
                value={settingsForm.host}
                onChange={(e) => setSettingsForm({ ...settingsForm, host: e.target.value })}
                className="w-full px-3.5 py-2.5 bg-gray-800/80 border border-gray-700 rounded-xl text-sm text-gray-200 focus:outline-none focus:border-emerald-500/50"
              />
            </div>

            <div>
              <label className="block text-xs font-medium text-gray-400 mb-1.5">HTTP Порт API (по умолчанию 3000)</label>
              <input
                type="number"
                value={settingsForm.http_port}
                onChange={(e) => setSettingsForm({ ...settingsForm, http_port: parseInt(e.target.value, 10) || 3000 })}
                className="w-full px-3.5 py-2.5 bg-gray-800/80 border border-gray-700 rounded-xl text-sm text-gray-200 focus:outline-none focus:border-emerald-500/50"
              />
            </div>

            <div>
              <label className="block text-xs font-medium text-gray-400 mb-1.5">DNS Порт (по умолчанию 53)</label>
              <input
                type="number"
                value={settingsForm.dns_port}
                onChange={(e) => setSettingsForm({ ...settingsForm, dns_port: parseInt(e.target.value, 10) || 53 })}
                className="w-full px-3.5 py-2.5 bg-gray-800/80 border border-gray-700 rounded-xl text-sm text-gray-200 focus:outline-none focus:border-emerald-500/50"
              />
            </div>

            <div>
              <label className="block text-xs font-medium text-gray-400 mb-1.5">Логин веб-панели AGH (опционально)</label>
              <input
                type="text"
                value={settingsForm.username || ''}
                onChange={(e) => setSettingsForm({ ...settingsForm, username: e.target.value })}
                placeholder="admin"
                className="w-full px-3.5 py-2.5 bg-gray-800/80 border border-gray-700 rounded-xl text-sm text-gray-200 focus:outline-none focus:border-emerald-500/50"
              />
            </div>

            <div>
              <label className="block text-xs font-medium text-gray-400 mb-1.5">Пароль веб-панели AGH (опционально)</label>
              <input
                type="password"
                value={settingsForm.password || ''}
                onChange={(e) => setSettingsForm({ ...settingsForm, password: e.target.value })}
                placeholder="••••••••"
                className="w-full px-3.5 py-2.5 bg-gray-800/80 border border-gray-700 rounded-xl text-sm text-gray-200 focus:outline-none focus:border-emerald-500/50"
              />
            </div>
          </div>

          <div className="pt-2 flex justify-end">
            <button
              type="submit"
              className="px-6 py-2.5 bg-emerald-600 hover:bg-emerald-500 text-white rounded-xl text-sm font-medium transition-colors shadow-md"
            >
              Сохранить параметры
            </button>
          </div>
        </form>
      )}
    </div>
  )
}
