import React from 'react'
import type { GamingConfig, GamingStatus } from '../../types'

export interface GamingSmartSectionProps {
  cfg: GamingConfig
  setCfg: React.Dispatch<React.SetStateAction<GamingConfig>>
  status: GamingStatus | null
  handleToggleSmartMode: (enabled: boolean) => void
  handleToggleIgnoreConn: (target: string, currentlyIgnored: boolean) => void
  loadData: () => void
}

export function GamingSmartSection({
  cfg,
  setCfg,
  status,
  handleToggleSmartMode,
  handleToggleIgnoreConn,
  loadData,
}: GamingSmartSectionProps) {
  return (
  <div className="card" style={{ padding: '16px 18px', border: cfg.smart_mode ? '1px solid rgba(56, 189, 248, 0.4)' : '1px solid var(--border)' }}>
    <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: 12 }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
        <span style={{ fontSize: 24 }}>🧠</span>
        <div>
          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <b style={{ fontSize: 14 }}>Умный игровой режим (Smart Auto-Gaming)</b>
            <span className="badge" style={{ background: cfg.smart_mode ? 'rgba(34, 197, 94, 0.2)' : 'rgba(255, 255, 255, 0.08)', color: cfg.smart_mode ? '#4ade80' : 'var(--muted)' }}>
              {cfg.smart_mode ? '🟢 Активен' : '⚪ Выключен'}
            </span>
            {status?.recent_gaming_conns && status.recent_gaming_conns.length > 0 && (
              <span className="badge" style={{ background: 'rgba(56, 189, 248, 0.15)', color: '#38bdf8', fontSize: 11 }}>
                {status.recent_gaming_conns.length} хостов в детекторе
              </span>
            )}
          </div>
          <p className="muted small" style={{ margin: '3px 0 0', fontSize: 12, lineHeight: 1.4 }}>
            Автоматически включает игровой туннель при обнаружении сетевой активности игр на выбранном устройстве и отключает после таймаута бездействия для экономии VPS.
          </p>
        </div>
      </div>

      <div style={{ display: 'flex', alignItems: 'center', gap: 16 }}>
        <label className="switch" title={cfg.smart_mode ? 'Отключить умный режим' : 'Включить умный режим'}>
          <input
            type="checkbox"
            checked={cfg.smart_mode ?? false}
            onChange={(e) => handleToggleSmartMode(e.target.checked)}
          />
          <span className="slider" />
        </label>
      </div>
    </div>

    {cfg.smart_mode && (
      <div style={{ marginTop: 14, paddingTop: 14, borderTop: '1px solid rgba(255, 255, 255, 0.06)', display: 'flex', alignItems: 'center', flexWrap: 'wrap', gap: 16 }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <label htmlFor="smart-idle-timeout" style={{ fontSize: 12.5, color: '#94a3b8' }}>
            ⏱️ Таймаут бездействия (мин):
          </label>
          <input
            id="smart-idle-timeout"
            type="number"
            min={1}
            max={180}
            value={cfg.smart_idle_timeout_mins ?? 15}
            onChange={(e) => setCfg({ ...cfg, smart_idle_timeout_mins: Math.max(1, parseInt(e.target.value, 10) || 15) })}
            style={{ width: 75, padding: '4px 8px', borderRadius: 6, border: '1px solid var(--border)', background: 'rgba(0,0,0,0.3)', color: '#fff', fontSize: 12 }}
          />
        </div>
        <span className="muted small" style={{ fontSize: 11.5 }}>
          Если в течение {cfg.smart_idle_timeout_mins ?? 15} мин нет сетевых игровых пакетов, туннель перейдет в режим ожидания.
        </span>
      </div>
    )}

    {/* Недавняя игровая активность и игнор-лист детектора внутри единой карточки */}
    <div style={{ marginTop: 16, paddingTop: 14, borderTop: '1px solid rgba(255, 255, 255, 0.06)' }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 10, flexWrap: 'wrap', gap: 8 }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <span style={{ fontSize: 16 }}>🕹️</span>
          <b style={{ fontSize: 13.5 }}>Недавняя игровая активность (детектор игр)</b>
        </div>
        <button
          type="button"
          className="btn sm"
          onClick={loadData}
          style={{ fontSize: 12 }}
        >
          🔄 Обновить
        </button>
      </div>

      {status?.recent_gaming_conns && status.recent_gaming_conns.length > 0 ? (
        <div style={{ overflowX: 'auto', maxHeight: 220, overflowY: 'auto' }}>
          <table style={{ width: '100%', fontSize: 12, borderCollapse: 'collapse' }}>
            <thead>
              <tr style={{ textAlign: 'left', borderBottom: '1px solid rgba(255, 255, 255, 0.08)', color: '#94a3b8' }}>
                <th style={{ padding: '6px 8px' }}>Хост / Назначение</th>
                <th style={{ padding: '6px 8px' }}>Протокол</th>
                <th style={{ padding: '6px 8px' }}>Время</th>
                <th style={{ padding: '6px 8px' }}>Статус</th>
                <th style={{ padding: '6px 8px', textAlign: 'right' }}>Действие</th>
              </tr>
            </thead>
            <tbody>
              {status.recent_gaming_conns.map((conn, idx) => {
                const target = conn.target || conn.host || conn.destination
                return (
                  <tr key={idx} style={{ borderBottom: '1px solid rgba(255, 255, 255, 0.04)', height: 32 }}>
                    <td style={{ padding: '6px 8px', fontFamily: 'Consolas, monospace', fontSize: 11.5 }}>
                      <span title={conn.destination}>{conn.target || conn.host || conn.destination}</span>
                    </td>
                    <td style={{ padding: '6px 8px' }}>
                      <span
                        className="badge"
                        style={{
                          background: conn.network === 'UDP' ? 'rgba(168, 85, 247, 0.2)' : 'rgba(56, 189, 248, 0.2)',
                          color: conn.network === 'UDP' ? '#c084fc' : '#38bdf8',
                          fontSize: 10,
                          padding: '1px 5px',
                        }}
                      >
                        {conn.network}
                      </span>
                    </td>
                    <td style={{ padding: '6px 8px', color: '#94a3b8', fontSize: 11 }}>
                      {conn.last_seen || 'Недавно'}
                    </td>
                    <td style={{ padding: '6px 8px' }}>
                      {conn.ignored ? (
                        <span className="badge" style={{ background: 'rgba(239, 68, 68, 0.15)', color: '#f87171', fontSize: 10 }}>
                          В игноре
                        </span>
                      ) : (
                        <span className="badge" style={{ background: 'rgba(34, 197, 94, 0.15)', color: '#4ade80', fontSize: 10 }}>
                          Отслеживается
                        </span>
                      )}
                    </td>
                    <td style={{ padding: '6px 8px', textAlign: 'right' }}>
                      <button
                        type="button"
                        className="btn sm"
                        onClick={() => handleToggleIgnoreConn(target, !!conn.ignored)}
                        style={{
                          fontSize: 11,
                          padding: '2px 8px',
                          background: conn.ignored ? 'rgba(56, 189, 248, 0.15)' : 'rgba(239, 68, 68, 0.12)',
                          color: conn.ignored ? '#38bdf8' : '#f87171',
                          border: conn.ignored ? '1px solid rgba(56, 189, 248, 0.3)' : '1px solid rgba(239, 68, 68, 0.3)',
                        }}
                      >
                        {conn.ignored ? 'Следить' : 'Не реагировать'}
                      </button>
                    </td>
                  </tr>
                )
              })}
            </tbody>
          </table>
        </div>
      ) : (
        <div className="muted small" style={{ padding: '12px 0', textAlign: 'center' }}>
          Пока нет зафиксированных игровых соединений. При сетевой активности игр они появятся здесь.
        </div>
      )}
    </div>
  </div>
  )
}
