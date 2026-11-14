import React from 'react'
import type { DpiTestResult, ZapretStatus } from '../../types'
import { renderMicroSpinner } from './zapretHelpers'

export interface ZapretMainCardProps {
  status: ZapretStatus | null
  isRunning: boolean
  isInstalled: boolean
  activeEngine: 'v1' | 'v2'
  v2Installed: boolean
  canRollbackV1: boolean
  hardwareHint: string
  targetArchLabel: string
  busy: boolean
  pendingKeys: Set<string>
  switchingEngine: boolean
  upgradingEngine: boolean
  stepLog: string | null
  setStepLog: React.Dispatch<React.SetStateAction<string | null>>
  testingDpi: boolean
  testResult: DpiTestResult | null
  showHostsEditor: boolean
  setShowHostsEditor: React.Dispatch<React.SetStateAction<boolean>>
  hostsDraft: string
  setHostsDraft: React.Dispatch<React.SetStateAction<string>>
  hostsDirtyRef: React.MutableRefObject<boolean>
  savingHosts: boolean
  showConfigEditor: boolean
  setShowConfigEditor: React.Dispatch<React.SetStateAction<boolean>>
  configDraft: string
  setConfigDraft: React.Dispatch<React.SetStateAction<string>>
  configDirtyRef: React.MutableRefObject<boolean>
  savingConfig: boolean
  handleToggle: () => void
  handleAction: (act: string) => void
  handleSwitchEngine: (targetEngine: 'v1' | 'v2') => void
  handleUpgradeZapret2: () => void
  handleTestDpi: () => void
  handleSaveHosts: () => void
  handleSaveConfig: () => void
}

