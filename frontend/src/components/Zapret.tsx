import { useState, useEffect } from 'react'
import { apiGet, apiPost } from '../api'
import type { ZapretStatus, DpiTestResult } from '../types'

interface ZapretProps {
  notify: (msg: string, error?: boolean) => void
}

const PRESETS = [
  {
    id: 'general',
    name: '⚡ Универсальный (Рекомендуемый)',
    tag: 'Базовый',
    desc: 'Фейковые пакеты + разделение split2 с autottl=2 и md5sig. Стабилен для большинства провайдеров (Ростелеком, МТС, Мегафон, Дом.ру).',
    args: '--daemon --qnum=200 --dpi-desync=fake,split2 --dpi-desync-autottl=2 --dpi-desync-fooling=md5sig',
  },
  {
    id: 'youtube',
    name: '🎥 YouTube Turbo (Fake + Disorder)',
    tag: 'YouTube 4K',
    desc: 'Нарушение порядка пакетов на позиции 1 (disorder2). Устраняет зависание и буферизацию googlevideo.com.',
    args: '--daemon --qnum=200 --dpi-desync=fake,disorder2 --dpi-desync-split-pos=1 --dpi-desync-autottl=2 --dpi-desync-fooling=md5sig',
  },
  {
    id: 'discord',
    name: '💬 Discord + Voice (TCP + UDP)',
    tag: 'Discord',
    desc: 'Десинхронизация TCP 443 + голосовых UDP шлюзов (any-protocol с отсечкой cutoff=d4). Восстанавливает звонки и каналы.',
    args: '--daemon --qnum=200 --dpi-desync=fake,split2 --dpi-desync-autottl=2 --dpi-desync-any-protocol --dpi-desync-cutoff=d4',
  },
  {
    id: 'aggressive',
    name: '🛡️ Агрессивный (Жесткий ТСПУ / Мобильные)',
    tag: 'Максимум',
    desc: 'Перекрытие последовательностей (seqovl=1), сплит по середине SNI (midsld) и подделка badseq. Для операторов с глубоким анализом.',
    args: '--daemon --qnum=200 --dpi-desync=fake,disorder2 --dpi-desync-split-seqovl=1 --dpi-desync-split-pos=midsld --dpi-desync-fooling=badseq,md5sig',
  },
  {
    id: 'custom',
    name: '⚙️ Пользовательская стратегия',
    tag: 'Кастом',
    desc: 'Ручной ввод флагов и аргументов командной строки для nfqws.',
    args: '',
  },
]

