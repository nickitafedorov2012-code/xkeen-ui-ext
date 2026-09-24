import { useState } from 'react'
import { apiPost } from '../api'

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
  currentVersion,
  latestVersion,
  notes = [],
  notify,
}: UpdateModalProps) {
  if (!isOpen) return null

  const [loading, setLoading] = useState(false)
  const [stage, setStage] = useState('')
  const [countdown, setCountdown] = useState<number | null>(null)

  const handleInstall = async () => {
    setLoading(true)
    setStage('Загрузка обновления из GitHub Releases…')

    try {
      await apiPost<{ installed: string; restarting: boolean }>('update/install')
      setStage(`Версия ${latestVersion} успешно установлена! Перезапуск службы…`)
      notify(`Обновление ${latestVersion} установлено. Служба перезапускается.`)

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
      setLoading(false)
      setStage('')
      notify(err?.message || 'Ошибка при установке обновления', true)
    }
  }

  return (
    <div className="modal-backdrop">
      <div className="modal-card update-modal-card" style={{ maxWidth: 520, width: '92vw' }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 16 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
            <span style={{ fontSize: 26, lineHeight: 1 }}>🚀</span>
            <h3 style={{ margin: 0, fontSize: 18 }}>Обновление XKeen Route</h3>
          </div>
          {!loading && (
            <button
              type="button"
              className="btn sm ghost"
              onClick={onClose}
              style={{ fontSize: 16, padding: '2px 8px', borderRadius: 6 }}
              title="Закрыть"
            >
              ✕
            </button>
          )}
        </div>

        {/* Баннер версий */}
        <div
          style={{
            background: 'rgba(56, 189, 248, 0.08)',
            border: '1px solid rgba(56, 189, 248, 0.25)',
            borderRadius: 10,
            padding: '12px 16px',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            marginBottom: 16,
          }}
        >
          <div>
            <div className="muted small">Текущая версия</div>
            <div style={{ fontWeight: 600, fontSize: 15 }}>{currentVersion || '—'}</div>
          </div>
          <div style={{ fontSize: 20, color: 'var(--muted)' }}>➔</div>
          <div>
            <div className="muted small">Доступная версия</div>
            <div style={{ fontWeight: 700, fontSize: 16, color: '#38bdf8' }}>{latestVersion || '—'}</div>
          </div>
        </div>

        {/* Список изменений (Release Notes) */}
        {notes && notes.length > 0 ? (
          <div style={{ marginBottom: 16 }}>
            <div style={{ fontWeight: 600, fontSize: 13, marginBottom: 8, color: 'var(--text)' }}>
              Что нового в {latestVersion}:
            </div>
            <div
              style={{
                maxHeight: 180,
                overflowY: 'auto',
                background: 'rgba(0, 0, 0, 0.2)',
                borderRadius: 8,
                padding: '10px 14px',
                border: '1px solid var(--border)',
              }}
            >
              <ul style={{ margin: 0, paddingLeft: 18, fontSize: 12.5, lineHeight: 1.5 }}>
                {notes.map((n, i) => (
                  <li key={i} style={{ marginBottom: 4 }}>
                    {n}
                  </li>
                ))}
              </ul>
            </div>
          </div>
        ) : (
          <p className="muted small" style={{ marginBottom: 16 }}>
            Новая версия с улучшениями производительности и исправлениями стабильности готова к установке в 1 клик.
          </p>
        )}

        {/* Индикатор прогресса */}
        {stage && (
          <div
            style={{
              background: 'rgba(34, 197, 94, 0.1)',
              border: '1px solid rgba(34, 197, 94, 0.3)',
              borderRadius: 8,
              padding: '10px 14px',
              marginBottom: 16,
              fontSize: 13,
              display: 'flex',
              alignItems: 'center',
              gap: 10,
            }}
          >
            <span className="spin-icon" style={{ fontSize: 16 }}>⏳</span>
            <div>
              <div style={{ fontWeight: 600, color: '#22c55e' }}>{stage}</div>
              {countdown !== null && (
                <div className="muted small" style={{ marginTop: 2 }}>
                  Перезагрузка интерфейса через {countdown} сек…
                </div>
              )}
            </div>
          </div>
        )}

        {/* Кнопки действий */}
        <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 10, marginTop: 8 }}>
          {!loading && (
            <button type="button" className="btn ghost" onClick={onClose}>
              Отмена
            </button>
          )}
          <button
            type="button"
            className="btn btn-primary"
            onClick={handleInstall}
            disabled={loading}
            style={{
              padding: '8px 18px',
              fontWeight: 600,
              boxShadow: '0 2px 10px rgba(37, 99, 235, 0.35)',
            }}
          >
            {loading ? '⏳ Установка обновления…' : `🚀 Установить ${latestVersion} в 1 клик`}
          </button>
        </div>
      </div>
    </div>
  )
}
