import React from 'react'
import { pingClass, getFlowStatus, type ServerInfo, type SpeedtestResult, type StatusInfo } from '../../types'
import { Sparkline } from './serversHelpers'

export interface ServerListProps {
  loading: boolean
  columns: number
  viewMode: 'detailed' | 'compact'
  filtered: ServerInfo[]
  limit: number
  pageSize: number
  setLimit: React.Dispatch<React.SetStateAction<number>>
  status?: StatusInfo | null
  providerColorMap: Map<string, string>
  failoverChain: string[]
  ignored: Set<string>
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
  activate: (s: ServerInfo) => void
  runSpeedtest: (s: ServerInfo) => void
  toggleWhitelist: (id: string) => void
  makeServerNormal: (id: string) => void
  addToFailover: (serverId: string, position: 'start' | 'end') => void
}

export function ServerList({
  loading,
  columns,
  viewMode,
  filtered,
  limit,
  pageSize,
  setLimit,
  status,
  providerColorMap,
  failoverChain,
  ignored,
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
  activate,
  runSpeedtest,
  toggleWhitelist,
  makeServerNormal,
  addToFailover,
}: ServerListProps) {
  const PAGE = pageSize
  return (
  <div className="servers-main-content">
    {loading ? (
    /* Скелетоны при загрузке (Skeleton Shimmers) */
    <div className={`server-list cols-${columns}`} style={{ marginTop: 12 }}>
      {Array.from({ length: 8 }).map((_, idx) => (
        <div
          key={idx}
          className="server-card skeleton-shimmer"
          style={{
            minHeight: 110,
            display: 'flex',
            flexDirection: 'column',
            justifyContent: 'space-between',
            padding: '12px 14px',
            border: '1px solid rgba(255, 255, 255, 0.06)',
          }}
        >
          <div style={{ display: 'flex', justifyContent: 'space-between' }}>
            <div style={{ width: 60, height: 16, borderRadius: 4, background: 'rgba(255, 255, 255, 0.06)' }} />
            <div style={{ width: 50, height: 16, borderRadius: 4, background: 'rgba(255, 255, 255, 0.06)' }} />
          </div>
          <div style={{ width: '70%', height: 18, borderRadius: 4, background: 'rgba(255, 255, 255, 0.08)', margin: '10px 0' }} />
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
            <div style={{ width: 80, height: 24, borderRadius: 6, background: 'rgba(255, 255, 255, 0.06)' }} />
            <div style={{ display: 'flex', gap: 6 }}>
              <div style={{ width: 24, height: 24, borderRadius: 6, background: 'rgba(255, 255, 255, 0.05)' }} />
              <div style={{ width: 24, height: 24, borderRadius: 6, background: 'rgba(255, 255, 255, 0.05)' }} />
            </div>
          </div>
        </div>
      ))}
    </div>
  ) : (
    <div>
      <div className="pinned-divider" style={{ margin: '14px 0 12px' }}>
        <span>Список серверов ({filtered.length})</span>
      </div>

      {/* ОСНОВНОЙ СПИСОК СЕРВЕРОВ */}
      {filtered.length === 0 ? (
        <p className="muted">Серверов по заданному фильтру не найдено.</p>
      ) : viewMode === 'detailed' ? (
        /* ПОДРОБНЫЙ ВИД (КАРТОЧКИ) */
        <div className={`server-list cols-${columns}`}>
          {filtered.slice(0, limit).map((s) => (
            <div key={s.id} className={'server-card' + (s.is_active ? ' active' : '')}>
              {/* 1. Верхняя строка: метаданные слева, метрики справа */}
              <div className="server-card-top">
                <div className="server-card-badges">
                  <span
                    className="badge protocol-badge"
                    style={s.protocol === 'VK-CALL' ? {
                      background: 'rgba(34, 197, 94, 0.16)',
                      border: '1px solid rgba(34, 197, 94, 0.42)',
                      color: '#4ade80',
                    } : undefined}
                  >{s.protocol}</span>
                  {s.provider && (
                    <span
                      className="tag-provider"
                      title={`Подписка: ${s.provider_name || s.provider} (кликните для управления)`}
                      onClick={openProvidersModal}
                    >
                      <span
                        className="sub-marker-dot"
                        style={{ backgroundColor: providerColorMap.get(s.provider) || '#a855f7' }}
                      />
                      <span className="tag-provider-name">{s.provider_name || s.provider}</span>
                    </span>
                  )}
                  {(s.is_whitelist || status?.whitelist_servers?.includes(s.id)) && (
                    <span className="badge" style={{ background: 'rgba(168, 85, 247, 0.2)', color: '#c084fc', border: '1px solid rgba(168, 85, 247, 0.4)', fontSize: 10 }}>
                      🟣 Белый список
                    </span>
                  )}
                  {ignored.has(s.id) && (
                    <span className="tag ignored" title="Сервер исключён из авто-выбора (Fastest / Fallback)">
                      🚫 Исключён
                    </span>
                  )}
                </div>

                <div className="server-card-metrics">
                  {getFlowStatus(s) === 'ok' ? (
                    <span className="badge badge-flow-ok" title="Сервер подходит для Google Flow и Gemini Labs">
                      🟢 Flow OK
                    </span>
                  ) : getFlowStatus(s) === 'blocked' ? (
                    <span className="badge badge-flow-blocked" title="Заблокирован для Google Flow">
                      🔴 Flow Блок
                    </span>
                  ) : null}
                  {speedResults[s.id] && (
                    <span
                      className="badge badge-speed"
                      title={`Замер скорости: ${speedResults[s.id].speed_mbps.toFixed(2)} Мбит/с (${speedResults[s.id].latency_ms} мс)`}
                    >
                      ⚡ {speedResults[s.id].speed_mbps.toFixed(1)} М
                    </span>
                  )}
                  <span className={'ping ' + pingClass(s.ping_ms)}>
                    {s.ping_ms > 0 ? `${s.ping_ms} мс` : '—'}
                  </span>
                  <Sparkline ping={s.ping_ms} />
                </div>
              </div>

              {/* 2. Средняя строка: Название сервера или inline-редактирование псевдонима */}
              <div className="server-card-main">
                {editingAliasId === s.id ? (
                  <div style={{ display: 'flex', gap: 6, alignItems: 'center', width: '100%', margin: '4px 0' }}>
                    <input
                      className="input sm"
                      style={{ flex: 1, fontSize: 13 }}
                      value={aliasInput}
                      onChange={(e) => setAliasInput(e.target.value)}
                      placeholder="Пользовательское имя (или пусто для сброса)"
                      autoFocus
                      onKeyDown={(e) => {
                        if (e.key === 'Enter') saveAlias(s.id)
                        if (e.key === 'Escape') setEditingAliasId(null)
                      }}
                    />
                    <button className="btn sm primary" onClick={() => saveAlias(s.id)} disabled={aliasSaving}>
                      ✓
                    </button>
                    <button className="btn sm ghost" onClick={() => setEditingAliasId(null)}>
                      ✕
                    </button>
                  </div>
                ) : (
                  <div className="server-card-name" style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                    <span title={`${s.name}\nХост: ${s.host}${s.port ? `:${s.port}` : ''}\nПротокол: ${s.protocol}`}>
                      {s.custom_alias ? `${s.custom_alias} (${s.name})` : s.name}
                    </span>
                    <button
                      type="button"
                      className="btn sm ghost"
                      style={{ padding: '0 4px', fontSize: 11, height: 20, opacity: 0.6 }}
                      onClick={(e) => {
                        e.stopPropagation()
                        setEditingAliasId(s.id)
                        setAliasInput(s.custom_alias || '')
                      }}
                      title="Задать псевдоним (пользовательское имя)"
                    >
                      ✏️
                    </button>
                    {s.custom_alias && (
                      <button
                        type="button"
                        className="btn sm ghost"
                        style={{ padding: '0 4px', fontSize: 11, height: 20, opacity: 0.6, color: 'var(--text-danger, #ef4444)' }}
                        onClick={(e) => {
                          e.stopPropagation()
                          saveAliasWithVal(s.id, '')
                        }}
                        title="Сбросить пользовательский псевдоним"
                      >
                        ✕
                      </button>
                    )}
                  </div>
                )}
              </div>

              {/* 3. Нижняя строка: Кнопки действий без разрыва строк */}
              <div className="server-card-actions">
                <div className="server-card-action-primary">
                  {s.is_active ? (
                    <span className="tag current-active-badge">
                      <span>✓</span>
                      <span className="btn-label-optional">ПОДКЛЮЧЁН</span>
                    </span>
                  ) : (
                    <button
                      type="button"
                      className="btn sm btn-connect"
                      onClick={() => activate(s)}
                      title="Сделать этот сервер активным"
                    >
                      <span>🔌</span>
                      <span className="btn-label-optional">Подключить</span>
                    </button>
                  )}
                </div>

                <div className="server-card-action-secondary">
                  <button
                    type="button"
                    className={`btn sm ghost btn-speedtest ${speedtestingId === s.id ? 'loading' : ''}`}
                    onClick={() => runSpeedtest(s)}
                    disabled={speedtestingId !== null}
                    title="Замерить реальную скорость загрузки через этот прокси"
                  >
                    <span>{speedtestingId === s.id ? '⏳' : '🚀'}</span>
                    <span className="btn-label-optional">{speedtestingId === s.id ? 'Замер…' : 'Скорость'}</span>
                  </button>

                  {s.protocol === 'VK-CALL' ? (
                    <button
                      type="button"
                      className="btn sm ghost"
                      onClick={() => window.dispatchEvent(new CustomEvent('xr:switch-tab', { detail: 'vkturn' }))}
                      title="Открыть настройки VK-туннеля"
                    >
                      <span>📞</span>
                      <span className="btn-label-optional">Настройки ВК</span>
                    </button>
                  ) : (
                    <button
                      type="button"
                      className="btn sm ghost"
                      onClick={() => setShareServer(s)}
                      title="Сгенерировать ссылку подключения (vless/vmess/ss) и QR-код"
                    >
                      <span>🔗</span>
                      <span className="btn-label-optional">Ссылка</span>
                    </button>
                  )}

                  {(() => {
                    const chainIdx = failoverChain.indexOf(s.id)
                    const isStart = chainIdx === 0

                    return (
                      <>
                        <button
                          type="button"
                          className={`btn sm ghost ${isStart ? 'is-priority' : ''}`}
                          onClick={() => addToFailover(s.id, 'start')}
                          style={isStart ? { background: 'rgba(56, 189, 248, 0.2)', color: '#38bdf8', borderColor: '#38bdf8', fontWeight: 700 } : undefined}
                          title={isStart ? 'Основной узел Failover (#1). Нажмите для снятия' : 'Сделать основным узлом (в начало Failover)'}
                        >
                          <span>{isStart ? '👑' : '⚡↑'}</span>
                          <span className="btn-label-optional">{isStart ? 'Основной' : 'В начало'}</span>
                        </button>
                        <button
                          type="button"
                          className={`btn sm ghost ${chainIdx > 0 ? 'is-priority' : ''}`}
                          onClick={() => addToFailover(s.id, 'end')}
                          style={chainIdx > 0 ? { background: 'rgba(245, 158, 11, 0.15)', color: '#fbbf24', borderColor: '#fbbf24', fontWeight: 600 } : undefined}
                          title={chainIdx > 0 ? `Резерв #${chainIdx + 1} в Failover. Нажмите для снятия` : 'Добавить резервом (в конец Failover)'}
                        >
                          <span>{chainIdx > 0 ? `⚡#${chainIdx + 1}` : '⚡↓'}</span>
                          <span className="btn-label-optional">{chainIdx > 0 ? 'Резерв' : 'В конец'}</span>
                        </button>
                      </>
                    )
                  })()}

                  <button
                    type="button"
                    className={`btn sm btn-whitelist ${s.is_whitelist ? 'is-whitelist' : 'ghost'}`}
                    onClick={() => toggleWhitelist(s.id)}
                    title={s.is_whitelist ? 'Удалить из белого списка (аварийный туннель)' : 'Включить в белый список (аварийный 100% туннель)'}
                  >
                    <span style={{ fontSize: 11 }}>{s.is_whitelist ? '🟣' : '⚪'}</span>
                    <span className="btn-label-full">{s.is_whitelist ? 'В белом списке' : 'Белый список'}</span>
                    <span className="btn-label-short">{s.is_whitelist ? 'БС ON' : 'БС'}</span>
                  </button>
                  {s.is_whitelist && (
                    <button
                      type="button"
                      className="btn sm ghost"
                      onClick={() => makeServerNormal(s.id)}
                      title="Сделать узел обычным: перекрыть тег БС и убрать из аварийного туннеля"
                    >
                      <span className="btn-label-full">Сделать обычным</span>
                      <span className="btn-label-short">Обычный</span>
                    </button>
                  )}
                </div>
              </div>
            </div>
          ))}
          {filtered.length > limit && (
            <button
              className="btn wide"
              style={{ gridColumn: '1 / -1', marginTop: 12, padding: '10px 16px', fontWeight: 600 }}
              onClick={() => setLimit(limit + PAGE)}
            >
              Показать ещё {PAGE} (осталось {filtered.length - limit})
            </button>
          )}
        </div>
      ) : (
        /* КОМПАКТНЫЙ ВИД (ОДНОСТРОЧНАЯ ТАБЛИЦА) */
        <div className={`server-compact-list cols-${columns}`}>
          {filtered.slice(0, limit).map((s) => (
            <div key={s.id} className={'server-compact-row' + (s.is_active ? ' active' : '')}>
              <span className="badge" style={{ minWidth: 46, textAlign: 'center' }}>
                {s.protocol}
              </span>
              {s.provider && (
                <span
                  className="sub-marker-dot"
                  title={s.provider_name || s.provider}
                  style={{ backgroundColor: providerColorMap.get(s.provider) || '#a855f7' }}
                />
              )}
              {(s.is_whitelist || status?.whitelist_servers?.includes(s.id)) && (
                <span className="badge" style={{ background: 'rgba(168, 85, 247, 0.2)', color: '#c084fc', border: '1px solid rgba(168, 85, 247, 0.4)', fontSize: 9, padding: '1px 4px' }}>
                  🟣
                </span>
              )}
              {editingAliasId === s.id ? (
                <div style={{ display: 'inline-flex', gap: 4, alignItems: 'center', flex: 1 }}>
                  <input
                    className="input sm"
                    style={{ height: 24, fontSize: 12 }}
                    value={aliasInput}
                    onChange={(e) => setAliasInput(e.target.value)}
                    placeholder="Псевдоним"
                    autoFocus
                    onKeyDown={(e) => {
                      if (e.key === 'Enter') saveAlias(s.id)
                      if (e.key === 'Escape') setEditingAliasId(null)
                    }}
                  />
                  <button className="btn sm primary" style={{ padding: '0 6px', height: 24 }} onClick={() => saveAlias(s.id)}>✓</button>
                  <button className="btn sm ghost" style={{ padding: '0 6px', height: 24 }} onClick={() => setEditingAliasId(null)}>✕</button>
                </div>
              ) : (
                <span
                  className="server-compact-name"
                  title={`${s.name}\nХост: ${s.host}${s.port ? `:${s.port}` : ''}`}
                  style={{ display: 'inline-flex', alignItems: 'center', gap: 4 }}
                >
                  <span>{s.custom_alias ? `${s.custom_alias} (${s.name})` : s.name}</span>
                  <button
                    type="button"
                    className="btn sm ghost"
                    style={{ padding: '0 2px', fontSize: 10, height: 18, opacity: 0.5 }}
                    onClick={() => {
                      setEditingAliasId(s.id)
                      setAliasInput(s.custom_alias || '')
                    }}
                    title="Переименовать"
                  >
                    ✏️
                  </button>
                  {s.custom_alias && (
                    <button
                      type="button"
                      className="btn sm ghost"
                      style={{ padding: '0 2px', fontSize: 10, height: 18, opacity: 0.5, color: 'var(--text-danger, #ef4444)' }}
                      onClick={() => saveAliasWithVal(s.id, '')}
                      title="Сбросить псевдоним"
                    >
                      ✕
                    </button>
                  )}
                </span>
              )}
              {getFlowStatus(s) === 'ok' ? (
                <span className="badge badge-flow-ok" style={{ fontSize: 10, padding: '1px 5px' }} title="Подходит для Google Flow">
                  🟢 Flow
                </span>
              ) : getFlowStatus(s) === 'blocked' ? (
                <span className="badge badge-flow-blocked" style={{ fontSize: 10, padding: '1px 5px' }} title="Заблокирован для Flow">
                  🔴 Блок
                </span>
              ) : null}
              {ignored.has(s.id) && (
                <span className="tag ignored" style={{ fontSize: 10, padding: '1px 5px' }} title="Исключён из авто-выбора">
                  🚫 Исключён
                </span>
              )}
              {speedResults[s.id] && (
                <span className="badge badge-speed" style={{ fontSize: 10, padding: '1px 5px' }}>
                  ⚡ {speedResults[s.id].speed_mbps.toFixed(1)}M
                </span>
              )}
              <span
                className={'ping ' + pingClass(s.ping_ms)}
                style={{ minWidth: 60, textAlign: 'right' }}
              >
                {s.ping_ms > 0 ? `${s.ping_ms} мс` : '—'}
              </span>
              <Sparkline ping={s.ping_ms} />
              <div style={{ display: 'inline-flex', gap: 6, alignItems: 'center' }}>
                {s.protocol === 'VK-CALL' ? (
                  <button
                    className="btn sm ghost"
                    onClick={() => window.dispatchEvent(new CustomEvent('xr:switch-tab', { detail: 'vkturn' }))}
                    style={{ padding: '3px 6px' }}
                    title="Открыть настройки VK-туннеля"
                  >
                    📞
                  </button>
                ) : (
                  <button
                    className="btn sm ghost"
                    onClick={() => setShareServer(s)}
                    style={{ padding: '3px 6px' }}
                    title="Ссылка подключения и QR-код"
                  >
                    🔗
                  </button>
                )}
                <button
                  className={`btn sm ghost btn-speedtest ${speedtestingId === s.id ? 'loading' : ''}`}
                  onClick={() => runSpeedtest(s)}
                  disabled={speedtestingId !== null}
                  style={{ padding: '3px 6px' }}
                  title="Замерить скорость"
                >
                  {speedtestingId === s.id ? '⏳' : '🚀'}
                </button>
                {s.is_active ? (
                  <span className="tag current" style={{ fontSize: 10, padding: '2px 6px' }}>✓</span>
                ) : (
                  <button className="btn sm btn-connect" onClick={() => activate(s)} style={{ padding: '3px 8px' }}>
                    🔌
                  </button>
                )}
                {(() => {
                  const chainIdx = failoverChain.indexOf(s.id)
                  const isStart = chainIdx === 0
                  return (
                    <>
                      <button
                        type="button"
                        className={`btn sm ghost ${isStart ? 'is-priority' : ''}`}
                        onClick={() => addToFailover(s.id, 'start')}
                        style={{
                          padding: '3px 6px',
                          ...(isStart ? { background: 'rgba(56, 189, 248, 0.2)', color: '#38bdf8', borderColor: '#38bdf8', fontWeight: 700 } : {}),
                        }}
                        title={isStart ? 'Основной узел Failover (#1). Нажмите для снятия' : 'В начало Failover (основной)'}
                      >
                        {isStart ? '👑' : '⚡↑'}
                      </button>
                      <button
                        type="button"
                        className={`btn sm ghost ${chainIdx > 0 ? 'is-priority' : ''}`}
                        onClick={() => addToFailover(s.id, 'end')}
                        style={{
                          padding: '3px 6px',
                          ...(chainIdx > 0 ? { background: 'rgba(245, 158, 11, 0.15)', color: '#fbbf24', borderColor: '#fbbf24', fontWeight: 600 } : {}),
                        }}
                        title={chainIdx > 0 ? `Резерв #${chainIdx + 1} в Failover. Нажмите для снятия` : 'В конец Failover (резерв)'}
                      >
                        {chainIdx > 0 ? `⚡#${chainIdx + 1}` : '⚡↓'}
                      </button>
                    </>
                  )
                })()}
                <button
                  type="button"
                  className={`btn sm btn-whitelist ${s.is_whitelist ? 'is-whitelist' : 'ghost'}`}
                  onClick={() => toggleWhitelist(s.id)}
                  style={{ padding: '3px 8px' }}
                  title={s.is_whitelist ? 'Удалить из белого списка' : 'В белый список (аварийный туннель)'}
                >
                  <span style={{ fontSize: 11 }}>{s.is_whitelist ? '🟣' : '⚪'}</span>
                  <span style={{ fontSize: 11, fontWeight: 600 }}>{s.is_whitelist ? 'БС ON' : 'БС'}</span>
                </button>
                {s.is_whitelist && (
                  <button
                    type="button"
                    className="btn sm ghost"
                    onClick={() => makeServerNormal(s.id)}
                    style={{ padding: '3px 8px' }}
                    title="Сделать узел обычным: перекрыть тег БС и убрать из аварийного туннеля"
                  >
                    <span style={{ fontSize: 11 }}>Обычный</span>
                  </button>
                )}
              </div>
            </div>
          ))}
          {filtered.length > limit && (
            <button
              className="btn wide"
              style={{ gridColumn: '1 / -1', marginTop: 12, padding: '10px 16px', fontWeight: 600 }}
              onClick={() => setLimit(limit + PAGE)}
            >
              Показать ещё {PAGE} (осталось {filtered.length - limit})
            </button>
          )}
        </div>
      )}
    </div>
  )}
  </div>
  )
}
