import { useState, useEffect } from 'react'
import { apiPost } from '../api'
import { copyToClipboard } from '../utils/clipboard'

interface FlowRepairModalProps {
  isOpen: boolean
  onClose: () => void
  notify: (msg: string, isError?: boolean) => void
  onRepaired?: (serverName: string) => void
}

type TabType = 'tampermonkey' | 'bookmarklet' | 'extension'

export default function FlowRepairModal({
  isOpen,
  onClose,
  notify,
  onRepaired,
}: FlowRepairModalProps) {
  if (!isOpen) return null

  const [activeTab, setActiveTab] = useState<TabType>('tampermonkey')
  const [repairing, setRepairing] = useState(false)
  const [activeServer, setActiveServer] = useState<string>('')
  const [copiedScript, setCopiedScript] = useState(false)
  const [copiedBookmark, setCopiedBookmark] = useState(false)
  const [copiedLink, setCopiedLink] = useState(false)

  const cleanUrl = 'https://flow.google.com/?authuser=0&hl=en'
  const userscriptUrl = '/api/flow/flow-unlock.user.js'
  const extensionZipUrl = '/api/flow/extension.zip'

  const resetScript = `(async () => { if ('serviceWorker' in navigator) { const regs = await navigator.serviceWorker.getRegistrations(); for (const r of regs) await r.unregister(); } if ('caches' in window) { const keys = await caches.keys(); for (const k of keys) await caches.delete(k); } localStorage.clear(); sessionStorage.clear(); if (window.indexedDB && indexedDB.databases) { try { const dbs = await indexedDB.databases(); for (const db of dbs) if (db.name) indexedDB.deleteDatabase(db.name); } catch(e){} } location.replace('${cleanUrl}'); })();`

  const bookmarkletCode = `javascript:(function(){try{Object.defineProperty(navigator,'language',{get:()=>'en-US',configurable:true});Object.defineProperty(navigator,'languages',{get:()=>['en-US','en'],configurable:true});}catch(e){}if(location.pathname.includes('unsupported-country')){history.replaceState(null,'','/');location.replace('https://flow.google.com/?authuser=0&hl=en');}else{alert('✓ Спуфинг языка en-US применён!');}})();`

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
              className={`btn btn-sm ${activeTab === 'tampermonkey' ? 'primary' : 'ghost'}`}
              style={{ flex: 1, fontSize: 12, fontWeight: activeTab === 'tampermonkey' ? 600 : 400 }}
              onClick={() => setActiveTab('tampermonkey')}
            >
              ⚡ 1. Tampermonkey (Рекомендуется)
            </button>
            <button
              type="button"
              className={`btn btn-sm ${activeTab === 'bookmarklet' ? 'primary' : 'ghost'}`}
              style={{ flex: 1, fontSize: 12, fontWeight: activeTab === 'bookmarklet' ? 600 : 400 }}
              onClick={() => setActiveTab('bookmarklet')}
            >
              ⭐ 2. Закладка & Консоль F12
            </button>
            <button
              type="button"
              className={`btn btn-sm ${activeTab === 'extension' ? 'primary' : 'ghost'}`}
              style={{ flex: 1, fontSize: 12, fontWeight: activeTab === 'extension' ? 600 : 400 }}
              onClick={() => setActiveTab('extension')}
            >
              📦 3. Расширение (ZIP)
            </button>
          </div>

          {/* ТАБ 1: ТАМПЕРМАНКИ (ГЛАВНЫЙ АКЦЕНТ) */}
          {activeTab === 'tampermonkey' && (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
              {/* Большой акцент на обязательность Tampermonkey */}
              <div
                style={{
                  background: 'rgba(245, 158, 11, 0.1)',
                  border: '1.5px solid rgba(245, 158, 11, 0.5)',
                  borderRadius: 8,
                  padding: '12px 14px',
                }}
              >
                <div style={{ display: 'flex', alignItems: 'center', gap: 8, color: '#f59e0b', fontWeight: 700, fontSize: 13.5 }}>
                  <span>⚠️</span>
                  <span>ОБЯЗАТЕЛЬНОЕ ТРЕБОВАНИЕ: ДОЛЖЕН БЫТЬ УСТАНОВЛЕН TAMPERMONKEY</span>
                </div>
                <p style={{ margin: '6px 0 10px', fontSize: 12, lineHeight: 1.5, color: 'var(--text)' }}>
                  Чтобы скрипт мог перехватить внутренний RPC-пакет Google <code>cPZSdc</code> до того, как страница начнет загрузку, в вашем браузере <b>обязательно должно быть установлено расширение Tampermonkey</b> (или Violentmonkey).
                </p>

                {/* Ссылки на установку Tampermonkey */}
                <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                  <div style={{ fontSize: 11.5, fontWeight: 600, color: 'var(--muted)' }}>
                    Выберите ваш браузер для установки Tampermonkey:
                  </div>
                  <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(130px, 1fr))', gap: 6 }}>
                    <a
                      href="https://chromewebstore.google.com/detail/tampermonkey/dhdgffkkebhmkfjojejmpbldmpobfkfo"
                      target="_blank"
                      rel="noreferrer"
                      className="btn btn-sm"
                      style={{
                        background: 'rgba(255, 255, 255, 0.05)',
                        border: '1px solid var(--border)',
                        textDecoration: 'none',
                        fontSize: 11,
                        padding: '6px 8px',
                        display: 'flex',
                        alignItems: 'center',
                        justifyContent: 'center',
                        gap: 4,
                      }}
                    >
                      🌐 Chrome / Яндекс
                    </a>
                    <a
                      href="https://microsoftedge.microsoft.com/addons/detail/tampermonkey/iikmkjmpaadaobahmlepeloendndfphd"
                      target="_blank"
                      rel="noreferrer"
                      className="btn btn-sm"
                      style={{
                        background: 'rgba(255, 255, 255, 0.05)',
                        border: '1px solid var(--border)',
                        textDecoration: 'none',
                        fontSize: 11,
                        padding: '6px 8px',
                        display: 'flex',
                        alignItems: 'center',
                        justifyContent: 'center',
                        gap: 4,
                      }}
                    >
                      🌊 Microsoft Edge
                    </a>
                    <a
                      href="https://addons.mozilla.org/firefox/addon/tampermonkey/"
                      target="_blank"
                      rel="noreferrer"
                      className="btn btn-sm"
                      style={{
                        background: 'rgba(255, 255, 255, 0.05)',
                        border: '1px solid var(--border)',
                        textDecoration: 'none',
                        fontSize: 11,
                        padding: '6px 8px',
                        display: 'flex',
                        alignItems: 'center',
                        justifyContent: 'center',
                        gap: 4,
                      }}
                    >
                      🦊 Firefox
                    </a>
                    <a
                      href="https://www.tampermonkey.net/"
                      target="_blank"
                      rel="noreferrer"
                      className="btn btn-sm"
                      style={{
                        background: 'rgba(255, 255, 255, 0.05)',
                        border: '1px solid var(--border)',
                        textDecoration: 'none',
                        fontSize: 11,
                        padding: '6px 8px',
                        display: 'flex',
                        alignItems: 'center',
                        justifyContent: 'center',
                        gap: 4,
                      }}
                    >
                      🏠 Официальный сайт
                    </a>
                  </div>
                </div>
              </div>

              {/* Шаг 2: Установка скрипта */}
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
                <div>
                  <div style={{ fontWeight: 600, fontSize: 13 }}>
                    Шаг 2: Установите скрипт XKeen Flow Unlocker
                  </div>
                  <p style={{ margin: '4px 0 0', fontSize: 11.5, color: 'var(--muted)', lineHeight: 1.45 }}>
                    После установки Tampermonkey нажмите кнопку ниже. Откроется вкладка Tampermonkey — нажмите кнопку <b>«Установить»</b> (Install). Скрипт будет работать автоматически навсегда.
                  </p>
                </div>

                <a
                  href={userscriptUrl}
                  target="_blank"
                  rel="noreferrer"
                  className="btn primary"
                  style={{
                    textDecoration: 'none',
                    textAlign: 'center',
                    padding: '10px 16px',
                    fontSize: 13,
                    fontWeight: 600,
                    background: 'linear-gradient(135deg, #10b981 0%, #059669 100%)',
                    boxShadow: '0 2px 10px rgba(16, 185, 129, 0.3)',
                    border: 'none',
                  }}
                >
                  ⚡ Установить скрипт Flow Unlock (в 1 клик через Tampermonkey) ↗
                </a>

                <div style={{ fontSize: 11, color: 'var(--muted)', display: 'flex', gap: 6, alignItems: 'center' }}>
                  <span>ℹ️</span>
                  <span>Скрипт перехватывает сетевой ответ <code>cPZSdc</code>, ставит <code>isSupported: true</code> и сменяет язык на <code>en-US</code>.</span>
                </div>
              </div>
            </div>
          )}

          {/* ТАБ 2: ЗАКЛАДКА И КОНСОЛЬ */}
          {activeTab === 'bookmarklet' && (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
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
                    ⭐ Закладка в браузере (Bookmarklet)
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
                <p style={{ margin: 0, fontSize: 11.5, color: 'var(--muted)', lineHeight: 1.45 }}>
                  Создайте новую закладку на панели закладок вашего браузера, назовите её <b>«Flow Unlock»</b> и вставьте код в поле адреса (URL). Нажмите на неё, когда находитесь на <code>flow.google.com</code>.
                </p>
                <textarea
                  readOnly
                  className="input mono"
                  style={{ fontSize: 11, height: 44, resize: 'none' }}
                  value={bookmarkletCode}
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
                    ⚡ Очистка кэша через консоль F12
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
                <p style={{ margin: 0, fontSize: 11.5, color: 'var(--muted)', lineHeight: 1.45 }}>
                  Нажмите <code>F12</code> на вкладке Flow ➔ перейдите в <b>Console</b> ➔ вставьте команду и нажмите <code>Enter</code>. Она удалит зависший Service Worker и кэш редиректа.
                </p>
                <textarea
                  readOnly
                  className="input mono"
                  style={{ fontSize: 11, height: 44, resize: 'none' }}
                  value={resetScript}
                  onClick={(e) => (e.target as HTMLTextAreaElement).select()}
                />
              </div>
            </div>
          )}

          {/* ТАБ 3: РАСШИРЕНИЕ ДЛЯ БРАУЗЕРА */}
          {activeTab === 'extension' && (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
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
                <div>
                  <div style={{ fontWeight: 600, fontSize: 13 }}>
                    📦 Готовое расширение для Chrome / Edge / Яндекс (ZIP)
                  </div>
                  <p style={{ margin: '4px 0 0', fontSize: 11.5, color: 'var(--muted)', lineHeight: 1.45 }}>
                    Если вы не хотите использовать Tampermonkey, скачайте автономное расширение <b>XKeen Flow Unlocker</b> прямо из панели.
                  </p>
                </div>

                <a
                  href={extensionZipUrl}
                  download="xkeen-flow-unlock.zip"
                  className="btn primary"
                  style={{
                    textDecoration: 'none',
                    textAlign: 'center',
                    padding: '8px 14px',
                    fontSize: 12.5,
                    fontWeight: 600,
                  }}
                >
                  📥 Скачать расширение xkeen-flow-unlock.zip (6 КБ)
                </a>

                <div style={{ borderTop: '1px solid var(--border)', paddingTop: 10 }}>
                  <div style={{ fontSize: 12, fontWeight: 600, marginBottom: 6 }}>
                    Инструкция по установке (1 минута):
                  </div>
                  <ol style={{ margin: 0, paddingLeft: 18, fontSize: 11.5, lineHeight: 1.55, color: 'var(--text)' }}>
                    <li>Распакуйте скачанный <code>xkeen-flow-unlock.zip</code> в любую постоянную папку.</li>
                    <li>Откройте в браузере страницу <code>chrome://extensions</code> (или <code>edge://extensions</code>).</li>
                    <li>Включите тумблер <b>«Режим разработчика»</b> (Developer mode) в правом верхнем углу.</li>
                    <li>Нажмите кнопку <b>«Загрузить распакованное»</b> (Load unpacked) и выберите распакованную папку.</li>
                  </ol>
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
              🚀 Открыть чистый Flow ↗
            </a>
          </div>
        </div>
      </div>
    </div>
  )
}
