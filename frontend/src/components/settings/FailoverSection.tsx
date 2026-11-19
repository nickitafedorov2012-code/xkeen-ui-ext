import React from 'react'
import { apiPost } from '../../api'
import {
  pingClass,
  type AppSettings,
  type ProviderInfo,
  type ServerInfo,
  type StatusInfo,
} from '../../types'
import NumberInput from '../NumberInput'
import { cleanServerDisplayName } from './SwapManager'
import { WhitelistDetectorSection } from './WhitelistDetectorSection'

export interface FailoverSectionProps {
  settings: AppSettings
  status?: StatusInfo | null
  servers: ServerInfo[]
  providers: ProviderInfo[]
  autoSaveStatus: 'idle' | 'saving' | 'saved' | 'error'
  failoverLastPolled: string
  canaryInfo: NonNullable<StatusInfo['failover']>['canary']
  canaryCountdown: number | null
  chain: string[]
  providerChain: string[]
  activePresets: NonNullable<AppSettings['failover']['presets']>
  selectedPresetId: string | null
  showNewPresetInput: boolean
  setShowNewPresetInput: React.Dispatch<React.SetStateAction<boolean>>
  newPresetName: string
  setNewPresetName: React.Dispatch<React.SetStateAction<string>>
  patch: (fn: (s: AppSettings) => void) => void
  toggleFailoverEnabled: (enabled: boolean) => Promise<void>
  testCheck: () => Promise<void>
  chainMove: (idx: number, dir: -1 | 1) => void
  toggleProviderInChain: (id: string) => void
  moveProviderChain: (idx: number, dir: -1 | 1) => void
  applyPresetSlot: (slot: { id: string; name: string; chain: string[] }) => void
  saveCurrentToPreset: (id: string) => void
  createNewPresetSlot: () => void
  deletePresetSlot: (id: string) => void
  notify: (msg: string, isError?: boolean) => void
  refresh?: () => void
}

