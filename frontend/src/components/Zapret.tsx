import { useState, useEffect } from 'react'
import { apiGet, apiPost } from '../api'
import type { ZapretStatus, DpiTestResult, ZapretFeatures } from '../types'

interface ZapretProps {
  notify: (msg: string, error?: boolean) => void
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
    hybrid_youtube: true,
    hybrid_discord: true,
    discord_voice_udp: true,
    youtube_turbo: false,
    general_bypass: true,
    aggressive_dpi: false,
    isolated_proxy: false,
  })
  const [togglingFeature, setTogglingFeature] = useState<string | null>(null)

  const loadStatus = async () => {
    try {
      const res = await apiGet<ZapretStatus>('zapret/status')
      setStatus(res)
      if (res.features) setFeatures(res.features)
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
    setBusy(true)
    try {
      const res = await apiPost<{ success: boolean; action: string; output?: string }>('zapret/action', {
        action: 'toggle',
      })
      notify(res.action === 'start' ? '🟢 Служба Zapret запущена' : '⚪ Служба Zapret остановлена')
      await loadStatus()
    } catch (e) {
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
        setFeatures(res.features)
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
        notify('✅ YouTube и Discord успешно доступны напрямую!')
      } else if (res.youtube.ok) {
        notify('⚠️ YouTube доступен напрямую, Discord проверяется')
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
        setFeatures(res.features)
      }
      notify(
        nextVal
          ? '🟢 Стратегия активирована и правила обновлены'
          : '⚪ Стратегия выключена'
      )
      await loadStatus()
    } catch (e) {
      setFeatures((prev) => ({ ...prev, [key]: !nextVal }))
      notify(e instanceof Error ? e.message : 'Ошибка переключения стратегии', true)
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
        setFeatures(res.features)
      }
      notify('Все стратегии сброшены к стандартным значениям')
      await loadStatus()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка сброса настроек', true)
    } finally {
      setBusy(false)
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
    inactiveInfo: string
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
          <div>
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
            title={isChecked ? 'Выключить стратегию' : 'Включить стратегию'}
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
                {status?.iptables_active ? '🟢 Активен (mangle -i br+)' : '⚪ Отключен'}
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
                  background: testResult.youtube.ok ? 'rgba(34, 197, 94, 0.2)' : 'rgba(239, 68, 68, 0.2)',
                  color: testResult.youtube.ok ? '#22c55e' : '#ef4444',
                }}
              >
                {testResult.youtube.ok ? `HTTP ${testResult.youtube.code} (${Math.round(testResult.youtube.time_secs * 1000)} мс)` : 'Блокируется'}
              </span>
            </div>

            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <span>💬 Discord:</span>
              <span
                className="badge"
                style={{
                  background: testResult.discord.ok ? 'rgba(34, 197, 94, 0.2)' : 'rgba(239, 68, 68, 0.2)',
                  color: testResult.discord.ok ? '#22c55e' : '#ef4444',
                }}
              >
                {testResult.discord.ok ? `HTTP ${testResult.discord.code} (${Math.round(testResult.discord.time_secs * 1000)} мс)` : 'Блокируется'}
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

      {/* 2. МУЛЬТИ-СТРАТЕГИИ ОБХОДА DPI И БЫСТРЫЕ НАБОРЫ */}
      <section className="card" style={{ padding: '22px 24px' }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: 12, marginBottom: 16 }}>
          <div>
            <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
              <h3 style={{ margin: 0, fontSize: 16, fontWeight: 700 }}>
                ⚡ Мульти-стратегии и режимы обхода DPI
              </h3>
              <span
                className="badge"
                style={{ background: 'rgba(56, 189, 248, 0.15)', color: '#38bdf8', border: '1px solid rgba(56, 189, 248, 0.3)' }}
              >
                Совместный выбор стратегий
              </span>
            </div>
            <div className="muted small" style={{ marginTop: 3 }}>
              Включайте любые стратегии одновременно: nfqws запускает независимый профиль для каждого протокола без конфликтов
            </div>
          </div>

          {/* БЫСТРЫЕ НАБОРЫ */}
          <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
            <button
              type="button"
              className="btn sm ghost"
              disabled={busy || !isInstalled}
              onClick={() => handleApplyPreset('gamer')}
              title="YouTube Turbo + Discord Web + Discord Voice RTC + Общий хостлист"
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
              title="Сбросить все стратегии к стандартным"
            >
              ↺ Сброс
            </button>
          </div>
        </div>

        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(340px, 1fr))', gap: 14 }}>
          {/* 1. YouTube Turbo */}
          {renderStrategyCard(
            'youtube_turbo',
            '🎥 YouTube Turbo (Fake + Disorder2)',
            'GGC DIRECT',
            'Нарушение очередности первого пакета (disorder2, pos=1) с отсечкой cutoff=d4. Устраняет буферизацию 4K видео с локальных кэшей Google GGC без нагрузки на VPS.',
            'disorder2 (pos=1, cutoff=d4) + DIRECT',
            'стандартный split2'
          )}

          {/* 2. Discord Web & Chat */}
          {renderStrategyCard(
            'hybrid_discord',
            '💬 Discord Web & Chat (Fake + Split2)',
            'DIRECT',
            'Прямое подключение к текстовым серверам, каналам и медиафайлам Discord напрямую через Zapret. Минимальный домашний пинг и быстрая загрузка картинок.',
            'split2 (cutoff=d4) + DIRECT',
            'через стандартный маршрут'
          )}

          {/* 3. Discord Voice RTC */}
          {renderStrategyCard(
            'discord_voice_udp',
            '🎙️ Discord Voice RTC (UDP 50000:65535)',
            'RTC VOICE',
            'Перехват голосовых шлюзов Discord в iptables mangle. Устраняет вечный статус «RTC Connecting» и потерю звука в голосовом канале.',
            'UDP 50000:65535 + L7 discord/stun',
            'только TCP/UDP 80/443'
          )}

          {/* 4. Общий веб-обход (Hostlist) */}
          {renderStrategyCard(
            'general_bypass',
            '🌐 Универсальный веб-обход (Hostlist)',
            'HOSTLIST',
            'Обход блокировок по списку доменов (/opt/etc/zapret/zapret-hosts.txt): RuTracker, NTC Party, Kinozal, Flibusta. Обычные сайты и банки не затрагиваются.',
            'zapret-hosts.txt (cutoff=d4)',
            'без фильтрации общего веб'
          )}

          {/* 5. Агрессивный режим ТСПУ */}
          {renderStrategyCard(
            'aggressive_dpi',
            '🔥 Агрессивный режим ТСПУ (seqovl + midsld + badseq)',
            'ТСПУ BOOST',
            'Перекрытие последовательностей (seqovl=1), сплит по середине SNI (midsld) и подделка badseq. Пробивает жесткие блокировки мобильных и кабельных операторов.',
            'seqovl=1, midsld, badseq, md5sig',
            'базовые стратегии'
          )}

          {/* 6. Изоляция IP-блокировок */}
          {renderStrategyCard(
            'isolated_proxy',
            '🔒 Изоляция IP-блокировок (ChatGPT, Claude, X -> PROXY)',
            'STRICT PROXY',
            'Разделение задач: Zapret обходит только цензуру по SNI (YouTube/Discord). Сервисы с жесткой блокировкой по IP (ChatGPT, Claude, Instagram, X/Twitter) гарантированно идут через VPS.',
            'AI и соцсети строго через VPS PROXY',
            'по общим правилам маршрутизации'
          )}
        </div>
      </section>
    </div>
  )
}
