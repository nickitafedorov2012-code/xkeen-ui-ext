import React from 'react'
import { pingClass, getFlowStatus, type ProviderInfo, type ServerInfo, type SpeedtestResult, type StatusInfo } from '../../types'
import { getJitterColor } from '../../utils/jitter'
import { Sparkline, formatSpeed, formatBytes, formatQuotaBytes } from './serversHelpers'

export interface ActiveServerBannerProps {
  activeServer: ServerInfo | null
  status?: StatusInfo | null
  providers: ProviderInfo[]
  providerColorMap: Map<string, string>
  failoverChain: string[]
  pingHistory: number[]
  jitterVal: number
  jitterMeasuring: boolean
  liveTraffic: {
    proxy?: { down: number; up: number }
    whitelist?: { down: number; up: number }
    whitelist_active?: boolean
    down?: number
    up?: number
    proxy_bytes?: number
    whitelist_bytes?: number
  } | null
  speedResults: Record<string, SpeedtestResult>
  speedtestingId: string | null
  editingAliasId: string | null
  aliasInput: string
  aliasSaving: boolean
  setEditingAliasId: React.Dispatch<React.SetStateAction<string | null>>
  setAliasInput: React.Dispatch<React.SetStateAction<string>>
  setShareServer: React.Dispatch<React.SetStateAction<ServerInfo | null>>
  openProvidersModal: () => void
  saveAlias: (id: string) => void
  saveAliasWithVal: (id: string, val: string) => void
  runSpeedtest: (s: ServerInfo) => void
  measureLiveJitter: () => void
  toggleWhitelist: (id: string) => void
  makeServerNormal: (id: string) => void
  addToFailover: (serverId: string, position: 'start' | 'end') => void
}

