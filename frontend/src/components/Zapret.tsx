import { useState, useEffect } from 'react'
import { apiGet, apiPost } from '../api'
import type { ZapretStatus, DpiTestResult, ZapretFeatures } from '../types'

interface ZapretProps {
  notify: (msg: string, error?: boolean) => void
}

export function normalizeDomainInput(input: string): string {
  let s = input.trim().toLowerCase()
  s = s.replace(/^[a-z]+:\/\//i, '')
  s = s.split('/')[0].split('?')[0].split('#')[0]
  s = s.split('@').pop() || s
  s = s.split(':')[0]
  s = s.replace(/^www\./i, '')
  s = s.replace(/^\.+|\.+$/g, '')
  return s.trim()
}

export default function Zapret({ notify }: ZapretProps) {
  const [status, setStatus] = useState<ZapretStatus | null>(null)
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState(false)
  const [testResult, setTestResult] = useState<DpiTestResult | null>(null)
  const [testingDpi, setTestingDpi] = useState(false)

  // Конфигурация и хостлист
  const [showConfigEditor, setShowConfigEditor] = useState(false)
  const [configDraft, setConfigDraft] = useState('')
  const [savingConfig, setSavingConfig] = useState(false)

  const [showHostsEditor, setShowHostsEditor] = useState(false)
  const [hostsDraft, setHostsDraft] = useState('')
  const [savingHosts, setSavingHosts] = useState(false)

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
    custom_entries: [],
  })
  const [togglingFeature, setTogglingFeature] = useState<string | null>(null)

  // Пользовательские сайты и CDN Boost (/boost)
  const [customDomainInput, setCustomDomainInput] = useState('')
  const [addingCustom, setAddingCustom] = useState(false)
  const [boostingDomain, setBoostingDomain] = useState<string | null>(null)

  const loadStatus = async () => {
    try {
      const res = await apiGet<ZapretStatus>('zapret/status')
      setStatus(res)
      if (res.features) {
        setFeatures({
          ...res.features,
          custom_entries: res.features.custom_entries || [],
        })
      }
      if (res.config) setConfigDraft(res.config)
      if (res.hosts) setHostsDraft(res.hosts)
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка загрузки статуса Zapret', true)
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    loadStatus()
    const timer = setInterval(loadStatus, 10000)
    return () => clearInterval(timer)
  }, [])

  const handleToggle = async () => {
    const nextVal = !isRunning
    setBusy(true)
    setStatus((prev) => (prev ? { ...prev, running: nextVal } : prev))
    setFeatures((prev) => ({ ...prev, enabled: nextVal }))
    try {
      const res = await apiPost<{ success: boolean; action: string; output?: string }>('zapret/action', {
        action: nextVal ? 'start' : 'stop',
        enabled: nextVal,
      })
      notify(res.action === 'start' ? '🟢 Служба Zapret запущена' : '⚪ Служба Zapret остановлена')
      // Даём nfqws время на инициализацию демона перед опросом PID
      await new Promise((r) => setTimeout(r, 800))
      let statusRes = await apiGet<ZapretStatus>('zapret/status')
      if (nextVal && !statusRes.running) {
        await new Promise((r) => setTimeout(r, 1200))
        statusRes = await apiGet<ZapretStatus>('zapret/status')
      }
      setStatus(statusRes)
      if (statusRes.features) {
        setFeatures({
          ...statusRes.features,
          custom_entries: statusRes.features.custom_entries || [],
        })
      }
    } catch (e) {
      setStatus((prev) => (prev ? { ...prev, running: !nextVal } : prev))
      setFeatures((prev) => ({ ...prev, enabled: !nextVal }))
      notify(e instanceof Error ? e.message : 'Ошибка переключения Zapret', true)
    } finally {
      setBusy(false)
    }
  }

  const handleAction = async (act: string) => {
    setBusy(true)
    try {
      const res = await apiPost<{ success: boolean; message?: string }>('zapret/action', {
        action: act,
      })
      notify(res.message || `Действие ${act} выполнено`)
      await loadStatus()
    } catch (e) {
      notify(e instanceof Error ? e.message : `Ошибка действия ${act}`, true)
    } finally {
      setBusy(false)
    }
  }

  const handleApplyPreset = async (presetId: string) => {
    setBusy(true)
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
      await loadStatus()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка применения набора стратегий', true)
    } finally {
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
    setTogglingFeature(key)
    setFeatures((prev) => ({ ...prev, [key]: nextVal }))
    try {
      const res = await apiPost<{ success: boolean; features?: ZapretFeatures; message?: string }>('zapret/action', {
        action: 'toggle_feature',
        feature: key,
        enabled: nextVal,
      })
      if (res.features) {
        setFeatures({
          ...res.features,
          custom_entries: res.features.custom_entries || [],
        })
      }
      notify(
        nextVal
          ? '🟢 Блок активирован и правила обновлены'
          : '⚪ Блок выключен'
      )
      await loadStatus()
    } catch (e) {
      setFeatures((prev) => ({ ...prev, [key]: !nextVal }))
      notify(e instanceof Error ? e.message : 'Ошибка переключения блока', true)
    } finally {
      setTogglingFeature(null)
    }
  }

  const handleResetFeatures = async () => {
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
      await loadStatus()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка сброса настроек', true)
    } finally {
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
      await loadStatus()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка добавления сайта', true)
    } finally {
      setAddingCustom(false)
    }
  }

  const handleRemoveCustomDomain = async (domain: string) => {
    const clean = normalizeDomainInput(domain)
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
      await loadStatus()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка удаления сайта', true)
    }
  }

  const handleToggleCustomDomain = async (domain: string, currentEnabled: boolean) => {
    const clean = normalizeDomainInput(domain)
    const nextVal = !currentEnabled
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
      if (res.features) {
        setFeatures({
          ...res.features,
          custom_entries: res.features.custom_entries || [],
        })
      }
      notify(nextVal ? `🟢 ${clean} включен в обход DPI` : `⚪ ${clean} выключен`)
      await loadStatus()
    } catch (e) {
      setFeatures((prev) => ({
        ...prev,
        custom_entries: (prev.custom_entries || []).map((e) =>
          normalizeDomainInput(e.domain) === clean ? { ...e, enabled: currentEnabled } : e
        ),
      }))
      notify(e instanceof Error ? e.message : 'Ошибка переключения сайта', true)
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

  const isRunning = !!status?.running
  const isInstalled = !!status?.installed

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
    tags?: string[]
  ) => {
    const isChecked = !!features[key]
    const isBusy = togglingFeature === key || busy

    return (
      <div
        key={key}
        style={{
          padding: '16px 18px',
          borderRadius: 14,
          background: isChecked ? 'rgba(56, 189, 248, 0.06)' : 'rgba(255, 255, 255, 0.02)',
          border: isChecked ? '1px solid rgba(56, 189, 248, 0.35)' : '1px solid var(--border)',
          display: 'flex',
          flexDirection: 'column',
          justifyContent: 'space-between',
          gap: 12,
          transition: 'all 0.2s ease',
          boxShadow: isChecked ? '0 4px 16px rgba(56, 189, 248, 0.06)' : 'none',
        }}
      >
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start', gap: 12 }}>
          <div style={{ flex: 1 }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
              <b style={{ fontSize: 14, color: isChecked ? 'var(--text)' : 'var(--muted)' }}>{title}</b>
              <span
                className="badge"
                style={{
                  fontSize: 10,
                  background: isChecked ? 'rgba(56, 189, 248, 0.2)' : 'rgba(255, 255, 255, 0.05)',
                  color: isChecked ? '#38bdf8' : 'var(--muted)',
                  border: isChecked ? '1px solid rgba(56, 189, 248, 0.4)' : '1px solid var(--border)',
                }}
              >
                {badgeText}
              </span>
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
                      background: isChecked ? 'rgba(56, 189, 248, 0.12)' : 'rgba(255, 255, 255, 0.04)',
                      color: isChecked ? '#38bdf8' : 'var(--muted)',
                      border: isChecked ? '1px solid rgba(56, 189, 248, 0.25)' : '1px solid var(--border)',
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
            disabled={isBusy || !isInstalled}
            onClick={() => handleToggleFeature(key)}
            style={{
              width: 50,
              height: 28,
              borderRadius: 16,
              border: 'none',
              cursor: isBusy || !isInstalled ? 'not-allowed' : 'pointer',
              background: isChecked
                ? 'linear-gradient(135deg, #38bdf8 0%, #2563eb 100%)'
                : 'rgba(255, 255, 255, 0.15)',
              position: 'relative',
              transition: 'all 0.25s cubic-bezier(0.4, 0, 0.2, 1)',
              padding: 2,
              flexShrink: 0,
              marginTop: 2,
              boxShadow: isChecked ? '0 0 12px rgba(56, 189, 248, 0.4)' : 'none',
            }}
            title={isChecked ? 'Выключить блок' : 'Включить блок'}
          >
            <div
              style={{
                width: 24,
                height: 24,
                borderRadius: '50%',
                background: '#fff',
                transform: isChecked ? 'translateX(22px)' : 'translateX(0)',
                transition: 'transform 0.25s cubic-bezier(0.4, 0, 0.2, 1)',
                boxShadow: '0 1px 4px rgba(0,0,0,0.3)',
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                fontSize: 10,
                color: isChecked ? '#2563eb' : '#888',
                fontWeight: 'bold',
              }}
            >
              {isBusy ? '…' : isChecked ? '✓' : '✕'}
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
          <span style={{ color: isChecked ? '#38bdf8' : 'var(--muted)', fontWeight: 500 }}>
            {isChecked ? `🟢 ${activeInfo}` : `⚪ ${inactiveInfo}`}
          </span>
          <span style={{ fontSize: 10, opacity: 0.8 }}>
            {isChecked ? 'Активна в nfqws' : 'Отключена'}
          </span>
        </div>
      </div>
    )
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 20 }}>
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
                  {isRunning ? `🟢 Работает (PID: ${status?.pid})` : isInstalled ? '⚪ Выключен' : '🔴 Не установлен'}
                </span>
              </div>
              <div className="muted small" style={{ marginTop: 4 }}>
                Локальная десинхронизация TCP/UDP пакетов (nfqws) для YouTube, Discord и сайтов без нагрузки на VPS
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
                  {isRunning ? 'LAN трафик фильтруется через nfqws' : 'Прямой трафик без изменений'}
                </div>
              </div>

              <button
                type="button"
                disabled={busy}
                onClick={handleToggle}
                style={{
                  width: 64,
                  height: 34,
                  borderRadius: 20,
                  border: 'none',
                  cursor: busy ? 'wait' : 'pointer',
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
                  }}
                >
                  {busy ? '⏳' : isRunning ? '✓' : '✕'}
                </div>
              </button>
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
              onClick={() => handleAction('restart')}
              style={{ background: '#eab308', color: '#000', fontWeight: 600, border: 'none' }}
              title="Перезапустить службу и восстановить правила iptables"
            >
              🔄 Включить перехват
            </button>
          </div>
        )}

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
              onChange={(e) => setHostsDraft(e.target.value)}
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
              onChange={(e) => setConfigDraft(e.target.value)}
              style={{ fontFamily: 'Consolas, monospace', fontSize: 12, resize: 'vertical' }}
            />
          </div>
        )}
      </section>

      {/* 2. СЛУЖБЫ И СЕРВИСЫ (ПРЯМОЙ ОБХОД DPI БЕЗ VPS) */}
      <section className="card" style={{ padding: '22px 24px' }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: 12, marginBottom: 16 }}>
          <div>
            <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
              <h3 style={{ margin: 0, fontSize: 16, fontWeight: 700 }}>
                🛡️ Службы и сервисы (прямой обход DPI без VPS)
              </h3>
              <span
                className="badge"
                style={{ background: 'rgba(56, 189, 248, 0.15)', color: '#38bdf8', border: '1px solid rgba(56, 189, 248, 0.3)' }}
              >
                Независимые блоки
              </span>
            </div>
            <div className="muted small" style={{ marginTop: 3 }}>
              Каждый сервис можно выключить или включить отдельно. Трафик включенных сервисов десинхронизируется локально и не расходует лимиты VPS.
            </div>
          </div>
        </div>

        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(340px, 1fr))', gap: 14 }}>
          {/* 1. YouTube Direct */}
          {renderStrategyCard(
            'hybrid_youtube',
            '🎥 YouTube Direct (без VPS)',
            'YOUTUBE DIRECT',
            'Направляет видеопотоки YouTube и кэш-серверы googlevideo напрямую через локальный nfqws в обход прокси. Экономит зарубежный трафик и ускоряет 4K/8K.',
            'DIRECT через Zapret (без VPS)',
            'через VLESS-прокси',
            ['googlevideo.com', 'youtube.com', 'ytimg.com', 'ggpht.com']
          )}

          {/* 2. Discord Web & Chat */}
          {renderStrategyCard(
            'hybrid_discord',
            '💬 Discord Web & Chat',
            'DISCORD DIRECT',
            'Прямое подключение к текстовым серверам, каналам и медиафайлам Discord напрямую через Zapret. Минимальный домашний пинг и быстрая загрузка картинок.',
            'DIRECT через Zapret',
            'через стандартный маршрут',
            ['discord.com', 'discord.gg', 'discordapp.com', 'discord.media']
          )}

          {/* 3. GitHub (Релизы, исходники & аватары) */}
          {renderStrategyCard(
            'bypass_github',
            '🐙 GitHub (Релизы & Исходники)',
            'GITHUB DIRECT',
            'Прямой обход блокировок и замедлений GitHub: моментальный git clone, высокая скорость загрузки релизов, raw-файлов и аватаров без нагрузки на VPS.',
            'DIRECT через Zapret',
            'через стандартный маршрут',
            ['github.com', 'raw.githubusercontent.com', 'assets-cdn.github.com', 'objects.githubusercontent.com']
          )}

          {/* 4. Торренты & Трекеры */}
          {renderStrategyCard(
            'bypass_torrents',
            '🧲 Торренты & Трекеры',
            'TRACKERS DIRECT',
            'Прямой доступ к анонсам трекеров и скачиванию .torrent файлов: RuTracker, Kinozal, Rutor, Flibusta, NNMClub, Torlook. Работает на максимальной скорости провайдера.',
            'DIRECT через Zapret',
            'через стандартный маршрут',
            ['rutracker.org', 'kinozal.tv', 'rutor.info', 'flibusta.is', 'nnmclub.to']
          )}

          {/* 5. 🔞 18+ Контент */}
          {renderStrategyCard(
            'bypass_adult',
            '🔞 18+ Контент',
            'ADULT DIRECT',
            'Прямой локальный обход блокировок ТСПУ для популярных сайтов 18+ (Pornhub, Xvideos, XHamster, XNXX, RedTube, YouPorn) и их медиа-CDN без расхода трафика VPS.',
            'DIRECT через Zapret',
            'через стандартный маршрут',
            ['pornhub.com', 'xvideos.com', 'xhamster.com', 'xnxx.com', 'redtube.com', 'youporn.com']
          )}

          {/* 6. Универсальный веб-обход (Hostlist) */}
          {renderStrategyCard(
            'general_bypass',
            '🌐 Универсальный веб-обход (Hostlist)',
            'HOSTLIST',
            'Обход блокировок по системному списку доменов (/opt/etc/zapret/zapret-hosts.txt). Обычные сайты и банки не затрагиваются.',
            'zapret-hosts.txt (cutoff=d4)',
            'без фильтрации общего веб',
            ['/opt/etc/zapret/zapret-hosts.txt']
          )}
        </div>
      </section>

      {/* 3. ПОЛЬЗОВАТЕЛЬСКИЕ САЙТЫ & CDN BOOST (/boost) */}
      <section
        className="card"
        style={{
          padding: '22px 24px',
          background: 'linear-gradient(135deg, rgba(168, 85, 247, 0.05) 0%, rgba(15, 23, 42, 0.5) 100%)',
          border: '1px solid rgba(168, 85, 247, 0.3)',
          borderRadius: 16,
          boxShadow: '0 8px 24px rgba(168, 85, 247, 0.05)',
        }}
      >
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start', flexWrap: 'wrap', gap: 14, marginBottom: 16 }}>
          <div>
            <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
              <div
                style={{
                  width: 32,
                  height: 32,
                  borderRadius: 8,
                  background: 'rgba(168, 85, 247, 0.2)',
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'center',
                  fontSize: 16,
                }}
              >
                ⚡
              </div>
              <h3 style={{ margin: 0, fontSize: 16, fontWeight: 700 }}>
                Свой сайт & CDN Boost (/boost)
              </h3>
              <span
                className="badge"
                style={{
                  background: 'rgba(168, 85, 247, 0.15)',
                  color: '#c084fc',
                  border: '1px solid rgba(168, 85, 247, 0.4)',
                  fontWeight: 600,
                  fontSize: 11,
                }}
              >
                ⚡ /boost активен
              </span>
            </div>
            <div className="muted small" style={{ marginTop: 4, lineHeight: 1.45 }}>
              Напишите адрес любого сайта. XKeen автоматически найдёт связанные CDN, картинки и медиа-сервера, подтянет их в карточку другим цветом и направит на максимальной скорости напрямую без расхода VPS.
            </div>
          </div>
        </div>

        {/* ПОЛЕ ВВОДА САЙТА */}
        <form
          onSubmit={(e) => {
            e.preventDefault()
            handleAddCustomDomain()
          }}
          style={{ display: 'flex', gap: 10, flexWrap: 'wrap', alignItems: 'center', marginBottom: 14 }}
        >
          <input
            type="text"
            className="input"
            value={customDomainInput}
            onChange={(e) => setCustomDomainInput(e.target.value)}
            placeholder="Например: mysku.club, habr.com, speedtest.net, twitch.tv..."
            disabled={addingCustom || !isInstalled}
            style={{ flex: 1, minWidth: 260 }}
          />
          <button
            type="submit"
            className="btn primary"
            disabled={addingCustom || !isInstalled || !customDomainInput.trim()}
            style={{
              background: 'linear-gradient(135deg, #a855f7 0%, #7c3aed 100%)',
              border: 'none',
              boxShadow: '0 2px 10px rgba(168, 85, 247, 0.35)',
              display: 'flex',
              alignItems: 'center',
              gap: 6,
            }}
          >
            {addingCustom ? '⏳ Сканирование CDN…' : '⚡ Добавить & Boost'}
          </button>
        </form>

        {/* БЫСТРЫЕ ПОДСКАЗКИ САЙТОВ ДЛЯ ДОБАВЛЕНИЯ В 1 КЛИК */}
        <div style={{ display: 'flex', alignItems: 'center', gap: 6, flexWrap: 'wrap', marginBottom: 18 }}>
          <span className="muted small" style={{ fontSize: 11 }}>Быстрый выбор:</span>
          {['mysku.club', 'habr.com', 'speedtest.net', 'twitch.tv', '4pda.to', 'hdrezka.ag'].map((presetDomain) => {
            const alreadyAdded = (features.custom_entries || []).some(
              (e) => e.domain.toLowerCase() === presetDomain.toLowerCase()
            )
            return (
              <button
                key={presetDomain}
                type="button"
                className="btn sm ghost"
                disabled={addingCustom || alreadyAdded || !isInstalled}
                onClick={() => handleAddCustomDomain(presetDomain)}
                style={{
                  fontSize: 11,
                  padding: '2px 8px',
                  borderRadius: 6,
                  opacity: alreadyAdded ? 0.4 : 1,
                  background: alreadyAdded ? 'rgba(255, 255, 255, 0.03)' : 'rgba(168, 85, 247, 0.08)',
                  borderColor: alreadyAdded ? 'var(--border)' : 'rgba(168, 85, 247, 0.25)',
                  color: alreadyAdded ? 'var(--muted)' : '#c084fc',
                }}
                title={alreadyAdded ? 'Уже добавлен' : `Добавить ${presetDomain} и подтянуть его CDN`}
              >
                {alreadyAdded ? `✓ ${presetDomain}` : `+ ${presetDomain}`}
              </button>
            )
          })}
        </div>

        {/* СПИСОК ДОБАВЛЕННЫХ САЙТОВ С РАЗНОЦВЕТНЫМИ CDN БЕЙДЖАМИ */}
        {(!features.custom_entries || features.custom_entries.length === 0) ? (
          <div
            style={{
              padding: '20px 16px',
              textAlign: 'center',
              borderRadius: 12,
              background: 'rgba(0, 0, 0, 0.2)',
              border: '1px dashed rgba(255, 255, 255, 0.1)',
            }}
          >
            <div style={{ fontSize: 24, marginBottom: 6 }}>🌐</div>
            <div className="muted small">
              Пользовательские сайты пока не добавлены. Введите адрес сайта выше (например, <code>mysku.club</code>) и нажмите <b>«⚡ Добавить & Boost»</b>.
            </div>
          </div>
        ) : (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
            {features.custom_entries.map((entry) => {
              const isChecked = entry.enabled !== false
              const isBoosting = boostingDomain === entry.domain

              return (
                <div
                  key={entry.domain}
                  style={{
                    padding: '14px 16px',
                    borderRadius: 12,
                    background: isChecked ? 'rgba(168, 85, 247, 0.04)' : 'rgba(255, 255, 255, 0.01)',
                    border: isChecked ? '1px solid rgba(168, 85, 247, 0.3)' : '1px solid var(--border)',
                    display: 'flex',
                    flexDirection: 'column',
                    gap: 10,
                    transition: 'all 0.2s ease',
                  }}
                >
                  <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: 10 }}>
                    <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
                      {/* ОСНОВНОЙ ДОМЕН САЙТА — ЦВЕТ 1 (НЕБЕСНО-ГОЛУБОЙ) */}
                      <span
                        className="badge"
                        style={{
                          fontSize: 13,
                          fontWeight: 700,
                          padding: '5px 12px',
                          borderRadius: 8,
                          background: isChecked ? 'rgba(56, 189, 248, 0.15)' : 'rgba(255, 255, 255, 0.05)',
                          color: isChecked ? '#38bdf8' : 'var(--muted)',
                          border: isChecked ? '1px solid rgba(56, 189, 248, 0.4)' : '1px solid var(--border)',
                          letterSpacing: 0.3,
                        }}
                      >
                        🌐 {entry.domain}
                      </span>

                      <span
                        className="badge"
                        style={{
                          fontSize: 10,
                          background: isChecked ? 'rgba(34, 197, 94, 0.15)' : 'rgba(255, 255, 255, 0.05)',
                          color: isChecked ? '#22c55e' : 'var(--muted)',
                          border: `1px solid ${isChecked ? 'rgba(34, 197, 94, 0.3)' : 'var(--border)'}`,
                        }}
                      >
                        {isChecked ? '🟢 DIRECT (Zapret)' : '⚪ Выключен'}
                      </span>

                      {entry.cdns && entry.cdns.length > 0 && (
                        <span className="muted small" style={{ fontSize: 11 }}>
                          +{entry.cdns.length} CDN подтянуто
                        </span>
                      )}
                    </div>

                    <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                      {/* КНОПКА RE-BOOST */}
                      <button
                        type="button"
                        className="btn sm ghost"
                        disabled={isBoosting || !isInstalled}
                        onClick={() => handleBoostCustomDomain(entry.domain)}
                        style={{
                          fontSize: 11,
                          padding: '3px 8px',
                          color: '#c084fc',
                          borderColor: 'rgba(168, 85, 247, 0.3)',
                        }}
                        title="Повторно проверить и до-подтянуть новые CDN адреса сайта"
                      >
                        {isBoosting ? '⏳ Boost…' : '⚡ Boost'}
                      </button>

                      {/* КНОПКА УДАЛЕНИЯ */}
                      <button
                        type="button"
                        className="btn sm ghost"
                        disabled={!isInstalled}
                        onClick={() => handleRemoveCustomDomain(entry.domain)}
                        style={{ fontSize: 11, padding: '3px 8px', color: '#ef4444' }}
                        title="Удалить сайт из списка"
                      >
                        🗑️
                      </button>

                      {/* СВИТЧ ВКЛЮЧЕНИЯ/ВЫКЛЮЧЕНИЯ САЙТА ("Блоки, которые можно выключить") */}
                      <button
                        type="button"
                        disabled={!isInstalled}
                        onClick={() => handleToggleCustomDomain(entry.domain, isChecked)}
                        style={{
                          width: 44,
                          height: 24,
                          borderRadius: 14,
                          border: 'none',
                          cursor: !isInstalled ? 'not-allowed' : 'pointer',
                          background: isChecked
                            ? 'linear-gradient(135deg, #a855f7 0%, #7c3aed 100%)'
                            : 'rgba(255, 255, 255, 0.15)',
                          position: 'relative',
                          transition: 'all 0.25s cubic-bezier(0.4, 0, 0.2, 1)',
                          padding: 2,
                          flexShrink: 0,
                          boxShadow: isChecked ? '0 0 10px rgba(168, 85, 247, 0.4)' : 'none',
                        }}
                        title={isChecked ? 'Выключить обход для этого сайта' : 'Включить обход для этого сайта'}
                      >
                        <div
                          style={{
                            width: 20,
                            height: 20,
                            borderRadius: '50%',
                            background: '#fff',
                            transform: isChecked ? 'translateX(20px)' : 'translateX(0)',
                            transition: 'transform 0.25s cubic-bezier(0.4, 0, 0.2, 1)',
                            boxShadow: '0 1px 3px rgba(0,0,0,0.3)',
                            display: 'flex',
                            alignItems: 'center',
                            justifyContent: 'center',
                            fontSize: 9,
                            color: isChecked ? '#7c3aed' : '#888',
                            fontWeight: 'bold',
                          }}
                        >
                          {isChecked ? '✓' : '✕'}
                        </div>
                      </button>
                    </div>
                  </div>

                  {/* ПОДТЯНУТЫЕ CDN ДОМЕНЫ — ЦВЕТ 2 (ФИОЛЕТОВО-РОЗОВЫЙ /BOOST) */}
                  {entry.cdns && entry.cdns.length > 0 ? (
                    <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6, alignItems: 'center', paddingTop: 6, borderTop: '1px solid rgba(255, 255, 255, 0.04)' }}>
                      <span style={{ fontSize: 11, color: isChecked ? '#c084fc' : 'var(--muted)', fontWeight: 600, display: 'flex', alignItems: 'center', gap: 4 }}>
                        ⚡ /boost CDN:
                      </span>
                      {entry.cdns.map((cdn) => (
                        <span
                          key={cdn}
                          className="badge"
                          style={{
                            fontSize: 10,
                            fontFamily: 'Consolas, monospace',
                            padding: '3px 8px',
                            borderRadius: 6,
                            background: isChecked
                              ? 'linear-gradient(135deg, rgba(168, 85, 247, 0.18) 0%, rgba(217, 70, 239, 0.18) 100%)'
                              : 'rgba(255, 255, 255, 0.03)',
                            color: isChecked ? '#e879f9' : 'var(--muted)',
                            border: isChecked ? '1px solid rgba(168, 85, 247, 0.45)' : '1px solid var(--border)',
                            boxShadow: isChecked ? '0 1px 4px rgba(168, 85, 247, 0.1)' : 'none',
                          }}
                        >
                          ⚡ {cdn}
                        </span>
                      ))}
                    </div>
                  ) : (
                    <div style={{ fontSize: 11, color: 'var(--muted)', display: 'flex', alignItems: 'center', gap: 6, paddingTop: 4 }}>
                      <span>Связанные CDN пока не обнаружены (трафик идёт на основной домен). Нажмите <b>«⚡ Boost»</b> для поиска.</span>
                    </div>
                  )}
                </div>
              )
            })}
          </div>
        )}
      </section>

      {/* 4. РЕЖИМЫ ДЕСИНХРОНИЗАЦИИ (DPI TUNING) И БЫСТРЫЕ НАБОРЫ */}
      <section className="card" style={{ padding: '22px 24px' }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: 12, marginBottom: 16 }}>
          <div>
            <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
              <h3 style={{ margin: 0, fontSize: 16, fontWeight: 700 }}>
                ⚡ Режимы десинхронизации (DPI Tuning) & Быстрые наборы
              </h3>
              <span
                className="badge"
                style={{ background: 'rgba(56, 189, 248, 0.15)', color: '#38bdf8', border: '1px solid rgba(56, 189, 248, 0.3)' }}
              >
                Тонкая настройка
              </span>
            </div>
            <div className="muted small" style={{ marginTop: 3 }}>
              Специальные параметры nfqws для пробития глубоких блокировок ТСПУ и изоляции защищенных ресурсов
            </div>
          </div>

          {/* БЫСТРЫЕ НАБОРЫ */}
          <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
            <button
              type="button"
              className="btn sm ghost"
              disabled={busy || !isInstalled}
              onClick={() => handleApplyPreset('gamer')}
              title="YouTube Turbo + Discord Web + Discord Voice RTC + GitHub + Торренты"
            >
              🎮 Геймер / Медиа
            </button>
            <button
              type="button"
              className="btn sm ghost"
              disabled={busy || !isInstalled}
              onClick={() => handleApplyPreset('aggressive')}
              title="Все стратегии + Агрессивный режим для жестких ТСПУ"
            >
              🔥 Максимум (ТСПУ)
            </button>
            <button
              type="button"
              className="btn sm ghost"
              disabled={busy || !isInstalled}
              onClick={() => handleApplyPreset('youtube')}
              title="Только YouTube Turbo"
            >
              📺 Только YouTube
            </button>
            <button
              type="button"
              className="btn sm ghost"
              disabled={busy || !isInstalled}
              onClick={handleResetFeatures}
              title="Сбросить все блоки к стандартным"
            >
              ↺ Сброс
            </button>
          </div>
        </div>

        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(340px, 1fr))', gap: 14 }}>
          {/* 1. YouTube Turbo */}
          {renderStrategyCard(
            'youtube_turbo',
            '⚡ YouTube Turbo (Fake + Split2)',
            'GGC DIRECT',
            'Десинхронизация ClientHello (fake,split2, pos=1) с repeats=6, fooling=ts и отсечкой cutoff=d4. Пробивает блокировки ТСПУ и устраняет буферизацию с кэшей Google GGC без расхода VPS.',
            'fake,split2 (pos=1, repeats=6, ts, cutoff=d4)',
            'стандартный режим'
          )}

          {/* 2. Discord Voice RTC */}
          {renderStrategyCard(
            'discord_voice_udp',
            '🎙️ Discord Voice RTC (UDP 50000:65535)',
            'RTC VOICE',
            'Перехват голосовых шлюзов Discord в iptables mangle. Устраняет вечный статус «RTC Connecting» и потерю звука в голосовом канале.',
            'UDP 50000:65535 + L7 discord/stun',
            'только TCP/UDP 80/443'
          )}

          {/* 3. Агрессивный режим ТСПУ */}
          {renderStrategyCard(
            'aggressive_dpi',
            '🔥 Агрессивный режим ТСПУ (seqovl + midsld + ts)',
            'ТСПУ BOOST',
            'Перекрытие последовательностей (seqovl=1), сплит по середине SNI (pos=1,midsld), повторы repeats=6 и подделка ts,md5sig. Пробивает жесткие блокировки мобильных и кабельных операторов.',
            'seqovl=1, pos=1,midsld, repeats=6, ts,md5sig',
            'базовые стратегии'
          )}

          {/* 4. Изоляция IP-блокировок */}
          {renderStrategyCard(
            'isolated_proxy',
            '🔒 Изоляция IP-блокировок (ChatGPT, Claude, X -> PROXY)',
            'STRICT PROXY',
            'Разделение задач: Zapret обходит только цензуру по SNI (YouTube/Discord/GitHub). Сервисы с жесткой блокировкой по IP (ChatGPT, Claude, Instagram, X/Twitter) гарантированно идут через VPS.',
            'AI и соцсети строго через VPS PROXY',
            'по общим правилам маршрутизации'
          )}
        </div>
      </section>
    </div>
  )
}
