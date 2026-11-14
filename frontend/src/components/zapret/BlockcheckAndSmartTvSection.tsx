import type { BlockcheckResult, BlockcheckStrategy, ZapretFeatures } from '../../types'
import { renderMicroSpinner } from './zapretHelpers'

interface BlockcheckAndSmartTvSectionProps {
  busy: boolean
  blockcheckResult: BlockcheckResult | null
  runningBlockcheck: boolean
  applyingStrategy: string | null
  activeStrategyId: string | null
  pendingKeys: Set<string>
  features: ZapretFeatures
  isRunning: boolean
  isInstalled: boolean
  activeEngine: 'v1' | 'v2'
  handleRunBlockcheck: () => void
  handleApplyStrategy: (strat: BlockcheckStrategy) => void
  handleToggleFeature: (key: keyof ZapretFeatures) => void
}

export function BlockcheckAndSmartTvSection({
  busy,
  blockcheckResult,
  runningBlockcheck,
  applyingStrategy,
  activeStrategyId,
  pendingKeys,
  features,
  isRunning,
  isInstalled,
  activeEngine,
  handleRunBlockcheck,
  handleApplyStrategy,
  handleToggleFeature,
}: BlockcheckAndSmartTvSectionProps) {
  return (
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(360px, 1fr))', gap: 16, alignItems: 'stretch' }}>
        {/* 3. ВСТРОЕННЫЙ АВТОПОДБОР СТРАТЕГИЙ (MINI-BLOCKCHECK) */}
        <section
          className="card"
          style={{
            margin: 0,
            padding: '22px 24px',
            background: 'linear-gradient(135deg, rgba(56, 189, 248, 0.05) 0%, rgba(15, 23, 42, 0.6) 100%)',
            border: '1px solid rgba(56, 189, 248, 0.3)',
            borderRadius: 16,
            boxShadow: '0 8px 24px rgba(56, 189, 248, 0.05)',
            display: 'flex',
            flexDirection: 'column',
            justifyContent: 'space-between',
            gridColumn: blockcheckResult ? '1 / -1' : undefined,
          }}
        >
          <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
            <div style={{ display: 'flex', alignItems: 'flex-start', gap: 12 }}>
              <div
                style={{
                  width: 40,
                  height: 40,
                  borderRadius: 10,
                  background: 'rgba(56, 189, 248, 0.15)',
                  border: '1px solid rgba(56, 189, 248, 0.35)',
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'center',
                  fontSize: 20,
                  flexShrink: 0,
                }}
              >
                🎯
              </div>
              <div style={{ flex: 1, minWidth: 0 }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
                  <h3 style={{ margin: 0, fontSize: 17, fontWeight: 700 }}>
                    Автоподбор стратегий десинхронизации (Mini-Blockcheck)
                  </h3>
                  <span
                    className="badge"
                    data-testid="blockcheck-engine-badge"
                    style={{
                      background: activeEngine === 'v2' ? 'rgba(56, 189, 248, 0.15)' : 'rgba(234, 179, 8, 0.15)',
                      color: activeEngine === 'v2' ? '#38bdf8' : '#facc15',
                      border: `1px solid ${activeEngine === 'v2' ? 'rgba(56, 189, 248, 0.4)' : 'rgba(234, 179, 8, 0.4)'}`,
                      fontSize: 11,
                    }}
                  >
                    ⚡ {activeEngine === 'v2' ? 'zapret2 engine' : 'zapret1 (legacy) engine'}
                  </span>
                </div>
                <div className="muted small" style={{ marginTop: 6, lineHeight: 1.45 }}>
                  Автоподбор временно недоступен: прежний алгоритм не измерял работу отдельных стратегий. Для достоверного результата требуется проверка клиентского трафика через Zapret; наличие процесса не подтверждает обход DPI.
                </div>
              </div>
            </div>
          </div>

          <div style={{ marginTop: 16, display: 'flex', alignItems: 'center' }}>
            <button
              type="button"
              className="btn primary"
              disabled={runningBlockcheck || busy || !isInstalled}
              onClick={handleRunBlockcheck}
              style={{
                background: 'linear-gradient(135deg, #0ea5e9 0%, #2563eb 100%)',
                border: 'none',
                boxShadow: '0 2px 12px rgba(14, 165, 233, 0.35)',
                display: 'flex',
                alignItems: 'center',
                gap: 8,
                padding: '8px 18px',
                fontSize: 13,
                fontWeight: 600,
              }}
            >
              {runningBlockcheck ? (
                <>
                  <span className="spinner sm" />
                  <span>Тестирование стратегий…</span>
                </>
              ) : (
                <>
                  <span>⚡</span>
                  <span>Запустить автоподбор стратегий</span>
                </>
              )}
            </button>
          </div>

        {/* РЕЗУЛЬТАТЫ СТРАТЕГИЙ */}
        {blockcheckResult && (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 12, marginTop: 12 }}>
            {/* РЕКОМЕНДОВАННАЯ СТРАТЕГИЯ ХИТ-БАННЕР */}
            {(() => {
              const best = blockcheckResult.strategies.find((s) => s.id === blockcheckResult.best_strategy_id) || blockcheckResult.strategies[0]
              if (!best) return null
              const isApplying = applyingStrategy === best.id
              const isSelected = activeStrategyId === best.id
              return (
                <div
                  style={{
                    padding: '14px 18px',
                    borderRadius: 12,
                    background: 'linear-gradient(135deg, rgba(34, 197, 94, 0.15) 0%, rgba(6, 182, 212, 0.15) 100%)',
                    border: '1px solid rgba(34, 197, 94, 0.4)',
                    display: 'flex',
                    justifyContent: 'space-between',
                    alignItems: 'center',
                    flexWrap: 'wrap',
                    gap: 12,
                  }}
                >
                  <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                    <span style={{ fontSize: 22 }}>🏆</span>
                    <div>
                      <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                        <b style={{ fontSize: 14, color: '#f8fafc' }}>Лучшая рекомендуемая стратегия: {best.name}</b>
                        <span className="badge" style={{ background: 'rgba(34, 197, 94, 0.25)', color: '#4ade80', border: '1px solid rgba(34, 197, 94, 0.5)', fontSize: 11 }}>
                          Score: {best.score}/100
                        </span>
                        {isSelected && (
                          <span className="badge" style={{ background: 'rgba(34, 197, 94, 0.2)', color: '#22c55e', border: '1px solid #22c55e', fontSize: 11 }}>
                            ✓ Активна
                          </span>
                        )}
                      </div>
                      <div className="muted small" style={{ marginTop: 2, color: '#cbd5e1' }}>
                        YouTube: {best.youtube_ok ? `🟢 ${best.youtube_time_ms} мс` : '🔴 Блок'} · Discord: {best.discord_ok ? `🟢 ${best.discord_time_ms} мс` : '🔴 Блок'} · {best.description}
                      </div>
                    </div>
                  </div>

                  <button
                    type="button"
                    className="btn primary sm"
                    disabled={isApplying || busy || isSelected}
                    onClick={() => handleApplyStrategy(best)}
                    style={{
                      background: isSelected
                        ? 'rgba(255, 255, 255, 0.08)'
                        : 'linear-gradient(135deg, #22c55e 0%, #16a34a 100%)',
                      color: isSelected ? 'var(--text-muted, #94a3b8)' : '#fff',
                      border: isSelected ? '1px solid rgba(255, 255, 255, 0.15)' : 'none',
                      boxShadow: isSelected ? 'none' : '0 2px 10px rgba(34, 197, 94, 0.4)',
                      fontWeight: 600,
                      cursor: isSelected ? 'default' : 'pointer',
                    }}
                  >
                    {isApplying
                      ? 'Применение…'
                      : isSelected
                      ? '✓ Выбрано'
                      : '🚀 Применить лучшую стратегию в 1 клик'}
                  </button>
                </div>
              )
            })()}

            {/* СПИСОК ВСЕХ ПРОТЕСТИРОВАННЫХ СТРАТЕГИЙ */}
            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(320px, 1fr))', gap: 12 }}>
              {blockcheckResult.strategies.map((strat) => {
                const isBest = strat.id === blockcheckResult.best_strategy_id
                const isApplying = applyingStrategy === strat.id
                const isSelected = activeStrategyId === strat.id
                return (
                  <div
                    key={strat.id}
                    style={{
                      padding: '14px 16px',
                      borderRadius: 12,
                      background: isSelected
                        ? 'rgba(34, 197, 94, 0.08)'
                        : isBest
                        ? 'rgba(56, 189, 248, 0.08)'
                        : 'rgba(0, 0, 0, 0.25)',
                      border: isSelected
                        ? '1px solid rgba(34, 197, 94, 0.5)'
                        : isBest
                        ? '1px solid rgba(56, 189, 248, 0.45)'
                        : '1px solid var(--border)',
                      display: 'flex',
                      flexDirection: 'column',
                      justifyContent: 'space-between',
                      gap: 10,
                    }}
                  >
                    <div>
                      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start', gap: 8 }}>
                        <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                          <b style={{ fontSize: 13, color: isSelected ? '#4ade80' : isBest ? '#38bdf8' : 'var(--text)' }}>
                            {strat.name}
                          </b>
                          {isSelected && (
                            <span className="badge" style={{ fontSize: 10, background: 'rgba(34, 197, 94, 0.2)', color: '#4ade80' }}>
                              ✓ Выбрано
                            </span>
                          )}
                        </div>
                        <span
                          className="badge"
                          style={{
                            fontSize: 10,
                            background: strat.score >= 80 ? 'rgba(34, 197, 94, 0.2)' : 'rgba(234, 179, 8, 0.2)',
                            color: strat.score >= 80 ? '#4ade80' : '#fde047',
                            border: `1px solid ${strat.score >= 80 ? 'rgba(34, 197, 94, 0.4)' : 'rgba(234, 179, 8, 0.4)'}`,
                          }}
                        >
                          {strat.score} pts
                        </span>
                      </div>
                      <p className="muted small" style={{ margin: '4px 0 8px', fontSize: 11.5, lineHeight: 1.4 }}>
                        {strat.description}
                      </p>

                      <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap', fontSize: 11 }}>
                        <span
                          style={{
                            padding: '2px 8px',
                            borderRadius: 6,
                            background: strat.youtube_ok ? 'rgba(34, 197, 94, 0.12)' : 'rgba(239, 68, 68, 0.12)',
                            color: strat.youtube_ok ? '#4ade80' : '#f87171',
                            border: `1px solid ${strat.youtube_ok ? 'rgba(34, 197, 94, 0.3)' : 'rgba(239, 68, 68, 0.3)'}`,
                            fontFamily: 'Consolas, monospace',
                          }}
                        >
                          🎥 YouTube: {strat.youtube_ok ? `${strat.youtube_time_ms} мс` : 'Блок'}
                        </span>
                        <span
                          style={{
                            padding: '2px 8px',
                            borderRadius: 6,
                            background: strat.discord_ok ? 'rgba(34, 197, 94, 0.12)' : 'rgba(239, 68, 68, 0.12)',
                            color: strat.discord_ok ? '#4ade80' : '#f87171',
                            border: `1px solid ${strat.discord_ok ? 'rgba(34, 197, 94, 0.3)' : 'rgba(239, 68, 68, 0.3)'}`,
                            fontFamily: 'Consolas, monospace',
                          }}
                        >
                          💬 Discord: {strat.discord_ok ? `${strat.discord_time_ms} мс` : 'Блок'}
                        </span>
                      </div>
                    </div>

                    <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', paddingTop: 8, borderTop: '1px solid rgba(255, 255, 255, 0.05)' }}>
                      <span className="muted small" style={{ fontSize: 10, fontFamily: 'Consolas, monospace', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', maxWidth: 180 }}>
                        {strat.args.split(' ')[0]}...
                      </span>
                      <button
                        type="button"
                        className="btn sm"
                        disabled={isApplying || busy || isSelected}
                        onClick={() => handleApplyStrategy(strat)}
                        style={
                          isSelected
                            ? {
                                background: 'rgba(255, 255, 255, 0.08)',
                                color: 'var(--text-muted, #94a3b8)',
                                border: '1px solid rgba(255, 255, 255, 0.12)',
                                cursor: 'default',
                              }
                            : isBest
                            ? { background: '#0284c7', color: '#fff', border: 'none' }
                            : {}
                        }
                      >
                        {isApplying ? 'Применение…' : isSelected ? '✓ Выбрано' : 'Применить'}
                      </button>
                    </div>
                  </div>
                )
              })}
            </div>
          </div>
        )}
      </section>

      {/* 4. СПЕЦИАЛЬНЫЙ ПРОФИЛЬ «SMART TV / КИНОТЕАТР» */}
      <section
        className="card"
        style={{
          margin: 0,
          padding: '22px 24px',
          background: features.smart_tv_mode
            ? 'linear-gradient(135deg, rgba(234, 179, 8, 0.08) 0%, rgba(15, 23, 42, 0.6) 100%)'
            : 'rgba(255, 255, 255, 0.02)',
          border: features.smart_tv_mode ? '1px solid rgba(234, 179, 8, 0.35)' : '1px solid var(--border)',
          borderRadius: 16,
          boxShadow: features.smart_tv_mode ? '0 8px 24px rgba(234, 179, 8, 0.08)' : 'none',
          transition: 'all 0.25s ease',
          display: 'flex',
          flexDirection: 'column',
          justifyContent: 'space-between',
        }}
      >
        <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
          <div style={{ display: 'flex', alignItems: 'flex-start', gap: 14 }}>
            <div
              style={{
                width: 44,
                height: 44,
                borderRadius: 12,
                background: features.smart_tv_mode ? 'rgba(234, 179, 8, 0.2)' : 'rgba(255, 255, 255, 0.05)',
                border: features.smart_tv_mode ? '1px solid rgba(234, 179, 8, 0.4)' : '1px solid var(--border)',
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                fontSize: 22,
                flexShrink: 0,
              }}
            >
              📺
            </div>
            <div style={{ flex: 1, minWidth: 0 }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
                <h3 style={{ margin: 0, fontSize: 17, fontWeight: 700 }}>
                  Профиль «Smart TV / Кинотеатр»
                </h3>
                <span
                  className="badge"
                  style={{
                    background: features.smart_tv_mode ? (isRunning ? 'rgba(234, 179, 8, 0.2)' : 'rgba(255, 255, 255, 0.06)') : 'rgba(255, 255, 255, 0.06)',
                    color: features.smart_tv_mode ? (isRunning ? '#fde047' : 'var(--muted)') : 'var(--muted)',
                    border: `1px solid ${features.smart_tv_mode ? (isRunning ? 'rgba(234, 179, 8, 0.4)' : 'var(--border)') : 'var(--border)'}`,
                    fontSize: 11,
                    fontWeight: 600,
                  }}
                >
                  {features.smart_tv_mode ? (isRunning ? '🟢 SMART TV АКТИВЕН' : '⚪ SMART TV (Zapret остановлен)') : '⚪ ВЫКЛЮЧЕН'}
                </span>
                <span
                  className="badge"
                  style={{ background: 'rgba(56, 189, 248, 0.12)', color: '#38bdf8', border: '1px solid rgba(56, 189, 248, 0.3)', fontSize: 10 }}
                >
                  4K HDR READY
                </span>
                <span
                  className="badge"
                  style={{ background: 'rgba(168, 85, 247, 0.12)', color: '#c084fc', border: '1px solid rgba(168, 85, 247, 0.3)', fontSize: 10 }}
                >
                  UDP 443 DROP (QUIC)
                </span>
              </div>
              <div className="muted small" style={{ marginTop: 6, lineHeight: 1.45 }}>
                Специальная оптимизация для телевизоров LG webOS, Samsung Tizen, Android TV, Apple TV и медиаплееров. Блокирует UDP 443 (QUIC / HTTP3) для перевода плееров на стабильный TCP HTTP/2 с аппаратным nfqws2 ускорением. Автоматически включает в hostlist кэш-серверы Google Video (<code>redirector.googlevideo.com</code>, <code>manifest.googlevideo.com</code>, <code>gvt1.com</code>, <code>play.google.com</code>) для устранения зависаний 4K и буферизации.
              </div>
            </div>
          </div>
        </div>

        {/* Свитч рубильник Smart TV */}
        <div style={{ marginTop: 16, display: 'flex', alignItems: 'center' }}>
          <button
            type="button"
            disabled={busy || !isInstalled || pendingKeys.has('smart_tv_mode')}
            onClick={() => handleToggleFeature('smart_tv_mode')}
            style={{
              width: 54,
              height: 30,
              borderRadius: 18,
              border: 'none',
              cursor: busy || !isInstalled ? 'not-allowed' : 'pointer',
              background: features.smart_tv_mode
                ? 'linear-gradient(135deg, #eab308 0%, #ca8a04 100%)'
                : 'rgba(255, 255, 255, 0.15)',
              position: 'relative',
              transition: 'all 0.25s cubic-bezier(0.4, 0, 0.2, 1)',
              padding: 2,
              flexShrink: 0,
              boxShadow: features.smart_tv_mode ? '0 0 14px rgba(234, 179, 8, 0.45)' : 'none',
            }}
            title={features.smart_tv_mode ? 'Выключить профиль Smart TV' : 'Включить профиль Smart TV'}
          >
            <div
              style={{
                width: 26,
                height: 26,
                borderRadius: '50%',
                background: '#fff',
                transform: features.smart_tv_mode ? 'translateX(24px)' : 'translateX(0)',
                transition: 'transform 0.25s cubic-bezier(0.4, 0, 0.2, 1)',
                boxShadow: '0 1px 4px rgba(0,0,0,0.3)',
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                fontSize: 11,
                color: features.smart_tv_mode ? '#ca8a04' : '#888',
                fontWeight: 'bold',
              }}
            >
              {pendingKeys.has('smart_tv_mode') ? renderMicroSpinner('#ca8a04', 13) : features.smart_tv_mode ? '✓' : '✕'}
            </div>
          </button>
        </div>
      </section>
    </div>
  )
}