export default function Zapret({ notify }: ZapretProps) {
  const [status, setStatus] = useState<ZapretStatus | null>(null)
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState(false)
  const [testResult, setTestResult] = useState<DpiTestResult | null>(null)
  const [testingDpi, setTestingDpi] = useState(false)

  // Кастомные аргументы
  const [customArgs, setCustomArgs] = useState('')
  const [showConfigEditor, setShowConfigEditor] = useState(false)
  const [configDraft, setConfigDraft] = useState('')
  const [savingConfig, setSavingConfig] = useState(false)

  const loadStatus = async () => {
    try {
      const res = await apiGet<ZapretStatus>('zapret/status')
      setStatus(res)
      if (res.config) setConfigDraft(res.config)
      if (res.cmdline) setCustomArgs(res.cmdline)
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

  const handleApplyPreset = async (presetId: string, args?: string) => {
    setBusy(true)
    try {
      const res = await apiPost<{ success: boolean; message?: string }>('zapret/action', {
        action: 'set_preset',
        preset: presetId,
        custom_args: args,
      })
      notify(res.message || `Пресет '${presetId}' успешно применен`)
      await loadStatus()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка применения пресета', true)
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
      notify('Конфигурация zapret.conf сохранена')
      await loadStatus()
      setShowConfigEditor(false)
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка сохранения конфига', true)
    } finally {
      setSavingConfig(false)
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
        notify('⚠️ YouTube доступен, но Discord заблокирован')
      } else {
        notify('❌ Проверка завершена: сервисы заблокированы текущим провайдером', true)
      }
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка тестирования DPI', true)
    } finally {
      setTestingDpi(false)
    }
  }

  if (loading && !status) {
    return (
      <div className="card" style={{ padding: 40, textAlign: 'center' }}>
        <div className="spinner" style={{ margin: '0 auto 16px' }} />
        <div className="muted">Загрузка модуля Zapret DPI…</div>
      </div>
    )
  }

  const isRunning = !!status?.running
  const isInstalled = !!status?.installed
  const activePreset = status?.preset || 'general'

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
                Локальная десинхронизация TCP/UDP пакетов (nfqws) для YouTube и Discord без расхода трафика VPS
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
                  {isRunning ? 'Трафик проходит через nfqws' : 'Прямой трафик без изменений'}
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
                {status?.iptables_active ? '🟢 Активен (mangle)' : '⚪ Отключен'}
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
              <div className="muted small">Режим безопасности</div>
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
                {testResult.youtube.ok ? `HTTP ${testResult.youtube.code} (${Math.round(testResult.youtube.time_secs * 1000)} мс)` : 'Заблокирован'}
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
                {testResult.discord.ok ? `HTTP ${testResult.discord.code} (${Math.round(testResult.discord.time_secs * 1000)} мс)` : 'Заблокирован'}
              </span>
            </div>
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

      {/* 2. СТРАТЕГИИ И ПРЕСЕТЫ ОБХОДА DPI */}
      <section className="card" style={{ padding: '22px 24px' }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 16 }}>
          <div>
            <h3 style={{ margin: 0, fontSize: 16, fontWeight: 700 }}>⚡ Стратегии обхода DPI (Пресеты)</h3>
            <div className="muted small" style={{ marginTop: 2 }}>
              Выберите проверенную стратегию для вашего интернет-провайдера
            </div>
          </div>
        </div>

        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(320px, 1fr))', gap: 14 }}>
          {PRESETS.map((p) => {
            const isActive = activePreset === p.id
            return (
              <div
                key={p.id}
                style={{
                  padding: '16px 18px',
                  borderRadius: 12,
                  background: isActive ? 'rgba(56, 189, 248, 0.08)' : 'rgba(255, 255, 255, 0.02)',
                  border: isActive ? '1px solid rgba(56, 189, 248, 0.4)' : '1px solid var(--border)',
                  display: 'flex',
                  flexDirection: 'column',
                  gap: 10,
                  transition: 'all 0.2s ease',
                  position: 'relative',
                }}
              >
                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start' }}>
                  <div>
                    <b style={{ fontSize: 14, color: isActive ? '#38bdf8' : 'var(--text)' }}>{p.name}</b>
                    <span
                      className="badge"
                      style={{
                        marginLeft: 8,
                        fontSize: 10,
                        background: isActive ? 'rgba(56, 189, 248, 0.2)' : 'rgba(255,255,255,0.05)',
                        color: isActive ? '#38bdf8' : 'var(--muted)',
                      }}
                    >
                      {p.tag}
                    </span>
                  </div>
                  {isActive && (
                    <span style={{ color: '#38bdf8', fontSize: 12, fontWeight: 700 }}>
                      ✓ Активен
                    </span>
                  )}
                </div>

                <div className="muted small" style={{ fontSize: 12, lineHeight: 1.4 }}>
                  {p.desc}
                </div>

                {p.args && (
                  <div
                    style={{
                      fontFamily: 'Consolas, monospace',
                      fontSize: 11,
                      background: 'rgba(0,0,0,0.3)',
                      padding: '6px 8px',
                      borderRadius: 6,
                      color: 'var(--muted)',
                      overflowX: 'auto',
                    }}
                  >
                    {p.args}
                  </div>
                )}

                {p.id === 'custom' && (
                  <input
                    className="input sm"
                    placeholder="Аргументы nfqws…"
                    value={customArgs}
                    onChange={(e) => setCustomArgs(e.target.value)}
                    style={{ fontFamily: 'Consolas, monospace', fontSize: 11 }}
                  />
                )}

                <div style={{ display: 'flex', justifyContent: 'flex-end', marginTop: 4 }}>
                  <button
                    type="button"
                    className={`btn sm ${isActive ? 'ghost' : 'primary'}`}
                    disabled={busy || !isInstalled}
                    onClick={() => handleApplyPreset(p.id, p.id === 'custom' ? customArgs : undefined)}
                  >
                    {isActive ? 'Применен' : 'Выбрать пресет'}
                  </button>
                </div>
              </div>
            )
          })}
        </div>
      </section>

      {/* 3. ЛУЧШИЕ СПОСОБЫ ИСПОЛЬЗОВАНИЯ И ГИБРИДНЫЙ РЕЖИМ */}
      <section className="card" style={{ padding: '22px 24px' }}>
        <h3 style={{ margin: '0 0 14px 0', fontSize: 16, fontWeight: 700 }}>
          💡 Лучшие способы использования Zapret в связке с XKeen Route
        </h3>

        <div style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
          <div style={{ display: 'flex', gap: 12, alignItems: 'flex-start' }}>
            <span style={{ fontSize: 20 }}>🚀</span>
            <div>
              <b style={{ fontSize: 13.5 }}>1. Гибридная маршрутизация (Экономия 100% трафика VPS)</b>
              <p className="muted small" style={{ margin: '4px 0 0', lineHeight: 1.5 }}>
                Направляйте <b>YouTube</b> и <b>Discord</b> напрямую через интернет провайдера (селектор <code>DIRECT</code>), включив Zapret. Так видео 4K воспроизводится без буферизации на полной гигабитной скорости домашнего тарифа, минуя узкий канал зарубежного сервера.
              </p>
            </div>
          </div>

          <div style={{ display: 'flex', gap: 12, alignItems: 'flex-start' }}>
            <span style={{ fontSize: 20 }}>🔒</span>
            <div>
              <b style={{ fontSize: 13.5 }}>2. Разделение обязанностей: Zapret для SNI, Mihomo для IP-блоков</b>
              <p className="muted small" style={{ margin: '4px 0 0', lineHeight: 1.5 }}>
                Запрет эффективен против замедлений по доменному имени (SNI). Для сервисов с жесткими блокировками по IP (Instagram, Twitter/X, OpenAI, Claude, Google AI) используйте селектор <code>PROXY</code> (VLESS/Shadowsocks).
              </p>
            </div>
          </div>

          <div style={{ display: 'flex', gap: 12, alignItems: 'flex-start' }}>
            <span style={{ fontSize: 20 }}>🛡️</span>
            <div>
              <b style={{ fontSize: 13.5 }}>3. Безопасность и отказоустойчивость (--queue-bypass)</b>
              <p className="muted small" style={{ margin: '4px 0 0', lineHeight: 1.5 }}>
                Правила перехвата на роутере Keenetic настроены с флагом <code>--queue-bypass</code>. Если служба остановлена выключателем выше или процесс завершился, трафик автоматически идёт напрямую, и интернет на устройствах не пропадет.
              </p>
            </div>
          </div>
        </div>
      </section>
    </div>
  )
}
