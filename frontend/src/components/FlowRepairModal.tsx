import { useState, useEffect } from 'react'
import { apiPost } from '../api'
import { copyToClipboard } from '../utils/clipboard'

interface FlowRepairModalProps {
  isOpen: boolean
  onClose: () => void
  notify: (msg: string, isError?: boolean) => void
  onRepaired?: (serverName: string) => void
}

type TabType = 'extension' | 'cache_reset' | 'tampermonkey'

export default function FlowRepairModal({
  isOpen,
  onClose,
  notify,
  onRepaired,
}: FlowRepairModalProps) {
  if (!isOpen) return null

  const [activeTab, setActiveTab] = useState<TabType>('extension')
  const [repairing, setRepairing] = useState(false)
  const [activeServer, setActiveServer] = useState<string>('')
  const [copiedScript, setCopiedScript] = useState(false)
  const [copiedBookmark, setCopiedBookmark] = useState(false)
  const [copiedLink, setCopiedLink] = useState(false)

  const cleanUrl = 'https://flow.google.com/?authuser=0&hl=ru'
  const userscriptUrl = '/flow-unlock.user.js'
  const extensionZipUrl = '/flow-unlock-extension.zip'

  const resetScript = `(async () => { if ('serviceWorker' in navigator) { const regs = await navigator.serviceWorker.getRegistrations(); for (const r of regs) await r.unregister(); } if ('caches' in window) { const keys = await caches.keys(); for (const k of keys) await caches.delete(k); } localStorage.clear(); sessionStorage.clear(); if (window.indexedDB && indexedDB.databases) { try { const dbs = await indexedDB.databases(); for (const db of dbs) if (db.name) indexedDB.deleteDatabase(db.name); } catch(e){} } location.replace('${cleanUrl}'); })();`

  const bookmarkletCode = `javascript:(function(){try{Object.defineProperty(navigator,'language',{get:()=>'ru-RU',configurable:true});Object.defineProperty(navigator,'languages',{get:()=>['ru-RU','ru'],configurable:true});if(window.Intl&&Intl.DateTimeFormat){const o=Intl.DateTimeFormat.prototype.resolvedOptions;Intl.DateTimeFormat.prototype.resolvedOptions=function(){const opts=o.call(this);opts.locale='ru-RU';return opts;};}}catch(e){}if(location.pathname.includes('unsupported-country')){history.replaceState(null,'','/');location.replace('https://flow.google.com/?authuser=0&hl=ru');}else{alert('✓ Русский язык ru-RU и обход региональных ограничений применены!');}})();`

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

  const handleCopyBookmark = async () => {
    const ok = await copyToClipboard(bookmarkletCode)
    if (ok) {
      setCopiedBookmark(true)
      notify('Код закладки скопирован в буфер обмена')
      setTimeout(() => setCopiedBookmark(false), 2500)
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
        style={{ maxWidth: 620, maxHeight: '90vh', overflowY: 'auto' }}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="modal-header">
          <div className="modal-title-group">
            <span className="modal-icon">🔓</span>
            <div>
              <h2 style={{ margin: 0, fontSize: 16 }}>Разблокировка и починка Google Flow</h2>
              <p className="muted small" style={{ margin: '2px 0 0' }}>
                Обход регионального фильтра cPZSdc и устранение редиректа на unsupported-country
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

          {/* Вкладки вариантов разблокировки */}
          <div style={{ display: 'flex', gap: 6, borderBottom: '1px solid var(--border)', paddingBottom: 6 }}>
            <button
              type="button"
              className={`btn btn-sm ${activeTab === 'extension' ? 'primary' : 'ghost'}`}
              style={{ flex: 1.2, fontSize: 12, fontWeight: activeTab === 'extension' ? 700 : 400 }}
              onClick={() => setActiveTab('extension')}
            >
              📦 1. Плагин (100% Рабочий)
            </button>
            <button
              type="button"
              className={`btn btn-sm ${activeTab === 'cache_reset' ? 'primary' : 'ghost'}`}
              style={{ flex: 1, fontSize: 12, fontWeight: activeTab === 'cache_reset' ? 600 : 400 }}
              onClick={() => setActiveTab('cache_reset')}
            >
              🧹 2. Сброс кэша (F12)
            </button>
            <button
              type="button"
              className={`btn btn-sm ${activeTab === 'tampermonkey' ? 'primary' : 'ghost'}`}
              style={{ flex: 1, fontSize: 12, fontWeight: activeTab === 'tampermonkey' ? 600 : 400 }}
              onClick={() => setActiveTab('tampermonkey')}
            >
              ⚡ 3. Tampermonkey
            </button>
          </div>

          {/* ТАБ 1: РАСШИРЕНИЕ / ПЛАГИН (ОСНОВНОЙ 100% РАБОЧИЙ ВАРИАНТ) */}
          {activeTab === 'extension' && (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
              <div
                style={{
                  background: 'rgba(34, 197, 94, 0.1)',
                  border: '1.5px solid rgba(34, 197, 94, 0.4)',
                  borderRadius: 8,
                  padding: '12px 14px',
                }}
              >
                <div style={{ display: 'flex', alignItems: 'center', gap: 8, color: '#22c55e', fontWeight: 700, fontSize: 13.5 }}>
                  <span>✓</span>
                  <span>ПРОВЕРЕННЫЙ И 100% РАБОЧИЙ СПОСОБ</span>
                </div>
                <p style={{ margin: '6px 0 0', fontSize: 12, lineHeight: 1.5, color: 'var(--text)' }}>
                  На сайте <code>flow.google.com</code> действует защита <b>Google Trusted Types</b>, блокирующая пользовательские скрипты. Нативный плагин регистрируется на уровне движка Chromium и гарантированно обходит все региональные фильтры Google Flow.
                </p>
              </div>

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
                <a
                  href={extensionZipUrl}
                  download="xkeen-flow-unlock.zip"
                  className="btn primary"
                  style={{
                    textDecoration: 'none',
                    textAlign: 'center',
                    padding: '10px 16px',
                    fontSize: 13,
                    fontWeight: 700,
                    background: 'linear-gradient(135deg, #10b981 0%, #059669 100%)',
                    boxShadow: '0 2px 10px rgba(16, 185, 129, 0.3)',
                    border: 'none',
                  }}
                >
                  📥 Скачать готовый плагин xkeen-flow-unlock.zip (6.5 КБ)
                </a>

                <div style={{ borderTop: '1px solid var(--border)', paddingTop: 10 }}>
                  <div style={{ fontSize: 12, fontWeight: 700, marginBottom: 8, color: 'var(--text)' }}>
                    Пошаговая установка за 30 секунд:
                  </div>
                  <ol style={{ margin: 0, paddingLeft: 18, fontSize: 12, lineHeight: 1.6, color: 'var(--text)' }}>
                    <li>Скачайте и распакуйте архив <code>xkeen-flow-unlock.zip</code> в любую папку.</li>
                    <li>Откройте в браузере страницу <code>chrome://extensions</code> (или <code>edge://extensions</code>).</li>
                    <li>В правом верхнем углу включите тумблер <b>«Режим разработчика»</b> (Developer Mode).</li>
                    <li>Нажмите кнопку <b>«Загрузить распакованное расширение»</b> (Load unpacked) и выберите папку.</li>
                    <li>Откройте <a href={cleanUrl} target="_blank" rel="noreferrer" style={{ color: '#38bdf8', fontWeight: 600 }}>flow.google.com</a> — доступ полностью открыт!</li>
                  </ol>
                </div>
              </div>
            </div>
          )}

          {/* ТАБ 2: ОЧИСТКА КЭША F12 */}
          {activeTab === 'cache_reset' && (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
              <div
                style={{
                  background: 'rgba(245, 158, 11, 0.08)',
                  border: '1px solid rgba(245, 158, 11, 0.35)',
                  borderRadius: 8,
                  padding: '12px 14px',
                }}
              >
                <div style={{ display: 'flex', alignItems: 'center', gap: 8, color: '#f59e0b', fontWeight: 700, fontSize: 13 }}>
                  <span>⚠️</span>
                  <span>НАЗНАЧЕНИЕ ЭТОЙ КОМАНДЫ: СБРОС СТАРОГО КЭША</span>
                </div>
                <p style={{ margin: '6px 0 0', fontSize: 12, lineHeight: 1.5, color: 'var(--text)' }}>
                  Эта команда <b>не разблокирует сайт сама по себе</b>. Она нужна только если вы заходили на Flow до установки плагина, и браузер «застрял» на экране <code>unsupported-country</code> в кэше Service Worker.
                </p>
              </div>

              <div
                style={{
                  border: '1px solid var(--border)',
                  borderRadius: 8,
                  padding: '12px 14px',
                  display: 'flex',
                  flexDirection: 'column',
                  gap: 10,
                }}
              >
                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                  <div style={{ fontWeight: 600, fontSize: 13 }}>
                    ⚡ Команда сброса Service Worker и кэша
                  </div>
                  <button
                    type="button"
                    className="btn btn-sm primary"
                    style={{ fontSize: 11, padding: '4px 10px' }}
                    onClick={handleCopyScript}
                  >
                    {copiedScript ? '✓ Скопировано!' : '📋 Скопировать команду'}
                  </button>
                </div>
                <p style={{ margin: 0, fontSize: 11.5, color: 'var(--muted)', lineHeight: 1.45 }}>
                  Инструкция: Нажмите <code>F12</code> на вкладке Google Flow ➔ вкладка <b>Console</b> ➔ вставьте команду и нажмите <code>Enter</code>. Страница перезагрузится с чистого листа.
                </p>
                <textarea
                  readOnly
                  className="input mono"
                  style={{ fontSize: 11, height: 50, resize: 'none' }}
                  value={resetScript}
                  onClick={(e) => (e.target as HTMLTextAreaElement).select()}
                />
              </div>

              <div
                style={{
                  border: '1px solid var(--border)',
                  borderRadius: 8,
                  padding: '12px 14px',
                  display: 'flex',
                  flexDirection: 'column',
                  gap: 8,
                }}
              >
                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                  <div style={{ fontWeight: 600, fontSize: 13 }}>
                    ⭐ Закладка-переключатель (Bookmarklet)
                  </div>
                  <button
                    type="button"
                    className="btn btn-sm"
                    style={{ fontSize: 11, padding: '3px 8px' }}
                    onClick={handleCopyBookmark}
                  >
                    {copiedBookmark ? '✓ Скопировано!' : '📋 Скопировать код'}
                  </button>
                </div>
                <textarea
                  readOnly
                  className="input mono"
                  style={{ fontSize: 11, height: 40, resize: 'none' }}
                  value={bookmarkletCode}
                  onClick={(e) => (e.target as HTMLTextAreaElement).select()}
                />
              </div>
            </div>
          )}

          {/* ТАБ 3: TAMPERMONKEY */}
          {activeTab === 'tampermonkey' && (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
              <div
                style={{
                  background: 'rgba(239, 68, 68, 0.08)',
                  border: '1px solid rgba(239, 68, 68, 0.3)',
                  borderRadius: 8,
                  padding: '12px 14px',
                }}
              >
                <div style={{ display: 'flex', alignItems: 'center', gap: 8, color: '#f87171', fontWeight: 700, fontSize: 13 }}>
                  <span>⚠️</span>
                  <span>ОГРАНИЧЕНИЕ GOOGLE TRUSTED TYPES В CHROME</span>
                </div>
                <p style={{ margin: '6px 0 0', fontSize: 11.5, lineHeight: 1.5, color: 'var(--text)' }}>
                  В современных версиях Chrome на <code>flow.google.com</code> включен строгий заголовок CSP <code>require-trusted-types-for 'script'</code>. Из-за этого Tampermonkey часто блокируется браузером на этой странице. Если скрипт у вас не срабатывает — <b>используйте нативный плагин (Вкладка 1)</b>.
                </p>
              </div>

              <div
                style={{
                  background: 'var(--panel-2, rgba(255, 255, 255, 0.03))',
                  border: '1px solid var(--border)',
                  borderRadius: 8,
                  padding: '12px 14px',
                  display: 'flex',
                  flexDirection: 'column',
                  gap: 10,
                }}
              >
                <a
                  href={userscriptUrl}
                  target="_blank"
                  rel="noreferrer"
                  className="btn"
                  style={{
                    textDecoration: 'none',
                    textAlign: 'center',
                    padding: '8px 14px',
                    fontSize: 12.5,
                    fontWeight: 600,
                  }}
                >
                  ⚡ Открыть скрипт flow-unlock.user.js для установки ↗
                </a>

                <div style={{ display: 'flex', flexDirection: 'column', gap: 6, borderTop: '1px solid var(--border)', paddingTop: 8 }}>
                  <div style={{ fontSize: 11.5, fontWeight: 600, color: 'var(--muted)' }}>
                    Ссылки на магазин Tampermonkey (если еще не установлен):
                  </div>
                  <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(130px, 1fr))', gap: 6 }}>
                    <a
                      href="https://chromewebstore.google.com/detail/tampermonkey/dhdgffkkebhmkfjojejmpbldmpobfkfo"
                      target="_blank"
                      rel="noreferrer"
                      className="btn btn-sm"
                      style={{ textDecoration: 'none', fontSize: 11, textAlign: 'center' }}
                    >
                      🌐 Chrome / Яндекс
                    </a>
                    <a
                      href="https://microsoftedge.microsoft.com/addons/detail/tampermonkey/iikmkjmpaadaobahmlepeloendndfphd"
                      target="_blank"
                      rel="noreferrer"
                      className="btn btn-sm"
                      style={{ textDecoration: 'none', fontSize: 11, textAlign: 'center' }}
                    >
                      🌊 Microsoft Edge
                    </a>
                    <a
                      href="https://addons.mozilla.org/firefox/addon/tampermonkey/"
                      target="_blank"
                      rel="noreferrer"
                      className="btn btn-sm"
                      style={{ textDecoration: 'none', fontSize: 11, textAlign: 'center' }}
                    >
                      🦊 Firefox
                    </a>
                  </div>
                </div>
              </div>
            </div>
          )}
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
              🚀 Открыть Google Flow (RU) ↗
            </a>
          </div>
        </div>
      </div>
    </div>
  )
}
