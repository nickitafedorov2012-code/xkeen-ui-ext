import { useState, useEffect, useCallback } from 'react'
import { apiGet, apiPost } from '../api'

interface UpdateInfoResponse {
  current: string
  latest: string
  update_available: boolean
  notes?: string[]
}

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

  const [updData, setUpdData] = useState<UpdateInfoResponse | null>(null)
  const [checking, setChecking] = useState(false)
  const [installing, setInstalling] = useState(false)
  const [stage, setStage] = useState('')
  const [countdown, setCountdown] = useState<number | null>(null)

  const checkUpdates = useCallback(async () => {
    setChecking(true)
    try {
      const res = await apiGet<UpdateInfoResponse>('update/check')
      setUpdData(res)
    } catch (e: any) {
      notify('Ошибка проверки обновлений: ' + (e?.message || e), true)
    } finally {
      setChecking(false)
    }
  }, [notify])

  useEffect(() => {
    checkUpdates()
  }, [checkUpdates])

  const curVer = updData?.current || initialCurrent || '—'
  const latVer = updData?.latest || initialLatest || '—'
  const hasUpdate = updData !== null ? Boolean(updData.update_available) : Boolean(latVer && curVer && latVer !== curVer && !curVer.includes(latVer))
  const notesList = updData?.notes || initialNotes || []

  const handleInstall = async () => {
    setInstalling(true)
    setStage('Загрузка обновления из GitHub Releases…')

    try {
      await apiPost<{ installed: string; restarting: boolean }>('update/install')
      setStage(`Версия ${latVer} успешно установлена! Перезапуск службы…`)
      notify(`Обновление ${latVer} установлено. Служба перезапускается.`)

      let left = 6
      setCountdown(left)
      const timer = setInterval(() => {
        left -= 1
        setCountdown(left)
        if (left <= 0) {
          clearInterval(timer)
          window.location.reload()
        }
      }, 1000)
    } catch (err: any) {
      setInstalling(false)
      setStage('')
      notify(err?.message || 'Ошибка при установке обновления', true)
    }
  }

  if (!isOpen) return null

  return (
    <div className="modal-backdrop">
      <div className="modal-card update-modal-card" style={{ maxWidth: 640, width: '94vw', maxHeight: '88vh', display: 'flex', flexDirection: 'column' }}>
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
        <div
          style={{
            background: 'linear-gradient(135deg, rgba(30, 41, 59, 0.5) 0%, rgba(15, 23, 42, 0.6) 100%)',
            border: '1px solid rgba(56, 189, 248, 0.2)',
            borderRadius: 10,
            padding: '14px 16px',
            marginBottom: 16,
            display: 'flex',
            justifyContent: 'space-between',
            alignItems: 'center',
            flexWrap: 'wrap',
            gap: 12,
            boxShadow: '0 4px 16px rgba(0, 0, 0, 0.2)',
          }}
        >
          <div>
            <div className="muted small" style={{ marginBottom: 4 }}>Текущая версия панели на роутере</div>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
              <span className="upd-dot" style={{ background: '#38bdf8', boxShadow: '0 0 8px #38bdf8' }} />
              <span style={{ fontWeight: 700, fontSize: 16, color: '#f8fafc' }}>
                {curVer}
              </span>
              {hasUpdate ? (
                <span className="badge" style={{ fontSize: 11, background: 'rgba(56, 189, 248, 0.18)', color: '#38bdf8', borderColor: 'rgba(56, 189, 248, 0.4)' }}>
                  ★ Доступна {latVer}
                </span>
              ) : (
                <span className="badge" style={{ fontSize: 11, background: 'rgba(56, 189, 248, 0.12)', color: '#38bdf8', borderColor: 'rgba(56, 189, 248, 0.3)' }}>
                  ✓ Актуальная версия
                </span>
              )}
            </div>
          </div>

          <div style={{ display: 'flex', gap: 8, alignItems: 'center', flexWrap: 'wrap' }}>
            <button
              className="btn sm"
              onClick={checkUpdates}
              disabled={checking || installing}
              title="Проверить наличие новой версии на GitHub"
            >
              {checking ? '⏳ Проверка…' : '🔄 Проверить обновления'}
            </button>
            {hasUpdate && !installing && (
              <button
                className="btn primary sm"
                onClick={handleInstall}
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

        {/* Блок списка изменений или статуса актуальности */}
        <div style={{ flex: 1, overflowY: 'auto', marginBottom: 14 }}>
          {hasUpdate && notesList.length > 0 ? (
            <div
              style={{
                background: 'rgba(0, 0, 0, 0.25)',
                border: '1px solid var(--border)',
                borderRadius: 10,
                padding: '14px 16px',
              }}
            >
              <div style={{ fontWeight: 600, fontSize: 13, marginBottom: 8, color: '#38bdf8' }}>
                Что нового в {latVer}:
              </div>
              <ul style={{ margin: 0, paddingLeft: 18, fontSize: 13, lineHeight: 1.6 }}>
                {notesList.map((n, i) => (
                  <li key={i} style={{ marginBottom: 4 }}>
                    {n}
                  </li>
                ))}
              </ul>
            </div>
          ) : (
            <div
              style={{
                background: 'rgba(0, 0, 0, 0.2)',
                border: '1px solid var(--border)',
                borderRadius: 10,
                padding: '24px 20px',
                textAlign: 'center',
              }}
            >
              <div style={{ fontSize: 32, marginBottom: 8 }}>✨</div>
              <div style={{ fontWeight: 600, fontSize: 14, color: 'var(--text-bright)', marginBottom: 4 }}>
                У вас установлена последняя стабильная версия панели ({curVer})
              </div>
              <div className="muted small">
                Все модули, компоненты и оптимизации работают в актуальном состоянии.
              </div>
            </div>
          )}
        </div>

        {/* Кнопка закрытия / отмены в футере */}
        <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 10 }}>
          {!installing && (
            <button type="button" className="btn ghost" onClick={onClose}>
              Закрыть
            </button>
          )}
          {hasUpdate && !installing && (
            <button
              type="button"
              className="btn btn-primary"
              onClick={handleInstall}
              style={{
                padding: '8px 18px',
                fontWeight: 600,
                background: 'linear-gradient(135deg, #0284c7, #2563eb)',
                boxShadow: '0 2px 10px rgba(37, 99, 235, 0.4)',
                borderColor: '#38bdf8',
              }}
            >
              🚀 Обновить до {latVer} в 1 клик
            </button>
          )}
        </div>
      </div>
    </div>
  )
}