export function FailoverSection({
  settings,
  status,
  servers,
  providers,
  autoSaveStatus,
  failoverLastPolled,
  canaryInfo,
  canaryCountdown,
  chain,
  providerChain,
  activePresets,
  selectedPresetId,
  showNewPresetInput,
  setShowNewPresetInput,
  newPresetName,
  setNewPresetName,
  patch,
  toggleFailoverEnabled,
  testCheck,
  chainMove,
  toggleProviderInChain,
  moveProviderChain,
  applyPresetSlot,
  saveCurrentToPreset,
  createNewPresetSlot,
  deletePresetSlot,
  notify,
  refresh,
}: FailoverSectionProps) {
  return (
  <section className="card" style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
          <h2 style={{ margin: 0, fontSize: 16 }}>⚡ Резервирование (Failover)</h2>
          {autoSaveStatus === 'saving' && (
            <span className="badge" style={{ borderColor: 'rgba(0, 211, 242, 0.4)', color: '#00D3F2', fontSize: 11 }}>
              сохранение…
            </span>
          )}
        </div>
        <span className={`badge ${settings.failover.enabled ? 'badge-online' : ''}`}>
          {settings.failover.enabled ? '🟢 включён' : '⚪ выключен'}
        </span>
      </div>

      <div className="row" style={{ justifyContent: 'space-between', alignItems: 'center' }}>
        <b>Включить автоматический failover</b>
        <div style={{ display: 'inline-flex', alignItems: 'center', gap: 10 }}>
          <label className="switch" title={settings.failover.enabled ? 'Выключить failover' : 'Включить failover'}>
            <input
              type="checkbox"
              checked={settings.failover.enabled}
              onChange={(e) => toggleFailoverEnabled(e.target.checked)}
            />
            <span className="slider" />
          </label>
          <span style={{ fontSize: 13, minWidth: 64, color: settings.failover.enabled ? 'var(--green)' : 'var(--muted)' }}>
            {settings.failover.enabled ? 'Включён' : 'Выключен'}
          </span>
        </div>
      </div>

      {/* Слоты пресетов Failover */}
      <div style={{ background: 'rgba(255, 255, 255, 0.02)', border: '1px solid var(--border)', borderRadius: 10, padding: '10px 12px', display: 'flex', flexDirection: 'column', gap: 8 }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
          <span style={{ fontSize: 12, fontWeight: 600, color: 'var(--text)' }}>
            📁 Слоты пресетов цепочек:
          </span>
          <span className="muted small">Быстрое переключение без ручной сборки</span>
        </div>

        <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', alignItems: 'center' }}>
          {activePresets.map((slot) => {
            const isSelected = selectedPresetId === slot.id
            return (
              <div
                key={slot.id}
                style={{
                  display: 'inline-flex',
                  alignItems: 'center',
                  borderRadius: 20,
                  background: isSelected ? 'rgba(56, 189, 248, 0.2)' : 'rgba(255, 255, 255, 0.05)',
                  border: isSelected ? '1px solid #38bdf8' : '1px solid var(--border)',
                  padding: '3px 8px 3px 10px',
                  gap: 6,
                }}
              >
                <button
                  type="button"
                  onClick={() => applyPresetSlot(slot)}
                  style={{
                    background: 'none',
                    border: 'none',
                    color: isSelected ? '#38bdf8' : 'var(--text)',
                    cursor: 'pointer',
                    fontSize: 12,
                    fontWeight: 500,
                    padding: 0,
                  }}
                  title={`Применить эту цепочку (${slot.chain?.length || 0} узлов)`}
                >
                  {slot.name} <span style={{ opacity: 0.6, fontSize: 11 }}>({slot.chain?.length || 0})</span>
                </button>
                <button
                  type="button"
                  onClick={() => saveCurrentToPreset(slot.id)}
                  style={{
                    background: 'rgba(56, 189, 248, 0.25)',
                    border: 'none',
                    borderRadius: 4,
                    color: '#38bdf8',
                    cursor: 'pointer',
                    fontSize: 10.5,
                    padding: '1px 5px',
                  }}
                  title="Записать текущую настроенную цепочку в этот слот"
                >
                  💾 Сохранить
                </button>
                {!['preset_default', 'preset_gaming', 'preset_streaming'].includes(slot.id) && (
                  <button
                    type="button"
                    onClick={() => deletePresetSlot(slot.id)}
                    style={{
                      background: 'none',
                      border: 'none',
                      color: '#ef4444',
                      cursor: 'pointer',
                      fontSize: 11,
                      padding: '0 2px',
                    }}
                    title="Удалить слот"
                  >
                    ✕
                  </button>
                )}
              </div>
            )
          })}

          {!showNewPresetInput ? (
            <button
              type="button"
              className="btn sm ghost"
              onClick={() => setShowNewPresetInput(true)}
              style={{ fontSize: 11, padding: '2px 8px', borderRadius: 20 }}
            >
              ➕ Создать слот
            </button>
          ) : (
            <div style={{ display: 'inline-flex', gap: 4, alignItems: 'center' }}>
              <input
                className="input sm"
                placeholder="Имя слота…"
                value={newPresetName}
                onChange={(e) => setNewPresetName(e.target.value)}
                style={{ width: 110, fontSize: 11, padding: '2px 6px' }}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') createNewPresetSlot()
                }}
              />
              <button type="button" className="btn sm primary" onClick={createNewPresetSlot} style={{ fontSize: 11, padding: '2px 6px' }}>
                ✓
              </button>
              <button type="button" className="btn sm ghost" onClick={() => setShowNewPresetInput(false)} style={{ fontSize: 11, padding: '2px 6px' }}>
                ✕
              </button>
            </div>
          )}
        </div>
      </div>

      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(150px, 1fr))', gap: 10 }}>
        <label className="row" style={{ margin: 0, flexDirection: 'column', alignItems: 'stretch', gap: 4 }}>
          <span style={{ fontSize: 12 }}>Порог пинга для смены узла, мс</span>
          <NumberInput
            min={50}
            max={5000}
            step={50}
            fallback={300}
            style={{ width: '100%', minWidth: 90, boxSizing: 'border-box' }}
            value={settings.failover.ping_threshold_ms}
            onChange={(val) => patch((s) => (s.failover.ping_threshold_ms = val))}
          />
        </label>

        <label className="row" style={{ margin: 0, flexDirection: 'column', alignItems: 'stretch', gap: 4 }}>
          <span style={{ fontSize: 12 }}>Интервал проверки пинга, сек</span>
          <NumberInput
            min={15}
            max={3600}
            step={5}
            fallback={60}
            style={{ width: '100%', minWidth: 90, boxSizing: 'border-box' }}
            value={settings.failover.interval_secs}
            onChange={(val) => patch((s) => (s.failover.interval_secs = val))}
          />
        </label>

        <label className="row" style={{ margin: 0, flexDirection: 'column', alignItems: 'stretch', gap: 4 }}>
          <span style={{ fontSize: 12 }} title="Длительность проверочного теста стабильности перед возвратом трафика на основной узел">
            Тест возврата, сек
          </span>
          <NumberInput
            min={15}
            max={600}
            step={15}
            fallback={180}
            style={{ width: '100%', minWidth: 90, boxSizing: 'border-box' }}
            value={settings.failover.canary_duration_secs || 180}
            onChange={(val) => patch((s) => (s.failover.canary_duration_secs = val))}
          />
        </label>
      </div>

      <div className="row" style={{ flexDirection: 'column', alignItems: 'stretch', gap: 6 }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
          <span style={{ fontWeight: 600, fontSize: 13 }}>Цепочка приоритетов серверов и пулов</span>
          <span className="muted small">{chain.length} узлов в цепочке</span>
        </div>

        {/* ДВЕ РАЗДЕЛЬНЫЕ КАРТОЧКИ: 1. Активный сервер, 2. Карантин серверов (зомби) */}
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(280px, 1fr))', gap: 10, marginBottom: 8 }}>
          {/* КАРТОЧКА 1: Активный сервер */}
          <div
            style={{
              background: 'rgba(255, 255, 255, 0.02)',
              border: '1px solid var(--border)',
              borderLeft: '4px solid #22c55e',
              borderRadius: 8,
              padding: '10px 12px',
              display: 'flex',
              flexDirection: 'column',
              gap: 6,
            }}
          >
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 6, minWidth: 0 }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 5, minWidth: 0, overflow: 'hidden' }}>
                {settings.failover.enabled && (
                  <span className="pulse-live-dot" style={{ flexShrink: 0 }} title="Мониторинг Failover активен" />
                )}
                <span style={{ fontSize: 12, fontWeight: 700, color: 'var(--text-bright)', whiteSpace: 'nowrap' }}>
                  Активный сервер
                </span>
                {status?.active_server ? (
                  <span className="badge badge-online" style={{ fontSize: 9.5, padding: '1px 5px', whiteSpace: 'nowrap', lineHeight: 1.2, flexShrink: 0 }}>
                    в сети
                  </span>
                ) : (
                  <span className="badge" style={{ fontSize: 9.5, padding: '1px 5px', color: 'var(--muted)', whiteSpace: 'nowrap', lineHeight: 1.2, flexShrink: 0 }}>
                    не выбран
                  </span>
                )}
              </div>
              <div style={{ display: 'flex', alignItems: 'center', gap: 4, flexShrink: 0 }}>
                {failoverLastPolled && (
                  <span className="muted" style={{ fontSize: 10, whiteSpace: 'nowrap' }}>
                    {failoverLastPolled}
                  </span>
                )}
                <button
                  type="button"
                  className="btn sm ghost"
                  style={{
                    height: 18,
                    width: 18,
                    padding: 0,
                    display: 'inline-flex',
                    alignItems: 'center',
                    justifyContent: 'center',
                    fontSize: 10,
                    borderRadius: 4,
                  }}
                  onClick={() => {
                    refresh?.()
                    notify('Запрос свежего статуса Failover...')
                  }}
                  title="Обновить статус прямо сейчас"
                >
                  🔄
                </button>
              </div>
            </div>

            <div style={{ display: 'flex', alignItems: 'baseline', justifyContent: 'space-between', gap: 6, minWidth: 0 }}>
              <div
                style={{
                  fontSize: 13,
                  fontWeight: 700,
                  color: '#4ade80',
                  overflow: 'hidden',
                  textOverflow: 'ellipsis',
                  whiteSpace: 'nowrap',
                  minWidth: 0,
                }}
                title={status?.active_server?.name || status?.active_server?.id || ''}
              >
                {cleanServerDisplayName(status?.active_server?.name || status?.active_server?.id || 'Сервер не выбран')}
              </div>
              {status?.active_server && status.active_server.ping_ms > 0 && (
                <span className={'ping ' + pingClass(status.active_server.ping_ms)} style={{ fontSize: 10.5, fontWeight: 700, whiteSpace: 'nowrap', flexShrink: 0 }}>
                  {status.active_server.ping_ms} мс
                </span>
              )}
            </div>

            {/* Роль в цепочке */}
            {status?.failover && (
              <div style={{ display: 'flex', alignItems: 'center', gap: 6, flexWrap: 'wrap' }}>
                <span className="muted" style={{ fontSize: 11 }}>Позиция:</span>
                <span
                  className="badge"
                  style={{
                    fontSize: 10,
                    padding: '1px 6px',
                    background:
                      status.failover.priority_server === status.active_server?.id || status.failover.priority_chain?.[0] === status.active_server?.id
                        ? 'rgba(56, 189, 248, 0.15)'
                        : 'rgba(255, 255, 255, 0.06)',
                    color:
                      status.failover.priority_server === status.active_server?.id || status.failover.priority_chain?.[0] === status.active_server?.id
                        ? '#38bdf8'
                        : 'var(--muted)',
                  }}
                >
                  {status.failover.priority_server === status.active_server?.id || status.failover.priority_chain?.[0] === status.active_server?.id
                    ? '👑 Основной сервер'
                    : status.failover.priority_chain?.includes(status.active_server?.id || '')
                    ? `⚡ Резерв #${status.failover.priority_chain.indexOf(status.active_server?.id || '') + 1}`
                    : '🌐 Ручной выбор'}
                </span>
              </div>
            )}

            {canaryInfo && (
              <div style={{ fontSize: 10.5, color: '#c084fc', background: 'rgba(168, 85, 247, 0.1)', padding: '2px 5px', borderRadius: 4, border: '1px solid rgba(168, 85, 247, 0.25)', whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>
                🧪 Тест <b>{cleanServerDisplayName(canaryInfo.target_name)}</b> ({canaryInfo.elapsed_secs}/{canaryInfo.duration_secs || 180}с)
              </div>
            )}
          </div>

          {/* КАРТОЧКА 2: Карантин серверов (зомби) */}
          {(() => {
            const qList = (status?.failover?.quarantine || []).filter((q) => q.remaining_secs > 0)
            const hasQuarantine = qList.length > 0

            return (
              <div
                style={{
                  background: hasQuarantine ? 'rgba(239, 68, 68, 0.05)' : 'rgba(255, 255, 255, 0.02)',
                  border: hasQuarantine ? '1px solid rgba(239, 68, 68, 0.35)' : '1px solid var(--border)',
                  borderLeft: hasQuarantine ? '4px solid #ef4444' : '4px solid #38bdf8',
                  borderRadius: 8,
                  padding: '10px 12px',
                  display: 'flex',
                  flexDirection: 'column',
                  gap: 6,
                }}
              >
                <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 6 }}>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                    <span style={{ fontSize: 13 }}>{hasQuarantine ? '🧟' : '🛡️'}</span>
                    <span style={{ fontSize: 12, fontWeight: 700, color: 'var(--text-bright)' }}>
                      Карантин серверов (зомби)
                    </span>
                  </div>
                  {hasQuarantine && (
                    <button
                      type="button"
                      className="btn sm"
                      style={{
                        padding: '2px 8px',
                        fontSize: 10.5,
                        height: 22,
                        background: 'rgba(239, 68, 68, 0.18)',
                        color: '#f87171',
                        border: '1px solid rgba(239, 68, 68, 0.4)',
                        cursor: 'pointer',
                        fontWeight: 600,
                      }}
                      onClick={async () => {
                        try {
                          await apiPost('failover/quarantine/clear', {})
                          notify('Карантин всех серверов сброшен')
                          refresh?.()
                        } catch (e: any) {
                          notify(e.message || 'Ошибка снятия карантина', true)
                        }
                      }}
                      title="Мгновенно амнистировать все серверы и вернуть их в цепочку Failover"
                    >
                      🛡️ Снять весь карантин
                    </button>
                  )}
                </div>

                <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                  {hasQuarantine ? (
                    <span className="badge" style={{ background: 'rgba(239, 68, 68, 0.2)', color: '#f87171', border: '1px solid rgba(239, 68, 68, 0.4)', fontSize: 10.5, fontWeight: 700 }}>
                      ⚠️ В карантине: {qList.length} {qList.length === 1 ? 'узел' : 'узла(ов)'}
                    </span>
                  ) : (
                    <span className="badge badge-online" style={{ fontSize: 10.5, padding: '2px 8px' }}>
                      🟢 Все узлы в строю
                    </span>
                  )}
                </div>

                {hasQuarantine ? (
                  <div style={{ display: 'flex', flexDirection: 'column', gap: 4, maxHeight: 90, overflowY: 'auto', marginTop: 2 }}>
                    {qList.map((q) => {
                      const sv = servers.find((s) => s.id === q.server_id)
                      const minutesLeft = Math.ceil(q.remaining_secs / 60)
                      return (
                        <div
                          key={q.server_id}
                          style={{
                            display: 'flex',
                            alignItems: 'center',
                            justifyContent: 'space-between',
                            gap: 6,
                            fontSize: 11,
                            padding: '2px 6px',
                            background: 'rgba(0, 0, 0, 0.25)',
                            borderRadius: 4,
                          }}
                        >
                          <span style={{ color: '#fca5a5', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                            {cleanServerDisplayName(sv?.name || q.server_id)}
                          </span>
                          <span className="muted" style={{ fontSize: 10, whiteSpace: 'nowrap' }}>
                            {q.reason || 'таймаут'} ({minutesLeft} мин)
                          </span>
                        </div>
                      )
                    })}
                  </div>
                ) : (
                  <div className="muted small" style={{ fontSize: 11 }}>
                    Защита от флэппинга активна. Зомби-узлы с L7-сбоями изолируются автоматически.
                  </div>
                )}
              </div>
            )
          })()}
        </div>

        {/* Список узлов цепочки в виде современных карточек с акцентным бордером */}
        <div className="modal-list" style={{ maxHeight: 380, display: 'flex', flexDirection: 'column', gap: 8, overflowY: 'auto' }}>
          {chain.length === 0 && (
            <p className="muted small" style={{ margin: '8px 0' }}>
              Цепочка не задана — failover автоматически выбирает лучший доступный сервер.
            </p>
          )}
          {chain.map((id, i) => {
            const sv = servers.find((x) => x.id === id)
            const livePing = (status?.failover?.priority_chain_pings && status.failover.priority_chain_pings[id] !== undefined)
              ? status.failover.priority_chain_pings[id]
              : (status?.active_server && status.active_server.id === id ? status.active_server.ping_ms : (sv?.ping_ms ?? 0))

            const isActive = Boolean(status?.active_server && status.active_server.id === id)
            const isCanary = Boolean(canaryInfo?.active && canaryInfo.target_id === id)
            const liveCanarySecs = isCanary ? (canaryCountdown ?? canaryInfo?.remaining_secs ?? 0) : 0
            const quarantineInfo = status?.failover?.quarantine?.find((q) => q.server_id === id)
            const isZombie = Boolean(
              status?.failover?.zombie_servers?.includes(id) ||
              quarantineInfo?.reason?.toLowerCase().includes('зомби') ||
              quarantineInfo?.reason?.toLowerCase().includes('zombie') ||
              quarantineInfo?.reason?.toLowerCase().includes('l7')
            )
            const isQuarantined = Boolean(quarantineInfo && quarantineInfo.remaining_secs > 0)
            const isTimeout = livePing <= 0 || livePing >= 9999

            // Определение цвета акцентного левого бордюра и фона карточки
            const accentColor = isActive
              ? '#22c55e'
              : isZombie
              ? '#ef4444'
              : isQuarantined
              ? '#f97316'
              : i === 0
              ? '#38bdf8'
              : 'rgba(255, 255, 255, 0.15)'

            const bgTint = isActive
              ? 'rgba(34, 197, 94, 0.05)'
              : isZombie
              ? 'rgba(239, 68, 68, 0.04)'
              : i === 0
              ? 'rgba(56, 189, 248, 0.03)'
              : 'rgba(255, 255, 255, 0.015)'

            return (
              <div
                key={id}
                className="check-row priority-chain-card"
                style={{
                  display: 'flex',
                  flexDirection: 'column',
                  gap: 6,
                  background: bgTint,
                  border: '1px solid var(--border)',
                  borderLeft: `4px solid ${accentColor}`,
                  borderRadius: 8,
                  padding: '8px 12px',
                  minWidth: 0,
                }}
              >
                {/* УРОВЕНЬ 1: Номер приоритета, Чистое имя --- Статус задержки */}
                <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 10, flexWrap: 'nowrap' }}>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 8, minWidth: 0, flex: 1, overflow: 'hidden' }}>
                    <span
                      style={{
                        fontSize: 10.5,
                        fontWeight: 700,
                        padding: '2px 7px',
                        borderRadius: 5,
                        background: i === 0 ? 'rgba(56, 189, 248, 0.2)' : 'rgba(255, 255, 255, 0.07)',
                        color: i === 0 ? '#38bdf8' : 'var(--muted)',
                        whiteSpace: 'nowrap',
                        flexShrink: 0,
                      }}
                    >
                      {i === 0 ? '👑 #1 Основной' : `⚡ #${i + 1} Резерв`}
                    </span>

                    <span
                      className="server-name"
                      style={{
                        fontWeight: 600,
                        fontSize: 13,
                        color: isActive ? '#4ade80' : isZombie ? '#f87171' : 'var(--text-bright)',
                        overflow: 'hidden',
                        textOverflow: 'ellipsis',
                        whiteSpace: 'nowrap',
                        minWidth: 80,
                      }}
                      title={sv ? `${cleanServerDisplayName(sv.name)} (${id})` : id}
                    >
                      {cleanServerDisplayName(sv ? sv.name : `${id} (недоступен)`)}
                    </span>
                  </div>

                  {/* Задержка / статус сети */}
                  <div style={{ display: 'flex', alignItems: 'center', gap: 6, flexShrink: 0 }}>
                    {livePing > (settings.failover.ping_threshold_ms || 300) && !isTimeout && (
                      <span
                        style={{
                          fontSize: 10,
                          padding: '1px 5px',
                          borderRadius: 4,
                          background: 'rgba(249, 115, 22, 0.15)',
                          color: '#fb923c',
                          border: '1px solid rgba(249, 115, 22, 0.3)',
                          fontWeight: 600,
                          whiteSpace: 'nowrap',
                        }}
                        title={`Пинг превышает порог ${settings.failover.ping_threshold_ms || 300} мс. При сбое узел будет пропущен.`}
                      >
                        ⚠️ &gt; {settings.failover.ping_threshold_ms || 300} мс
                      </span>
                    )}
                    {isTimeout ? (
                      <span
                        style={{
                          fontSize: 11,
                          fontWeight: 600,
                          padding: '2px 6px',
                          borderRadius: 4,
                          background: 'rgba(239, 68, 68, 0.15)',
                          color: '#f87171',
                          border: '1px solid rgba(239, 68, 68, 0.3)',
                          whiteSpace: 'nowrap',
                        }}
                      >
                        ❌ Таймаут
                      </span>
                    ) : (
                      <span className={'ping ' + pingClass(livePing)} style={{ fontSize: 11.5, fontWeight: 700, whiteSpace: 'nowrap' }}>
                        {livePing > 0 && livePing < 9999 ? `${livePing} мс` : '—'}
                      </span>
                    )}
                  </div>
                </div>

                {/* УРОВЕНЬ 2: Метаданные (Состояние, Пул/Провайдер, Протокол) --- Панель действий */}
                <div
                  style={{
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'space-between',
                    gap: 8,
                    paddingTop: 5,
                    borderTop: '1px solid rgba(255, 255, 255, 0.04)',
                  }}
                >
                  <div style={{ display: 'flex', alignItems: 'center', gap: 6, flexWrap: 'wrap', minWidth: 0, flex: 1 }}>
                    {isActive ? (
                      <span
                        style={{
                          fontSize: 10,
                          fontWeight: 700,
                          padding: '1px 6px',
                          borderRadius: 4,
                          background: 'rgba(34, 197, 94, 0.18)',
                          color: '#4ade80',
                          border: '1px solid rgba(34, 197, 94, 0.35)',
                          whiteSpace: 'nowrap',
                        }}
                      >
                        🟢 Активен сейчас
                      </span>
                    ) : isCanary ? (
                      <span
                        style={{
                          fontSize: 10,
                          fontWeight: 700,
                          padding: '1px 6px',
                          borderRadius: 4,
                          background: 'rgba(168, 85, 247, 0.2)',
                          color: '#c084fc',
                          border: '1px solid rgba(168, 85, 247, 0.4)',
                          whiteSpace: 'nowrap',
                        }}
                        title="Канареечный тест стабильности узла перед возвратом трафика. Клиенты остаются на резерве, пока проверяется отсутствие обрывов."
                      >
                        🧪 Тест ({liveCanarySecs}с)
                      </span>
                    ) : isZombie ? (
                      <span
                        style={{
                          fontSize: 10,
                          fontWeight: 700,
                          padding: '2px 7px',
                          borderRadius: 4,
                          background: 'rgba(239, 68, 68, 0.2)',
                          color: '#f87171',
                          border: '1px solid rgba(239, 68, 68, 0.45)',
                          cursor: 'help',
                          whiteSpace: 'nowrap',
                        }}
                        title="Сервер отвечает на TCP-пинг, но блокирует реальный веб-трафик (L7-сбой передачи данных). Временно исключён из цепочки в карантине."
                      >
                        🧟 Зомби {quarantineInfo ? `(ещё ${Math.ceil(quarantineInfo.remaining_secs / 60)} мин)` : '(в карантине)'}
                      </span>
                    ) : isQuarantined ? (
                      <span
                        style={{
                          fontSize: 10,
                          fontWeight: 700,
                          padding: '1px 6px',
                          borderRadius: 4,
                          background: 'rgba(249, 115, 22, 0.2)',
                          color: '#fb923c',
                          border: '1px solid rgba(249, 115, 22, 0.4)',
                          whiteSpace: 'nowrap',
                        }}
                      >
                        🛡️ Карантин ({quarantineInfo ? `ещё ${Math.ceil(quarantineInfo.remaining_secs / 60)} мин` : 'активен'})
                      </span>
                    ) : (
                      <span
                        style={{
                          fontSize: 10,
                          fontWeight: 500,
                          padding: '1px 6px',
                          borderRadius: 4,
                          background: 'rgba(255, 255, 255, 0.05)',
                          color: 'var(--muted)',
                          whiteSpace: 'nowrap',
                        }}
                      >
                        💤 В резерве
                      </span>
                    )}

                    {sv?.is_pool ? (
                      <span
                        style={{
                          fontSize: 10,
                          background: 'rgba(168, 85, 247, 0.2)',
                          color: '#c084fc',
                          padding: '1px 6px',
                          borderRadius: 4,
                          fontWeight: 600,
                        }}
                      >
                        🔀 ПУЛ
                      </span>
                    ) : sv?.provider ? (
                      <span className="tag-provider" style={{ fontSize: 10, padding: '1px 5px', cursor: 'default' }}>
                        📦 {sv.provider_name || sv.provider}
                      </span>
                    ) : null}

                    {sv?.protocol && (
                      <span className="badge-subtle" style={{ fontSize: 9.5, opacity: 0.8 }}>
                        {sv.protocol.toUpperCase()}
                      </span>
                    )}

                    {/* Адрес подключения (хост:порт): только если не дублирует имя сервера и не содержит пробелов */}
                    {sv?.host &&
                      sv.host !== sv.name &&
                      !sv.name.toLowerCase().includes(sv.host.toLowerCase()) &&
                      !sv.host.includes(' ') && (
                        <span
                          className="muted"
                          style={{
                            fontSize: 10,
                            fontFamily: 'monospace',
                            maxWidth: 140,
                            overflow: 'hidden',
                            textOverflow: 'ellipsis',
                            whiteSpace: 'nowrap',
                          }}
                          title={`Хост: ${sv.host}:${sv.port || 443}`}
                        >
                          {sv.host}:{sv.port || 443}
                        </span>
                      )}
                  </div>

                  {/* Панель кнопок действий */}
                  <div style={{ display: 'flex', alignItems: 'center', gap: 4, flexShrink: 0 }}>
                    {(isZombie || isQuarantined) && (
                      <button
                        type="button"
                        className="btn sm ghost"
                        style={{
                          height: 22,
                          padding: '0 7px',
                          fontSize: 10,
                          lineHeight: '20px',
                          borderRadius: 5,
                          color: '#f87171',
                          border: '1px solid rgba(239, 68, 68, 0.35)',
                          background: 'rgba(239, 68, 68, 0.1)',
                          cursor: 'pointer',
                          fontWeight: 600,
                          whiteSpace: 'nowrap',
                        }}
                        title="Досрочно снять карантин с узла"
                        onClick={async () => {
                          try {
                            await apiPost('failover/quarantine/clear', { server_id: id })
                            notify(`Карантин снят с ${cleanServerDisplayName(sv?.name || id)}`)
                            refresh?.()
                          } catch (e: any) {
                            notify(e.message || 'Ошибка снятия карантина', true)
                          }
                        }}
                      >
                        🛡️ Снять карантин
                      </button>
                    )}

                    {!isActive && (
                      <button
                        type="button"
                        className="btn sm"
                        style={{
                          height: 22,
                          padding: '0 8px',
                          fontSize: 10.5,
                          lineHeight: '20px',
                          borderRadius: 5,
                          background: 'rgba(56, 189, 248, 0.12)',
                          border: '1px solid rgba(56, 189, 248, 0.35)',
                          color: '#38bdf8',
                          cursor: 'pointer',
                          fontWeight: 600,
                        }}
                        title="Принудительно переключиться на этот сервер"
                        onClick={async () => {
                          try {
                            if (isZombie || isQuarantined) {
                              await apiPost('failover/quarantine/clear', { server_id: id })
                            }
                            await apiPost('servers/switch', { id })
                            notify(`Переключение на ${cleanServerDisplayName(sv?.name || id)}...`)
                            refresh?.()
                          } catch (e: any) {
                            notify(e.message || 'Ошибка переключения', true)
                          }
                        }}
                      >
                        ⚡ Включить
                      </button>
                    )}

                    <button
                      type="button"
                      className="btn sm ghost"
                      style={{ height: 22, width: 22, padding: 0, fontSize: 10, display: 'inline-flex', alignItems: 'center', justifyContent: 'center' }}
                      disabled={i === 0}
                      onClick={() => chainMove(i, -1)}
                      title="Повысить приоритет"
                    >
                      ▲
                    </button>
                    <button
                      type="button"
                      className="btn sm ghost"
                      style={{ height: 22, width: 22, padding: 0, fontSize: 10, display: 'inline-flex', alignItems: 'center', justifyContent: 'center' }}
                      disabled={i === chain.length - 1}
                      onClick={() => chainMove(i, 1)}
                      title="Понизить приоритет"
                    >
                      ▼
                    </button>
                    <button
                      type="button"
                      className="btn sm ghost"
                      style={{ height: 22, width: 22, padding: 0, color: '#ef4444', fontSize: 11, display: 'inline-flex', alignItems: 'center', justifyContent: 'center' }}
                      onClick={() => patch((s) => (s.failover.priority_chain = chain.filter((x) => x !== id)))}
                      title="Удалить из цепочки"
                    >
                      ✕
                    </button>
                  </div>
                </div>
              </div>
            )
          })}
        </div>

        {/* Аккуратный футер в одну строку: выбор добавления сервера + кнопка очистки */}
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 10, marginTop: 4 }}>
          <select
            className="select"
            value=""
            onChange={(e) => {
              if (!e.target.value) return
              if (!chain.includes(e.target.value)) {
                patch((s) => (s.failover.priority_chain = [...chain, e.target.value]))
              }
            }}
            style={{ flex: 1, minWidth: 0, margin: 0 }}
          >
            <option value="">+ добавить сервер или пул в цепочку…</option>
            {servers.some((s) => s.is_pool && !chain.includes(s.id)) && (
              <optgroup label="🔀 Прокси-пулы и селекторы">
                {servers
                  .filter((s) => s.is_pool && !chain.includes(s.id))
                  .map((s) => (
                    <option key={s.id} value={s.id}>
                      🔀 {s.name} ({s.protocol || 'pool'})
                    </option>
                  ))}
              </optgroup>
            )}
            <optgroup label="🌐 Узлы и серверы">
              {servers
                .filter((s) => !s.is_pool && !chain.includes(s.id))
                .map((s) => {
                  const optPing = (status?.failover?.priority_chain_pings && status.failover.priority_chain_pings[s.id] !== undefined)
                    ? status.failover.priority_chain_pings[s.id]
                    : s.ping_ms
                  const isOptZombie = Boolean(
                    status?.failover?.zombie_servers?.includes(s.id) ||
                    status?.failover?.quarantine?.some((q) => q.server_id === s.id && (
                      q.reason.toLowerCase().includes('зомби') ||
                      q.reason.toLowerCase().includes('zombie') ||
                      q.reason.toLowerCase().includes('l7')
                    ))
                  )
                  return (
                    <option key={s.id} value={s.id}>
                      {s.provider_name ? `[${s.provider_name}] ` : s.provider ? `[${s.provider}] ` : ''}{s.name} · {isOptZombie ? '🧟 Зомби' : optPing > 0 ? `${optPing} мс` : '—'}
                    </option>
                  )
                })}
            </optgroup>
          </select>

          {chain.length > 0 && (
            <button
              type="button"
              className="btn sm ghost"
              style={{ whiteSpace: 'nowrap', flexShrink: 0, color: 'var(--muted)' }}
              onClick={() => patch((s) => (s.failover.priority_chain = []))}
              title="Полностью очистить цепочку приоритетов"
            >
              🗑️ Очистить цепочку
            </button>
          )}
        </div>
      </div>

      {/* Приоритет провайдеров в Failover (Tier 1 vs Tier 2) */}
      <div className="row" style={{ flexDirection: 'column', alignItems: 'stretch', gap: 8, background: 'rgba(255, 255, 255, 0.02)', padding: '12px 14px', borderRadius: 8, border: '1px solid var(--border)' }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: 6 }}>
          <div>
            <b style={{ display: 'block', fontSize: 13 }}>📡 Приоритет провайдеров подписок</b>
            <span className="muted small">
              Порядок переключения между поставщиками. Серверы Tier 1 (VPS) имеют преимущество над Tier 2 (белые списки).
            </span>
          </div>
          {providerChain.length > 0 && (
            <button
              type="button"
              className="btn sm ghost"
              style={{ color: 'var(--muted)', fontSize: 11 }}
              onClick={() => patch((s) => (s.failover.provider_chain = []))}
              title="Сбросить порядок провайдеров на стандартный"
            >
              Сбросить
            </button>
          )}
        </div>

        {providers.length === 0 ? (
          <span className="muted small">Провайдеры подписок не обнаружены.</span>
        ) : (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
            {(() => {
              // Combine providers: explicitly ordered ones first, then remaining
              const ordered = [...providerChain]
                .map((id) => providers.find((p) => p.id === id))
                .filter((p): p is ProviderInfo => Boolean(p))
              const remaining = providers.filter((p) => !providerChain.includes(p.id))
              const allList = [...ordered, ...remaining]

              return allList.map((p) => {
                const inChain = providerChain.includes(p.id)
                const chainIdx = providerChain.indexOf(p.id)
                const isTier2 = Boolean(p.is_whitelist)

                return (
                  <div
                    key={p.id}
                    className="provider-chain-row"
                    style={{
                      display: 'flex',
                      alignItems: 'center',
                      justifyContent: 'space-between',
                      gap: 10,
                      padding: '6px 10px',
                      background: inChain ? 'rgba(56, 189, 248, 0.04)' : 'rgba(255, 255, 255, 0.015)',
                      borderRadius: 6,
                      border: `1px solid ${inChain ? 'rgba(56, 189, 248, 0.25)' : 'rgba(255, 255, 255, 0.06)'}`,
                    }}
                  >
                    <div style={{ display: 'flex', alignItems: 'center', gap: 8, minWidth: 0, flex: 1 }}>
                      <input
                        type="checkbox"
                        checked={inChain}
                        onChange={() => toggleProviderInChain(p.id)}
                        title={inChain ? 'Исключить из приоритетной цепочки' : 'Включить в приоритетную цепочку'}
                        style={{ cursor: 'pointer', margin: 0 }}
                      />
                      {inChain && (
                        <span
                          style={{
                            fontSize: 10,
                            fontWeight: 700,
                            padding: '1px 5px',
                            borderRadius: 4,
                            background: chainIdx === 0 ? 'rgba(56, 189, 248, 0.25)' : 'rgba(255, 255, 255, 0.08)',
                            color: chainIdx === 0 ? '#38bdf8' : 'var(--muted)',
                          }}
                        >
                          #{chainIdx + 1}
                        </span>
                      )}
                      <span style={{ fontWeight: 600, fontSize: 12.5, color: inChain ? 'var(--text-bright)' : 'var(--muted)' }}>
                        {p.name || p.id}
                      </span>
                      {/* Tier индикатор: Tier 1 (VPS) или Tier 2 (БС) */}
                      {isTier2 ? (
                        <span
                          className="badge"
                          style={{
                            fontSize: 9.5,
                            padding: '1px 6px',
                            background: 'rgba(234, 179, 8, 0.15)',
                            color: '#eab308',
                            border: '1px solid rgba(234, 179, 8, 0.3)',
                            fontWeight: 600,
                          }}
                        >
                          🛡️ Tier 2 (БС)
                        </span>
                      ) : (
                        <span
                          className="badge"
                          style={{
                            fontSize: 9.5,
                            padding: '1px 6px',
                            background: 'rgba(56, 189, 248, 0.15)',
                            color: '#38bdf8',
                            border: '1px solid rgba(56, 189, 248, 0.3)',
                            fontWeight: 600,
                          }}
                        >
                          ⚡ Tier 1 (VPS)
                        </span>
                      )}
                      <span className="muted" style={{ fontSize: 11 }}>({p.count} узлов)</span>
                    </div>

                    {inChain && (
                      <div style={{ display: 'flex', alignItems: 'center', gap: 4, flexShrink: 0 }}>
                        <button
                          type="button"
                          className="btn sm ghost"
                          style={{ height: 22, width: 22, padding: 0, fontSize: 10, display: 'inline-flex', alignItems: 'center', justifyContent: 'center' }}
                          disabled={chainIdx === 0}
                          onClick={() => moveProviderChain(chainIdx, -1)}
                          title="Повысить приоритет провайдера"
                        >
                          ▲
                        </button>
                        <button
                          type="button"
                          className="btn sm ghost"
                          style={{ height: 22, width: 22, padding: 0, fontSize: 10, display: 'inline-flex', alignItems: 'center', justifyContent: 'center' }}
                          disabled={chainIdx === providerChain.length - 1}
                          onClick={() => moveProviderChain(chainIdx, 1)}
                          title="Понизить приоритет провайдера"
                        >
                          ▼
                        </button>
                      </div>
                    )}
                  </div>
                )
              })
            })()}
          </div>
        )}
      </div>

      <div className="row" style={{ justifyContent: 'space-between', alignItems: 'center' }}>
        <span>Возвращаться на приоритетный при восстановлении</span>
        <div style={{ display: 'inline-flex', alignItems: 'center', gap: 10 }}>
          <label className="switch" title="Автовозврат при восстановлении связи">
            <input
              type="checkbox"
              checked={settings.failover.auto_restore_priority}
              onChange={(e) => patch((s) => (s.failover.auto_restore_priority = e.target.checked))}
            />
            <span className="slider" />
          </label>
          <span style={{ fontSize: 13, minWidth: 64, color: settings.failover.auto_restore_priority ? 'var(--green)' : 'var(--muted)' }}>
            {settings.failover.auto_restore_priority ? 'Да' : 'Нет'}
          </span>
        </div>
      </div>

      {/* Карантин сбойных серверов (зомби) */}
      <div className="row" style={{ justifyContent: 'space-between', alignItems: 'center' }}>
        <div>
          <b style={{ display: 'block', fontSize: 13 }}>🛡️ Карантин сбойных серверов (зомби)</b>
          <span className="muted small">Автоматически изолирует нестабильные узлы. Если выключено — серверы никогда не блокируются в карантине</span>
        </div>
        <div style={{ display: 'inline-flex', alignItems: 'center', gap: 10 }}>
          <label className="switch" title="Включить или выключить автоматический карантин">
            <input
              type="checkbox"
              checked={settings.failover.quarantine_enabled ?? true}
              onChange={(e) => patch((s) => (s.failover.quarantine_enabled = e.target.checked))}
              data-testid="settings-quarantine-toggle"
            />
            <span className="slider" />
          </label>
          <span style={{ fontSize: 13, minWidth: 64, color: (settings.failover.quarantine_enabled ?? true) ? 'var(--green)' : 'var(--muted)' }}>
            {(settings.failover.quarantine_enabled ?? true) ? 'Включён' : 'Выключен'}
          </span>
        </div>
      </div>

      {/* 🧠 Умный фейловер (приоритет VPS над белыми списками) */}
      <div className="row" style={{ justifyContent: 'space-between', alignItems: 'center' }}>
        <div>
          <b style={{ display: 'block', fontSize: 13 }}>🧠 Умный фейловер (приоритет VPS над белыми списками)</b>
          <span className="muted small">Автоматически возвращает трафик на VPS, когда они оживают, защищая лимитированные узлы [БС]</span>
        </div>
        <div style={{ display: 'inline-flex', alignItems: 'center', gap: 10 }}>
          <label className="switch" title="Включить или выключить умный фейловер">
            <input
              type="checkbox"
              checked={settings.failover.smart_failover_enabled ?? true}
              onChange={(e) => patch((s) => (s.failover.smart_failover_enabled = e.target.checked))}
              data-testid="settings-smart-failover-toggle"
            />
            <span className="slider" />
          </label>
          <span style={{ fontSize: 13, minWidth: 64, color: (settings.failover.smart_failover_enabled ?? true) ? 'var(--green)' : 'var(--muted)' }}>
            {(settings.failover.smart_failover_enabled ?? true) ? 'Включён' : 'Выключен'}
          </span>
        </div>
      </div>

      {/* Best-Ping Failover (Динамический выбор узла с наименьшей задержкой) */}
      <div className="row" style={{ justifyContent: 'space-between', alignItems: 'center' }}>
        <div>
          <b style={{ display: 'block', fontSize: 13 }}>🚀 Режим Best-Ping (пул до 15 узлов)</b>
          <span className="muted small">Автоматический опрос пула серверов и переключение на узел с минимальным пингом</span>
        </div>
        <div style={{ display: 'inline-flex', alignItems: 'center', gap: 10 }}>
          <label className="switch" title="Включить режим динамического выбора лучшего пинга">
            <input
              type="checkbox"
              checked={Boolean(settings.failover.best_ping_enabled)}
              onChange={(e) => patch((s) => (s.failover.best_ping_enabled = e.target.checked))}
              data-testid="settings-best-ping-toggle"
            />
            <span className="slider" />
          </label>
          <span style={{ fontSize: 13, minWidth: 64, color: settings.failover.best_ping_enabled ? 'var(--green)' : 'var(--muted)' }}>
            {settings.failover.best_ping_enabled ? 'Включён' : 'Выключен'}
          </span>
        </div>
      </div>

      {settings.failover.best_ping_enabled && (
        <div className="row-grid" style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))', gap: 10, background: 'rgba(0, 211, 242, 0.04)', padding: '10px 12px', borderRadius: 8, border: '1px solid rgba(0, 211, 242, 0.15)' }}>
          <label className="row" style={{ margin: 0, flexDirection: 'column', alignItems: 'flex-start', gap: 4 }}>
            <span style={{ fontSize: 12 }}>Размер пула серверов (шт.)</span>
            <NumberInput
              min={3}
              max={30}
              step={1}
              fallback={15}
              value={settings.failover.best_ping_pool_size ?? 15}
              onChange={(val) => patch((s) => (s.failover.best_ping_pool_size = val))}
            />
          </label>
          <label className="row" style={{ margin: 0, flexDirection: 'column', alignItems: 'flex-start', gap: 4 }}>
            <span style={{ fontSize: 12 }}>Гистерезис переключения (мс)</span>
            <NumberInput
              min={10}
              max={200}
              step={5}
              fallback={40}
              value={settings.failover.best_ping_hysteresis_ms ?? 40}
              onChange={(val) => patch((s) => (s.failover.best_ping_hysteresis_ms = val))}
            />
          </label>
        </div>
      )}

      {/* Whitelist Detector Card */}
      <WhitelistDetectorSection
        settings={settings}
        servers={servers}
        providers={providers}
        patch={patch}
        notify={notify}
        refresh={refresh}
      />

      <div style={{ display: 'flex', gap: 8, justifyContent: 'space-between', alignItems: 'center', marginTop: 4, flexWrap: 'wrap' }}>
        <button className="btn" onClick={testCheck}>🔍 Тестовая проверка сейчас</button>
        <div style={{ display: 'inline-flex', alignItems: 'center', gap: 6, fontSize: 13 }}>
          {autoSaveStatus === 'saving' && (
            <span style={{ color: '#00D3F2', display: 'inline-flex', alignItems: 'center', gap: 5 }}>
              ⏳ Сохранение…
            </span>
          )}
          {autoSaveStatus === 'saved' && (
            <span style={{ color: '#34d399', display: 'inline-flex', alignItems: 'center', gap: 5 }}>
              ✓ Сохранено автоматически
            </span>
          )}
          {autoSaveStatus === 'error' && (
            <span style={{ color: '#f87171', display: 'inline-flex', alignItems: 'center', gap: 5 }}>
              ⚠️ Ошибка автосохранения
            </span>
          )}
        </div>
      </div>
    </section>

  )
}
