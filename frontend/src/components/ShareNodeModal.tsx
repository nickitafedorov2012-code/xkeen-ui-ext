import { useState, useMemo } from 'react'
import type { ServerInfo } from '../types'
import { exportServerToLink } from '../utils/nodeParser'
import { generateQrSvg } from '../utils/qrCode'
import { copyToClipboard } from '../utils/clipboard'

interface ShareNodeModalProps {
  server: ServerInfo | null
  isOpen?: boolean
  onClose: () => void
  notify: (msg: string, error?: boolean) => void
}

export default function ShareNodeModal({
  server,
  isOpen = true,
  onClose,
  notify,
}: ShareNodeModalProps) {
  const [copied, setCopied] = useState(false)

  // Базовая генерация ссылки подключения
  const defaultLink = useMemo(() => {
    if (!server) return ''
    return exportServerToLink({
      name: server.name,
      protocol: server.protocol,
      host: server.host,
      port: server.port,
      resolved_ip: server.resolved_ip,
      raw: server.raw,
    })
  }, [server])

  const [customLink, setCustomLink] = useState<string>('')

  // Актуальная ссылка (пользовательская или базовая)
  const link = customLink.trim() ? customLink : defaultLink

  // Проверка на наличие placeholder UUID / паролей из ядра
  const isPlaceholder = useMemo(() => {
    return (
      link.includes('00000000-0000-0000-0000-000000000000') ||
      link.includes(':password@') ||
      link.includes('auth=password')
    )
  }, [link])

  const qrImageUrl = useMemo(() => {
    return generateQrSvg(link, 220)
  }, [link])

  const handleCopy = async () => {
    const ok = await copyToClipboard(link)
    if (ok) {
      setCopied(true)
      notify('Ссылка подключения скопирована в буфер обмена')
      setTimeout(() => setCopied(false), 2500)
    } else {
      notify('Не удалось скопировать в буфер обмена', true)
    }
  }

  if (!isOpen || !server) return null

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal-card share-node-card" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <div className="modal-title-group">
            <span className="modal-icon">🔗</span>
            <div>
              <h2 style={{ margin: 0, fontSize: 16 }}>Ссылка подключения</h2>
              <p className="muted small" style={{ margin: '2px 0 0' }}>
                Экспорт прокси для телефона, ноутбука или роутера
              </p>
            </div>
          </div>
          <button type="button" className="btn btn-sm" onClick={onClose} title="Закрыть">
            ✕
          </button>
        </div>

        <div className="share-node-body">
          {/* Инфо о ноде */}
          <div className="share-node-info">
            <span className="badge" style={{ fontSize: 12, padding: '3px 8px' }}>
              {server.protocol.toUpperCase()}
            </span>
            <span className="share-node-name" title={server.name}>
              {server.name}
            </span>
            {server.host && server.host !== server.name && server.port > 0 && (
              <span className="muted small mono">
                {server.host}:{server.port}
              </span>
            )}
          </div>

          {isPlaceholder && (
            <div
              style={{
                background: 'rgba(234, 179, 8, 0.08)',
                border: '1px solid rgba(234, 179, 8, 0.25)',
                borderRadius: 8,
                padding: '8px 12px',
                fontSize: 11.5,
                color: '#eab308',
                lineHeight: 1.4,
              }}
            >
              ℹ️ <b>Безопасность ядра:</b> Mihomo скрывает секретные UUID серверов. Вы можете вставить рабочий ключ в поле ниже — QR-код обновится на лету.
            </div>
          )}

          <div className="share-node-grid">
            {/* Левая колонка: QR-код */}
            <div className="share-qr-box">
              <div className="share-qr-wrapper">
                <img
                  src={qrImageUrl}
                  alt="QR Code"
                  className="share-qr-img"
                  width={200}
                  height={200}
                />
              </div>
              <p className="muted small" style={{ textAlign: 'center', margin: '8px 0 0', fontSize: 11 }}>
                Сканируйте камерой телефона для быстрого импорта
              </p>
            </div>

            {/* Правая колонка: Ссылка и быстрое копирование */}
            <div className="share-link-box">
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                <label className="form-label" style={{ fontWeight: 600, fontSize: 12, margin: 0 }}>
                  Ссылка конфигурации:
                </label>
                {customLink && (
                  <button
                    type="button"
                    className="btn btn-sm ghost"
                    style={{ fontSize: 11, padding: '2px 6px' }}
                    onClick={() => setCustomLink('')}
                  >
                    Сброс
                  </button>
                )}
              </div>
              <textarea
                className="input share-link-textarea mono"
                value={customLink || defaultLink}
                onChange={(e) => setCustomLink(e.target.value)}
                rows={4}
                placeholder="Вставьте ссылку конфигурации..."
              />

              <div style={{ display: 'flex', gap: 8, marginTop: 10 }}>
                <button
                  type="button"
                  className="btn primary"
                  style={{ flex: 1 }}
                  onClick={handleCopy}
                >
                  {copied ? '✓ Скопировано!' : '📋 Скопировать ссылку'}
                </button>
              </div>

              <div className="share-instructions">
                <b>💡 Клиенты для подключения:</b>
                <ul>
                  <li><b>Android:</b> v2rayNG, NekoBox, Hiddify, Happ.</li>
                  <li><b>iOS / macOS:</b> Streisand, Shadowrocket, Sing-box, V2Box.</li>
                  <li><b>Windows:</b> Hiddify, Nekoray, v2rayN, Clash Verge.</li>
                </ul>
              </div>
            </div>
          </div>
        </div>

        <div className="modal-footer" style={{ justifyContent: 'flex-end' }}>
          <button type="button" className="btn" onClick={onClose}>
            Закрыть
          </button>
        </div>
      </div>
    </div>
  )
}
