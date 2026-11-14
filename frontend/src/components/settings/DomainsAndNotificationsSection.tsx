import type { Dispatch, SetStateAction } from 'react'

interface DomainsAndNotificationsSectionProps {
  notify: (msg: string, isError?: boolean) => void
  directDomains: string
  setDirectDomains: (val: string) => void
  forceDomains: string
  setForceDomains: (val: string) => void
  savedDirectDomains: string
  savedForceDomains: string
  autoCdns: string[]
  cdnsCollapsed: boolean
  setCdnsCollapsed: (fn: (prev: boolean) => boolean) => void
  savingDomains: boolean
  scanningCdn: boolean
  hasUnsavedDirect: boolean
  hasUnsavedForce: boolean
  hasUnsavedDomains: boolean
  setPresetCatalogOpen: (open: boolean) => void
  saveDomains: () => void
  scanCdnManual: () => void
  telegramEnabled: boolean
  setTelegramEnabled: (val: boolean) => void
  telegramBotToken: string
  setTelegramBotToken: (val: string) => void
  telegramChatId: string
  setTelegramChatId: (val: string) => void
  webhookUrl: string
  setWebhookUrl: (val: string) => void
  notifTesting: boolean
  notifSaving: boolean
  activeNotifHelp: 'token' | 'chatId' | 'webhook' | null
  setActiveNotifHelp: Dispatch<SetStateAction<'token' | 'chatId' | 'webhook' | null>>
  notifTokenWarning: string | null
  notifChatIdWarning: string | null
  handleSaveNotifications: () => void
  handleTestNotification: () => void
}

