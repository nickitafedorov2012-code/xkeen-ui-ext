import { useState, useEffect, useRef } from 'react'
import { apiGet, apiPost } from '../api'
import type {
  ZapretStatus,
  DpiTestResult,
  ZapretFeatures,
  ZapretAnalytics,
  BlockcheckResult,
  BlockcheckStrategy,
} from '../types'
import { normalizeDomainInput, renderMicroSpinner } from './zapret/zapretHelpers'
import { AnalyticsSection } from './zapret/AnalyticsSection'
import { BlockcheckAndSmartTvSection } from './zapret/BlockcheckAndSmartTvSection'
import { CommunityHostlistsSection } from './zapret/CommunityHostlistsSection'
import { CustomSitesBoostSection } from './zapret/CustomSitesBoostSection'
import { DirectServicesSection, DpiTuningSection } from './zapret/DirectServicesAndTuningSection'

export { fmtUptime, normalizeDomainInput } from './zapret/zapretHelpers'
interface ZapretProps {
  notify: (msg: string, error?: boolean) => void
}


export default function Zapret({ notify }: ZapretProps) {
  const [status, setStatus] = useState<ZapretStatus | null>(null)
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState(false)
  const [testResult, setTestResult] = useState<DpiTestResult | null>(null)
  const [testingDpi, setTestingDpi] = useState(false)

  // DPI Analytics (Live-монитор)
  const [analytics, setAnalytics] = useState<ZapretAnalytics | null>(null)
  const [resettingAnalytics, setResettingAnalytics] = useState(false)

  // Mini-Blockcheck (автоподбор стратегий)
  const [blockcheckResult, setBlockcheckResult] = useState<BlockcheckResult | null>(null)
  const [runningBlockcheck, setRunningBlockcheck] = useState(false)
  const [applyingStrategy, setApplyingStrategy] = useState<string | null>(null)
  const [activeStrategyId, setActiveStrategyId] = useState<string | null>(null)

  // Community Hostlists
  const [communityUrl, setCommunityUrl] = useState(
    'https://raw.githubusercontent.com/zapret-info/z-block/master/hosts.txt'
  )
  const [syncingCommunity, setSyncingCommunity] = useState(false)

  // Защита оптимистичного UI: ключи операций в полёте защищены от затирания фоновым polling
  const pendingKeysRef = useRef<Set<string>>(new Set())
  const [pendingKeys, setPendingKeys] = useState<Set<string>>(new Set())

  const addPendingKey = (key: string) => {
    pendingKeysRef.current.add(key)
    setPendingKeys(new Set(pendingKeysRef.current))
  }

  const removePendingKey = (key: string) => {
    pendingKeysRef.current.delete(key)
    setPendingKeys(new Set(pendingKeysRef.current))
  }

  // Конфигурация и хостлист
  const [showConfigEditor, setShowConfigEditor] = useState(false)
  const [configDraft, setConfigDraft] = useState('')
  const [savingConfig, setSavingConfig] = useState(false)

  const [showHostsEditor, setShowHostsEditor] = useState(false)
  const [hostsDraft, setHostsDraft] = useState('')
  const [savingHosts, setSavingHosts] = useState(false)
  // Refs are read by the polling callback created on mount.
  const configDirtyRef = useRef(false)
  const hostsDirtyRef = useRef(false)

  // Мульти-стратегии и независимые выключатели
  const [features, setFeatures] = useState<ZapretFeatures>({
    enabled: true,
    hybrid_youtube: false,
    hybrid_discord: false,
    discord_voice_udp: true,
    youtube_turbo: true,
    general_bypass: true,
    aggressive_dpi: false,
    isolated_proxy: true,
    bypass_github: true,
    bypass_torrents: true,
    bypass_adult: true,
    smart_tv_mode: false,
    community_hostlist_enabled: false,
    community_hostlist_url: 'https://raw.githubusercontent.com/zapret-info/z-block/master/hosts.txt',
    community_hostlist_auto_update: false,
    community_hostlist_count: 0,
    excluded_devices: [],
    custom_entries: [],
  })
  const [togglingFeature, setTogglingFeature] = useState<string | null>(null)

  // Пользовательские сайты и CDN Boost (/boost)
  const [customDomainInput, setCustomDomainInput] = useState('')
  const [addingCustom, setAddingCustom] = useState(false)
  const [boostingDomain, setBoostingDomain] = useState<string | null>(null)
  const [expandedCdnDomains, setExpandedCdnDomains] = useState<Set<string>>(new Set())

  // Z-19: защита поллинга — не опрашиваем фоновую вкладку и не допускаем наложение
  // запросов (раньше наложение триггерило побочные эффекты GET /api/zapret/status)
  const statusInFlightRef = useRef(false)
  const loadStatus = async () => {
    if (document.hidden || statusInFlightRef.current) return
    statusInFlightRef.current = true
    try {
      const res = await apiGet<ZapretStatus>('zapret/status')
      setStatus((prev) => {
        if (!prev) return res
        if (pendingKeysRef.current.has('enabled')) {
          return { ...res, running: prev.running }
        }
        return res
      })
      if (res.active_strategy_id) {
        setActiveStrategyId(res.active_strategy_id)
      }
      if (res.analytics) {
        setAnalytics(res.analytics)
      }
      if (res.features) {
        if (res.features.community_hostlist_url) {
          setCommunityUrl(res.features.community_hostlist_url)
        }
        setFeatures((prev) => {
          if (pendingKeysRef.current.has('preset_in_flight')) {
            return prev
          }
          const merged: ZapretFeatures = {
            ...res.features!,
            custom_entries: res.features!.custom_entries || [],
          }
          // Защищаем ключи, которые прямо сейчас меняются пользователем
          for (const k of pendingKeysRef.current) {
            if (k in prev && k !== 'custom_entries') {
              (merged as any)[k] = (prev as any)[k]
            } else if (k.startsWith('custom:')) {
              const domain = k.slice(7)
              if (domain === 'all') {
                const prevEntries = prev.custom_entries || []
                if (merged.custom_entries) {
                  merged.custom_entries = merged.custom_entries.map((e) => {
                    const match = prevEntries.find((pe) => normalizeDomainInput(pe.domain) === normalizeDomainInput(e.domain))
                    return match ? { ...e, enabled: match.enabled } : e
                  })
                }
              } else {
                const prevEntry = prev.custom_entries?.find((e) => normalizeDomainInput(e.domain) === domain)
                if (prevEntry && merged.custom_entries) {
                  merged.custom_entries = merged.custom_entries.map((e) =>
                    normalizeDomainInput(e.domain) === domain ? { ...e, enabled: prevEntry.enabled } : e
                  )
                }
              }
            } else if (k.startsWith('delete:')) {
              const domain = k.slice(7)
              if (merged.custom_entries) {
                merged.custom_entries = merged.custom_entries.filter((e) => normalizeDomainInput(e.domain) !== domain)
              }
            }
          }
          return merged
        })
      }
      if (!configDirtyRef.current && typeof res.config === 'string') setConfigDraft(res.config)
      if (!hostsDirtyRef.current && typeof res.hosts === 'string') setHostsDraft(res.hosts)
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка загрузки статуса Zapret', true)
    } finally {
      statusInFlightRef.current = false
      setLoading(false)
    }
  }

  const loadAnalytics = async () => {
    if (document.hidden) return
    try {
      const res = await apiGet<ZapretAnalytics>('zapret/analytics')
      if (res) setAnalytics(res)
    } catch {
      /* non-critical */
    }
  }

  useEffect(() => {
    loadStatus()
    loadAnalytics()
    const timer = setInterval(loadStatus, 10000)
    const aTimer = setInterval(loadAnalytics, 3500)
    return () => {
      clearInterval(timer)
      clearInterval(aTimer)
    }
  }, [])

  const handleResetAnalytics = async () => {
    setResettingAnalytics(true)
    try {
      await apiPost('zapret/action', { action: 'reset_analytics' })
      notify('Счётчики перехваченного трафика DPI сброшены')
      await loadAnalytics()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка сброса аналитики DPI', true)
    } finally {
      setResettingAnalytics(false)
    }
  }

  const handleRunBlockcheck = async () => {
    setBlockcheckResult(null)
    setRunningBlockcheck(true)
    try {
      const res = await apiPost<BlockcheckResult>('zapret/blockcheck')
      setBlockcheckResult(res)
      notify('✅ Автоподбор стратегий завершен! Выберите лучшую стратегию')
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка выполнения автоподбора стратегий', true)
    } finally {
      setRunningBlockcheck(false)
    }
  }

  const handleApplyStrategy = async (strat: BlockcheckStrategy) => {
    setApplyingStrategy(strat.id)
    try {
      const res = await apiPost<{ success: boolean; message?: string }>('zapret/action', {
        action: 'apply_strategy',
        custom_args: strat.args,
        strategy_id: strat.id,
      })
      setActiveStrategyId(strat.id)
      notify(res.message || `Стратегия '${strat.name}' успешно применена`)
      await loadStatus()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка применения стратегии', true)
    } finally {
      setApplyingStrategy(null)
    }
  }

  const handleSyncCommunityHostlist = async () => {
    setSyncingCommunity(true)
    try {
      const res = await apiPost<{ success: boolean; count: number; message: string; last_updated: string }>(
        'zapret/community-hostlist/sync',
        { url: communityUrl }
      )
      notify(res.message || `Синхронизировано ${res.count} доменов`)
      await loadStatus()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка синхронизации community hostlist', true)
    } finally {
      setSyncingCommunity(false)
    }
  }

  const handleSaveCommunityHostlistUrl = async (newUrl: string) => {
    try {
      const res = await apiPost<{ success: boolean; features?: ZapretFeatures; message?: string }>('zapret/action', {
        action: 'toggle_feature',
        feature: 'community_hostlist_enabled',
        enabled: features.community_hostlist_enabled ?? false,
        url: newUrl,
      })
      if (res.features) {
        setFeatures((prev) => ({ ...prev, ...res.features }))
      }
      notify('URL внешнего списка сохранен')
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка сохранения URL внешнего списка', true)
    }
  }

  const handleToggle = async () => {
    const nextVal = !isRunning
    addPendingKey('enabled')
    setBusy(true)
    setStatus((prev) => (prev ? { ...prev, running: nextVal } : prev))
    setFeatures((prev) => ({ ...prev, enabled: nextVal }))
    try {
      const res = await apiPost<{ success: boolean; action: string; output?: string; features?: ZapretFeatures }>('zapret/action', {
        action: nextVal ? 'start' : 'stop',
        enabled: nextVal,
      })
      if (res.features) {
        setFeatures((prev) => ({
          ...prev,
          ...res.features,
          custom_entries: res.features?.custom_entries || prev.custom_entries || [],
        }))
      }
      notify(res.action === 'start' ? '🟢 Служба Zapret запущена' : '⚪ Служба Zapret остановлена')
      // Даём nfqws2 время на инициализацию или остановку перед опросом PID
      await new Promise((r) => setTimeout(r, 600))
      let statusRes = await apiGet<ZapretStatus>('zapret/status')
      if (nextVal && !statusRes.running) {
        await new Promise((r) => setTimeout(r, 1200))
        statusRes = await apiGet<ZapretStatus>('zapret/status')
      } else if (!nextVal && statusRes.running) {
        await new Promise((r) => setTimeout(r, 800))
        statusRes = await apiGet<ZapretStatus>('zapret/status')
      }
      if (!nextVal) {
        statusRes = {
          ...statusRes,
          running: false,
          features: {
            ...(statusRes.features || {}),
            enabled: false,
          } as ZapretFeatures,
        }
      }
      setStatus(statusRes)
      if (statusRes.features) {
        setFeatures({
          ...statusRes.features,
          enabled: nextVal,
          custom_entries: statusRes.features.custom_entries || [],
        })
      }
    } catch (e) {
      setStatus((prev) => (prev ? { ...prev, running: !nextVal } : prev))
      setFeatures((prev) => ({ ...prev, enabled: !nextVal }))
      notify(e instanceof Error ? e.message : 'Ошибка переключения Zapret', true)
    } finally {
      removePendingKey('enabled')
      setBusy(false)
    }
  }

  const [upgradingEngine, setUpgradingEngine] = useState(false)
  const [switchingEngine, setSwitchingEngine] = useState(false)
  const [stepLog, setStepLog] = useState<string | null>(null)

  const handleAction = async (act: string) => {
    setBusy(true)
    try {
      const res = await apiPost<{ success: boolean; message?: string; output?: string }>('zapret/action', {
        action: act,
      })
      if (res.output) {
        setStepLog(res.output)
      }
      notify(res.message || `Действие ${act} выполнено`)
      await loadStatus()
    } catch (e) {
      if (e instanceof Error && (e.message.includes('Лог:') || e.message.includes('[1/4]'))) {
        setStepLog(e.message)
      }
      notify(e instanceof Error ? e.message : `Ошибка действия ${act}`, true)
    } finally {
      setBusy(false)
    }
  }

  const handleUpgradeZapret2 = async () => {
    setUpgradingEngine(true)
    setBusy(true)
    try {
      const res = await apiPost<{
        success: boolean
        engine?: 'v1' | 'v2'
        v2_installed?: boolean
        can_rollback_v1?: boolean
        features?: ZapretFeatures
        message?: string
        output?: string
      }>('zapret/action', {
        action: 'upgrade_zapret2',
        engine: 'v2',
      })
      if (res.output) {
        setStepLog(res.output)
      }
      setStatus((prev) =>
        prev
          ? {
              ...prev,
              engine: 'v2',
              v2_installed: res.v2_installed ?? true,
              can_rollback_v1: res.can_rollback_v1 ?? true,
              update_available: false,
            }
          : prev
      )
      if (res.features) {
        setFeatures((prev) => ({
          ...prev,
          ...res.features,
          engine: 'v2',
          custom_entries: res.features?.custom_entries || prev.custom_entries || [],
        }))
      } else {
        setFeatures((prev) => ({ ...prev, engine: 'v2' }))
      }
      try {
        localStorage.removeItem('xr_zapret_last_check')
      } catch {
        /* ignore */
      }
      try {
        await apiGet('zapret/update/check?force=1')
      } catch {
        /* ignore */
      }
      notify(res.message || '🚀 Zapret 2.0 (nfqws2 + Lua) успешно установлен в /opt/zapret2')
      const resolvedVersion = status?.version || status?.latest_version || 'v2'
      window.dispatchEvent(
        new CustomEvent('xr:zapret-updated', {
          detail: {
            update_available: false,
            engine: 'v2',
            version: resolvedVersion,
            latest_version: resolvedVersion,
          },
        })
      )
      window.dispatchEvent(new CustomEvent('xr:refresh-status'))
      await loadStatus()
    } catch (e) {
      if (e instanceof Error && (e.message.includes('Лог:') || e.message.includes('[1/4]'))) {
        setStepLog(e.message)
      }
      notify(e instanceof Error ? e.message : 'Ошибка обновления до Zapret 2 (nfqws2)', true)
    } finally {
      setUpgradingEngine(false)
      setBusy(false)
    }
  }

  const handleSwitchEngine = async (targetEngine: 'v1' | 'v2') => {
    setSwitchingEngine(true)
    setBusy(true)
    const prevEngine = status?.engine || features.engine || 'v2'
    setStatus((prev) => (prev ? { ...prev, engine: targetEngine } : prev))
    setFeatures((prev) => ({ ...prev, engine: targetEngine }))
    try {
      const res = await apiPost<{
        success: boolean
        engine?: 'v1' | 'v2'
        features?: ZapretFeatures
        message?: string
      }>('zapret/action', {
        action: targetEngine === 'v1' ? 'rollback_v1' : 'switch_engine',
        engine: targetEngine,
      })
      if (res.features) {
        setFeatures((prev) => ({
          ...prev,
          ...res.features,
          engine: targetEngine,
          custom_entries: res.features?.custom_entries || prev.custom_entries || [],
        }))
      }
      notify(
        res.message ||
          (targetEngine === 'v1'
            ? '⏪ Выполнен безопасный откат на Legacy 1.x (nfqws)'
            : '🚀 Переключено на Modern 2.0 (nfqws2)')
      )
      window.dispatchEvent(
        new CustomEvent('xr:zapret-updated', {
          detail: {
            update_available: false,
            engine: targetEngine,
          },
        })
      )
      window.dispatchEvent(new CustomEvent('xr:refresh-status'))
      await loadStatus()
    } catch (e) {
      setStatus((prev) => (prev ? { ...prev, engine: prevEngine } : prev))
      setFeatures((prev) => ({ ...prev, engine: prevEngine }))
      notify(e instanceof Error ? e.message : 'Ошибка переключения движка Zapret', true)
    } finally {
      setSwitchingEngine(false)
      setBusy(false)
    }
  }


  const handleApplyPreset = async (presetId: string) => {
    const presetKey = `preset:${presetId}`
    addPendingKey(presetKey)
    addPendingKey('preset_in_flight')
    setBusy(true)

    // Оптимистичное обновление стейта в соответствии с пресетом
    setFeatures((prev) => {
      let opt = { ...prev }
      if (presetId === 'youtube') {
        opt = {
          ...opt,
          youtube_turbo: true,
          hybrid_youtube: true,
          hybrid_discord: false,
          discord_voice_udp: false,
          general_bypass: false,
          aggressive_dpi: false,
          bypass_github: false,
          bypass_torrents: false,
          bypass_adult: false,
        }
      } else if (presetId === 'discord') {
        opt = {
          ...opt,
          youtube_turbo: false,
          hybrid_youtube: false,
          hybrid_discord: true,
          discord_voice_udp: true,
          general_bypass: false,
          aggressive_dpi: false,
          bypass_github: false,
          bypass_torrents: false,
          bypass_adult: false,
        }
      } else if (presetId === 'gamer' || presetId === 'media') {
        opt = {
          ...opt,
          youtube_turbo: true,
          hybrid_youtube: true,
          hybrid_discord: true,
          discord_voice_udp: true,
          general_bypass: true,
          aggressive_dpi: false,
          isolated_proxy: true,
          bypass_github: true,
          bypass_torrents: true,
          bypass_adult: false,
        }
      } else if (presetId === 'aggressive') {
        opt = {
          ...opt,
          youtube_turbo: true,
          hybrid_youtube: true,
          hybrid_discord: true,
          discord_voice_udp: true,
          general_bypass: true,
          aggressive_dpi: true,
          isolated_proxy: true,
          bypass_github: true,
          bypass_torrents: true,
          bypass_adult: true,
        }
      } else if (presetId === 'default' || presetId === 'general') {
        opt = {
          ...opt,
          youtube_turbo: true,
          hybrid_youtube: true,
          hybrid_discord: true,
          discord_voice_udp: true,
          general_bypass: true,
          aggressive_dpi: false,
          isolated_proxy: true,
          bypass_github: true,
          bypass_torrents: true,
          bypass_adult: true,
        }
      }
      return opt
    })

    setStatus((prev) => (prev ? { ...prev, preset: presetId } : prev))

    try {
      const res = await apiPost<{ success: boolean; features?: ZapretFeatures; message?: string }>('zapret/action', {
        action: 'set_preset',
        preset: presetId,
      })
      if (res.features) {
        setFeatures({
          ...res.features,
          custom_entries: res.features.custom_entries || [],
        })
      }
      notify(res.message || `Применен набор стратегий '${presetId}'`)
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка применения набора стратегий', true)
      await loadStatus()
    } finally {
      removePendingKey(presetKey)
      removePendingKey('preset_in_flight')
      setBusy(false)
    }
  }

  const handleSaveConfig = async () => {
    setSavingConfig(true)
    try {
      await apiPost('zapret/action', {
        action: 'save_config',
        config_content: configDraft,
      })
      configDirtyRef.current = false
      notify('Конфигурация zapret.conf сохранена и перезапущена')
      await loadStatus()
      setShowConfigEditor(false)
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка сохранения конфига', true)
    } finally {
      setSavingConfig(false)
    }
  }

  const handleSaveHosts = async () => {
    setSavingHosts(true)
    try {
      await apiPost('zapret/action', {
        action: 'save_hosts',
        hosts_content: hostsDraft,
      })
      hostsDirtyRef.current = false
      notify('Список доменов zapret-hosts.txt сохранен')
      await loadStatus()
      setShowHostsEditor(false)
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка сохранения списка доменов', true)
    } finally {
      setSavingHosts(false)
    }
  }

  const handleTestDpi = async () => {
    setTestResult(null)
    setTestingDpi(true)
    try {
      const res = await apiPost<DpiTestResult>('zapret/action', { action: 'test_dpi' })
      setTestResult(res)
      if (res.youtube.ok && res.discord.ok) {
        notify('✅ YouTube и Discord доступны напрямую через Zapret!')
      } else if (res.youtube.ok) {
        notify('✅ YouTube доступен напрямую через Zapret')
      } else if (res.youtube.proxy_ok) {
        notify('ℹ️ YouTube доступен через прокси-туннель (прямой DPI блокируется ТСПУ)')
      } else {
        notify('ℹ️ Проверка завершена: получены ответы от серверов')
      }
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка тестирования DPI', true)
    } finally {
      setTestingDpi(false)
    }
  }

  const handleToggleFeature = async (key: keyof ZapretFeatures) => {
    const nextVal = !features[key]
    addPendingKey(key)
    setTogglingFeature(key)
    setFeatures((prev) => ({ ...prev, [key]: nextVal }))
    try {
      const res = await apiPost<{ success: boolean; features?: ZapretFeatures; message?: string }>('zapret/action', {
        action: 'toggle_feature',
        feature: key,
        enabled: nextVal,
      })
      if (res.features) {
        setFeatures((prev) => {
          const merged: ZapretFeatures = {
            ...res.features!,
            custom_entries: res.features!.custom_entries || [],
          }
          for (const k of pendingKeysRef.current) {
            if (k !== key && k in prev && k !== 'custom_entries') {
              (merged as any)[k] = (prev as any)[k]
            }
          }
          return merged
        })
      }
      notify(
        nextVal
          ? '🟢 Блок активирован и правила обновлены'
          : '⚪ Блок выключен'
      )
    } catch (e) {
      setFeatures((prev) => ({ ...prev, [key]: !nextVal }))
      notify(e instanceof Error ? e.message : 'Ошибка переключения блока', true)
    } finally {
      removePendingKey(key)
      setTogglingFeature(null)
    }
  }

  const handleResetFeatures = async () => {
    addPendingKey('preset_in_flight')
    setBusy(true)
    try {
      const res = await apiPost<{ success: boolean; features?: ZapretFeatures; message?: string }>('zapret/action', {
        action: 'reset_features',
      })
      if (res.features) {
        setFeatures({
          ...res.features,
          custom_entries: res.features.custom_entries || [],
        })
      }
      notify('Все блоки сброшены к стандартным значениям')
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка сброса настроек', true)
      await loadStatus()
    } finally {
      removePendingKey('preset_in_flight')
      setBusy(false)
    }
  }

  const handleAddCustomDomain = async (domainToAdd?: string) => {
    const raw = (domainToAdd || customDomainInput).trim()
    if (!raw) {
      notify('Укажите домен сайта (например, mysku.club или habr.com)', true)
      return
    }
    const clean = normalizeDomainInput(raw)

    if (!clean || clean.includes(' ') || !clean.includes('.')) {
      notify('Некорректный домен сайта (например, mysku.club)', true)
      return
    }

    setAddingCustom(true)
    try {
      const res = await apiPost<{ success: boolean; features?: ZapretFeatures; message?: string }>('zapret/action', {
        action: 'add_custom_domain',
        domain: clean,
      })
      if (res.features) {
        setFeatures({
          ...res.features,
          custom_entries: res.features.custom_entries || [],
        })
      }
      setCustomDomainInput('')
      notify(res.message || `Сайт ${clean} добавлен и ускорен (/boost)`)
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка добавления сайта', true)
    } finally {
      setAddingCustom(false)
    }
  }

  const handleRemoveCustomDomain = async (domain: string) => {
    const clean = normalizeDomainInput(domain)
    const pendingKey = `delete:${clean}`
    addPendingKey(pendingKey)
    setFeatures((prev) => ({
      ...prev,
      custom_entries: (prev.custom_entries || []).filter((e) => normalizeDomainInput(e.domain) !== clean),
    }))
    try {
      const res = await apiPost<{ success: boolean; features?: ZapretFeatures; message?: string }>('zapret/action', {
        action: 'remove_custom_domain',
        domain: clean,
      })
      if (res.features) {
        setFeatures({
          ...res.features,
          custom_entries: res.features.custom_entries || [],
        })
      }
      notify(res.message || `Сайт ${clean} удален из Zapret`)
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка удаления сайта', true)
      await loadStatus()
    } finally {
      removePendingKey(pendingKey)
    }
  }

  const handleToggleCustomDomain = async (domain: string, currentEnabled: boolean) => {
    const clean = normalizeDomainInput(domain)
    const nextVal = !currentEnabled
    const pendingKey = `custom:${clean}`
    addPendingKey(pendingKey)
    setFeatures((prev) => ({
      ...prev,
      custom_entries: (prev.custom_entries || []).map((e) =>
        normalizeDomainInput(e.domain) === clean ? { ...e, enabled: nextVal } : e
      ),
    }))
    try {
      const res = await apiPost<{ success: boolean; features?: ZapretFeatures; message?: string }>('zapret/action', {
        action: 'toggle_custom_domain',
        domain: clean,
        enabled: nextVal,
      })
      removePendingKey(pendingKey)
      if (res.features) {
        setFeatures({
          ...res.features,
          custom_entries: res.features.custom_entries || [],
        })
      }
      notify(nextVal ? `🟢 ${clean} включен в обход DPI` : `⚪ ${clean} выключен`)
    } catch (e) {
      removePendingKey(pendingKey)
      setFeatures((prev) => ({
        ...prev,
        custom_entries: (prev.custom_entries || []).map((e) =>
          normalizeDomainInput(e.domain) === clean ? { ...e, enabled: currentEnabled } : e
        ),
      }))
      notify(e instanceof Error ? e.message : 'Ошибка переключения сайта', true)
    }
  }

  const handleToggleAllCustomDomains = async () => {
    if (!features.custom_entries || features.custom_entries.length === 0) return
    const activeCount = features.custom_entries.filter((e) => e.enabled !== false).length
    const nextVal = activeCount === 0
    const pendingKey = 'custom:all'
    addPendingKey(pendingKey)
    setFeatures((prev) => ({
      ...prev,
      custom_entries: (prev.custom_entries || []).map((e) => ({ ...e, enabled: nextVal })),
    }))
    try {
      const res = await apiPost<{ success: boolean; features?: ZapretFeatures; message?: string }>('zapret/action', {
        action: 'toggle_custom_domain',
        domain: 'all',
        enabled: nextVal,
      })
      removePendingKey(pendingKey)
      if (res.features) {
        setFeatures({
          ...res.features,
          custom_entries: res.features.custom_entries || [],
        })
      }
      notify(nextVal ? '🟢 Все сайты /boost включены в обход DPI' : '⚪ Все сайты /boost выключены')
    } catch (e) {
      removePendingKey(pendingKey)
      notify(e instanceof Error ? e.message : 'Ошибка переключения сайтов /boost', true)
      await loadStatus()
    }
  }

  const handleBoostCustomDomain = async (domain: string) => {
    const clean = normalizeDomainInput(domain)
    setBoostingDomain(clean)
    try {
      const res = await apiPost<{ success: boolean; features?: ZapretFeatures; message?: string }>('zapret/action', {
        action: 'boost_custom_domain',
        domain: clean,
      })
      if (res.features) {
        setFeatures({
          ...res.features,
          custom_entries: res.features.custom_entries || [],
        })
      }
      notify(res.message || `⚡ Boost: домены CDN обновлены для ${clean}`)
      await loadStatus()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка обновления CDN', true)
    } finally {
      setBoostingDomain(null)
    }
  }

  const isRunning = !!status?.running && features.enabled !== false
  const isInstalled = !!status?.installed
  const activeEngine: 'v1' | 'v2' =
    status?.engine ||
    features.engine ||
    (status?.cmdline?.includes('nfqws2') || status?.cmdline?.includes('--lua-desync') ? 'v2' : 'v1')
  const v2Installed =
    status?.v2_installed ?? (status?.cmdline?.includes('nfqws2') || status?.cmdline?.includes('--lua-desync') || false)
  const canRollbackV1 = status?.can_rollback_v1 ?? true
  const hardwareHint =
    status?.hardware?.hint_text || (loading ? 'Определение архитектуры и RAM роутера…' : 'Роутер Keenetic — рекомендуется Zapret 2.0')
  const targetArchLabel = status?.hardware?.target_arch || (loading ? 'определение…' : 'авто')

  if (loading && !status) {
    return (
      <div className="card" style={{ padding: 40, textAlign: 'center' }}>
        <div className="spinner" style={{ margin: '0 auto 16px' }} />
        <div className="muted">Загрузка модуля Zapret DPI…</div>
      </div>
    )
  }


  const renderStrategyCard = (
    key: keyof ZapretFeatures,
    title: string,
    badgeText: string,
    desc: string,
    activeInfo: string,
    inactiveInfo: string,
    tags?: string[],
    supportedEngines?: ('v1' | 'v2')[]
  ) => {
    const isChecked = !!features[key]
    const isPending = pendingKeys.has(key) || togglingFeature === key
    const isEngineSupported = !supportedEngines || supportedEngines.includes(activeEngine)
    const isCardDisabled = isPending || busy || !isInstalled || !isEngineSupported

    return (
      <div
        key={key}
        data-testid={`zapret-card-${key}`}
        style={{
          padding: '16px 18px',
          borderRadius: 14,
          background: isChecked && isEngineSupported ? 'rgba(56, 189, 248, 0.06)' : 'rgba(255, 255, 255, 0.02)',
          border: isChecked && isEngineSupported ? '1px solid rgba(56, 189, 248, 0.35)' : '1px solid var(--border)',
          display: 'flex',
          flexDirection: 'column',
          justifyContent: 'space-between',
          gap: 12,
          transition: 'all 0.2s ease',
          boxShadow: isChecked && isEngineSupported ? '0 4px 16px rgba(56, 189, 248, 0.06)' : 'none',
          opacity: isEngineSupported ? 1 : 0.45,
          filter: isEngineSupported ? 'none' : 'grayscale(0.8)',
        }}
      >
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start', gap: 12 }}>
          <div style={{ flex: 1 }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
              <b style={{ fontSize: 14, color: isChecked && isEngineSupported ? 'var(--text)' : 'var(--muted)' }}>{title}</b>
              <span
                className="badge"
                style={{
                  fontSize: 10,
                  background: isChecked && isEngineSupported ? 'rgba(56, 189, 248, 0.2)' : 'rgba(255, 255, 255, 0.05)',
                  color: isChecked && isEngineSupported ? '#38bdf8' : 'var(--muted)',
                  border: isChecked && isEngineSupported ? '1px solid rgba(56, 189, 248, 0.4)' : '1px solid var(--border)',
                }}
              >
                {badgeText}
              </span>
              {!isEngineSupported && (
                <span
                  className="badge"
                  style={{
                    fontSize: 10,
                    background: 'rgba(239, 68, 68, 0.15)',
                    color: '#f87171',
                    border: '1px solid rgba(239, 68, 68, 0.35)',
                    fontWeight: 600,
                  }}
                  title={`Данная функция поддерживается только движком Zapret ${supportedEngines?.map((e) => (e === 'v2' ? '2.0 (Modern)' : '1.x (Legacy)')).join(', ')}`}
                >
                  🔒 Только для Zapret {supportedEngines?.map((e) => (e === 'v2' ? '2.0 (Modern)' : '1.x (Legacy)')).join(', ')}
                </span>
              )}
            </div>
            <p className="muted small" style={{ margin: '6px 0 0', lineHeight: 1.45 }}>
              {desc}
            </p>
            {tags && tags.length > 0 && (
              <div style={{ display: 'flex', flexWrap: 'wrap', gap: 4, marginTop: 8 }}>
                {tags.map((t) => (
                  <span
                    key={t}
                    style={{
                      fontSize: 10,
                      padding: '1px 6px',
                      borderRadius: 4,
                      background: isChecked && isEngineSupported ? 'rgba(56, 189, 248, 0.12)' : 'rgba(255, 255, 255, 0.04)',
                      color: isChecked && isEngineSupported ? '#38bdf8' : 'var(--muted)',
                      border: isChecked && isEngineSupported ? '1px solid rgba(56, 189, 248, 0.25)' : '1px solid var(--border)',
                      fontFamily: 'Consolas, monospace',
                    }}
                  >
                    {t}
                  </span>
                ))}
              </div>
            )}
          </div>

          <button
            type="button"
            disabled={isCardDisabled}
            onClick={() => handleToggleFeature(key)}
            className={isPending ? 'zapret-glow-pulse-blue' : ''}
            style={{
              width: 50,
              height: 28,
              borderRadius: 16,
              border: 'none',
              cursor: isCardDisabled ? 'not-allowed' : 'pointer',
              background: isChecked && isEngineSupported
                ? 'linear-gradient(135deg, #38bdf8 0%, #2563eb 100%)'
                : 'rgba(255, 255, 255, 0.15)',
              position: 'relative',
              transition: 'all 0.25s cubic-bezier(0.4, 0, 0.2, 1)',
              padding: 2,
              flexShrink: 0,
              marginTop: 2,
              boxShadow: isChecked && isEngineSupported ? '0 0 12px rgba(56, 189, 248, 0.4)' : 'none',
            }}
            title={
              !isEngineSupported
                ? `Недоступно для текущего движка (требуется Zapret ${supportedEngines?.join(', ').toUpperCase()})`
                : isChecked
                ? 'Выключить блок'
                : 'Включить блок'
            }
          >
            <div
              style={{
                width: 24,
                height: 24,
                borderRadius: '50%',
                background: '#fff',
                transform: isChecked && isEngineSupported ? 'translateX(22px)' : 'translateX(0)',
                transition: 'transform 0.25s cubic-bezier(0.4, 0, 0.2, 1)',
                boxShadow: '0 1px 4px rgba(0,0,0,0.3)',
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                fontSize: 10,
                color: isChecked && isEngineSupported ? '#2563eb' : '#888',
                fontWeight: 'bold',
              }}
            >
              {isPending ? renderMicroSpinner(isChecked ? '#2563eb' : '#888', 13) : isChecked && isEngineSupported ? '✓' : '✕'}
            </div>
          </button>
        </div>

        <div
          style={{
            fontSize: 11,
            padding: '6px 10px',
            borderRadius: 6,
            background: 'rgba(0, 0, 0, 0.25)',
            display: 'flex',
            justifyContent: 'space-between',
            alignItems: 'center',
            color: 'var(--muted)',
            border: '1px solid rgba(255, 255, 255, 0.04)',
            overflow: 'hidden',
          }}
        >
          <span style={{ color: isChecked && isRunning && isEngineSupported ? '#38bdf8' : 'var(--muted)', fontWeight: 500 }}>
            {!isEngineSupported
              ? `🔒 Не поддерживается в Zapret ${activeEngine.toUpperCase()}`
              : !isRunning && isChecked
              ? `⚪ ${activeInfo} (Zapret остановлен)`
              : isChecked
              ? `🟢 ${activeInfo}`
              : `⚪ ${inactiveInfo}`}
          </span>
          <span style={{ fontSize: 10, opacity: 0.8 }}>
            {!isEngineSupported
              ? 'Только ' + supportedEngines?.join(', ').toUpperCase()
              : !isRunning && isChecked
              ? 'Остановлена со службой'
              : isChecked
              ? 'Активна в ' + (activeEngine === 'v2' ? 'nfqws2' : 'nfqws')
              : 'Отключена'}
          </span>
        </div>
      </div>
    )
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 20 }}>
      <style>{`
        @keyframes zapretSpin {
          0% { transform: rotate(0deg); }
          100% { transform: rotate(360deg); }
        }
        .zapret-spin {
          animation: zapretSpin 0.75s linear infinite;
        }
        @keyframes zapretGlowPulse {
          0%, 100% {
            box-shadow: 0 0 8px rgba(56, 189, 248, 0.4), 0 0 0 2px rgba(56, 189, 248, 0.3);
          }
          50% {
            box-shadow: 0 0 18px rgba(56, 189, 248, 0.8), 0 0 0 4px rgba(56, 189, 248, 0.5);
          }
        }
        @keyframes zapretGlowPulseGreen {
          0%, 100% {
            box-shadow: 0 0 8px rgba(34, 197, 94, 0.4), 0 0 0 2px rgba(34, 197, 94, 0.3);
          }
          50% {
            box-shadow: 0 0 18px rgba(34, 197, 94, 0.8), 0 0 0 4px rgba(34, 197, 94, 0.5);
          }
        }
        @keyframes zapretGlowPulsePurple {
          0%, 100% {
            box-shadow: 0 0 8px rgba(168, 85, 247, 0.4), 0 0 0 2px rgba(168, 85, 247, 0.3);
          }
          50% {
            box-shadow: 0 0 18px rgba(168, 85, 247, 0.8), 0 0 0 4px rgba(168, 85, 247, 0.5);
          }
        }
        .zapret-glow-pulse-blue {
          animation: zapretGlowPulse 1.2s ease-in-out infinite !important;
        }
        .zapret-glow-pulse-green {
          animation: zapretGlowPulseGreen 1.2s ease-in-out infinite !important;
        }
        .zapret-glow-pulse-purple {
          animation: zapretGlowPulsePurple 1.2s ease-in-out infinite !important;
        }
      `}</style>
      <div className="beta-banner">
        <span style={{ fontSize: 16 }}>🧪</span>
        <div>
          <strong>Бета-функция:</strong> Сетевые службы (Zapret, Игровой режим, AdGuard Home) работают с изоляцией netfilter/iptables.
        </div>
      </div>
      {/* 1. ГЛАВНАЯ КАРТОЧКА С ВЫКЛЮЧАТЕЛЕМ */}
      <section
        className="card"
        style={{
          position: 'relative',
          overflow: 'hidden',
          padding: '24px 26px',
          background: isRunning
            ? 'linear-gradient(135deg, rgba(34, 197, 94, 0.08) 0%, rgba(15, 23, 42, 0.6) 100%)'
            : 'rgba(255, 255, 255, 0.02)',
          border: isRunning ? '1px solid rgba(34, 197, 94, 0.3)' : '1px solid var(--border)',
          borderRadius: 16,
          boxShadow: isRunning ? '0 10px 30px rgba(34, 197, 94, 0.08)' : 'none',
        }}
      >
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: 16 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 16 }}>
            <div
              style={{
                width: 52,
                height: 52,
                borderRadius: 14,
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                background: isRunning ? 'rgba(34, 197, 94, 0.15)' : 'rgba(255, 255, 255, 0.05)',
                color: isRunning ? '#22c55e' : 'var(--muted)',
                fontSize: 26,
                boxShadow: isRunning ? '0 0 20px rgba(34, 197, 94, 0.25)' : 'none',
                transition: 'all 0.3s ease',
              }}
            >
              🛡️
            </div>
            <div>
              <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                <h2 style={{ margin: 0, fontSize: 20, fontWeight: 700 }}>Zapret — Обход DPI</h2>
                <span
                  className="badge"
                  style={{
                    background: isRunning ? 'rgba(34, 197, 94, 0.15)' : 'rgba(255, 255, 255, 0.06)',
                    color: isRunning ? '#22c55e' : 'var(--muted)',
                    border: `1px solid ${isRunning ? 'rgba(34, 197, 94, 0.3)' : 'var(--border)'}`,
                  }}
                >
                  {isRunning ? `🟢 Работает: ${activeEngine === 'v2' ? 'Zapret 2.0' : 'Zapret 1.x'} (PID: ${status?.pid})` : isInstalled ? '⚪ Выключен' : '🔴 Не установлен'}
                </span>
              </div>
              <div className="muted small" style={{ marginTop: 4 }}>
                Локальная десинхронизация TCP/UDP пакетов ({activeEngine === 'v2' ? 'Zapret 2.0 Lua Engine' : 'Zapret 1.x Legacy'}) для YouTube, Discord и сайтов без нагрузки на VPS
              </div>
            </div>
          </div>

          {/* ГЛАВНЫЙ ВЫКЛЮЧАТЕЛЬ (SWITCH) */}
          {isInstalled && (
            <div style={{ display: 'flex', alignItems: 'center', gap: 14 }}>
              <div style={{ textAlign: 'right' }}>
                <div style={{ fontSize: 13, fontWeight: 600, color: isRunning ? '#22c55e' : 'var(--muted)' }}>
                  {isRunning ? 'СЛУЖБА АКТИВНА' : 'СЛУЖБА ВЫКЛЮЧЕНА'}
                </div>
                <div className="muted small" style={{ fontSize: 11 }}>
                  {isRunning ? `LAN трафик фильтруется через ${activeEngine === 'v2' ? 'nfqws2' : 'nfqws'}` : 'Прямой трафик без изменений'}
                </div>
              </div>

              {(() => {
                const isMainPending = pendingKeys.has('enabled')
                return (
                  <button
                    type="button"
                    disabled={busy || isMainPending}
                    onClick={handleToggle}
                    className={isMainPending ? 'zapret-glow-pulse-green' : ''}
                    style={{
                      width: 64,
                      height: 34,
                      borderRadius: 20,
                      border: 'none',
                      cursor: busy || isMainPending ? 'wait' : 'pointer',
                      background: isRunning
                        ? 'linear-gradient(135deg, #22c55e 0%, #16a34a 100%)'
                        : 'rgba(255, 255, 255, 0.15)',
                      position: 'relative',
                      transition: 'all 0.25s cubic-bezier(0.4, 0, 0.2, 1)',
                      padding: 3,
                      boxShadow: isRunning ? '0 0 16px rgba(34, 197, 94, 0.4)' : 'none',
                    }}
                    title={isRunning ? 'Выключить Zapret' : 'Включить Zapret'}
                  >
                    <div
                      style={{
                        width: 28,
                        height: 28,
                        borderRadius: '50%',
                        background: '#fff',
                        transform: isRunning ? 'translateX(30px)' : 'translateX(0)',
                        transition: 'transform 0.25s cubic-bezier(0.4, 0, 0.2, 1)',
                        boxShadow: '0 2px 6px rgba(0,0,0,0.3)',
                        display: 'flex',
                        alignItems: 'center',
                        justifyContent: 'center',
                        fontSize: 12,
                        color: isRunning ? '#16a34a' : '#888',
                      }}
                    >
                      {isMainPending ? renderMicroSpinner('#16a34a', 15) : isRunning ? '✓' : '✕'}
                    </div>
                  </button>
                )
              })()}
            </div>
          )}
        </div>

        {/* СТАТУС-БАР ПОДСЕТИ И IPTABLES */}
        <div
          style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))',
            gap: 12,
            marginTop: 20,
            paddingTop: 16,
            borderTop: '1px solid var(--border)',
          }}
        >
          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <span style={{ fontSize: 16 }}>🎯</span>
            <div>
              <div className="muted small">Очередь ядра</div>
              <div style={{ fontSize: 13, fontWeight: 600 }}>NFQUEUE #200</div>
            </div>
          </div>

          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <span style={{ fontSize: 16 }}>🔥</span>
            <div>
              <div className="muted small">Перехват Netfilter</div>
              <div style={{ fontSize: 13, fontWeight: 600, color: status?.iptables_active ? '#22c55e' : 'var(--muted)' }}>
                {status?.iptables_active ? '🟢 Активен (Netfilter mangle)' : '⚪ Отключен'}
              </div>
            </div>
          </div>

          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <span style={{ fontSize: 16 }}>🚀</span>
            <div>
              <div className="muted small">Автозапуск</div>
              <div style={{ fontSize: 13, fontWeight: 600, color: status?.autostart ? '#22c55e' : 'var(--muted)' }}>
                {status?.autostart ? '🟢 S51zapret включен' : '⚪ Отключен'}
              </div>
            </div>
          </div>

          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <span style={{ fontSize: 16 }}>⚡</span>
            <div>
              <div className="muted small">Защита от сбоев</div>
              <div style={{ fontSize: 13, fontWeight: 600, color: '#38bdf8' }}>--queue-bypass (100% аптайм)</div>
            </div>
          </div>
        </div>

        {/* ПРЕДУПРЕЖДЕНИЕ: СЛУЖБА ЗАПУЩЕНА, НО IPTABLES ОТКЛЮЧЕН */}
        {isRunning && !status?.iptables_active && (
          <div
            style={{
              marginTop: 14,
              padding: '10px 14px',
              borderRadius: 8,
              background: 'rgba(234, 179, 8, 0.1)',
              border: '1px solid rgba(234, 179, 8, 0.3)',
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'space-between',
              gap: 12,
              flexWrap: 'wrap',
            }}
          >
            <div style={{ fontSize: 12, color: '#eab308' }}>
              ⚠️ Служба Zapret активна (PID: {status?.pid}), но перехват Netfilter отключен. Трафик не попадает в nfqws.
            </div>
            <button
              type="button"
              className="btn sm"
              disabled={busy}
              onClick={() => handleAction('start-fw')}
              style={{ background: '#eab308', color: '#000', fontWeight: 600, border: 'none' }}
              title="Восстановить перехват Netfilter (iptables) без перезапуска процесса nfqws"
            >
              🔄 Включить перехват
            </button>
          </div>
        )}

        {/* ПЕРЕКЛЮЧАТЕЛЬ ДВИЖКОВ (LEGACY 1.X / MODERN 2.0) И АВТОПОДСКАЗКА АРХИТЕКТУРЫ */}
        <div
          data-testid="zapret-engine-panel"
          style={{
            marginTop: 16,
            padding: '14px 16px',
            borderRadius: 12,
            background: 'linear-gradient(135deg, rgba(56, 189, 248, 0.08) 0%, rgba(168, 85, 247, 0.06) 100%)',
            border: '1px solid rgba(56, 189, 248, 0.25)',
            display: 'flex',
            flexDirection: 'column',
            gap: 12,
          }}
        >
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: 12 }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
              <span style={{ fontSize: 20 }}>🧠</span>
              <div>
                {activeEngine !== 'v2' && (
                  <div
                    data-testid="zapret-hardware-hint"
                    style={{ fontSize: 13, fontWeight: 700, color: '#38bdf8' }}
                  >
                    {hardwareHint}
                  </div>
                )}
                <div className="muted small" style={{ marginTop: 2, fontSize: 11.5 }}>
                  Автоопределение архитектуры ({targetArchLabel}) • каталог Zapret 2.0:{' '}
                  <code>/opt/zapret2</code> (<code>nfqws2</code> + Lua-библиотеки) • доступно переключение и безопасный откат на v1
                </div>
              </div>
            </div>

            {/* Переключатель движков: Legacy 1.x / Modern 2.0 */}
            <div
              role="group"
              aria-label="Переключатель движков Zapret"
              style={{
                display: 'inline-flex',
                background: 'rgba(15, 23, 42, 0.65)',
                padding: 3,
                borderRadius: 10,
                border: '1px solid var(--border)',
                gap: 4,
              }}
            >
              <button
                type="button"
                data-testid="engine-switch-v1"
                disabled={busy || switchingEngine || upgradingEngine}
                onClick={() => handleSwitchEngine('v1')}
                style={{
                  padding: '6px 12px',
                  borderRadius: 7,
                  border: 'none',
                  fontSize: 12,
                  fontWeight: 600,
                  cursor: busy ? 'wait' : 'pointer',
                  background: activeEngine === 'v1' ? 'rgba(234, 179, 8, 0.22)' : 'transparent',
                  color: activeEngine === 'v1' ? '#facc15' : 'var(--muted)',
                  transition: 'all 0.2s ease',
                }}
                title="Переключить на классический движок Legacy 1.x (nfqws)"
              >
                Legacy 1.x (nfqws)
              </button>
              <button
                type="button"
                data-testid="engine-switch-v2"
                disabled={busy || switchingEngine || upgradingEngine}
                onClick={() => (v2Installed ? handleSwitchEngine('v2') : handleUpgradeZapret2())}
                style={{
                  padding: '6px 12px',
                  borderRadius: 7,
                  border: 'none',
                  fontSize: 12,
                  fontWeight: 600,
                  cursor: busy ? 'wait' : 'pointer',
                  background:
                    activeEngine === 'v2'
                      ? 'linear-gradient(135deg, rgba(56, 189, 248, 0.25) 0%, rgba(168, 85, 247, 0.25) 100%)'
                      : 'transparent',
                  color: activeEngine === 'v2' ? '#38bdf8' : 'var(--muted)',
                  transition: 'all 0.2s ease',
                }}
                title="Переключить на современный движок Modern 2.0 (nfqws2 + Lua)"
              >
                Modern 2.0 (nfqws2)
              </button>
            </div>
          </div>

          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', flexWrap: 'wrap', gap: 10, width: '100%' }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
              <button
                type="button"
                data-testid="upgrade-zapret2-btn"
                className="btn sm"
                disabled={busy || upgradingEngine}
                onClick={handleUpgradeZapret2}
                style={{
                  background: status?.update_available
                    ? 'linear-gradient(135deg, #16a34a 0%, #22c55e 100%)'
                    : 'linear-gradient(135deg, #0284c7 0%, #7c3aed 100%)',
                  color: '#fff',
                  border: 'none',
                  fontWeight: 600,
                  boxShadow: status?.update_available
                    ? '0 4px 14px rgba(34, 197, 94, 0.35)'
                    : '0 4px 14px rgba(56, 189, 248, 0.25)',
                }}
                title="В 1 клик скачать актуальный бинарник nfqws2 и Lua-библиотеки в /opt/zapret2 с сохранением резервной копии v1"
              >
                {upgradingEngine
                  ? '⏳ Скачивание nfqws2 и Lua в /opt/zapret2…'
                  : status?.update_available
                  ? `🚀 Обновить Zapret 2 (${status.latest_version || 'v2'})`
                  : activeEngine === 'v2'
                  ? '🚀 Переустановить Zapret 2 (nfqws2)'
                  : '🚀 Обновить до Zapret 2 (nfqws2)'}
              </button>

              {canRollbackV1 && activeEngine === 'v2' && (
                <button
                  type="button"
                  data-testid="rollback-v1-btn"
                  className="btn sm ghost"
                  disabled={busy || switchingEngine}
                  onClick={() => handleSwitchEngine('v1')}
                  style={{
                    border: '1px solid rgba(250, 204, 21, 0.35)',
                    color: '#facc15',
                  }}
                  title="Безопасный откат на движок Zapret v1 (nfqws) с автоматической конвертацией аргументов"
                >
                  {switchingEngine ? '⏳ Откат на v1…' : '⏪ Откатить на v1 (Legacy 1.x)'}
                </button>
              )}
            </div>

            <div className="muted small" style={{ fontSize: 11, flexShrink: 0, marginLeft: 'auto' }}>
              Активный движок:{' '}
              <b style={{ color: activeEngine === 'v2' ? '#38bdf8' : '#facc15' }}>
                {activeEngine === 'v2' ? 'Modern 2.0 (nfqws2 + Lua)' : 'Legacy 1.x (nfqws)'}
              </b>
            </div>
          </div>

          {stepLog && (
            <div
              data-testid="zapret-step-log"
              style={{
                marginTop: 10,
                padding: '10px 14px',
                borderRadius: 8,
                background: 'rgba(15, 23, 42, 0.95)',
                border: '1px solid rgba(56, 189, 248, 0.35)',
                fontSize: 12,
                fontFamily: 'Consolas, monospace',
                whiteSpace: 'pre-wrap',
                maxHeight: 180,
                overflowY: 'auto',
                color: '#e2e8f0',
                lineHeight: 1.45,
              }}
            >
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 6, color: '#38bdf8', fontWeight: 600 }}>
                <span>📋 Лог установки:</span>
                <button
                  type="button"
                  onClick={() => setStepLog(null)}
                  style={{ background: 'transparent', border: 'none', color: 'var(--muted)', cursor: 'pointer', fontSize: 11 }}
                >
                  ✕ Закрыть
                </button>
              </div>
              {stepLog}
            </div>
          )}
        </div>

        {/* КНОПКИ ДЕЙСТВИЙ */}
        <div style={{ display: 'flex', gap: 10, flexWrap: 'wrap', marginTop: 16 }}>
          {isInstalled ? (
            <>
              <button
                type="button"
                className="btn sm"
                disabled={busy}
                onClick={() => handleAction('restart')}
                title="Перезапустить процесс nfqws и обновить iptables правила"
              >
                🔄 Перезапустить
              </button>

              <button
                type="button"
                className="btn sm"
                disabled={testingDpi || busy}
                onClick={handleTestDpi}
                style={{
                  background: 'linear-gradient(135deg, rgba(56, 189, 248, 0.15) 0%, rgba(59, 130, 246, 0.15) 100%)',
                  border: '1px solid rgba(56, 189, 248, 0.3)',
                  color: '#38bdf8',
                }}
                title="Проверить доступность YouTube и Discord напрямую с роутера"
              >
                {testingDpi ? '⏳ Тестирование DPI…' : '🧪 Тест YouTube & Discord'}
              </button>

              <button
                type="button"
                className="btn sm ghost"
                onClick={() => setShowHostsEditor((prev) => !prev)}
              >
                📋 {showHostsEditor ? 'Скрыть список доменов' : 'Список доменов (Hostlist)'}
              </button>

              <button
                type="button"
                className="btn sm ghost"
                onClick={() => setShowConfigEditor((prev) => !prev)}
              >
                📝 {showConfigEditor ? 'Скрыть конфиг' : 'zapret.conf'}
              </button>
            </>
          ) : (
            <button
              type="button"
              className="btn primary"
              disabled={busy}
              onClick={() => handleAction('install')}
            >
              {busy ? '⏳ Установка Zapret…' : '📥 Установить Zapret в 1 клик'}
            </button>
          )}
        </div>

        {/* РЕЗУЛЬТАТ ТЕСТА DPI */}
        {testResult && (
          <div
            style={{
              marginTop: 16,
              padding: '12px 16px',
              background: 'rgba(0, 0, 0, 0.25)',
              borderRadius: 10,
              border: '1px solid var(--border)',
              display: 'flex',
              gap: 20,
              flexWrap: 'wrap',
              alignItems: 'center',
            }}
          >
            <b style={{ fontSize: 13 }}>Результаты теста:</b>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <span>🎥 YouTube:</span>
              <span
                className="badge"
                style={{
                  background: testResult.youtube.ok ? 'rgba(34, 197, 94, 0.2)' : testResult.youtube.proxy_ok ? 'rgba(56, 189, 248, 0.2)' : 'rgba(239, 68, 68, 0.2)',
                  color: testResult.youtube.ok ? '#22c55e' : testResult.youtube.proxy_ok ? '#38bdf8' : '#ef4444',
                }}
              >
                {testResult.youtube.ok
                  ? `🟢 Прямой Zapret: HTTP ${testResult.youtube.code} (${Math.round(testResult.youtube.time_secs * 1000)} мс)`
                  : testResult.youtube.proxy_ok
                    ? `🔵 Через прокси: HTTP ${testResult.youtube.proxy_code || 200}`
                    : '🔴 Заблокирован ТСПУ'}
              </span>
            </div>

            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <span>💬 Discord:</span>
              <span
                className="badge"
                style={{
                  background: testResult.discord.ok ? 'rgba(34, 197, 94, 0.2)' : testResult.discord.proxy_ok ? 'rgba(56, 189, 248, 0.2)' : 'rgba(239, 68, 68, 0.2)',
                  color: testResult.discord.ok ? '#22c55e' : testResult.discord.proxy_ok ? '#38bdf8' : '#ef4444',
                }}
              >
                {testResult.discord.ok
                  ? `🟢 Прямой: HTTP ${testResult.discord.code} (${Math.round(testResult.discord.time_secs * 1000)} мс)`
                  : testResult.discord.proxy_ok
                    ? `🔵 Через прокси: HTTP ${testResult.discord.proxy_code || 200}`
                    : '🔴 Заблокирован'}
              </span>
            </div>
          </div>
        )}

        {/* РЕДАКТОР ZAPRET-HOSTS.TXT */}
        {showHostsEditor && (
          <div style={{ marginTop: 16, display: 'flex', flexDirection: 'column', gap: 10 }}>
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
              <span className="muted small">
                Редактирование <code>/opt/etc/zapret/zapret-hosts.txt</code> (по одному домену на строку):
              </span>
              <button
                type="button"
                className="btn sm primary"
                disabled={savingHosts}
                onClick={handleSaveHosts}
              >
                {savingHosts ? 'Сохранение…' : '💾 Сохранить список доменов'}
              </button>
            </div>
            <textarea
              className="input"
              rows={8}
              value={hostsDraft}
              disabled={savingHosts}
              onChange={(e) => { hostsDirtyRef.current = true; setHostsDraft(e.target.value) }}
              placeholder="rutracker.org&#10;ntc.party&#10;kinozal.tv"
              style={{ fontFamily: 'Consolas, monospace', fontSize: 12, resize: 'vertical' }}
            />
          </div>
        )}

        {/* РЕДАКТОР ZAPRET.CONF */}
        {showConfigEditor && (
          <div style={{ marginTop: 16, display: 'flex', flexDirection: 'column', gap: 10 }}>
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
              <span className="muted small">Редактирование <code>/opt/etc/zapret/zapret.conf</code>:</span>
              <button
                type="button"
                className="btn sm primary"
                disabled={savingConfig}
                onClick={handleSaveConfig}
              >
                {savingConfig ? 'Сохранение…' : '💾 Сохранить и применить'}
              </button>
            </div>
            <div style={{ fontSize: 12, padding: '6px 10px', background: 'rgba(234, 179, 8, 0.1)', border: '1px solid rgba(234, 179, 8, 0.3)', borderRadius: 6, color: '#eab308' }}>
              ⚠️ Прямое редактирование конфигурации демона Zapret. Изменяйте параметры только если уверены в их назначении. Спецсимволы командной строки запрещены.
            </div>
            <textarea
              className="input"
              rows={6}
              value={configDraft}
              disabled={savingConfig}
              onChange={(e) => { configDirtyRef.current = true; setConfigDraft(e.target.value) }}
              style={{ fontFamily: 'Consolas, monospace', fontSize: 12, resize: 'vertical' }}
            />
          </div>
        )}
      </section>

      {/* 2. LIVE-МОНИТОР И СЧЁТЧИК СПАСЁННОГО ТРАФИКА (DPI ANALYTICS) */}
      <AnalyticsSection
        analytics={analytics}
        resettingAnalytics={resettingAnalytics}
        busy={busy}
        isRunning={isRunning}
        handleResetAnalytics={handleResetAnalytics}
      />

      {/* 3 & 4. АВТОПОДБОР И SMART TV (2 КАРТОЧКИ В ОДНУ СТРОКУ) */}
      <BlockcheckAndSmartTvSection
        busy={busy}
        blockcheckResult={blockcheckResult}
        runningBlockcheck={runningBlockcheck}
        applyingStrategy={applyingStrategy}
        activeStrategyId={activeStrategyId}
        pendingKeys={pendingKeys}
        features={features}
        isRunning={isRunning}
        isInstalled={isInstalled}
        activeEngine={activeEngine}
        handleRunBlockcheck={handleRunBlockcheck}
        handleApplyStrategy={handleApplyStrategy}
        handleToggleFeature={handleToggleFeature}
      />

      {/* 5. АВТООБНОВЛЕНИЕ СПИСКОВ (COMMUNITY HOSTLISTS) */}
      <CommunityHostlistsSection
        busy={busy}
        communityUrl={communityUrl}
        setCommunityUrl={setCommunityUrl}
        syncingCommunity={syncingCommunity}
        pendingKeys={pendingKeys}
        features={features}
        isRunning={isRunning}
        isInstalled={isInstalled}
        handleSyncCommunityHostlist={handleSyncCommunityHostlist}
        handleSaveCommunityHostlistUrl={handleSaveCommunityHostlistUrl}
        handleToggleFeature={handleToggleFeature}
      />

      {/* 6. СЛУЖБЫ И СЕРВИСЫ (ПРЯМОЙ ОБХОД DPI БЕЗ VPS) */}
      <DirectServicesSection renderStrategyCard={renderStrategyCard} />

      {/* 3. ПОЛЬЗОВАТЕЛЬСКИЕ САЙТЫ & CDN BOOST (/boost) */}
      <CustomSitesBoostSection
        busy={busy}
        pendingKeys={pendingKeys}
        features={features}
        customDomainInput={customDomainInput}
        setCustomDomainInput={setCustomDomainInput}
        addingCustom={addingCustom}
        boostingDomain={boostingDomain}
        expandedCdnDomains={expandedCdnDomains}
        setExpandedCdnDomains={setExpandedCdnDomains}
        isRunning={isRunning}
        isInstalled={isInstalled}
        handleAddCustomDomain={handleAddCustomDomain}
        handleRemoveCustomDomain={handleRemoveCustomDomain}
        handleToggleCustomDomain={handleToggleCustomDomain}
        handleToggleAllCustomDomains={handleToggleAllCustomDomains}
        handleBoostCustomDomain={handleBoostCustomDomain}
      />

      {/* 4. РЕЖИМЫ ДЕСИНХРОНИЗАЦИИ (DPI TUNING) И БЫСТРЫЕ НАБОРЫ */}
      <DpiTuningSection
        status={status}
        busy={busy}
        pendingKeys={pendingKeys}
        isInstalled={isInstalled}
        activeEngine={activeEngine}
        setShowConfigEditor={setShowConfigEditor}
        handleApplyPreset={handleApplyPreset}
        handleResetFeatures={handleResetFeatures}
        renderStrategyCard={renderStrategyCard}
      />
    </div>
  )
}
