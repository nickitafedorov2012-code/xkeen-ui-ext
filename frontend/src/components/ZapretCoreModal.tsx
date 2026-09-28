import { useState, useEffect, useCallback } from 'react'
import { apiGet, apiPost } from '../api'
import type { ZapretStatus } from '../types'

interface ZapretUpdateCheckResp {
  current_engine?: 'v1' | 'v2'
  current_version?: string
  latest_version?: string
  update_available?: boolean
  label?: string
  v1_installed?: boolean
  v2_installed?: boolean
  notes?: string[]
}

interface Props {
  isOpen: boolean
  onClose: () => void
  notify: (msg: string, isError?: boolean) => void
  onUpdated?: () => void
}

export default function ZapretCoreModal({ isOpen, onClose, notify, onUpdated }: Props) {
  const [status, setStatus] = useState<ZapretStatus | null>(null)
  const [updInfo, setUpdInfo] = useState<ZapretUpdateCheckResp | null>(null)
  const [busy, setBusy] = useState(false)
  const [stage, setStage] = useState('')

  const loadData = useCallback(async () => {
    try {
      const [st, upd] = await Promise.all([
        apiGet<ZapretStatus>('zapret/status').catch(() => null),
        apiGet<ZapretUpdateCheckResp>('zapret/update/check').catch(() => null),
      ])
      if (st) setStatus(st)
      if (upd) setUpdInfo(upd)
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка загрузки данных Запрет', true)
    }
  }, [notify])

  useEffect(() => {
    if (isOpen) {
      loadData()
    }
  }, [isOpen, loadData])

  const handleSwitchEngine = async (targetEngine: 'v1' | 'v2') => {
    if (busy) return
    setBusy(true)
    setStage(targetEngine === 'v2' ? 'Переключение на Запрет 2 (Modern 2.0)…' : 'Переключение на Запрет 1 (Legacy 1.x)…')
    try {
      const res = await apiPost<{ success: boolean; message?: string }>('zapret/action', {
        action: 'switch_engine',
        engine: targetEngine,
      })
      notify(res?.message || `Движок Запрет успешно переключён на ${targetEngine === 'v2' ? 'Запрет 2.0' : 'Запрет 1.x'}`)
      window.dispatchEvent(
        new CustomEvent('xr:zapret-updated', {
          detail: {
            engine: targetEngine,
            update_available: false,
          },
        })
      )
      if (onUpdated) onUpdated()
      await loadData()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка смены движка Запрет', true)
    } finally {
      setBusy(false)
      setStage('')
    }
  }

  const handleUpgradeZapret2 = async () => {
    if (busy) return
    setBusy(true)
    setStage('Установка Zapret 2.0 (nfqws2 + Lua библиотеки)…')
    try {
      const res = await apiPost<{ success: boolean; message?: string }>('zapret/action', {
        action: 'upgrade_zapret2',
      })
      notify(res?.message || 'Zapret 2.0 успешно установлен')
      window.dispatchEvent(
        new CustomEvent('xr:zapret-updated', {
          detail: {
            engine: 'v2',
            version: 'v1.0.5.2',
            update_available: false,
          },
        })
      )
      if (onUpdated) onUpdated()
      await loadData()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка установки Zapret 2.0', true)
    } finally {
      setBusy(false)
      setStage('')
    }
  }

  if (!isOpen) return null

  const activeEngine = status?.engine || updInfo?.current_engine || 'v1'
  const isV2Installed = Boolean(status?.v2_installed || updInfo?.v2_installed || activeEngine === 'v2')
  const curVersion = status?.version || updInfo?.current_version || (activeEngine === 'v2' ? 'v1.0.5.2' : 'v72.13')
  const latVersion = updInfo?.latest_version || status?.latest_version || ''
  const hasUpdate = Boolean(updInfo?.update_available)

  const changelogItems = [
    {
      title: '⚡ Новый движок nfqws2 на базе Lua',
      desc: 'Поддержка современных модулей десинхронизации --lua-desync, Lua-скриптов обработки трафика и расширенной модификации пакетов.',
    },
    {
      title: '🎥 YouTube Web + googlevideo CDN',
      desc: '100% стабильный обход замедления и блокировок YouTube в 4K/60fps без прокси и нагрузки на VPS-канал.',
    },
    {
      title: '💬 Discord Web + Голосовые каналы (UDP Voice)',
      desc: 'Полноценный пробой блокировок текстовых сообщений и голосовых протоколов Discord без потери пакетов.',
    },
    {
      title: '🛡️ Стойкость к ТСПУ v2 нового поколения',
      desc: 'Алгоритмы multisplit, multidisorder, фейковые TLS SNI, рандомизация Session ID (rnd_sid) и перекрытие последовательностей (seqovl_ack).',
    },
    {
      title: '🔄 Мгновенный и безопасный откат',
      desc: 'Автоматическое сохранение резервной копии конфигурации v1 в /opt/etc/zapret/zapret.v1.conf.bak для быстрого возврата в любой момент.',
    },
    {
      title: '📊 Детальная аналитика за период',
      desc: 'Накопительный счётчик спасённого трафика, сохраняющийся между перезапусками служб роутера.',
    },
  ]

  return (
    <div className="modal-backdrop" onClick={() => !busy && onClose()}>
      <div
        className="modal-card zapret-modal-card"
        onClick={(e) => e.stopPropagation()}
        style={{ maxWidth: 660, width: '94vw', maxHeight: '88vh', display: 'flex', flexDirection: 'column', padding: '24px 28px' }}
      >
        {/* Заголовок */}
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 16 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
            <div
              style={{
                width: 38,
                height: 38,
                borderRadius: 10,
                background: 'linear-gradient(135deg, rgba(34, 197, 94, 0.2), rgba(16, 185, 129, 0.2))',
                border: '1px solid rgba(34, 197, 94, 0.35)',
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                color: '#22c55e',
                flexShrink: 0,
              }}
            >
              <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z" />
              </svg>
            </div>
            <div>
              <h3 style={{ margin: 0, fontSize: 18, fontWeight: 700 }}>Управление ядром Запрет (DPI)</h3>
              <div className="muted small">Выбор ядра 1 или 2, обновление версий и журнал нововведений</div>
            </div>
          </div>
          {!busy && (
            <button
              type="button"
              className="btn sm ghost"
              onClick={onClose}
              style={{ fontSize: 16, width: 32, height: 32, display: 'inline-flex', alignItems: 'center', justifyContent: 'center', borderRadius: 8, padding: 0 }}
              title="Закрыть"
            >
              ✕
            </button>
          )}
        </div>

        {/* Карточка текущей версии и выбор ядра */}
        <div
          style={{
            background: 'linear-gradient(135deg, rgba(30, 41, 59, 0.5) 0%, rgba(15, 23, 42, 0.6) 100%)',
            border: '1px solid rgba(34, 197, 94, 0.25)',
            borderRadius: 12,
            padding: '16px 18px',
            marginBottom: 16,
            display: 'flex',
            justifyContent: 'space-between',
            alignItems: 'center',
            flexWrap: 'wrap',
            gap: 14,
            boxShadow: '0 4px 16px rgba(0, 0, 0, 0.2)',
          }}
        >
          <div>
            <div className="muted small" style={{ marginBottom: 4 }}>Текущее активное ядро на роутере</div>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
              <span className="upd-dot" style={{ background: '#22c55e', boxShadow: '0 0 8px #22c55e' }} />
              <span style={{ fontWeight: 700, fontSize: 16, color: '#f8fafc' }}>
                {activeEngine === 'v2' ? 'Запрет 2 (nfqws2)' : 'Запрет 1 (nfqws)'} {curVersion}
              </span>
              <span
                className="badge"
                style={{
                  fontSize: 11,
                  background: activeEngine === 'v2' ? 'rgba(56, 189, 248, 0.15)' : 'rgba(234, 179, 8, 0.15)',
                  color: activeEngine === 'v2' ? '#38bdf8' : '#facc15',
                  borderColor: activeEngine === 'v2' ? 'rgba(56, 189, 248, 0.35)' : 'rgba(234, 179, 8, 0.35)',
                }}
              >
                {activeEngine === 'v2' ? 'Modern 2.0' : 'Legacy 1.x'}
              </span>
              {hasUpdate && (
                <span className="badge" style={{ fontSize: 11, background: 'rgba(34, 197, 94, 0.2)', color: '#4ade80', borderColor: 'rgba(34, 197, 94, 0.4)' }}>
                  Доступно {latVersion}
                </span>
              )}
            </div>
          </div>

          {/* Переключатель Ядро 1 / Ядро 2 */}
          <div style={{ display: 'flex', alignItems: 'center', gap: 6, background: 'rgba(15, 23, 42, 0.7)', padding: 4, borderRadius: 10, border: '1px solid var(--border)' }}>
            <button
              type="button"
              className="btn sm"
              disabled={busy}
              onClick={() => handleSwitchEngine('v1')}
              style={{
                background: activeEngine === 'v1' ? 'rgba(234, 179, 8, 0.25)' : 'transparent',
                color: activeEngine === 'v1' ? '#facc15' : 'var(--muted)',
                border: activeEngine === 'v1' ? '1px solid rgba(234, 179, 8, 0.4)' : 'none',
                fontWeight: 600,
                fontSize: 12,
                padding: '6px 12px',
              }}
            >
              Ядро 1 (Legacy)
            </button>
            <button
              type="button"
              className="btn sm"
              disabled={busy}
              onClick={() => {
                if (isV2Installed) {
                  handleSwitchEngine('v2')
                } else {
                  handleUpgradeZapret2()
                }
              }}
              style={{
                background: activeEngine === 'v2' ? 'rgba(56, 189, 248, 0.25)' : 'transparent',
                color: activeEngine === 'v2' ? '#38bdf8' : 'var(--muted)',
                border: activeEngine === 'v2' ? '1px solid rgba(56, 189, 248, 0.4)' : 'none',
                fontWeight: 600,
                fontSize: 12,
                padding: '6px 12px',
              }}
            >
              {isV2Installed ? 'Ядро 2 (Modern)' : '⚡ Установить Ядро 2'}
            </button>
          </div>
        </div>

        {/* Прогресс переключения / установки */}
        {busy && (
          <div
            style={{
              background: 'linear-gradient(135deg, rgba(34, 197, 94, 0.15) 0%, rgba(16, 185, 129, 0.15) 100%)',
              border: '1px solid rgba(34, 197, 94, 0.4)',
              borderRadius: 10,
              padding: '12px 16px',
              marginBottom: 16,
              display: 'flex',
              alignItems: 'center',
              gap: 12,
            }}
          >
            <span className="spin-icon" style={{ fontSize: 18, color: '#22c55e' }}>⏳</span>
            <div>
              <div style={{ fontWeight: 600, color: '#22c55e' }}>{stage || 'Выполнение операции…'}</div>
              <div className="muted small">Служба S51zapret перезапускается с выбранной конфигурацией</div>
            </div>
          </div>
        )}

        {/* Список нововведений */}
        <div style={{ flex: 1, overflowY: 'auto', marginBottom: 16, display: 'flex', flexDirection: 'column', gap: 10 }}>
          <div style={{ fontWeight: 600, fontSize: 13, color: '#38bdf8', marginBottom: 2 }}>
            Основные изменения и возможности Запрет 2.0:
          </div>
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(280px, 1fr))', gap: 10 }}>
            {changelogItems.map((item, idx) => (
              <div
                key={idx}
                style={{
                  background: 'rgba(0, 0, 0, 0.25)',
                  border: '1px solid var(--border)',
                  borderRadius: 10,
                  padding: '12px 14px',
                  display: 'flex',
                  flexDirection: 'column',
                  gap: 4,
                }}
              >
                <div style={{ fontWeight: 600, fontSize: 13, color: 'var(--text-bright)' }}>{item.title}</div>
                <div className="muted small" style={{ fontSize: 11.5, lineHeight: 1.45 }}>{item.desc}</div>
              </div>
            ))}
          </div>
        </div>

        {/* Футер */}
        <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 10 }}>
          {!busy && (
            <button type="button" className="btn ghost" onClick={onClose}>
              Закрыть
            </button>
          )}
          {hasUpdate && !busy && (
            <button
              type="button"
              className="btn btn-primary"
              onClick={handleUpgradeZapret2}
              style={{
                padding: '8px 18px',
                fontWeight: 600,
                background: 'linear-gradient(135deg, #16a34a, #22c55e)',
                boxShadow: '0 2px 10px rgba(34, 197, 94, 0.4)',
                borderColor: '#22c55e',
              }}
            >
              🚀 Обновить Запрет до {latVersion}
            </button>
          )}
        </div>
      </div>
    </div>
  )
}