export function DomainsAndNotificationsSection({
  notify,
  directDomains,
  setDirectDomains,
  forceDomains,
  setForceDomains,
  savedDirectDomains,
  savedForceDomains,
  autoCdns,
  cdnsCollapsed,
  setCdnsCollapsed,
  savingDomains,
  scanningCdn,
  hasUnsavedDirect,
  hasUnsavedForce,
  hasUnsavedDomains,
  setPresetCatalogOpen,
  saveDomains,
  scanCdnManual,
  telegramEnabled,
  setTelegramEnabled,
  telegramBotToken,
  setTelegramBotToken,
  telegramChatId,
  setTelegramChatId,
  webhookUrl,
  setWebhookUrl,
  notifTesting,
  notifSaving,
  activeNotifHelp,
  setActiveNotifHelp,
  notifTokenWarning,
  notifChatIdWarning,
  handleSaveNotifications,
  handleTestNotification,
}: DomainsAndNotificationsSectionProps) {
  return (
    <>
          {/* ДОМЕНЫ */}
          <section className="card" style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', flexWrap: 'wrap', gap: 8 }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                <h2 style={{ margin: 0, fontSize: 16 }}>🌐 Домены</h2>
                <span className="badge" style={{ background: 'rgba(56, 189, 248, 0.1)', color: '#38bdf8' }}>
                  {directDomains.split('\n').filter(Boolean).length} DIRECT · {forceDomains.split('\n').filter(Boolean).length} PROXY
                </span>
              </div>
              <button type="button" className="btn sm" onClick={() => setPresetCatalogOpen(true)} title="Добавить готовые списки (YouTube, Discord, AI...)">
                ✨ Каталог пресетов
              </button>
            </div>
            <p className="muted small" style={{ margin: 0 }}>
              По одному домену в строке. Вставляются в начало <code>rules:</code> (DOMAIN-SUFFIX). Сопутствующие CDN и медиа-серверы определяются автоматически.
            </p>

            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(280px, 1fr))', gap: 14, marginTop: 4 }}>
              {/* DIRECT */}
              <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', minHeight: 24 }}>
                  <b style={{ fontSize: 13, color: '#34d399', whiteSpace: 'nowrap' }}>⏭ Напрямую (DIRECT)</b>
                  <div style={{ display: 'flex', gap: 6, alignItems: 'center', whiteSpace: 'nowrap' }}>
                    <span className="muted small">{directDomains.split('\n').filter(Boolean).length} шт.</span>
                    {hasUnsavedDirect ? (
                      <span style={{ fontSize: 10.5, background: 'rgba(245, 158, 11, 0.2)', color: '#f59e0b', padding: '1px 6px', borderRadius: 4, fontWeight: 600 }}>
                        🟠 Не применено
                      </span>
                    ) : (
                      <span style={{ fontSize: 10.5, background: 'rgba(16, 185, 129, 0.15)', color: '#10b981', padding: '1px 6px', borderRadius: 4 }}>
                        ✅ Активно
                      </span>
                    )}
                  </div>
                </div>
                <textarea
                  className="input"
                  rows={6}
                  placeholder={'example.com\nlocal-service.net\nw3.org'}
                  value={directDomains}
                  onChange={(e) => setDirectDomains(e.target.value)}
                  style={{ width: '100%', boxSizing: 'border-box', fontFamily: 'Consolas, monospace', fontSize: 12, resize: 'vertical' }}
                />
              </div>

              {/* PROXY */}
              <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', minHeight: 24 }}>
                  <b style={{ fontSize: 13, color: '#38bdf8', whiteSpace: 'nowrap' }}>🔒 Через прокси (PROXY)</b>
                  <div style={{ display: 'flex', gap: 6, alignItems: 'center', whiteSpace: 'nowrap' }}>
                    <span className="muted small">{forceDomains.split('\n').filter(Boolean).length} шт.</span>
                    {hasUnsavedForce ? (
                      <span style={{ fontSize: 10.5, background: 'rgba(245, 158, 11, 0.2)', color: '#f59e0b', padding: '1px 6px', borderRadius: 4, fontWeight: 600 }}>
                        🟠 Не применено
                      </span>
                    ) : (
                      <span style={{ fontSize: 10.5, background: 'rgba(16, 185, 129, 0.15)', color: '#10b981', padding: '1px 6px', borderRadius: 4 }}>
                        ✅ Активно
                      </span>
                    )}
                  </div>
                </div>
                <textarea
                  className="input"
                  rows={6}
                  placeholder={'openai.com\nyoutube.com\ngithub.com'}
                  value={forceDomains}
                  onChange={(e) => setForceDomains(e.target.value)}
                  style={{ width: '100%', boxSizing: 'border-box', fontFamily: 'Consolas, monospace', fontSize: 12, resize: 'vertical' }}
                />
              </div>
            </div>

            {autoCdns.length > 0 && (
              <div style={{ marginTop: 4, padding: '8px 10px', background: 'rgba(56, 189, 248, 0.08)', border: '1px solid rgba(56, 189, 248, 0.2)', borderRadius: 6, fontSize: 12 }}>
                <div
                  style={{ color: '#38bdf8', fontWeight: 600, display: 'flex', alignItems: 'center', justifyContent: 'space-between', cursor: 'pointer', userSelect: 'none' }}
                  onClick={() => setCdnsCollapsed((prev) => !prev)}
                  title={cdnsCollapsed ? 'Развернуть список найденных CDN' : 'Свернуть список CDN'}
                >
                  <span>⚡ Автоматически подключенные CDN и медиа-сервера ({autoCdns.length})</span>
                  <span style={{ fontSize: 12, opacity: 0.85, background: 'rgba(56, 189, 248, 0.15)', padding: '2px 8px', borderRadius: 4 }}>
                    {cdnsCollapsed ? '▼ Развернуть' : '▲ Свернуть'}
                  </span>
                </div>
                {!cdnsCollapsed && (
                  <div style={{ display: 'flex', gap: 4, flexWrap: 'wrap', marginTop: 8 }}>
                    {autoCdns.map((cdn) => (
                      <span key={cdn} style={{ fontSize: 11, background: 'rgba(56, 189, 248, 0.15)', color: '#38bdf8', padding: '1px 6px', borderRadius: 4, fontFamily: 'monospace' }}>
                        {cdn}
                      </span>
                    ))}
                  </div>
                )}
              </div>
            )}

            <div style={{ display: 'flex', gap: 8, marginTop: 4, flexWrap: 'wrap', alignItems: 'center' }}>
              <button
                className="btn primary"
                onClick={saveDomains}
                disabled={savingDomains}
                style={hasUnsavedDomains ? { boxShadow: '0 0 10px rgba(56, 189, 248, 0.4)', borderColor: '#38bdf8' } : undefined}
              >
                {savingDomains ? 'Применение доменов…' : hasUnsavedDomains ? '💾 Применить изменения' : '🌐 Применить домены'}
              </button>
              {hasUnsavedDomains && (
                <button
                  type="button"
                  className="btn"
                  onClick={() => {
                    setDirectDomains(savedDirectDomains)
                    setForceDomains(savedForceDomains)
                    notify('Введённые домены сброшены до сохранённых на роутере')
                  }}
                  title="Сбросить введённый черновик к тому, что сейчас сохранено на роутере"
                  style={{ fontSize: 12 }}
                >
                  ↺ Сбросить к сохранённым
                </button>
              )}
              <button className="btn" onClick={scanCdnManual} disabled={scanningCdn || savingDomains}>
                {scanningCdn ? '🔍 Поиск CDN…' : '🔍 Сканировать CDN сейчас'}
              </button>
            </div>
            <p className="muted small" style={{ marginTop: 2, marginBottom: 0 }}>
              💡 Фоновое сканирование выполняется автоматически 1 раз в неделю (в понедельник в 05:00) или вручную кнопкой выше, чтобы не нагружать процессор роутера.
            </p>
          </section>

          {/* ОПОВЕЩЕНИЯ (TELEGRAM / WEBHOOK) */}
          <section className="card">
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 8 }}>
              <h2 style={{ margin: 0 }}>🔔 Оповещения о сбоях</h2>
              <span className={`badge ${telegramEnabled ? 'badge-online' : ''}`}>
                {telegramEnabled ? '🟢 Telegram вкл' : '⚪ выкл'}
              </span>
            </div>
            <p className="muted small">Мгновенные уведомления в Telegram при падении серверов и переключении Failover.</p>
            <div className="row" style={{ justifyContent: 'space-between', alignItems: 'center' }}>
              <b>Telegram оповещения</b>
              <label className="switch">
                <input
                  type="checkbox"
                  checked={telegramEnabled}
                  onChange={(e) => setTelegramEnabled(e.target.checked)}
                />
                <span className="slider" />
              </label>
            </div>

            {/* BOT TOKEN */}
            <label className="row" style={{ alignItems: 'flex-start', marginTop: 8 }}>
              <span style={{ display: 'inline-flex', alignItems: 'center', gap: 6, paddingTop: 6 }}>
                Bot Token
                <button
                  type="button"
                  className={`badge-help-btn ${activeNotifHelp === 'token' ? 'active' : ''}`}
                  title="Где взять токен бота? Нажмите для инструкции"
                  onClick={() => setActiveNotifHelp((prev) => (prev === 'token' ? null : 'token'))}
                >
                  ?
                </button>
              </span>
              <div style={{ flex: 1, display: 'flex', flexDirection: 'column', gap: 3 }}>
                <input
                  className="input"
                  placeholder="123456789:ABCdefGhIJKlmNoPQRstuVWXyz"
                  value={telegramBotToken}
                  onChange={(e) => setTelegramBotToken(e.target.value)}
                  style={notifTokenWarning ? { borderColor: 'var(--red, #ef4444)' } : undefined}
                />
                {notifTokenWarning && (
                  <span style={{ fontSize: 11, color: 'var(--red, #ef4444)', lineHeight: 1.3 }}>
                    ⚠️ {notifTokenWarning}
                  </span>
                )}
              </div>
            </label>
            {activeNotifHelp === 'token' && (
              <div className="help-box-tip" style={{ marginTop: 4, marginBottom: 8 }}>
                <div style={{ fontWeight: 600, marginBottom: 4, color: 'var(--accent, #38bdf8)' }}>🤖 Как получить Bot Token:</div>
                <ol style={{ margin: 0, paddingLeft: 18, fontSize: 12, lineHeight: 1.5 }}>
                  <li>Откройте официального бота <a href="https://t.me/BotFather" target="_blank" rel="noreferrer" style={{ color: 'var(--accent, #38bdf8)', textDecoration: 'underline' }}>@BotFather</a> в Telegram.</li>
                  <li>Отправьте команду <code>/newbot</code>, укажите имя и username (должен оканчиваться на <code>bot</code>).</li>
                  <li>Скопируйте выданный токен <b>целиком</b> (вид: <code>123456789:AAAAAA...</code>) и вставьте в это поле. Не удаляйте цифры в начале!</li>
                </ol>
              </div>
            )}

            {/* CHAT ID */}
            <label className="row" style={{ alignItems: 'flex-start', marginTop: 8 }}>
              <span style={{ display: 'inline-flex', alignItems: 'center', gap: 6, paddingTop: 6 }}>
                Chat ID
                <button
                  type="button"
                  className={`badge-help-btn ${activeNotifHelp === 'chatId' ? 'active' : ''}`}
                  title="Как узнать свой Chat ID? Нажмите для инструкции"
                  onClick={() => setActiveNotifHelp((prev) => (prev === 'chatId' ? null : 'chatId'))}
                >
                  ?
                </button>
              </span>
              <div style={{ flex: 1, display: 'flex', flexDirection: 'column', gap: 3 }}>
                <input
                  className="input"
                  placeholder="123456789 или -1001234567890"
                  value={telegramChatId}
                  onChange={(e) => setTelegramChatId(e.target.value)}
                  style={notifChatIdWarning ? { borderColor: 'var(--red, #ef4444)' } : undefined}
                />
                {notifChatIdWarning && (
                  <span style={{ fontSize: 11, color: 'var(--red, #ef4444)', lineHeight: 1.3 }}>
                    {notifChatIdWarning}
                  </span>
                )}
              </div>
            </label>
            {activeNotifHelp === 'chatId' && (
              <div className="help-box-tip" style={{ marginTop: 4, marginBottom: 8 }}>
                <div style={{ fontWeight: 600, marginBottom: 4, color: 'var(--accent, #38bdf8)' }}>🆔 Как узнать ваш Chat ID:</div>
                <ol style={{ margin: 0, paddingLeft: 18, fontSize: 12, lineHeight: 1.5 }}>
                  <li>Откройте бота <a href="https://t.me/userinfobot" target="_blank" rel="noreferrer" style={{ color: 'var(--accent, #38bdf8)', textDecoration: 'underline' }}>@userinfobot</a> (или @getmyid_bot) и нажмите <b>/start</b>.</li>
                  <li>Скопируйте ваш числовой <code>Id</code> (например, <code>712345678</code>) и вставьте в это поле.</li>
                  <li>⚠️ <b>Обязательно:</b> зайдите в вашего <b>созданного бота</b> и нажмите кнопку <b>СТАРТ (/start)</b>, иначе Telegram запретит боту писать вам!</li>
                  <li><i>Для группы: добавьте бота в группу и укажите ID группы (начинается с <code>-100...</code>).</i></li>
                </ol>
              </div>
            )}

            {/* WEBHOOK URL */}
            <label className="row" style={{ alignItems: 'flex-start', marginTop: 8 }}>
              <span style={{ display: 'inline-flex', alignItems: 'center', gap: 6, paddingTop: 6 }}>
                Webhook URL
                <button
                  type="button"
                  className={`badge-help-btn ${activeNotifHelp === 'webhook' ? 'active' : ''}`}
                  title="Что такое Webhook URL? Нажмите для справки"
                  onClick={() => setActiveNotifHelp((prev) => (prev === 'webhook' ? null : 'webhook'))}
                >
                  ?
                </button>
              </span>
              <div style={{ flex: 1, display: 'flex', flexDirection: 'column', gap: 3 }}>
                <input
                  className="input"
                  placeholder="https://my-server.com/api/failover-hook"
                  value={webhookUrl}
                  onChange={(e) => setWebhookUrl(e.target.value)}
                />
                <span className="muted small" style={{ fontSize: 11 }}>
                  💡 Не требуется для Telegram! Используется только для Discord, Home Assistant или внешних серверов.
                </span>
              </div>
            </label>
            {activeNotifHelp === 'webhook' && (
              <div className="help-box-tip" style={{ marginTop: 4, marginBottom: 8 }}>
                <div style={{ fontWeight: 600, marginBottom: 4, color: 'var(--accent, #38bdf8)' }}>🔗 Для чего нужно поле Webhook URL:</div>
                <p style={{ margin: 0, fontSize: 12, lineHeight: 1.5 }}>
                  Если вы настраиваете оповещения в <b>Telegram</b> — оставьте это поле <b>пустым</b>.<br />
                  Webhook URL нужен только для автоматической отправки JSON-событий в сторонние сервисы:
                </p>
                <ul style={{ margin: '4px 0 0 0', paddingLeft: 18, fontSize: 12, lineHeight: 1.5 }}>
                  <li><b>Discord</b> (URL вебхука текстового канала из настроек сервера)</li>
                  <li><b>Home Assistant / Node-RED / n8n</b> (умный дом)</li>
                  <li>Собственные сервера и скрипты мониторинга</li>
                </ul>
              </div>
            )}

            <div style={{ display: 'flex', gap: 8, marginTop: 12, flexWrap: 'wrap' }}>
              <button className="btn primary" onClick={handleSaveNotifications} disabled={notifSaving}>
                {notifSaving ? 'Сохранение…' : '💾 Сохранить оповещения'}
              </button>
              <button className="btn" onClick={handleTestNotification} disabled={notifTesting}>
                {notifTesting ? 'Отправка…' : '💬 Тестовое сообщение'}
              </button>
            </div>
          </section>
    </>
  )
}