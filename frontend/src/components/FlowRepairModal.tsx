import { useState, useEffect } from 'react'
import { apiPost } from '../api'
import { copyToClipboard } from '../utils/clipboard'

interface FlowRepairModalProps {
  isOpen: boolean
  onClose: () => void
  notify: (msg: string, isError?: boolean) => void
  onRepaired?: (serverName: string) => void
}

export default function FlowRepairModal({
  isOpen,
  onClose,
  notify,
  onRepaired,
}: FlowRepairModalProps) {

  const [repairing, setRepairing] = useState(false)
  const [activeServer, setActiveServer] = useState<string>('')
  const [isVerified, setIsVerified] = useState<boolean>(false)
  const [repairError, setRepairError] = useState<string | null>(null)
  const [copiedLink, setCopiedLink] = useState(false)

  const cleanUrl = 'https://flow.google.com/?authuser=0&hl=en'
  const extensionZipUrl = '/flow-unlock-extension.zip'

  const handleRepair = async () => {
    setRepairing(true)
    setRepairError(null)
    try {
      const res = await apiPost<{ success?: boolean; flow_server?: string; verified?: boolean; message?: string }>('flow/repair', {})
      if (res && res.flow_server) {
        setActiveServer(res.flow_server)
        setIsVerified(Boolean(res.verified))
        notify(`✓ Маршрут Flow переключен на ${res.flow_server}, сокеты сброшены`)
        if (onRepaired) onRepaired(res.flow_server)
      } else {
        setIsVerified(false)
        notify(res?.message || '✓ Сокеты ядра Mihomo сброшены')
      }
    } catch (e: any) {
      setRepairError(e.message || 'Ошибка выполнения сброса на роутере')
      setIsVerified(false)
      notify(e.message || 'Ошибка выполнения сброса на роутере', true)
    } finally {
      setRepairing(false)
    }
  }

  useEffect(() => {
    if (isOpen) {
      handleRepair()
    }
  }, [isOpen])

  const handleCopyLink = async () => {
    const ok = await copyToClipboard(cleanUrl)
    if (ok) {
      setCopiedLink(true)
      notify('Чистая ссылка скопирована в буфер обмена')
      setTimeout(() => setCopiedLink(false), 2500)
    }
  }

  if (!isOpen) return null

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div
        className="modal-card"
        style={{ maxWidth: 580, maxHeight: '90vh', overflowY: 'auto' }}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="modal-header">
          <div className="modal-title-group">
            <span className="modal-icon">🔓</span>
            <div>
              <h2 style={{ margin: 0, fontSize: 16 }}>Разблокировка Google Flow</h2>
              <p className="muted small" style={{ margin: '2px 0 0' }}>
                Обход регионального фильтра cPZSdc (Google AI Sandbox)
              </p>
            </div>
          </div>
          <button type="button" className="btn btn-sm" onClick={onClose} title="Закрыть">
            ✕
          </button>
        </div>

        <div className="modal-body" style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
          {/* Статус маршрута на роутере */}
          {repairing ? (
            <div
              style={{
                background: 'rgba(59, 130, 246, 0.08)',
                border: '1px solid rgba(59, 130, 246, 0.25)',
                borderRadius: 8,
                padding: '10px 14px',
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'space-between',
              }}
            >
              <div>
                <div style={{ fontWeight: 600, color: '#3b82f6', fontSize: 13 }}>
                  ⏳ Настройка маршрута и сброс сокетов…
                </div>
                <div className="muted small" style={{ marginTop: 2 }}>
                  Применение правил маршрутизации в ядре Mihomo
                </div>
              </div>
            </div>
          ) : repairError ? (
            <div
              style={{
                background: 'rgba(239, 68, 68, 0.08)',
                border: '1px solid rgba(239, 68, 68, 0.25)',
                borderRadius: 8,
                padding: '10px 14px',
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'space-between',
              }}
            >
              <div>
                <div style={{ fontWeight: 600, color: '#ef4444', fontSize: 13 }}>
                  ⚠️ Не удалось подтвердить маршрут Flow
                </div>
                <div className="muted small" style={{ marginTop: 2 }}>
                  {repairError}
                </div>
              </div>
              <button
                type="button"
                className="btn btn-sm ghost"
                onClick={handleRepair}
                disabled={repairing}
                title="Повторить сброс сокетов на роутере"
              >
                🔄 Повторить
              </button>
            </div>
          ) : (
            <div
              style={{
                background: 'rgba(34, 197, 94, 0.08)',
                border: '1px solid rgba(34, 197, 94, 0.25)',
                borderRadius: 8,
                padding: '10px 14px',
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'space-between',
              }}
            >
              <div>
                <div style={{ fontWeight: 600, color: '#22c55e', fontSize: 13 }}>
                  {isVerified ? '✓ Маршрут подтвержден и активен' : '✓ Сокеты сброшены'}
                </div>
                <div className="muted small" style={{ marginTop: 2 }}>
                  Узел: <b>{activeServer || 'Маршрут по умолчанию'}</b> {isVerified ? '• Проверен в рантайме ядра' : ''}
                </div>
              </div>
              <button
                type="button"
                className="btn btn-sm ghost"
                onClick={handleRepair}
                disabled={repairing}
                title="Повторить сброс сокетов на роутере"
              >
                🔄 Повторить
              </button>
            </div>
          )}

          {/* Карточка установки плагина */}
          <div
            style={{
              background: 'var(--panel-2, rgba(255, 255, 255, 0.03))',
              border: '1px solid var(--border)',
              borderRadius: 8,
              padding: '14px',
              display: 'flex',
              flexDirection: 'column',
              gap: 12,
            }}
          >
            <div>
              <div style={{ fontWeight: 700, fontSize: 13.5, marginBottom: 4 }}>
                Плагин для браузера Flow Unlocker
              </div>
              <p style={{ margin: 0, fontSize: 12, lineHeight: 1.5, color: 'var(--muted)' }}>
                Google Flow блокирует скрипты и закладки через политику Trusted Types. Нативный плагин внедряется на уровне самого браузера в главный контекст страницы (MAIN world) и гарантированно снимает геоблокировку.
              </p>
            </div>

            <a
              href={extensionZipUrl}
              download="xkeen-flow-unlock.zip"
              className="btn primary"
              style={{
                textDecoration: 'none',
                textAlign: 'center',
                padding: '11px 18px',
                fontSize: 13.5,
                fontWeight: 700,
                background: 'linear-gradient(135deg, #10b981 0%, #059669 100%)',
                boxShadow: '0 2px 10px rgba(16, 185, 129, 0.3)',
                border: 'none',
              }}
            >
              📥 Скачать плагин xkeen-flow-unlock.zip (6.5 КБ)
            </a>

            <div style={{ borderTop: '1px solid var(--border)', paddingTop: 12 }}>
              <div style={{ fontSize: 12, fontWeight: 700, marginBottom: 8, color: 'var(--text)' }}>
                Простая установка (30 секунд):
              </div>
              <ol style={{ margin: 0, paddingLeft: 18, fontSize: 12, lineHeight: 1.6, color: 'var(--text)' }}>
                <li>Скачайте и распакуйте <code>xkeen-flow-unlock.zip</code> в любую папку.</li>
                <li>В браузере откройте <code>chrome://extensions</code> (или <code>edge://extensions</code>).</li>
                <li>В правом верхнем углу включите тумблер <b>«Режим разработчика»</b> (Developer Mode).</li>
                <li>Нажмите кнопку <b>«Загрузить распакованное расширение»</b> (Load unpacked) и укажите папку.</li>
              </ol>
            </div>

            <div
              style={{
                background: 'rgba(56, 189, 248, 0.06)',
                border: '1px solid rgba(56, 189, 248, 0.2)',
                borderRadius: 6,
                padding: '8px 12px',
                fontSize: 11.5,
                lineHeight: 1.45,
                color: 'var(--muted)',
              }}
            >
              💡 <b>Совет:</b> Если вы ранее уже заходили на flow.google.com и браузер запомнил страницу ошибки в кэше — откройте сайт в новой вкладке режима <b>Инкогнито</b> или перезапустите браузер.
            </div>
          </div>
        </div>

        <div className="modal-footer" style={{ justifyContent: 'space-between', marginTop: 16 }}>
          <button
            type="button"
            className="btn ghost"
            onClick={handleCopyLink}
          >
            {copiedLink ? '✓ Ссылка скопирована!' : '🔗 Скопировать ссылку'}
          </button>
          <div style={{ display: 'flex', gap: 8 }}>
            <button type="button" className="btn" onClick={onClose}>
              Закрыть
            </button>
            <a
              href={cleanUrl}
              target="_blank"
              rel="noreferrer"
              className="btn primary"
              style={{ textDecoration: 'none' }}
              onClick={onClose}
            >
              🚀 Открыть Google Flow ↗
            </a>
          </div>
        </div>
      </div>
    </div>
  )
}
