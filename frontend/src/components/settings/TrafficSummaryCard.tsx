import type { ProviderInfo } from '../../types'
import { formatQuotaBytes } from '../servers/serversHelpers'

export interface TrafficSummaryData {
  direct_bytes: number
  proxy_bytes: number
  whitelist_bytes: number
  whitelist_active: boolean
}

interface TrafficSummaryCardProps {
  trafficSummary: TrafficSummaryData
  providers: ProviderInfo[]
  fmtTrafficBytes: (bytes: number) => string
  handleResetTraffic: (target: string, label: string) => void
}

export function TrafficSummaryCard({
  trafficSummary,
  providers,
  fmtTrafficBytes,
  handleResetTraffic,
}: TrafficSummaryCardProps) {
  return (
    <section className="card" style={{ marginBottom: 16 }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 14 }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <span style={{ fontSize: 18 }}>📊</span>
          <h3 style={{ margin: 0, fontSize: 15 }}>Сводка сетевого трафика</h3>
          {trafficSummary.whitelist_active && (
            <span className="badge" style={{ background: 'rgba(168, 85, 247, 0.2)', color: '#c084fc', border: '1px solid rgba(168, 85, 247, 0.4)', fontSize: 11, fontWeight: 600 }}>
              🟣 БС активны
            </span>
          )}
        </div>
        <button
          type="button"
          className="btn-icon-subtle"
          title="Сбросить счетчики трафика всех направлений"
          onClick={() => handleResetTraffic('all', 'всего трафика')}
          style={{ fontSize: 12, padding: '4px 10px', background: 'rgba(255,255,255,0.04)', border: '1px solid var(--border)', borderRadius: 6, cursor: 'pointer', color: 'var(--text-muted)' }}
        >
          🔄 Сбросить статистику
        </button>
      </div>

      <p className="muted small" style={{ margin: '0 0 14px' }}>
        Накопленный объем трафика по направлениям маршрутизации. Сохраняется между перезагрузками роутера.
      </p>

      {/* 3 равных сбалансированных сегмента */}
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(220px, 1fr))', gap: 12, marginBottom: 12 }}>
        {/* 1. Прямой трафик */}
        <div style={{ background: 'rgba(255,255,255,0.02)', border: '1px solid var(--border)', borderRadius: 10, padding: 12, display: 'flex', flexDirection: 'column', justifyContent: 'space-between' }}>
          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 6 }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 6, color: '#4ade80', fontSize: 13, fontWeight: 600 }}>
              <span>🟢</span>
              <span>Прямой трафик</span>
            </div>
            <button
              type="button"
              className="btn-icon-subtle"
              title="Сбросить счетчик прямого трафика"
              aria-label="Сбросить счетчик прямого трафика"
              onClick={() => handleResetTraffic('direct', 'прямого трафика')}
              style={{ fontSize: 13, padding: '2px 6px', background: 'rgba(255,255,255,0.04)', border: '1px solid var(--border)', borderRadius: 5, cursor: 'pointer', color: 'var(--text-muted)' }}
            >
              ↺
            </button>
          </div>
          <div style={{ fontSize: 22, fontWeight: 700, color: 'var(--text-bright)' }}>
            {fmtTrafficBytes(trafficSummary.direct_bytes)}
          </div>
          <div className="muted small" style={{ marginTop: 4, fontSize: 11 }}>
            Мимо прокси в WAN
          </div>
        </div>

        {/* 2. VPS трафик (Обычные серверы) */}
        <div style={{ background: 'rgba(255,255,255,0.02)', border: '1px solid var(--border)', borderRadius: 10, padding: 12, display: 'flex', flexDirection: 'column', justifyContent: 'space-between' }}>
          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 6 }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 6, color: '#38bdf8', fontSize: 13, fontWeight: 600 }}>
              <span>🔵</span>
              <span>VPS трафик</span>
            </div>
            <button
              type="button"
              className="btn-icon-subtle"
              title="Сбросить счетчик VPS трафика"
              aria-label="Сбросить счетчик VPS трафика"
              onClick={() => handleResetTraffic('vps', 'VPS трафика')}
              style={{ fontSize: 13, padding: '2px 6px', background: 'rgba(255,255,255,0.04)', border: '1px solid var(--border)', borderRadius: 5, cursor: 'pointer', color: 'var(--text-muted)' }}
            >
              ↺
            </button>
          </div>
          <div style={{ fontSize: 22, fontWeight: 700, color: 'var(--text-bright)' }}>
            {fmtTrafficBytes(trafficSummary.proxy_bytes)}
          </div>
          <div className="muted small" style={{ marginTop: 4, fontSize: 11 }}>
            Обычные серверы
          </div>
        </div>

        {/* 3. Туннель белых списков */}
        <div style={{ background: trafficSummary.whitelist_active ? 'rgba(168, 85, 247, 0.08)' : 'rgba(255,255,255,0.02)', border: trafficSummary.whitelist_active ? '1px solid rgba(168, 85, 247, 0.4)' : '1px solid var(--border)', borderRadius: 10, padding: 12, display: 'flex', flexDirection: 'column', justifyContent: 'space-between' }}>
          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 6 }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 6, color: '#c084fc', fontSize: 13, fontWeight: 600 }}>
              <span>🟣</span>
              <span>Туннель белых списков</span>
            </div>
            <button
              type="button"
              className="btn-icon-subtle"
              title="Сбросить счетчик трафика белых списков"
              aria-label="Сбросить счетчик трафика белых списков"
              onClick={() => handleResetTraffic('whitelist', 'трафика белых списков')}
              style={{ fontSize: 13, padding: '2px 6px', background: 'rgba(255,255,255,0.04)', border: '1px solid var(--border)', borderRadius: 5, cursor: 'pointer', color: 'var(--text-muted)' }}
            >
              ↺
            </button>
          </div>
          <div style={{ fontSize: 22, fontWeight: 700, color: 'var(--text-bright)' }}>
            {fmtTrafficBytes(trafficSummary.whitelist_bytes)}
          </div>
          <div className="muted small" style={{ marginTop: 4, fontSize: 11 }}>
            {trafficSummary.whitelist_active ? 'Активен для LAN устройств' : 'Серверы с меткой [БС]'}
          </div>
        </div>
      </div>

      {/* Квота подписки (во всю ширину карточки внизу) */}
      {(() => {
        const activeWlProv = providers.find((p) => p.is_whitelist && (p.total_bytes ?? 0) > 0) || providers.find((p) => (p.total_bytes ?? 0) > 0)
        if (!activeWlProv || !activeWlProv.total_bytes || activeWlProv.total_bytes <= 0) return null
        const used = (activeWlProv.upload_bytes || 0) + (activeWlProv.download_bytes || 0)
        const total = activeWlProv.total_bytes
        const remaining = Math.max(0, total - used)
        const percent = Math.min(100, Math.round((used / total) * 100))
        const expireStr = activeWlProv.expire_timestamp
          ? new Date(activeWlProv.expire_timestamp * 1000).toLocaleDateString()
          : null

        return (
          <div style={{ background: 'rgba(255,255,255,0.02)', border: '1px solid var(--border)', borderRadius: 10, padding: '10px 14px', fontSize: 11.5 }}>
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 6, flexWrap: 'wrap', gap: 6 }}>
              <span style={{ color: '#c084fc', fontWeight: 600 }}>
                📊 Квота подписки ({activeWlProv.name || activeWlProv.id}): {formatQuotaBytes(used)} / {formatQuotaBytes(total)} ({percent}%)
              </span>
              <span style={{ color: remaining === 0 ? '#ef4444' : '#4ade80', fontWeight: 600 }}>
                Осталось: {formatQuotaBytes(remaining)}
              </span>
            </div>
            <div style={{ width: '100%', height: 6, background: 'rgba(255,255,255,0.08)', borderRadius: 3, overflow: 'hidden' }}>
              <div
                style={{
                  width: `${percent}%`,
                  height: '100%',
                  background: percent > 90 ? '#ef4444' : percent > 75 ? '#fbbf24' : 'linear-gradient(90deg, #a855f7, #c084fc)',
                  borderRadius: 3,
                }}
              />
            </div>
            {expireStr && (
              <div className="muted" style={{ fontSize: 10.5, marginTop: 4 }}>
                Действует до: {expireStr}
              </div>
            )}
          </div>
        )
      })()}
    </section>
  )
}
