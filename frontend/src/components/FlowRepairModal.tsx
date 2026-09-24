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
  if (!isOpen) return null

  const [repairing, setRepairing] = useState(false)
  const [activeServer, setActiveServer] = useState<string>('')
  const [copiedScript, setCopiedScript] = useState(false)
  const [copiedLink, setCopiedLink] = useState(false)

  const cleanUrl = 'https://flow.google.com/?authuser=0&hl=en'
  const resetScript = `localStorage.clear(); sessionStorage.clear(); navigator.serviceWorker?.getRegistrations().then(r => r.forEach(reg => reg.unregister())); caches.keys().then(keys => keys.forEach(k => caches.delete(k))); location.href = '${cleanUrl}';`

  const handleRepair = async () => {
    setRepairing(true)
    try {
      const res = await apiPost<{ flow_server?: string; message?: string }>('flow/repair', {})
      if (res && res.flow_server) {
        setActiveServer(res.flow_server)
        notify(`✓ Маршрут Flow переключен на ${res.flow_server}, сокеты сброшены`)
        if (onRepaired) onRepaired(res.flow_server)
      } else {
        notify('✓ Сокеты ядра Mihomo сброшены')
      }
    } catch (e: any) {
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

  const handleCopyScript = async () => {
    const ok = await copyToClipboard(resetScript)
    if (ok) {
      setCopiedScript(true)
      notify('Команда сброса кэша скопирована в буфер обмена')
      setTimeout(() => setCopiedScript(false), 2500)
    }
  }

  const handleCopyLink = async () => {
    const ok = await copyToClipboard(cleanUrl)
    if (ok) {
      setCopiedLink(true)
      notify('Чистая ссылка скопирована в буфер обмена')
      setTimeout(() => setCopiedLink(false), 2500)
    }
  }

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div
        className="modal-card"
        style={{ maxWidth: 560 }}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="modal-header">
          <div className="modal-title-group">
            <span className="modal-icon">🛠️</span>
            <div>
              <h2 style={{ margin: 0, fontSize: 16 }}>Мастер починки Google Flow</h2>
              <p className="muted small" style={{ margin: '2px 0 0' }}>
                Устранение зависшего редиректа на unsupported-country в обычном окне
              </p>
            </div>
          </div>
          <button type="button" className="btn btn-sm" onClick={onClose} title="Закрыть">
            ✕
          </button>
        </div>

        <div className="modal-body" style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
          {/* Статус на роутере */}
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
                ✓ Роутер готов (маршрут США активен)
              </div>
              <div className="muted small" style={{ marginTop: 2 }}>
                Узел: <b>{activeServer || '🇺🇸 США Вашингтон'}</b> • Сокеты ядра сброшены
              </div>
            </div>
            <button
              type="button"
              className="btn btn-sm ghost"
              onClick={handleRepair}
              disabled={repairing}
              title="Повторить сброс сокетов на роутере"
            >
              {repairing ? '⏳ Сброс…' : '🔄 Повторить'}
            </button>
          </div>

          {/* Причина редиректа */}
          <div
            style={{
              background: 'var(--panel-2, rgba(255, 255, 255, 0.03))',
              border: '1px solid var(--border)',
              borderRadius: 8,
              padding: '10px 14px',
              fontSize: 12.5,
              lineHeight: 1.5,
            }}
          >
            💡 <b>Почему в Инкогнито всё работает, а в обычном окне редиректит?</b>
            <p style={{ margin: '6px 0 0', color: 'var(--muted)' }}>
              При первом неудачном открытии через европейский IP сайт <code>flow.google.com</code> сохранил в вашем обычном профиле браузера Service Worker и кэш с флагом <code>unsupported-country</code>.
            </p>
          </div>

          {/* Способ 1: Самый быстрый */}
          <div
            style={{
              border: '1px solid var(--border)',
              borderRadius: 8,
              padding: '12px 14px',
            }}
          >
            <div style={{ fontWeight: 600, fontSize: 13, marginBottom: 6 }}>
              ✨ Способ 1: Очистить данные сайта в браузере (10 секунд)
            </div>
            <ol style={{ margin: 0, paddingLeft: 18, fontSize: 12, lineHeight: 1.6, color: 'var(--text)' }}>
              <li>Перейдите на вкладку <code>flow.google.com</code>.</li>
              <li>Нажмите на значок <b>🔒 (настройки сайта)</b> слева от адреса в строке браузера.</li>
              <li>Выберите <b>«Файлы cookie и данные сайта»</b> ➔ <b>«Удалить»</b> (или <b>Clear site data</b>).</li>
              <li>Обновите страницу через <b>Ctrl + F5</b>.</li>
            </ol>
          </div>

          {/* Способ 2: 1-клик скрипт сброса */}
          <div
            style={{
              border: '1px solid var(--border)',
              borderRadius: 8,
              padding: '12px 14px',
            }}
          >
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 6 }}>
              <div style={{ fontWeight: 600, fontSize: 13 }}>
                ⚡ Способ 2: Команда сброса Service Worker (F12 Консоль)
              </div>
              <button
                type="button"
                className="btn btn-sm primary"
                style={{ fontSize: 11, padding: '3px 8px' }}
                onClick={handleCopyScript}
              >
                {copiedScript ? '✓ Скопировано!' : '📋 Скопировать команду'}
              </button>
            </div>
            <div className="muted small" style={{ marginBottom: 6 }}>
              Нажмите <code>F12</code> на странице Flow ➔ вкладка <b>Console</b> ➔ вставьте команду и нажмите <code>Enter</code>:
            </div>
            <textarea
              readOnly
              className="input mono"
              style={{ fontSize: 11, height: 48, resize: 'none' }}
              value={resetScript}
              onClick={(e) => (e.target as HTMLTextAreaElement).select()}
            />
          </div>
        </div>

        <div className="modal-footer" style={{ justifyContent: 'space-between', marginTop: 16 }}>
          <button
            type="button"
            className="btn ghost"
            onClick={handleCopyLink}
          >
            {copiedLink ? '✓ Ссылка скопирована!' : '🔗 Скопировать чистую ссылку'}
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
              🚀 Открыть чистый Flow ↗
            </a>
          </div>
        </div>
      </div>
    </div>
  )
}
