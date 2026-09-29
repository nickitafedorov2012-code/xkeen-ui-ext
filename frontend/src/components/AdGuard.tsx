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
      if (action === 'start' && !isInstalled) {
        notify('Запуск AdGuard Home: установка и подготовка службы...')
      }
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

  // Автоматическая загрузка с официального CDN AdGuard и установка
  const handleInstallAdGuard = async () => {
    setActionPending(true)
    try {
      notify('Скачивание AdGuard Home с официального CDN и установка на роутер (ожидайте 15-45 сек)...')
      const res = await apiPost<{ output: string }>('/adguard/install')
      notify(`Установка завершена: ${res.output || 'успешно'}`)
      await handleServiceAction('start')
    } catch (e: any) {
      notify(`Ошибка установки AdGuard Home: ${e.message || e}`, true)
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

  // Проверка факта установки AdGuard Home в системе
  const isInstalled = useMemo(() => {
    return Boolean(
      diagnostics?.detected_service_path ||
      diagnostics?.detected_config_path ||
      status?.version
    )
  }, [diagnostics?.detected_service_path, diagnostics?.detected_config_path, status?.version])

  // Ссылка на родную веб-панель AGH
  const nativePanelUrl = useMemo(() => {
    const host = config?.host || '127.0.0.1'
    const targetHost = host === '127.0.0.1' || host === '0.0.0.0' ? window.location.hostname : host
    const port = config?.http_port || 3000
    return `http://${targetHost}:${port}`
  }, [config?.host, config?.http_port])

  // Вспомогательный расчет максимальных значений для шкал топов
  const maxQueries = useMemo(() => {
    if (!overview?.top_queried_domains?.length) return 1
    return Math.max(...overview.top_queried_domains.map((d) => d.count), 1)
  }, [overview?.top_queried_domains])

  const maxBlocked = useMemo(() => {
    if (!overview?.top_blocked_domains?.length) return 1
    return Math.max(...overview.top_blocked_domains.map((d) => d.count), 1)
  }, [overview?.top_blocked_domains])

  const maxClients = useMemo(() => {
    if (!overview?.top_clients?.length) return 1
    return Math.max(...overview.top_clients.map((c) => c.count), 1)
  }, [overview?.top_clients])

  return (
    <div className="agh-container" data-testid="adguard-view">
      {/* 1. ГЛАВНЫЙ СТАТУСНЫЙ HERO-БАННЕР */}
      <section className="card agh-hero-card">
        <div className="agh-hero-header">
          <div className="agh-hero-title-group">
            <div className="agh-hero-icon" title="Сетевой щит AdGuard Home">
              🛡️
            </div>
            <div>
              <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
                <h2 className="agh-hero-title">AdGuard Home</h2>
                {status?.version && (
                  <span
                    className="badge"
                    style={{
                      background: 'rgba(16, 185, 129, 0.15)',
                      color: '#34d399',
                      borderColor: 'rgba(16, 185, 129, 0.35)',
                    }}
                  >
                    v{status.version}
                  </span>
                )}
                {loading && <span className="muted" style={{ fontSize: 12 }}>⏳ Обновление…</span>}
                <span
                  className={`agh-status-badge ${status?.running ? 'running' : 'stopped'}`}
                  title={status?.running ? 'Служба активна' : isInstalled ? 'Служба остановлена' : 'Не установлен'}
                >
                  <span>{status?.running ? '🟢 В сети' : isInstalled ? '🔴 Не запущен' : '⚪ Не установлен'}</span>
                </span>
                {health?.loop_risk && (
                  <span
                    className="badge"
                    style={{
                      background: 'rgba(245, 158, 11, 0.18)',
                      color: '#fbbf24',
                      borderColor: 'rgba(245, 158, 11, 0.4)',
                    }}
                  >
                    ⚠️ Риск петли DNS
                  </span>
                )}
              </div>
              <div className="agh-hero-subtitle">
                Сетевой DNS-сервер: фильтрация рекламы, трекеров, фишинга и локальное переопределение доменов в Keenetic
              </div>
            </div>
          </div>

          <div className="agh-hero-controls">
            {/* Тумблер защиты */}
            <button
              type="button"
              onClick={handleToggleProtection}
              disabled={actionPending}
              data-testid="adguard-protection-toggle"
              className={`btn btn-sm ${status?.protection_enabled ? 'btn-primary' : 'btn-secondary'}`}
              style={{
                background: status?.protection_enabled
                  ? 'linear-gradient(135deg, #10b981 0%, #059669 100%)'
                  : 'rgba(239, 68, 68, 0.15)',
                borderColor: status?.protection_enabled ? '#10b981' : 'rgba(239, 68, 68, 0.35)',
                color: status?.protection_enabled ? '#ffffff' : '#f87171',
                gap: 7,
              }}
              title="Включить или временно отключить блокировку DNS"
            >
              <span
                style={{
                  width: 8,
                  height: 8,
                  borderRadius: '50%',
                  background: status?.protection_enabled ? '#34d399' : '#ef4444',
                  boxShadow: status?.protection_enabled ? '0 0 6px #34d399' : 'none',
                }}
              />
              <b>{status?.protection_enabled ? 'Защита включена' : 'Защита отключена'}</b>
              {actionPending && <span style={{ fontSize: 12 }}>⏳</span>}
            </button>

            {/* Ссылка в веб-панель AGH */}
            <a
              href={nativePanelUrl}
              target="_blank"
              rel="noopener noreferrer"
              className="btn btn-sm btn-secondary"
              title="Открыть нативную веб-панель AdGuard Home в новой вкладке"
            >
              <span>Веб-панель AGH</span>
              <span style={{ fontSize: 11, opacity: 0.8 }}>↗</span>
            </a>

            {/* Кнопка установки при отсутствии пакета */}
            {!isInstalled && !status?.running && (
              <button
                type="button"
                onClick={handleInstallAdGuard}
                disabled={actionPending}
                data-testid="adguard-install-btn"
                className="btn btn-sm btn-primary"
                style={{
                  background: 'linear-gradient(135deg, #3b82f6 0%, #2563eb 100%)',
                  borderColor: '#2563eb',
                  color: '#ffffff',
                }}
                title="Установить официальный пакет adguardhome через Entware opkg"
              >
                {actionPending ? '⏳ Установка…' : '📦 Установить AGH'}
              </button>
            )}

            {/* Быстрый перезапуск / запуск службы */}
            <button
              type="button"
              onClick={() => handleServiceAction(status?.running ? 'restart' : 'start')}
              disabled={actionPending}
              data-testid="adguard-service-action-btn"
              className="btn btn-sm btn-secondary"
              title={
                status?.running
                  ? 'Перезапустить службу AdGuard Home'
                  : isInstalled
                  ? 'Запустить службу AdGuard Home'
                  : 'Установить и запустить службу AdGuard Home'
              }
            >
              {actionPending
                ? '⏳'
                : status?.running
                ? '🔄 Перезапуск'
                : isInstalled
                ? '▶ Запуск'
                : '🚀 Установить и запустить'}
            </button>
          </div>
        </div>

        {/* Предупреждение о циклической пересылке (Loop Warning) */}
        {health?.loop_warning && (
          <div className="agh-warning-banner">
            <span style={{ fontSize: 18 }}>⚠️</span>
            <div>
              <strong>Внимание! Обнаружен риск петли DNS:</strong> {health.loop_warning}
            </div>
          </div>
        )}

        {/* Информационный баннер при отсутствии установки */}
        {!isInstalled && !status?.running && (
          <div
            className="agh-warning-banner"
            style={{
              borderColor: 'rgba(59, 130, 246, 0.4)',
              background: 'rgba(59, 130, 246, 0.08)',
              color: '#93c5fd',
              marginTop: 14,
            }}
          >
            <span style={{ fontSize: 18 }}>ℹ️</span>
            <div style={{ flex: 1 }}>
              <strong>AdGuard Home не установлен на роутере:</strong> Служба или бинарный файл не обнаружены.
              Нажмите <b>«📦 Установить AGH»</b> или <b>«🚀 Установить и запустить»</b> — система автоматически определит архитектуру процессора роутера, скачает бинарный файл с официального CDN AdGuard, создаст службу и применит готовую конфигурацию без ручной настройки.
            </div>
          </div>
        )}
      </section>

      {/* 2. НАВИГАЦИОННЫЕ ВКЛАДКИ РАЗДЕЛА */}
      <div className="agh-tabs">
        <button
          type="button"
          data-testid="subtab-overview"
          onClick={() => setActiveSubTab('overview')}
          className={`agh-tab ${activeSubTab === 'overview' ? 'active' : ''}`}
        >
          <span>📊</span>
          <span>Обзор и статистика</span>
        </button>
        <button
          type="button"
          data-testid="subtab-querylog"
          onClick={() => setActiveSubTab('querylog')}
          className={`agh-tab ${activeSubTab === 'querylog' ? 'active' : ''}`}
        >
          <span>📜</span>
          <span>Журнал запросов</span>
        </button>
        <button
          type="button"
          data-testid="subtab-filtering"
          onClick={() => setActiveSubTab('filtering')}
          className={`agh-tab ${activeSubTab === 'filtering' ? 'active' : ''}`}
        >
          <span>🛡️</span>
          <span>Фильтры и правила</span>
        </button>
        <button
          type="button"
          data-testid="subtab-rewrites"
          onClick={() => setActiveSubTab('rewrites')}
          className={`agh-tab ${activeSubTab === 'rewrites' ? 'active' : ''}`}
        >
          <span>🔀</span>
          <span>DNS Переопределения</span>
        </button>
        <button
          type="button"
          data-testid="subtab-diagnostics"
          onClick={() => setActiveSubTab('diagnostics')}
          className={`agh-tab ${activeSubTab === 'diagnostics' ? 'active' : ''}`}
        >
          <span>🩺</span>
          <span>Диагностика и сеть</span>
        </button>
        <button
          type="button"
          data-testid="subtab-settings"
          onClick={() => setActiveSubTab('settings')}
          className={`agh-tab ${activeSubTab === 'settings' ? 'active' : ''}`}
        >
          <span>⚙️</span>
          <span>Настройки</span>
        </button>
      </div>

      {/* 3. СОДЕРЖИМОЕ ВКЛАДОК */}

      {/* TAB 1: ОБЗОР И СТАТИСТИКА */}
      {activeSubTab === 'overview' && (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
          {/* 4-Колоночная сетка ключевых метрик */}
          <div className="agh-metrics-grid">
            <div className="agh-metric-card">
              <div className="agh-metric-top">
                <span className="agh-metric-label">DNS Запросов (24ч)</span>
                <div className="agh-metric-icon-box blue">🌐</div>
              </div>
              <div className="agh-metric-val">
                {overview?.num_dns_queries !== undefined ? overview.num_dns_queries.toLocaleString() : '0'}
              </div>
              <div className="agh-metric-sub">Все входящие DNS-резолвы сети</div>
            </div>

            <div className="agh-metric-card">
              <div className="agh-metric-top">
                <span className="agh-metric-label">Заблокировано</span>
                <div className="agh-metric-icon-box green">🛑</div>
              </div>
              <div className="agh-metric-val">
                <span style={{ color: '#34d399' }}>
                  {overview?.num_blocked_filtering !== undefined ? overview.num_blocked_filtering.toLocaleString() : '0'}
                </span>
                <span style={{ fontSize: 14, fontWeight: 500, color: 'var(--muted)' }}>
                  ({overview?.block_percentage !== undefined ? overview.block_percentage : 0}%)
                </span>
              </div>
              <div className="agh-metric-sub">Реклама, трекеры и угрозы</div>
            </div>

            <div className="agh-metric-card">
              <div className="agh-metric-top">
                <span className="agh-metric-label">Активных правил</span>
                <div className="agh-metric-icon-box purple">📑</div>
              </div>
              <div className="agh-metric-val">
                {overview?.active_rules_count !== undefined ? overview.active_rules_count.toLocaleString() : '0'}
              </div>
              <div className="agh-metric-sub">Правил в подписках и вручную</div>
            </div>

            <div className="agh-metric-card">
              <div className="agh-metric-top">
                <span className="agh-metric-label">Время обработки</span>
                <div className="agh-metric-icon-box amber">⚡</div>
              </div>
              <div className="agh-metric-val">
                {overview?.avg_processing_time_ms ? `${overview.avg_processing_time_ms.toFixed(1)} мс` : '—'}
              </div>
              <div className="agh-metric-sub">Средняя задержка ответа клиенту</div>
            </div>
          </div>

          {/* 3-Колоночная сетка топов (Запросы / Блокировки / Клиенты) */}
          <div className="agh-stats-grid">
            {/* Карточка 1: Топ запросов */}
            <div className="agh-stat-card">
              <div className="agh-stat-card-header">
                <h3 className="agh-stat-card-title">
                  <span>📈</span>
                  <span>Топ запросов</span>
                </h3>
                <span className="badge">24ч</span>
              </div>
              {overview?.top_queried_domains && overview.top_queried_domains.length > 0 ? (
                <ul className="agh-stat-list">
                  {overview.top_queried_domains.map((d, i) => {
                    const pct = Math.min(100, Math.round((d.count / maxQueries) * 100))
                    return (
                      <li key={i} className="agh-stat-item">
                        <div className="agh-stat-row">
                          <span className="agh-stat-rank">#{i + 1}</span>
                          <span className="agh-stat-domain" title={d.domain}>
                            {d.domain}
                          </span>
                          <span className="agh-stat-count">{d.count.toLocaleString()}</span>
                        </div>
                        <div className="agh-stat-bar-track">
                          <div className="agh-stat-bar-fill blue" style={{ width: `${pct}%` }} />
                        </div>
                      </li>
                    )
                  })}
                </ul>
              ) : (
                <div className="agh-empty-state">
                  <div className="agh-empty-icon">📊</div>
                  <div className="agh-empty-text">Нет данных о запросах</div>
                </div>
              )}
            </div>

            {/* Карточка 2: Топ заблокированных */}
            <div className="agh-stat-card">
              <div className="agh-stat-card-header">
                <h3 className="agh-stat-card-title">
                  <span>🚫</span>
                  <span>Топ заблокированных</span>
                </h3>
                <span className="badge" style={{ color: '#f87171', borderColor: 'rgba(239, 68, 68, 0.3)' }}>
                  24ч
                </span>
              </div>
              {overview?.top_blocked_domains && overview.top_blocked_domains.length > 0 ? (
                <ul className="agh-stat-list">
                  {overview.top_blocked_domains.map((d, i) => {
                    const pct = Math.min(100, Math.round((d.count / maxBlocked) * 100))
                    return (
                      <li key={i} className="agh-stat-item">
                        <div className="agh-stat-row">
                          <span className="agh-stat-rank" style={{ color: '#f87171' }}>
                            #{i + 1}
                          </span>
                          <span className="agh-stat-domain" title={d.domain} style={{ color: '#fca5a5' }}>
                            {d.domain}
                          </span>
                          <span
                            className="agh-stat-count"
                            style={{
                              background: 'rgba(239, 68, 68, 0.12)',
                              color: '#f87171',
                              borderColor: 'rgba(239, 68, 68, 0.25)',
                            }}
                          >
                            {d.count.toLocaleString()}
                          </span>
                        </div>
                        <div className="agh-stat-bar-track">
                          <div className="agh-stat-bar-fill red" style={{ width: `${pct}%` }} />
                        </div>
                      </li>
                    )
                  })}
                </ul>
              ) : (
                <div className="agh-empty-state">
                  <div className="agh-empty-icon">🛡️</div>
                  <div className="agh-empty-text">Блокировок не зафиксировано</div>
                </div>
              )}
            </div>

            {/* Карточка 3: Топ клиентов */}
            <div className="agh-stat-card">
              <div className="agh-stat-card-header">
                <h3 className="agh-stat-card-title">
                  <span>📱</span>
                  <span>Топ клиентов</span>
                </h3>
                <span className="badge">24ч</span>
              </div>
              {overview?.top_clients && overview.top_clients.length > 0 ? (
                <ul className="agh-stat-list">
                  {overview.top_clients.map((c, i) => {
                    const pct = Math.min(100, Math.round((c.count / maxClients) * 100))
                    return (
                      <li key={i} className="agh-stat-item">
                        <div className="agh-stat-row">
                          <span className="agh-stat-rank">#{i + 1}</span>
                          <div style={{ flex: 1, minWidth: 0 }}>
                            <span className="agh-stat-domain" title={c.ip}>
                              {c.ip}
                            </span>
                            {c.name && (
                              <div style={{ fontSize: 11, color: 'var(--muted)', marginTop: 1 }} className="truncate">
                                {c.name}
                              </div>
                            )}
                          </div>
                          <span className="agh-stat-count">{c.count.toLocaleString()}</span>
                        </div>
                        <div className="agh-stat-bar-track">
                          <div className="agh-stat-bar-fill emerald" style={{ width: `${pct}%` }} />
                        </div>
                      </li>
                    )
                  })}
                </ul>
              ) : (
                <div className="agh-empty-state">
                  <div className="agh-empty-icon">👥</div>
                  <div className="agh-empty-text">Нет данных о клиентах</div>
                </div>
              )}
            </div>
          </div>

          {/* Дополнительная карточка: Апстримы и порты */}
          <div className="grid2">
            <div className="card">
              <h2>🌐 Вышестоящие DNS-серверы (Upstreams)</h2>
              <p className="muted" style={{ fontSize: 12.5, margin: '0 0 10px' }}>
                Серверы разрешения внешних имен, настроенные в конфигурации AdGuard Home
              </p>
              <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                {(config?.upstream_dns && config.upstream_dns.length > 0
                  ? config.upstream_dns
                  : ['tls://1.1.1.1', 'tls://8.8.8.8']
                ).map((u, i) => (
                  <div
                    key={i}
                    style={{
                      display: 'flex',
                      alignItems: 'center',
                      justifyContent: 'space-between',
                      padding: '7px 10px',
                      borderRadius: 8,
                      background: 'var(--panel-2)',
                      border: '1px solid var(--border)',
                      fontSize: 12.5,
                      fontFamily: 'monospace',
                    }}
                  >
                    <span>{u}</span>
                    <span className="badge badge-flow-ok">Активен</span>
                  </div>
                ))}
              </div>
            </div>

            <div className="card">
              <h2>⚡ Сетевые порты и статус</h2>
              <ul className="kv">
                <li>
                  <span>Порт DNS (UDP/TCP):</span>
                  <span className="mono">{status?.dns_port || 53}</span>
                </li>
                <li>
                  <span>Порт Веб-панели (HTTP):</span>
                  <span className="mono">{status?.http_port || 3000}</span>
                </li>
                <li>
                  <span>IP-адреса слушателя:</span>
                  <span className="mono">
                    {status?.dns_addresses?.length ? status.dns_addresses.join(', ') : '0.0.0.0'}
                  </span>
                </li>
                <li>
                  <span>Режим работы:</span>
                  <span className="badge" style={{ color: '#38bdf8' }}>
                    {config?.integration_mode === 'external' ? 'Внешний сервер' : 'Локальный (Entware)'}
                  </span>
                </li>
              </ul>
            </div>
          </div>
        </div>
      )}

      {/* TAB 2: ЖУРНАЛ ЗАПРОСОВ (QUERY LOG) */}
      {activeSubTab === 'querylog' && (
        <div className="card">
          <div className="agh-toolbar">
            <div className="agh-search-group">
              <input
                type="text"
                value={logSearch}
                onChange={(e) => setLogSearch(e.target.value)}
                onKeyDown={(e) => e.key === 'Enter' && loadQueryLog()}
                placeholder="Поиск по домену или IP клиента (Enter)..."
                className="input"
                style={{ flex: 1, minWidth: 220 }}
              />
              <select
                value={logStatusFilter}
                onChange={(e) => setLogStatusFilter(e.target.value)}
                className="select"
                style={{ minWidth: 160 }}
              >
                <option value="">Все статусы</option>
                <option value="filtered">Заблокировано (Blocked)</option>
                <option value="rewritten">Переопределено (Rewrite)</option>
                <option value="ok">Разрешено (OK)</option>
              </select>
            </div>

            <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
              <span className="badge">Записей: {queryLog.length}</span>
              <button
                type="button"
                onClick={loadQueryLog}
                disabled={logLoading}
                className="btn btn-sm btn-secondary"
                title="Обновить журнал запросов"
              >
                <span>{logLoading ? 'Загрузка…' : '🔄 Обновить'}</span>
              </button>
            </div>
          </div>

          <div className="agh-table-container">
            <table className="agh-table">
              <thead>
                <tr>
                  <th>Время</th>
                  <th>Клиент</th>
                  <th>Домен</th>
                  <th>Тип</th>
                  <th>Статус</th>
                  <th>Ответ</th>
                  <th style={{ textAlign: 'right' }}>Действие</th>
                </tr>
              </thead>
              <tbody>
                {queryLog.length > 0 ? (
                  queryLog.map((item, idx) => {
                    const isBlocked =
                      item.status === 'Filtered' ||
                      item.status === 'Blocked' ||
                      item.reason === 'FilteredBlockedService'
                    const isRewrite = item.status === 'Rewrite' || item.reason === 'Rewrite'
                    const timePart = item.timestamp ? new Date(item.timestamp).toLocaleTimeString() : '—'

                    return (
                      <tr key={idx}>
                        <td className="mono" style={{ color: 'var(--muted)', whiteSpace: 'nowrap' }}>
                          {timePart}
                        </td>
                        <td style={{ whiteSpace: 'nowrap' }}>
                          <span className="mono" style={{ display: 'block', fontWeight: 600 }}>
                            {item.client_ip}
                          </span>
                          {item.client_name && (
                            <span style={{ fontSize: 11, color: 'var(--muted)', display: 'block' }}>
                              {item.client_name}
                            </span>
                          )}
                        </td>
                        <td>
                          <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
                            <span className="mono" style={{ fontWeight: 600 }} title={item.question_name}>
                              {item.question_name}
                            </span>
                            {item.rule && (
                              <span
                                className="badge"
                                style={{
                                  fontSize: 10,
                                  background: 'rgba(239, 68, 68, 0.1)',
                                  color: '#f87171',
                                }}
                                title={item.rule}
                              >
                                {item.rule}
                              </span>
                            )}
                          </div>
                        </td>
                        <td>
                          <span className="badge mono">{item.question_type}</span>
                        </td>
                        <td>
                          {isBlocked ? (
                            <span className="agh-pill blocked">Блокировка</span>
                          ) : isRewrite ? (
                            <span className="agh-pill rewrite">Rewrite</span>
                          ) : (
                            <span className="agh-pill ok">ОК</span>
                          )}
                        </td>
                        <td className="mono" style={{ color: 'var(--muted)', fontSize: 12 }}>
                          {item.elapsed_ms ? `${item.elapsed_ms.toFixed(1)} мс` : '<1 мс'}
                        </td>
                        <td style={{ textAlign: 'right', whiteSpace: 'nowrap' }}>
                          {!isBlocked ? (
                            <button
                              type="button"
                              onClick={() => handleQuickBlockDomain(item.question_name)}
                              className="btn btn-xs btn-danger"
                              title="Добавить домен в правила блокировки"
                            >
                              Заблокировать
                            </button>
                          ) : (
                            <span className="muted" style={{ fontSize: 12 }}>—</span>
                          )}
                        </td>
                      </tr>
                    )
                  })
                ) : (
                  <tr>
                    <td colSpan={7}>
                      <div className="agh-empty-state">
                        <div className="agh-empty-icon">📜</div>
                        <div className="agh-empty-text">
                          {logLoading ? 'Загрузка записей журнала…' : 'Записей журнала запросов не найдено'}
                        </div>
                      </div>
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
        <div className="grid2">
          {/* Списки подписок */}
          <div className="card" style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
              <div>
                <h2>Подписки фильтрации (Blocklists)</h2>
                <p className="muted" style={{ fontSize: 12, margin: 0 }}>
                  Официальные и сторонние базы блокировки рекламы и угроз
                </p>
              </div>
              <button
                type="button"
                className="btn btn-sm btn-secondary"
                onClick={loadFiltering}
                title="Обновить список подписок"
              >
                🔄
              </button>
            </div>

            <div style={{ display: 'flex', flexDirection: 'column', gap: 8, marginTop: 6 }}>
              {filters.length > 0 ? (
                filters.map((flt) => (
                  <div key={flt.id} className="agh-filter-item">
                    <div style={{ flex: 1, minWidth: 0 }}>
                      <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
                        <span style={{ fontWeight: 600, fontSize: 13, color: 'var(--text-bright)' }}>{flt.name}</span>
                        <span className="badge mono">{flt.rules_count?.toLocaleString()} правил</span>
                      </div>
                      <div
                        className="mono muted"
                        style={{
                          fontSize: 11,
                          marginTop: 3,
                          overflow: 'hidden',
                          textOverflow: 'ellipsis',
                          whiteSpace: 'nowrap',
                        }}
                      >
                        {flt.url}
                      </div>
                    </div>

                    <label className="switch" title={flt.enabled ? 'Отключить подписку' : 'Включить подписку'}>
                      <input
                        type="checkbox"
                        checked={flt.enabled}
                        onChange={() => handleToggleFilter(flt.url, flt.enabled)}
                      />
                      <span className="slider" />
                    </label>
                  </div>
                ))
              ) : (
                <div className="agh-empty-state">
                  <div className="agh-empty-icon">🛡️</div>
                  <div className="agh-empty-text">Списки фильтров не настроены</div>
                </div>
              )}
            </div>
          </div>

          {/* Пользовательские правила */}
          <div className="card" style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
              <div>
                <h2>Пользовательские правила</h2>
                <p className="muted" style={{ fontSize: 12, margin: 0 }}>
                  Собственные правила блокировки (||domain^) и исключений (@@||domain^)
                </p>
              </div>
              <button
                type="button"
                onClick={handleSaveRules}
                disabled={rulesSaving}
                className="btn btn-sm btn-primary"
              >
                {rulesSaving ? 'Сохранение…' : '💾 Сохранить правила'}
              </button>
            </div>

            {/* Быстрые вставки синтаксиса */}
            <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap' }}>
              <button
                type="button"
                className="badge"
                style={{ cursor: 'pointer', background: 'var(--panel-2)' }}
                onClick={() => setUserRulesText((prev) => (prev ? `${prev}\n||example.com^` : '||example.com^'))}
                title="Добавить шаблон блокировки домена"
              >
                + Блок: ||domain^
              </button>
              <button
                type="button"
                className="badge"
                style={{ cursor: 'pointer', background: 'var(--panel-2)' }}
                onClick={() => setUserRulesText((prev) => (prev ? `${prev}\n@@||example.com^` : '@@||example.com^'))}
                title="Добавить шаблон белого списка"
              >
                + Белый список: @@||domain^
              </button>
              <button
                type="button"
                className="badge"
                style={{ cursor: 'pointer', background: 'var(--panel-2)' }}
                onClick={() => setUserRulesText((prev) => (prev ? `${prev}\n! Комментарий` : '! Комментарий'))}
                title="Добавить комментарий"
              >
                + Комментарий: ! Текст
              </button>
            </div>

            <textarea
              value={userRulesText}
              onChange={(e) => setUserRulesText(e.target.value)}
              placeholder="||ad.example.com^&#10;! Мой комментарий&#10;@@||whitelist.example.com^"
              rows={14}
              className="agh-rules-editor"
            />
          </div>
        </div>
      )}

      {/* TAB 4: DNS ПЕРЕОПРЕДЕЛЕНИЯ (REWRITES) */}
      {activeSubTab === 'rewrites' && (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
          {/* Форма добавления */}
          <div className="card">
            <h2>Добавить DNS Rewrite</h2>
            <p className="muted" style={{ fontSize: 12.5, margin: '0 0 14px' }}>
              Переопределение доменного имени на произвольный IP-адрес или CNAME для локальной сети и сервисов
            </p>

            <form onSubmit={handleAddRewrite} style={{ display: 'flex', gap: 10, flexWrap: 'wrap', alignItems: 'center' }}>
              <input
                type="text"
                value={newRewriteDomain}
                onChange={(e) => setNewRewriteDomain(e.target.value)}
                placeholder="Домен (например, router.lan или *.local)"
                className="input"
                style={{ flex: 1, minWidth: 200 }}
              />
              <input
                type="text"
                value={newRewriteAnswer}
                onChange={(e) => setNewRewriteAnswer(e.target.value)}
                placeholder="IP-адрес или целевой CNAME (192.168.1.1)"
                className="input"
                style={{ flex: 1, minWidth: 200 }}
              />
              <button
                type="submit"
                disabled={rewriteAdding}
                className="btn btn-primary"
                style={{ whiteSpace: 'nowrap' }}
              >
                {rewriteAdding ? 'Добавление…' : '+ Добавить переопределение'}
              </button>
            </form>
          </div>

          {/* Таблица существующих rewrites */}
          <div className="card">
            <h2>Текущие переопределения ({rewrites.length})</h2>

            <div className="agh-table-container" style={{ marginTop: 12 }}>
              <table className="agh-table">
                <thead>
                  <tr>
                    <th>Домен</th>
                    <th>Ответ (IP / CNAME)</th>
                    <th style={{ textAlign: 'right' }}>Действие</th>
                  </tr>
                </thead>
                <tbody>
                  {rewrites.length > 0 ? (
                    rewrites.map((r, i) => (
                      <tr key={i}>
                        <td className="mono" style={{ fontWeight: 600 }}>
                          {r.domain}
                        </td>
                        <td className="mono" style={{ color: '#34d399' }}>
                          {r.answer}
                        </td>
                        <td style={{ textAlign: 'right' }}>
                          <button
                            type="button"
                            onClick={() => handleDeleteRewrite(r)}
                            className="btn btn-xs btn-danger"
                          >
                            Удалить
                          </button>
                        </td>
                      </tr>
                    ))
                  ) : (
                    <tr>
                      <td colSpan={3}>
                        <div className="agh-empty-state">
                          <div className="agh-empty-icon">🔀</div>
                          <div className="agh-empty-text">Нет настроенных DNS переопределений</div>
                        </div>
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
        <div style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
          <div className="grid2">
            <div className="card">
              <h2>Состояние портов и слушателей</h2>
              <ul className="kv">
                <li>
                  <span>Порт 53 (DNS UDP Listener):</span>
                  <span
                    className={
                      diagnostics?.port_53_status === 'active' ? 'badge badge-flow-ok' : 'badge badge-flow-blocked'
                    }
                  >
                    {diagnostics?.port_53_status === 'active' ? 'Открыт и отвечает' : 'Не отвечает'}
                  </span>
                </li>
                <li>
                  <span>Порт 3000 (Веб-интерфейс):</span>
                  <span
                    className={
                      diagnostics?.port_3000_status === 'active' ? 'badge badge-flow-ok' : 'badge badge-flow-blocked'
                    }
                  >
                    {diagnostics?.port_3000_status === 'active' ? 'Активен' : 'Не отвечает'}
                  </span>
                </li>
                <li>
                  <span>Перехват iptables NAT (Port 53):</span>
                  <span className={diagnostics?.iptables_redirect_active ? 'badge badge-flow-ok' : 'badge'}>
                    {diagnostics?.iptables_redirect_active ? 'Активен' : 'Отключен'}
                  </span>
                </li>
              </ul>
            </div>

            <div className="card">
              <h2>Системные файлы и watchdog</h2>
              <ul className="kv">
                <li>
                  <span>Служба Entware:</span>
                  <span className="mono" style={{ fontSize: 11 }}>
                    {diagnostics?.detected_service_path || 'Не найдена'}
                  </span>
                </li>
                <li>
                  <span>Файл конфигурации:</span>
                  <span className="mono" style={{ fontSize: 11 }}>
                    {diagnostics?.detected_config_path || 'Не найден'}
                  </span>
                </li>
                <li>
                  <span>Защитный откат DNS (Watchdog):</span>
                  <span className="badge badge-flow-ok">Активен</span>
                </li>
              </ul>
            </div>
          </div>

          {/* Рекомендации */}
          {diagnostics?.recommendations && diagnostics.recommendations.length > 0 && (
            <div className="card">
              <h2>Рекомендации системы</h2>
              <ul style={{ margin: '8px 0 0', paddingLeft: 18, color: 'var(--text)' }}>
                {diagnostics.recommendations.map((rec, i) => (
                  <li key={i} style={{ marginBottom: 4, fontSize: 13 }}>
                    {rec}
                  </li>
                ))}
              </ul>
            </div>
          )}
        </div>
      )}

      {/* TAB 6: НАСТРОЙКИ ИНТЕГРАЦИИ */}
      {activeSubTab === 'settings' && (
        <form onSubmit={handleSaveSettings} className="card" style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
          <div>
            <h2>Параметры подключения к AdGuard Home</h2>
            <p className="muted" style={{ fontSize: 12.5, margin: '2px 0 0' }}>
              Настройка хоста, порта и авторизации панели управления
            </p>
          </div>

          <div className="grid2" style={{ gap: 14 }}>
            <div>
              <label style={{ display: 'block', fontSize: 12, color: 'var(--muted)', marginBottom: 6, fontWeight: 500 }}>
                Режим интеграции
              </label>
              <select
                value={settingsForm.integration_mode}
                onChange={(e) => setSettingsForm({ ...settingsForm, integration_mode: e.target.value as any })}
                className="select"
                style={{ width: '100%' }}
              >
                <option value="managed">Managed (на роутере Keenetic / Entware)</option>
                <option value="external">External (внешний сервер AdGuard Home)</option>
              </select>
            </div>

            <div>
              <label style={{ display: 'block', fontSize: 12, color: 'var(--muted)', marginBottom: 6, fontWeight: 500 }}>
                Хост API
              </label>
              <input
                type="text"
                value={settingsForm.host}
                onChange={(e) => setSettingsForm({ ...settingsForm, host: e.target.value })}
                className="input"
                style={{ width: '100%' }}
              />
            </div>

            <div>
              <label style={{ display: 'block', fontSize: 12, color: 'var(--muted)', marginBottom: 6, fontWeight: 500 }}>
                HTTP Порт API (по умолчанию 3000)
              </label>
              <input
                type="number"
                value={settingsForm.http_port}
                onChange={(e) => setSettingsForm({ ...settingsForm, http_port: parseInt(e.target.value, 10) || 3000 })}
                className="input"
                style={{ width: '100%' }}
              />
            </div>

            <div>
              <label style={{ display: 'block', fontSize: 12, color: 'var(--muted)', marginBottom: 6, fontWeight: 500 }}>
                DNS Порт (по умолчанию 53)
              </label>
              <input
                type="number"
                value={settingsForm.dns_port}
                onChange={(e) => setSettingsForm({ ...settingsForm, dns_port: parseInt(e.target.value, 10) || 53 })}
                className="input"
                style={{ width: '100%' }}
              />
            </div>

            <div>
              <label style={{ display: 'block', fontSize: 12, color: 'var(--muted)', marginBottom: 6, fontWeight: 500 }}>
                Логин веб-панели AGH (опционально)
              </label>
              <input
                type="text"
                value={settingsForm.username || ''}
                onChange={(e) => setSettingsForm({ ...settingsForm, username: e.target.value })}
                placeholder="admin"
                className="input"
                style={{ width: '100%' }}
              />
            </div>

            <div>
              <label style={{ display: 'block', fontSize: 12, color: 'var(--muted)', marginBottom: 6, fontWeight: 500 }}>
                Пароль веб-панели AGH (опционально)
              </label>
              <input
                type="password"
                value={settingsForm.password || ''}
                onChange={(e) => setSettingsForm({ ...settingsForm, password: e.target.value })}
                placeholder="••••••••"
                className="input"
                style={{ width: '100%' }}
              />
            </div>
          </div>

          <div style={{ display: 'flex', justifyContent: 'flex-end', marginTop: 8 }}>
            <button type="submit" className="btn btn-primary">
              💾 Сохранить параметры
            </button>
          </div>
        </form>
      )}
    </div>
  )
}
