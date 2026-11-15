import { useState, useEffect, useCallback, useRef } from 'react'
import { apiGet, apiPost } from '../api'
import { useUpdateCheck, type UpdateChannel } from '../hooks/useUpdateCheck'
import { compareVersions } from '../utils/version'

interface UpdateModalProps {
  isOpen: boolean
  onClose: () => void
  currentVersion: string
  latestVersion: string
  notes?: string[]
  notify: (msg: string, error?: boolean) => void
}

export default function UpdateModal({
  isOpen,
  onClose,
  currentVersion: initialCurrent,
  latestVersion: initialLatest,
  notes: initialNotes = [],
  notify,
}: UpdateModalProps) {

  const { updateInfo: sharedInfo, loading: hookLoading, refresh, setChannel } = useUpdateCheck()
  const [checking, setChecking] = useState(false)
  const [installing, setInstalling] = useState(false)
  const [stage, setStage] = useState('')
  const [countdown, setCountdown] = useState<number | null>(null)
  const [installError, setInstallError] = useState<string | null>(null)
  const [lastAllowDowngrade, setLastAllowDowngrade] = useState(false)
  const timerRef = useRef<ReturnType<typeof setInterval> | null>(null)
  const pollTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const channelRef = useRef<UpdateChannel | undefined>(sharedInfo?.channel)
  channelRef.current = sharedInfo?.channel

  useEffect(() => {
    return () => {
      if (timerRef.current) clearInterval(timerRef.current)
      if (pollTimeoutRef.current) clearTimeout(pollTimeoutRef.current)
    }
  }, [])

  const checkUpdates = useCallback(async (force = true) => {
    setChecking(true)
    setInstallError(null)
    try {
      await refresh({ channel: channelRef.current, force })
    } catch (e: any) {
      notify('Ошибка проверки обновлений: ' + (e?.message || e), true)
    } finally {
      setChecking(false)
    }
  }, [notify, refresh])

  useEffect(() => {
    if (!isOpen) return
    checkUpdates(false)
  }, [isOpen, checkUpdates])

  const channel: UpdateChannel = sharedInfo?.channel ?? (initialCurrent.includes('-') ? 'beta' : 'stable')
  const curVer = sharedInfo?.current || initialCurrent || '—'
  const latVer = sharedInfo?.latest || initialLatest || '—'
  const hasUpdate = sharedInfo !== null ? Boolean(sharedInfo.update_available) : Boolean(latVer && curVer && latVer !== curVer && !curVer.includes(latVer))
  const assetReady = sharedInfo?.asset_ready !== false
  const apiError = sharedInfo?.error
  const notesList = sharedInfo?.notes || initialNotes || []
  const canDowngrade =
    !hasUpdate &&
    channel === 'stable' &&
    curVer.includes('-') &&
    latVer !== '—' &&
    compareVersions(curVer, latVer) === 1

  const handleChannelSwitch = async (next: UpdateChannel) => {
    if (installing || next === channel) return
    setChecking(true)
    setInstallError(null)
    try {
      await setChannel(next)
    } catch (e: any) {
      notify('Не удалось переключить канал обновлений: ' + (e?.message || e), true)
    } finally {
      setChecking(false)
    }
  }

  const startStatusPolling = () => {
    if (timerRef.current) clearInterval(timerRef.current)
    if (pollTimeoutRef.current) clearTimeout(pollTimeoutRef.current)

    let elapsed = 0
    let sawRestart = false
    setCountdown(30)
    timerRef.current = setInterval(() => {
      elapsed += 1
      setCountdown((prev) => (prev !== null && prev > 0 ? prev - 1 : 0))
      if (elapsed >= 30) {
        if (timerRef.current) clearInterval(timerRef.current)
        window.location.reload()
      }
    }, 1000)

    const poll = async () => {
      try {
        const res = await apiGet<{ version?: string }>('status')
        const newVer = (res?.version || '').replace(/^v/, '')
        const oldVer = curVer.replace(/^v/, '')
        if (sawRestart || elapsed >= 6 || (newVer && newVer !== oldVer)) {
          if (timerRef.current) clearInterval(timerRef.current)
          window.location.reload()
          return
        }
      } catch {
        sawRestart = true
      }
      if (elapsed < 30) {
        pollTimeoutRef.current = setTimeout(poll, 1500)
      }
    }
    pollTimeoutRef.current = setTimeout(poll, 3000)
  }

  const handleInstall = async (allowDowngrade = false) => {
    setLastAllowDowngrade(allowDowngrade)
    setInstalling(true)
    setInstallError(null)
    setStage('Загрузка артефакта, проверка ELF-заголовка и создание бэкапа…')

    try {
      await apiPost<{ installed: string; restarting: boolean; success?: boolean }>(
        'update/install',
        {
          channel,
          target_version: latVer,
          allow_downgrade: allowDowngrade,
        },
        undefined,
        300_000,
      )
      setStage(`Версия ${latVer} успешно установлена! Ожидание запуска службы…`)
      notify(`Обновление ${latVer} установлено. Служба перезапускается.`)
      startStatusPolling()
    } catch (err: any) {
      setInstalling(false)
      setStage('')
      const errMsg = err?.message || 'Ошибка при установке обновления'
      setInstallError(errMsg)
      notify(errMsg, true)
    }
  }

  if (!isOpen) return null

  return (
    <div className="modal-backdrop" onClick={() => !installing && onClose()}>
      <div
        className="modal-card update-modal-card"
        onClick={(e) => e.stopPropagation()}
        style={{ maxWidth: 660, width: '94vw', maxHeight: '88vh', display: 'flex', flexDirection: 'column', padding: '22px 24px' }}
      >
        {/* Заголовок */}
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 16 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
            <div
              style={{
                width: 38,
                height: 38,
                borderRadius: 10,
                background: 'linear-gradient(135deg, rgba(56, 189, 248, 0.2), rgba(37, 99, 235, 0.2))',
                border: '1px solid rgba(56, 189, 248, 0.35)',
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                color: '#38bdf8',
                flexShrink: 0,
              }}
            >
              <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <path d="M21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16z" />
                <polyline points="3.27 6.96 12 12.01 20.73 6.96" />
                <line x1="12" y1="22.08" x2="12" y2="12" />
              </svg>
            </div>
            <div>
              <h3 style={{ margin: 0, fontSize: 18, fontWeight: 700 }}>Управление версией XKeen Route</h3>
              <div className="muted small">Официальные релизы веб-панели и обновление в 1 клик</div>
            </div>
          </div>
          {!installing && (
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

        {/* Карточка текущей версии */}
        <div className="update-version-card">
          <div>
            <div className="muted small" style={{ marginBottom: 4 }}>Текущая версия панели на роутере</div>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
              <span className="upd-dot" style={{ background: '#38bdf8', boxShadow: '0 0 8px #38bdf8' }} />
              <span style={{ fontWeight: 700, fontSize: 16, color: 'var(--text-bright)' }}>
                {curVer}
              </span>
              {hasUpdate ? (
                <span className="badge update-version-badge">
                  ★ Доступна {latVer}
                </span>
              ) : (
                <span className="badge update-version-badge is-current">
                  ✓ Актуальная версия
                </span>
              )}
            </div>
          </div>

          <div style={{ display: 'flex', gap: 8, alignItems: 'center', flexWrap: 'wrap' }}>
            <button
              className="btn sm"
              onClick={() => checkUpdates(true)}
              disabled={checking || installing}
              title="Проверить наличие новой версии на GitHub"
            >
              {checking ? '⏳ Проверка…' : '🔄 Проверить обновления'}
            </button>
            {hasUpdate && !installing && (
              <button
                className="btn primary sm"
                disabled={!assetReady}
                onClick={() => handleInstall(false)}
                title={`Обновить панель до ${latVer}`}
                style={{
                  background: 'linear-gradient(135deg, #0284c7, #2563eb)',
                  boxShadow: '0 2px 10px rgba(37, 99, 235, 0.4)',
                  borderColor: '#38bdf8',
                  fontWeight: 600,
                }}
              >
                🚀 Установить {latVer}
              </button>
            )}
          </div>
        </div>

        {/* Индикатор прогресса установки */}
        {installing && (
          <div
            style={{
              background: 'linear-gradient(135deg, rgba(2, 132, 199, 0.15) 0%, rgba(37, 99, 235, 0.15) 100%)',
              border: '1px solid rgba(56, 189, 248, 0.4)',
              borderRadius: 10,
              padding: '12px 16px',
              marginBottom: 16,
              display: 'flex',
              alignItems: 'center',
              gap: 12,
            }}
          >
            <span className="spin-icon" style={{ fontSize: 18, color: '#38bdf8' }}>⏳</span>
            <div>
              <div style={{ fontWeight: 600, color: '#38bdf8' }}>{stage}</div>
              {countdown !== null && (
                <div className="muted small" style={{ marginTop: 2 }}>
                  Перезагрузка веб-интерфейса через {countdown} сек…
                </div>
              )}
            </div>
          </div>
        )}

        {/* Ошибка установки с кнопкой повтора */}
        {installError && !installing && (
          <div
            style={{
              background: 'rgba(239, 68, 68, 0.12)',
              border: '1px solid rgba(239, 68, 68, 0.35)',
              borderRadius: 10,
              padding: '12px 16px',
              marginBottom: 16,
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'space-between',
              gap: 12,
            }}
          >
            <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
              <span style={{ fontSize: 20 }}>⚠️</span>
              <div>
                <div style={{ fontWeight: 600, color: '#f87171', fontSize: 13.5 }}>Ошибка установки обновления</div>
                <div className="muted small" style={{ color: '#fca5a5' }}>{installError}</div>
              </div>
            </div>
            <button
              type="button"
              className="btn sm"
              onClick={() => handleInstall(lastAllowDowngrade)}
              style={{ borderColor: '#f87171', color: '#f87171', flexShrink: 0 }}
            >
              🔄 Повторить
            </button>
          </div>
        )}

        {/* Переключатель канала обновлений + кнопка проверки */}
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 10, marginBottom: 12, flexWrap: 'wrap' }}>
          <div
            role="group"
            aria-label="Канал обновлений"
            style={{
              display: 'inline-flex',
              background: 'rgba(15, 23, 42, 0.5)',
              border: '1px solid var(--border-color)',
              borderRadius: 8,
              padding: 2,
              gap: 2,
            }}
          >
            {(['stable', 'beta'] as const).map((ch) => {
              const active = channel === ch
              return (
                <button
                  key={ch}
                  type="button"
                  disabled={installing || checking || hookLoading}
                  onClick={() => handleChannelSwitch(ch)}
                  style={{
                    padding: '5px 12px',
                    borderRadius: 6,
                    border: 'none',
                    fontSize: 12.5,
                    fontWeight: active ? 600 : 400,
                    cursor: installing ? 'not-allowed' : 'pointer',
                    background: active
                      ? ch === 'beta'
                        ? 'rgba(245, 158, 11, 0.2)'
                        : 'rgba(56, 189, 248, 0.2)'
                      : 'transparent',
                    color: active
                      ? ch === 'beta'
                        ? '#fbbf24'
                        : '#38bdf8'
                      : 'var(--text-muted)',
                  }}
                >
                  {ch === 'stable' ? 'Стабильный' : 'Бета'}
                </button>
              )
            })}
          </div>
          <button
            type="button"
            className="btn sm"
            disabled={installing || checking || hookLoading}
            onClick={() => checkUpdates(true)}
          >
            {checking || hookLoading ? '⏳ Проверка…' : '🔄 Проверить обновления'}
          </button>
        </div>

        {channel === 'beta' && (
          <div
            style={{
              background: 'rgba(245, 158, 11, 0.08)',
              border: '1px solid rgba(245, 158, 11, 0.3)',
              borderRadius: 8,
              padding: '8px 12px',
              marginBottom: 12,
              fontSize: 12,
              color: '#fbbf24',
            }}
          >
            ⚠️ Тестовые бета-сборки могут содержать экспериментальные изменения и работать нестабильно.
          </div>
        )}

        {apiError && (
          <div
            style={{
              background: 'rgba(239, 68, 68, 0.08)',
              border: '1px solid rgba(239, 68, 68, 0.3)',
              borderRadius: 8,
              padding: '8px 12px',
              marginBottom: 12,
              fontSize: 12,
              color: '#f87171',
            }}
          >
            {apiError}
          </div>
        )}

        {/* Блок списка изменений или статуса актуальности */}
        <div style={{ flex: 1, overflowY: 'auto', marginBottom: 14, display: 'flex', flexDirection: 'column', gap: 12 }}>
          {!hasUpdate && (
            <div
              style={{
                background: 'rgba(34, 197, 94, 0.08)',
                border: '1px solid rgba(34, 197, 94, 0.28)',
                borderRadius: 10,
                padding: '12px 16px',
                display: 'flex',
                alignItems: 'center',
                gap: 12,
              }}
            >
              <span style={{ fontSize: 22 }}>✨</span>
              <div>
                <div style={{ fontWeight: 600, fontSize: 13.5, color: 'var(--text-bright)' }}>
                  У вас установлена последняя стабильная версия панели ({curVer})
                </div>
                <div className="muted small">
                  Все модули, компоненты и оптимизации работают в актуальном состоянии.
                </div>
              </div>
            </div>
          )}

          {notesList.length > 0 && (
            <div className="update-notes-card">
              <div className="update-notes-title">
                <span>✨</span>
                <span>Что нового в {hasUpdate ? latVer : curVer}:</span>
              </div>
              <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
                {notesList.map((n, i) => {
                  const parts = n.split(/(\*\*.*?\*\*)/g)
                  return (
                    <div
                      key={i}
                      className="update-note-item"
                    >
                      <span style={{ color: '#38bdf8', fontSize: 14, flexShrink: 0, marginTop: 1 }}>•</span>
                      <div style={{ flex: 1 }}>
                        {parts.map((p, idx) => {
                          if (p.startsWith('**') && p.endsWith('**')) {
                            return (
                              <strong key={idx} style={{ color: 'var(--text-bright)', fontWeight: 600 }}>
                                {p.slice(2, -2)}
                              </strong>
                            )
                          }
                          return <span key={idx}>{p}</span>
                        })}
                      </div>
                    </div>
                  )
                })}
              </div>
            </div>
          )}
        </div>

        {/* Кнопка закрытия / отмены в футере */}
        <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 10, flexWrap: 'wrap', alignItems: 'center' }}>
          {!installing && (
            <button type="button" className="btn ghost" onClick={onClose}>
              Закрыть
            </button>
          )}
          {hasUpdate && !assetReady && !installing && (
            <span className="muted small" style={{ color: '#fbbf24' }}>
              ⏳ Сборка для вашей архитектуры ещё готовится CI, попробуйте через пару минут
            </span>
          )}
          {hasUpdate && !installing && (
            <button
              type="button"
              className="btn btn-primary"
              disabled={!assetReady}
              onClick={() => handleInstall(false)}
              style={{
                padding: '8px 18px',
                fontWeight: 600,
                background: assetReady ? 'linear-gradient(135deg, #0284c7, #2563eb)' : 'rgba(100, 116, 139, 0.3)',
                boxShadow: assetReady ? '0 2px 10px rgba(37, 99, 235, 0.4)' : 'none',
                borderColor: assetReady ? '#38bdf8' : 'transparent',
                cursor: assetReady ? 'pointer' : 'not-allowed',
              }}
            >
              🚀 Обновить до {latVer} в 1 клик
            </button>
          )}
          {canDowngrade && !installing && (
            <button
              type="button"
              className="btn btn-primary"
              disabled={!assetReady}
              onClick={() => handleInstall(true)}
              style={{
                padding: '8px 18px',
                fontWeight: 600,
                background: 'linear-gradient(135deg, #0ea5e9, #0284c7)',
                borderColor: '#38bdf8',
              }}
            >
              ↩️ Вернуться на стабильную {latVer}
            </button>
          )}
        </div>
      </div>
    </div>
  )
}