export function ActiveServerBanner({
  activeServer,
  status,
  providers,
  providerColorMap,
  failoverChain,
  pingHistory,
  jitterVal,
  jitterMeasuring,
  liveTraffic,
  speedResults,
  speedtestingId,
  editingAliasId,
  aliasInput,
  aliasSaving,
  setEditingAliasId,
  setAliasInput,
  setShareServer,
  openProvidersModal,
  saveAlias,
  saveAliasWithVal,
  runSpeedtest,
  measureLiveJitter,
  toggleWhitelist,
  makeServerNormal,
  addToFailover,
}: ActiveServerBannerProps) {
  return (
  <div className="servers-active-deck-card">
    {activeServer ? (
      <>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', gap: 6, flexWrap: 'wrap' }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 6, flexWrap: 'wrap' }}>
            <span className="pinned-header-tag" style={{ background: '#22c55e', color: '#0f172a', fontWeight: 700, padding: '2px 8px', borderRadius: 4, fontSize: 10 }}>
              🟢 АКТИВНЫЙ УЗЕЛ
            </span>
            <span className="badge protocol-badge">{activeServer.protocol}</span>
            {activeServer.provider && (
              <span className="tag-provider" onClick={openProvidersModal} style={{ cursor: 'pointer' }}>
                <span className="sub-marker-dot" style={{ backgroundColor: providerColorMap.get(activeServer.provider) || '#a855f7' }} />
                <span className="tag-provider-name">{activeServer.provider_name || activeServer.provider}</span>
              </span>
            )}
            {status?.whitelist_mode_active && (
              <span className="badge" style={{ background: 'rgba(168, 85, 247, 0.25)', color: '#c084fc', border: '1px solid rgba(168, 85, 247, 0.5)', fontSize: 10, fontWeight: 700 }}>
                🟣 ОБХОД БЕЛЫХ СПИСКОВ
              </span>
            )}
          </div>

          <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
            {getFlowStatus(activeServer) === 'ok' && (
              <span className="badge badge-flow-ok" style={{ fontSize: 10, padding: '1px 5px' }}>🟢 Flow</span>
            )}
            {speedResults[activeServer.id] && (
              <span className="badge badge-speed" style={{ fontSize: 10, padding: '1px 5px' }}>
                ⚡ {speedResults[activeServer.id].speed_mbps.toFixed(1)} М
              </span>
            )}
            <span className={'ping ' + pingClass(activeServer.ping_ms)}>
              {activeServer.ping_ms > 0 ? `${activeServer.ping_ms} мс` : '—'}
            </span>
            <Sparkline ping={activeServer.ping_ms} />
          </div>
        </div>

        {/* Название и псевдоним */}
        <div>
          {editingAliasId === activeServer.id ? (
            <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
              <input
                className="input sm"
                style={{ flex: 1, fontSize: 13 }}
                value={aliasInput}
                onChange={(e) => setAliasInput(e.target.value)}
                placeholder="Пользовательский псевдоним (пусто = сброс)"
                autoFocus
                onKeyDown={(e) => {
                  if (e.key === 'Enter') saveAlias(activeServer.id)
                  if (e.key === 'Escape') setEditingAliasId(null)
                }}
              />
              <button className="btn sm primary" onClick={() => saveAlias(activeServer.id)} disabled={aliasSaving}>
                ✓
              </button>
              <button className="btn sm ghost" onClick={() => setEditingAliasId(null)}>
                ✕
              </button>
            </div>
          ) : (
            <div style={{ display: 'flex', alignItems: 'center', gap: 6, flexWrap: 'wrap' }}>
              <b style={{ fontSize: 14, color: 'var(--text-bright)' }}>
                {activeServer.custom_alias ? `${activeServer.custom_alias} (${activeServer.name})` : activeServer.name}
              </b>
              <button
                type="button"
                className="btn sm ghost"
                style={{ padding: '0 4px', fontSize: 12, height: 22, opacity: 0.7 }}
                onClick={() => {
                  setEditingAliasId(activeServer.id)
                  setAliasInput(activeServer.custom_alias || '')
                }}
                title="Переименовать узел (задать псевдоним)"
              >
                ✏️
              </button>
              {activeServer.custom_alias && (
                <button
                  type="button"
                  className="btn sm ghost"
                  style={{ padding: '0 4px', fontSize: 12, height: 22, opacity: 0.7, color: 'var(--text-danger, #ef4444)' }}
                  onClick={() => saveAliasWithVal(activeServer.id, '')}
                  title="Сбросить пользовательский псевдоним"
                >
                  ✕
                </button>
              )}
            </div>
          )}
          <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginTop: 4, flexWrap: 'wrap' }}>
            <div className="muted small" style={{ fontSize: 11 }}>
              {activeServer.protocol === 'VK-CALL' ? 'Адрес VPS:' : 'IP сервера:'}{' '}
              <code>
                {activeServer.protocol === 'VK-CALL' && (!activeServer.port || activeServer.port <= 0)
                  ? activeServer.host || 'Интерфейс csq0 (Звонки ВК)'
                  : (
                    <>
                      {activeServer.resolved_ip || activeServer.host}
                      {activeServer.resolved_ip && activeServer.resolved_ip !== activeServer.host
                        ? ` (${activeServer.host})`
                        : ''}
                      {activeServer.port ? `:${activeServer.port}` : ''}
                    </>
                  )}
              </code>
            </div>
            {/* Роль в Failover-цепочке */}
            {status?.failover && (
              <span
                className="badge"
                style={{
                  fontSize: 10.5,
                  fontWeight: 700,
                  padding: '2px 7px',
                  borderRadius: 4,
                  background:
                    status.failover.priority_server === activeServer.id || status.failover.priority_chain?.[0] === activeServer.id
                      ? 'rgba(56, 189, 248, 0.2)'
                      : status.failover.priority_chain?.includes(activeServer.id)
                      ? 'rgba(245, 158, 11, 0.2)'
                      : 'rgba(255, 255, 255, 0.06)',
                  color:
                    status.failover.priority_server === activeServer.id || status.failover.priority_chain?.[0] === activeServer.id
                      ? '#38bdf8'
                      : status.failover.priority_chain?.includes(activeServer.id)
                      ? '#fbbf24'
                      : 'var(--muted)',
                  border: '1px solid rgba(255, 255, 255, 0.1)',
                }}
                title="Позиция узла в автоматической цепочке защиты от сбоев (Failover)"
              >
                {status.failover.priority_server === activeServer.id || status.failover.priority_chain?.[0] === activeServer.id
                  ? '👑 Основной сервер'
                  : status.failover.priority_chain?.includes(activeServer.id)
                  ? `⚡ Резерв #${status.failover.priority_chain.indexOf(activeServer.id) + 1}`
                  : '🌐 Ручной выбор'}
              </span>
            )}
          </div>

          {/* Apple-style Live-телеметрия узла: скорость, трафик и спарклайн задержки */}
          <div
            className="active-node-telemetry"
            style={{
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'space-between',
              gap: 12,
              padding: '6px 12px',
              borderRadius: 8,
              marginTop: 8,
              marginBottom: 4,
              flexWrap: 'wrap',
            }}
          >
            {(() => {
              const isWl = Boolean(status?.whitelist_mode_active || activeServer.is_whitelist)
              const downSpeed = isWl
                ? (liveTraffic?.whitelist?.down ?? liveTraffic?.down ?? 0)
                : (liveTraffic?.proxy?.down ?? liveTraffic?.down ?? 0)
              const upSpeed = isWl
                ? (liveTraffic?.whitelist?.up ?? liveTraffic?.up ?? 0)
                : (liveTraffic?.proxy?.up ?? liveTraffic?.up ?? 0)
              const totalBytes = (liveTraffic?.proxy_bytes ?? 0) + (liveTraffic?.whitelist_bytes ?? 0)

              return (
                <div style={{ display: 'flex', alignItems: 'center', gap: 8, fontSize: 12 }}>
                  <span className="active-node-telemetry-label" style={{ fontSize: 11, color: 'var(--muted)', fontWeight: 600 }}>Live-трафик:</span>
                  <span style={{ color: '#38bdf8', fontWeight: 700, fontFamily: 'Consolas, monospace' }}>
                    ↓ {formatSpeed(downSpeed)}
                  </span>
                  <span style={{ color: '#c084fc', fontWeight: 700, fontFamily: 'Consolas, monospace' }}>
                    ↑ {formatSpeed(upSpeed)}
                  </span>
                  {totalBytes > 0 && (
                    <span className="muted" style={{ fontSize: 11, marginLeft: 2 }}>
                      (всего {formatBytes(totalBytes)})
                    </span>
                  )}
                </div>
              )
            })()}

            {/* Квота подписки активного сервера */}
            {(() => {
              const prov = providers.find((p) => p.id === activeServer.provider)
              if (!prov || !prov.total_bytes || prov.total_bytes <= 0) return null
              const used = (prov.upload_bytes || 0) + (prov.download_bytes || 0)
              const total = prov.total_bytes
              const remaining = Math.max(0, total - used)
              const percent = Math.min(100, Math.round((used / total) * 100))
              return (
                <div
                  style={{
                    display: 'flex',
                    alignItems: 'center',
                    gap: 6,
                    fontSize: 11,
                    padding: '2px 8px',
                    background: 'rgba(56, 189, 248, 0.08)',
                    border: '1px solid rgba(56, 189, 248, 0.25)',
                    borderRadius: 6,
                  }}
                  title={`Лимит: ${formatQuotaBytes(used)} из ${formatQuotaBytes(total)} (${percent}%)${prov.expire_timestamp ? ` · До ${new Date(prov.expire_timestamp * 1000).toLocaleDateString()}` : ''}`}
                >
                  <span style={{ color: '#38bdf8', fontWeight: 600 }}>📊 Трафик подписки:</span>
                  <span style={{ color: remaining === 0 ? '#ef4444' : '#4ade80', fontWeight: 700 }}>
                    {formatQuotaBytes(remaining)} ост.
                  </span>
                  <span className="muted" style={{ fontSize: 10 }}>({percent}%)</span>
                </div>
              )
            })()}

            {activeServer && (
              <div
                onClick={measureLiveJitter}
                style={{
                  display: 'inline-flex',
                  alignItems: 'center',
                  gap: 5,
                  padding: '2px 8px',
                  borderRadius: 6,
                  cursor: jitterMeasuring ? 'wait' : 'pointer',
                  background:
                    pingHistory.length < 2
                      ? 'rgba(148, 163, 184, 0.1)'
                      : jitterVal > 25
                      ? 'rgba(239, 68, 68, 0.12)'
                      : jitterVal > 10
                      ? 'rgba(234, 179, 8, 0.12)'
                      : 'rgba(34, 197, 94, 0.12)',
                  border: `1px solid ${
                    pingHistory.length < 2
                      ? 'rgba(148, 163, 184, 0.25)'
                      : jitterVal > 25
                      ? 'rgba(239, 68, 68, 0.35)'
                      : jitterVal > 10
                      ? 'rgba(234, 179, 8, 0.35)'
                      : 'rgba(34, 197, 94, 0.35)'
                  }`,
                  fontSize: 11,
                  userSelect: 'none',
                  transition: 'all 0.2s ease',
                }}
                title={
                  jitterMeasuring
                    ? 'Выполняется измерение задержки...'
                    : pingHistory.length < 2
                    ? `Задержка: ${activeServer.ping_ms > 0 ? `${activeServer.ping_ms} мс` : '—'} | Джиттер: требуется от 2 замеров (нажмите для экспресс-замера)`
                    : `Задержка: ${activeServer.ping_ms} мс | Джиттер: ±${jitterVal} мс (разброс по ${pingHistory.length} точкам) | Мин: ${Math.min(...pingHistory)} мс, Макс: ${Math.max(...pingHistory)} мс | Нажмите для экспресс-замера`
                }
              >
                <span>{jitterMeasuring ? '⏳' : '📶'}</span>
                <span style={{ color: 'var(--muted)', fontSize: 10.5 }}>Джиттер:</span>
                <span
                  style={{
                    fontFamily: 'Consolas, monospace',
                    fontWeight: 600,
                    color: getJitterColor(jitterVal, pingHistory.length),
                  }}
                >
                  {jitterMeasuring ? '...' : pingHistory.length < 2 ? '—' : `±${jitterVal} мс`}
                </span>
              </div>
            )}
          </div>
        </div>

        {/* Действия */}
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
          <span className="tag current-active-badge" style={{ fontSize: 11, padding: '2px 8px' }}>
            <span className="dot on" style={{ width: 6, height: 6, marginRight: 4, display: 'inline-block' }} />
            Активен
          </span>
          {/* FE-MOBILE: перенос кнопок на новую строку вместо распирания карточки на мобильных */}
          <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap' }}>
            <button
              type="button"
              className={`btn sm ghost btn-speedtest ${speedtestingId === activeServer.id ? 'loading' : ''}`}
              onClick={() => runSpeedtest(activeServer)}
              disabled={speedtestingId !== null}
              title="Замерить скорость"
            >
              <span>{speedtestingId === activeServer.id ? '⏳' : '🚀'}</span>
              <span>{speedtestingId === activeServer.id ? 'Замер…' : 'Скорость'}</span>
            </button>
            {activeServer.protocol === 'VK-CALL' ? (
              <button
                type="button"
                className="btn sm ghost"
                onClick={() => window.dispatchEvent(new CustomEvent('xr:switch-tab', { detail: 'vkturn' }))}
                title="Открыть настройки VK-туннеля"
              >
                📞 Настройки ВК
              </button>
            ) : (
              <button
                type="button"
                className="btn sm ghost"
                onClick={() => setShareServer(activeServer)}
                title="Ссылка подключения и QR-код"
              >
                🔗 Ссылка
              </button>
            )}
            {(() => {
              const chainIdx = failoverChain.indexOf(activeServer.id)
              const isStart = chainIdx === 0

              return (
                <>
                  <button
                    type="button"
                    className={`btn sm ghost ${isStart ? 'is-priority' : ''}`}
                    onClick={() => addToFailover(activeServer.id, 'start')}
                    style={isStart ? { background: 'rgba(56, 189, 248, 0.2)', color: '#38bdf8', borderColor: '#38bdf8', fontWeight: 700 } : undefined}
                    title={isStart ? 'Основной узел Failover (#1). Нажмите для снятия' : 'Сделать основным узлом (в начало Failover)'}
                  >
                    <span>{isStart ? '👑 Основной' : '⚡↑ В начало'}</span>
                  </button>
                  <button
                    type="button"
                    className={`btn sm ghost ${chainIdx > 0 ? 'is-priority' : ''}`}
                    onClick={() => addToFailover(activeServer.id, 'end')}
                    style={chainIdx > 0 ? { background: 'rgba(245, 158, 11, 0.15)', color: '#fbbf24', borderColor: '#fbbf24', fontWeight: 600 } : undefined}
                    title={chainIdx > 0 ? `Резерв #${chainIdx + 1} в Failover. Нажмите для снятия` : 'Добавить резервом (в конец Failover)'}
                  >
                    <span>{chainIdx > 0 ? `⚡#${chainIdx + 1} Резерв` : '⚡↓ В конец'}</span>
                  </button>
                </>
              )
            })()}
            <button
              type="button"
              className={`btn sm btn-whitelist ${activeServer.is_whitelist ? 'is-whitelist' : 'ghost'}`}
              onClick={() => toggleWhitelist(activeServer.id)}
              title={activeServer.is_whitelist ? 'Удалить из белого списка' : 'Включить в белый список (аварийный туннель)'}
            >
              <span style={{ fontSize: 11 }}>{activeServer.is_whitelist ? '🟣' : '⚪'}</span>
              <span style={{ fontSize: 11, fontWeight: 600 }}>{activeServer.is_whitelist ? 'В белом списке' : 'Белый список'}</span>
            </button>
            {activeServer.is_whitelist && (
              <button
                type="button"
                className="btn sm ghost"
                onClick={() => makeServerNormal(activeServer.id)}
                title="Сделать узел обычным: перекрыть тег БС и убрать из аварийного туннеля"
              >
                <span style={{ fontSize: 11 }}>Сделать обычным</span>
              </button>
            )}
          </div>
        </div>
      </>
    ) : (
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', minHeight: 90 }} className="muted">
        Нет подключенного сервера
      </div>
    )}
  </div>
  )
}
