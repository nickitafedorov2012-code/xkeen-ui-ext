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
import { ZapretMainCard } from './zapret/ZapretMainCard'

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
      <ZapretMainCard
        status={status}
        isRunning={isRunning}
        isInstalled={isInstalled}
        activeEngine={activeEngine}
        v2Installed={v2Installed}
        canRollbackV1={canRollbackV1}
        hardwareHint={hardwareHint}
        targetArchLabel={targetArchLabel}
        busy={busy}
        pendingKeys={pendingKeys}
        switchingEngine={switchingEngine}
        upgradingEngine={upgradingEngine}
        stepLog={stepLog}
        setStepLog={setStepLog}
        testingDpi={testingDpi}
        testResult={testResult}
        showHostsEditor={showHostsEditor}
        setShowHostsEditor={setShowHostsEditor}
        hostsDraft={hostsDraft}
        setHostsDraft={setHostsDraft}
        hostsDirtyRef={hostsDirtyRef}
        savingHosts={savingHosts}
        showConfigEditor={showConfigEditor}
        setShowConfigEditor={setShowConfigEditor}
        configDraft={configDraft}
        setConfigDraft={setConfigDraft}
        configDirtyRef={configDirtyRef}
        savingConfig={savingConfig}
        handleToggle={handleToggle}
        handleAction={handleAction}
        handleSwitchEngine={handleSwitchEngine}
        handleUpgradeZapret2={handleUpgradeZapret2}
        handleTestDpi={handleTestDpi}
        handleSaveHosts={handleSaveHosts}
        handleSaveConfig={handleSaveConfig}
      />

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