export function ZapretMainCard({
  status,
  isRunning,
  isInstalled,
  activeEngine,
  v2Installed,
  canRollbackV1,
  hardwareHint,
  targetArchLabel,
  busy,
  pendingKeys,
  switchingEngine,
  upgradingEngine,
  stepLog,
  setStepLog,
  testingDpi,
  testResult,
  showHostsEditor,
  setShowHostsEditor,
  hostsDraft,
  setHostsDraft,
  hostsDirtyRef,
  savingHosts,
  showConfigEditor,
  setShowConfigEditor,
  configDraft,
  setConfigDraft,
  configDirtyRef,
  savingConfig,
  handleToggle,
  handleAction,
  handleSwitchEngine,
  handleUpgradeZapret2,
  handleTestDpi,
  handleSaveHosts,
  handleSaveConfig,
}: ZapretMainCardProps) {
  return (
      <section
        className="card"
        style={{
          position: 'relative',
          overflow: 'hidden',
          padding: '24px 26px',
          background: isRunning
            ? 'linear-gradient(135deg, rgba(34, 197, 94, 0.08) 0%, rgba(15, 23, 42, 0.6) 100%)'
            : 'rgba(255, 255, 255, 0.02)',
          border: isRunning ? '1px solid rgba(34, 197, 94, 0.3)' : '1px solid var(--border)',
          borderRadius: 16,
          boxShadow: isRunning ? '0 10px 30px rgba(34, 197, 94, 0.08)' : 'none',
        }}
      >
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: 16 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 16 }}>
            <div
              style={{
                width: 52,
                height: 52,
                borderRadius: 14,
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                background: isRunning ? 'rgba(34, 197, 94, 0.15)' : 'rgba(255, 255, 255, 0.05)',
                color: isRunning ? '#22c55e' : 'var(--muted)',
                fontSize: 26,
                boxShadow: isRunning ? '0 0 20px rgba(34, 197, 94, 0.25)' : 'none',
                transition: 'all 0.3s ease',
              }}
            >
              🛡️
            </div>
            <div>
              <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                <h2 style={{ margin: 0, fontSize: 20, fontWeight: 700 }}>Zapret — Обход DPI</h2>
                <span
                  className="badge"
                  style={{
                    background: isRunning ? 'rgba(34, 197, 94, 0.15)' : 'rgba(255, 255, 255, 0.06)',
                    color: isRunning ? '#22c55e' : 'var(--muted)',
                    border: `1px solid ${isRunning ? 'rgba(34, 197, 94, 0.3)' : 'var(--border)'}`,
                  }}
                >
                  {isRunning ? `🟢 Работает: ${activeEngine === 'v2' ? 'Zapret 2.0' : 'Zapret 1.x'} (PID: ${status?.pid})` : isInstalled ? '⚪ Выключен' : '🔴 Не установлен'}
                </span>
              </div>
              <div className="muted small" style={{ marginTop: 4 }}>
                Локальная десинхронизация TCP/UDP пакетов ({activeEngine === 'v2' ? 'Zapret 2.0 Lua Engine' : 'Zapret 1.x Legacy'}) для YouTube, Discord и сайтов без нагрузки на VPS
              </div>
            </div>
          </div>

          {/* ГЛАВНЫЙ ВЫКЛЮЧАТЕЛЬ (SWITCH) */}
          {isInstalled && (
            <div style={{ display: 'flex', alignItems: 'center', gap: 14 }}>
              <div style={{ textAlign: 'right' }}>
                <div style={{ fontSize: 13, fontWeight: 600, color: isRunning ? '#22c55e' : 'var(--muted)' }}>
                  {isRunning ? 'СЛУЖБА АКТИВНА' : 'СЛУЖБА ВЫКЛЮЧЕНА'}
                </div>
                <div className="muted small" style={{ fontSize: 11 }}>
                  {isRunning ? `LAN трафик фильтруется через ${activeEngine === 'v2' ? 'nfqws2' : 'nfqws'}` : 'Прямой трафик без изменений'}
                </div>
              </div>

              {(() => {
                const isMainPending = pendingKeys.has('enabled')
                return (
                  <button
                    type="button"
                    disabled={busy || isMainPending}
                    onClick={handleToggle}
                    className={isMainPending ? 'zapret-glow-pulse-green' : ''}
                    style={{
                      width: 64,
                      height: 34,
                      borderRadius: 20,
                      border: 'none',
                      cursor: busy || isMainPending ? 'wait' : 'pointer',
                      background: isRunning
                        ? 'linear-gradient(135deg, #22c55e 0%, #16a34a 100%)'
                        : 'rgba(255, 255, 255, 0.15)',
                      position: 'relative',
                      transition: 'all 0.25s cubic-bezier(0.4, 0, 0.2, 1)',
                      padding: 3,
                      boxShadow: isRunning ? '0 0 16px rgba(34, 197, 94, 0.4)' : 'none',
                    }}
                    title={isRunning ? 'Выключить Zapret' : 'Включить Zapret'}
                  >
                    <div
                      style={{
                        width: 28,
                        height: 28,
                        borderRadius: '50%',
                        background: '#fff',
                        transform: isRunning ? 'translateX(30px)' : 'translateX(0)',
                        transition: 'transform 0.25s cubic-bezier(0.4, 0, 0.2, 1)',
                        boxShadow: '0 2px 6px rgba(0,0,0,0.3)',
                        display: 'flex',
                        alignItems: 'center',
                        justifyContent: 'center',
                        fontSize: 12,
                        color: isRunning ? '#16a34a' : '#888',
                      }}
                    >
                      {isMainPending ? renderMicroSpinner('#16a34a', 15) : isRunning ? '✓' : '✕'}
                    </div>
                  </button>
                )
              })()}
            </div>
          )}
        </div>

        {/* СТАТУС-БАР ПОДСЕТИ И IPTABLES */}
        <div
          style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))',
            gap: 12,
            marginTop: 20,
            paddingTop: 16,
            borderTop: '1px solid var(--border)',
          }}
        >
          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <span style={{ fontSize: 16 }}>🎯</span>
            <div>
              <div className="muted small">Очередь ядра</div>
              <div style={{ fontSize: 13, fontWeight: 600 }}>NFQUEUE #200</div>
            </div>
          </div>

          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <span style={{ fontSize: 16 }}>🔥</span>
            <div>
              <div className="muted small">Перехват Netfilter</div>
              <div style={{ fontSize: 13, fontWeight: 600, color: status?.iptables_active ? '#22c55e' : 'var(--muted)' }}>
                {status?.iptables_active ? '🟢 Активен (Netfilter mangle)' : '⚪ Отключен'}
              </div>
            </div>
          </div>

          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <span style={{ fontSize: 16 }}>🚀</span>
            <div>
              <div className="muted small">Автозапуск</div>
              <div style={{ fontSize: 13, fontWeight: 600, color: status?.autostart ? '#22c55e' : 'var(--muted)' }}>
                {status?.autostart ? '🟢 S51zapret включен' : '⚪ Отключен'}
              </div>
            </div>
          </div>

          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <span style={{ fontSize: 16 }}>⚡</span>
            <div>
              <div className="muted small">Защита от сбоев</div>
              <div style={{ fontSize: 13, fontWeight: 600, color: '#38bdf8' }}>--queue-bypass (100% аптайм)</div>
            </div>
          </div>
        </div>

        {/* ПРЕДУПРЕЖДЕНИЕ: СЛУЖБА ЗАПУЩЕНА, НО IPTABLES ОТКЛЮЧЕН */}
        {isRunning && !status?.iptables_active && (
          <div
            style={{
              marginTop: 14,
              padding: '10px 14px',
              borderRadius: 8,
              background: 'rgba(234, 179, 8, 0.1)',
              border: '1px solid rgba(234, 179, 8, 0.3)',
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'space-between',
              gap: 12,
              flexWrap: 'wrap',
            }}
          >
            <div style={{ fontSize: 12, color: '#eab308' }}>
              ⚠️ Служба Zapret активна (PID: {status?.pid}), но перехват Netfilter отключен. Трафик не попадает в nfqws.
            </div>
            <button
              type="button"
              className="btn sm"
              disabled={busy}
              onClick={() => handleAction('start-fw')}
              style={{ background: '#eab308', color: '#000', fontWeight: 600, border: 'none' }}
              title="Восстановить перехват Netfilter (iptables) без перезапуска процесса nfqws"
            >
              🔄 Включить перехват
            </button>
          </div>
        )}

        {/* ПЕРЕКЛЮЧАТЕЛЬ ДВИЖКОВ (LEGACY 1.X / MODERN 2.0) И АВТОПОДСКАЗКА АРХИТЕКТУРЫ */}
        <div
          data-testid="zapret-engine-panel"
          style={{
            marginTop: 16,
            padding: '14px 16px',
            borderRadius: 12,
            background: 'linear-gradient(135deg, rgba(56, 189, 248, 0.08) 0%, rgba(168, 85, 247, 0.06) 100%)',
            border: '1px solid rgba(56, 189, 248, 0.25)',
            display: 'flex',
            flexDirection: 'column',
            gap: 12,
          }}
        >
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: 12 }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
              <span style={{ fontSize: 20 }}>🧠</span>
              <div>
                {activeEngine !== 'v2' && (
                  <div
                    data-testid="zapret-hardware-hint"
                    style={{ fontSize: 13, fontWeight: 700, color: '#38bdf8' }}
                  >
                    {hardwareHint}
                  </div>
                )}
                <div className="muted small" style={{ marginTop: 2, fontSize: 11.5 }}>
                  Автоопределение архитектуры ({targetArchLabel}) • каталог Zapret 2.0:{' '}
                  <code>/opt/zapret2</code> (<code>nfqws2</code> + Lua-библиотеки) • доступно переключение и безопасный откат на v1
                </div>
              </div>
            </div>

            {/* Переключатель движков: Legacy 1.x / Modern 2.0 */}
            <div
              role="group"
              aria-label="Переключатель движков Zapret"
              style={{
                display: 'inline-flex',
                background: 'rgba(15, 23, 42, 0.65)',
                padding: 3,
                borderRadius: 10,
                border: '1px solid var(--border)',
                gap: 4,
              }}
            >
              <button
                type="button"
                data-testid="engine-switch-v1"
                disabled={busy || switchingEngine || upgradingEngine}
                onClick={() => handleSwitchEngine('v1')}
                style={{
                  padding: '6px 12px',
                  borderRadius: 7,
                  border: 'none',
                  fontSize: 12,
                  fontWeight: 600,
                  cursor: busy ? 'wait' : 'pointer',
                  background: activeEngine === 'v1' ? 'rgba(234, 179, 8, 0.22)' : 'transparent',
                  color: activeEngine === 'v1' ? '#facc15' : 'var(--muted)',
                  transition: 'all 0.2s ease',
                }}
                title="Переключить на классический движок Legacy 1.x (nfqws)"
              >
                Legacy 1.x (nfqws)
              </button>
              <button
                type="button"
                data-testid="engine-switch-v2"
                disabled={busy || switchingEngine || upgradingEngine}
                onClick={() => (v2Installed ? handleSwitchEngine('v2') : handleUpgradeZapret2())}
                style={{
                  padding: '6px 12px',
                  borderRadius: 7,
                  border: 'none',
                  fontSize: 12,
                  fontWeight: 600,
                  cursor: busy ? 'wait' : 'pointer',
                  background:
                    activeEngine === 'v2'
                      ? 'linear-gradient(135deg, rgba(56, 189, 248, 0.25) 0%, rgba(168, 85, 247, 0.25) 100%)'
                      : 'transparent',
                  color: activeEngine === 'v2' ? '#38bdf8' : 'var(--muted)',
                  transition: 'all 0.2s ease',
                }}
                title="Переключить на современный движок Modern 2.0 (nfqws2 + Lua)"
              >
                Modern 2.0 (nfqws2)
              </button>
            </div>
          </div>

          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', flexWrap: 'wrap', gap: 10, width: '100%' }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
              <button
                type="button"
                data-testid="upgrade-zapret2-btn"
                className="btn sm"
                disabled={busy || upgradingEngine}
                onClick={handleUpgradeZapret2}
                style={{
                  background: status?.update_available
                    ? 'linear-gradient(135deg, #16a34a 0%, #22c55e 100%)'
                    : 'linear-gradient(135deg, #0284c7 0%, #7c3aed 100%)',
                  color: '#fff',
                  border: 'none',
                  fontWeight: 600,
                  boxShadow: status?.update_available
                    ? '0 4px 14px rgba(34, 197, 94, 0.35)'
                    : '0 4px 14px rgba(56, 189, 248, 0.25)',
                }}
                title="В 1 клик скачать актуальный бинарник nfqws2 и Lua-библиотеки в /opt/zapret2 с сохранением резервной копии v1"
              >
                {upgradingEngine
                  ? '⏳ Скачивание nfqws2 и Lua в /opt/zapret2…'
                  : status?.update_available
                  ? `🚀 Обновить Zapret 2 (${status.latest_version || 'v2'})`
                  : activeEngine === 'v2'
                  ? '🚀 Переустановить Zapret 2 (nfqws2)'
                  : '🚀 Обновить до Zapret 2 (nfqws2)'}
              </button>

              {canRollbackV1 && activeEngine === 'v2' && (
                <button
                  type="button"
                  data-testid="rollback-v1-btn"
                  className="btn sm ghost"
                  disabled={busy || switchingEngine}
                  onClick={() => handleSwitchEngine('v1')}
                  style={{
                    border: '1px solid rgba(250, 204, 21, 0.35)',
                    color: '#facc15',
                  }}
                  title="Безопасный откат на движок Zapret v1 (nfqws) с автоматической конвертацией аргументов"
                >
                  {switchingEngine ? '⏳ Откат на v1…' : '⏪ Откатить на v1 (Legacy 1.x)'}
                </button>
              )}
            </div>

            <div className="muted small" style={{ fontSize: 11, flexShrink: 0, marginLeft: 'auto' }}>
              Активный движок:{' '}
              <b style={{ color: activeEngine === 'v2' ? '#38bdf8' : '#facc15' }}>
                {activeEngine === 'v2' ? 'Modern 2.0 (nfqws2 + Lua)' : 'Legacy 1.x (nfqws)'}
              </b>
            </div>
          </div>

          {stepLog && (
            <div
              data-testid="zapret-step-log"
              style={{
                marginTop: 10,
                padding: '10px 14px',
                borderRadius: 8,
                background: 'rgba(15, 23, 42, 0.95)',
                border: '1px solid rgba(56, 189, 248, 0.35)',
                fontSize: 12,
                fontFamily: 'Consolas, monospace',
                whiteSpace: 'pre-wrap',
                maxHeight: 180,
                overflowY: 'auto',
                color: '#e2e8f0',
                lineHeight: 1.45,
              }}
            >
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 6, color: '#38bdf8', fontWeight: 600 }}>
                <span>📋 Лог установки:</span>
                <button
                  type="button"
                  onClick={() => setStepLog(null)}
                  style={{ background: 'transparent', border: 'none', color: 'var(--muted)', cursor: 'pointer', fontSize: 11 }}
                >
                  ✕ Закрыть
                </button>
              </div>
              {stepLog}
            </div>
          )}
        </div>

        {/* КНОПКИ ДЕЙСТВИЙ */}
        <div style={{ display: 'flex', gap: 10, flexWrap: 'wrap', marginTop: 16 }}>
          {isInstalled ? (
            <>
              <button
                type="button"
                className="btn sm"
                disabled={busy}
                onClick={() => handleAction('restart')}
                title="Перезапустить процесс nfqws и обновить iptables правила"
              >
                🔄 Перезапустить
              </button>

              <button
                type="button"
                className="btn sm"
                disabled={testingDpi || busy}
                onClick={handleTestDpi}
                style={{
                  background: 'linear-gradient(135deg, rgba(56, 189, 248, 0.15) 0%, rgba(59, 130, 246, 0.15) 100%)',
                  border: '1px solid rgba(56, 189, 248, 0.3)',
                  color: '#38bdf8',
                }}
                title="Проверить доступность YouTube и Discord напрямую с роутера"
              >
                {testingDpi ? '⏳ Тестирование DPI…' : '🧪 Тест YouTube & Discord'}
              </button>

              <button
                type="button"
                className="btn sm ghost"
                onClick={() => setShowHostsEditor((prev) => !prev)}
              >
                📋 {showHostsEditor ? 'Скрыть список доменов' : 'Список доменов (Hostlist)'}
              </button>

              <button
                type="button"
                className="btn sm ghost"
                onClick={() => setShowConfigEditor((prev) => !prev)}
              >
                📝 {showConfigEditor ? 'Скрыть конфиг' : 'zapret.conf'}
              </button>
            </>
          ) : (
            <button
              type="button"
              className="btn primary"
              disabled={busy}
              onClick={() => handleAction('install')}
            >
              {busy ? '⏳ Установка Zapret…' : '📥 Установить Zapret в 1 клик'}
            </button>
          )}
        </div>

        {/* РЕЗУЛЬТАТ ТЕСТА DPI */}
        {testResult && (
          <div
            style={{
              marginTop: 16,
              padding: '12px 16px',
              background: 'rgba(0, 0, 0, 0.25)',
              borderRadius: 10,
              border: '1px solid var(--border)',
              display: 'flex',
              gap: 20,
              flexWrap: 'wrap',
              alignItems: 'center',
            }}
          >
            <b style={{ fontSize: 13 }}>Результаты теста:</b>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <span>🎥 YouTube:</span>
              <span
                className="badge"
                style={{
                  background: testResult.youtube.ok ? 'rgba(34, 197, 94, 0.2)' : testResult.youtube.proxy_ok ? 'rgba(56, 189, 248, 0.2)' : 'rgba(239, 68, 68, 0.2)',
                  color: testResult.youtube.ok ? '#22c55e' : testResult.youtube.proxy_ok ? '#38bdf8' : '#ef4444',
                }}
              >
                {testResult.youtube.ok
                  ? `🟢 Прямой Zapret: HTTP ${testResult.youtube.code} (${Math.round(testResult.youtube.time_secs * 1000)} мс)`
                  : testResult.youtube.proxy_ok
                    ? `🔵 Через прокси: HTTP ${testResult.youtube.proxy_code || 200}`
                    : '🔴 Заблокирован ТСПУ'}
              </span>
            </div>

            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <span>💬 Discord:</span>
              <span
                className="badge"
                style={{
                  background: testResult.discord.ok ? 'rgba(34, 197, 94, 0.2)' : testResult.discord.proxy_ok ? 'rgba(56, 189, 248, 0.2)' : 'rgba(239, 68, 68, 0.2)',
                  color: testResult.discord.ok ? '#22c55e' : testResult.discord.proxy_ok ? '#38bdf8' : '#ef4444',
                }}
              >
                {testResult.discord.ok
                  ? `🟢 Прямой: HTTP ${testResult.discord.code} (${Math.round(testResult.discord.time_secs * 1000)} мс)`
                  : testResult.discord.proxy_ok
                    ? `🔵 Через прокси: HTTP ${testResult.discord.proxy_code || 200}`
                    : '🔴 Заблокирован'}
              </span>
            </div>
          </div>
        )}

        {/* РЕДАКТОР ZAPRET-HOSTS.TXT */}
        {showHostsEditor && (
          <div style={{ marginTop: 16, display: 'flex', flexDirection: 'column', gap: 10 }}>
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
              <span className="muted small">
                Редактирование <code>/opt/etc/zapret/zapret-hosts.txt</code> (по одному домену на строку):
              </span>
              <button
                type="button"
                className="btn sm primary"
                disabled={savingHosts}
                onClick={handleSaveHosts}
              >
                {savingHosts ? 'Сохранение…' : '💾 Сохранить список доменов'}
              </button>
            </div>
            <textarea
              className="input"
              rows={8}
              value={hostsDraft}
              disabled={savingHosts}
              onChange={(e) => { hostsDirtyRef.current = true; setHostsDraft(e.target.value) }}
              placeholder="rutracker.org&#10;ntc.party&#10;kinozal.tv"
              style={{ fontFamily: 'Consolas, monospace', fontSize: 12, resize: 'vertical' }}
            />
          </div>
        )}

        {/* РЕДАКТОР ZAPRET.CONF */}
        {showConfigEditor && (
          <div style={{ marginTop: 16, display: 'flex', flexDirection: 'column', gap: 10 }}>
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
              <span className="muted small">Редактирование <code>/opt/etc/zapret/zapret.conf</code>:</span>
              <button
                type="button"
                className="btn sm primary"
                disabled={savingConfig}
                onClick={handleSaveConfig}
              >
                {savingConfig ? 'Сохранение…' : '💾 Сохранить и применить'}
              </button>
            </div>
            <div style={{ fontSize: 12, padding: '6px 10px', background: 'rgba(234, 179, 8, 0.1)', border: '1px solid rgba(234, 179, 8, 0.3)', borderRadius: 6, color: '#eab308' }}>
              ⚠️ Прямое редактирование конфигурации демона Zapret. Изменяйте параметры только если уверены в их назначении. Спецсимволы командной строки запрещены.
            </div>
            <textarea
              className="input"
              rows={6}
              value={configDraft}
              disabled={savingConfig}
              onChange={(e) => { configDirtyRef.current = true; setConfigDraft(e.target.value) }}
              style={{ fontFamily: 'Consolas, monospace', fontSize: 12, resize: 'vertical' }}
            />
          </div>
        )}
      </section>
  )
}
