import { fmtBytes, type ZapretAnalytics } from '../../types'
import { fmtUptime } from './zapretHelpers'

interface AnalyticsSectionProps {
  analytics: ZapretAnalytics | null
  resettingAnalytics: boolean
  busy: boolean
  isRunning: boolean
  handleResetAnalytics: () => void
}

export function AnalyticsSection({
  analytics,
  resettingAnalytics,
  busy,
  isRunning,
  handleResetAnalytics,
}: AnalyticsSectionProps) {
  return (
      <section
        className="card"
        style={{
          padding: '20px 24px',
          background: 'linear-gradient(135deg, rgba(6, 182, 212, 0.06) 0%, rgba(15, 23, 42, 0.6) 100%)',
          border: '1px solid rgba(56, 189, 248, 0.25)',
          borderRadius: 16,
          boxShadow: '0 8px 24px rgba(6, 182, 212, 0.05)',
        }}
      >
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: 12, marginBottom: 16 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
            <div
              style={{
                width: 38,
                height: 38,
                borderRadius: 10,
                background: 'rgba(56, 189, 248, 0.15)',
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                fontSize: 18,
              }}
            >
              📊
            </div>
            <div>
              <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                <h3 style={{ margin: 0, fontSize: 16, fontWeight: 700 }}>
                  Live-монитор и счётчик спасённого трафика (DPI Analytics)
                </h3>
                <span
                  className="badge"
                  style={{
                    background: 'rgba(34, 197, 94, 0.15)',
                    color: '#22c55e',
                    border: '1px solid rgba(34, 197, 94, 0.3)',
                    fontSize: 11,
                  }}
                >
                  ● LIVE NETFILTER
                </span>
              </div>
              <div className="muted small" style={{ marginTop: 2 }}>
                Объём десинхронизированного трафика, прошедшего через NFQUEUE ядра роутера без расхода зарубежного лимита VPS
              </div>
            </div>
          </div>

          <button
            type="button"
            className="btn sm ghost"
            disabled={resettingAnalytics || busy}
            onClick={handleResetAnalytics}
            style={{ fontSize: 12, color: '#94a3b8' }}
            title="Сбросить байтовые счётчики iptables zapret"
          >
            {resettingAnalytics ? 'Сброс…' : '↺ Сбросить счётчики'}
          </button>
        </div>

        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(200px, 1fr))', gap: 12 }}>
          {/* Трафик за весь период */}
          <div
            style={{
              padding: '14px 16px',
              borderRadius: 12,
              background: 'rgba(0, 0, 0, 0.25)',
              border: '1px solid rgba(255, 255, 255, 0.05)',
              display: 'flex',
              flexDirection: 'column',
              gap: 4,
            }}
          >
            <div className="muted small" style={{ fontSize: 11, textTransform: 'uppercase', letterSpacing: 0.5 }}>
              Трафик за весь период
            </div>
            <div style={{ fontSize: 20, fontWeight: 700, color: '#38bdf8', fontFamily: 'Consolas, monospace' }}>
              {fmtBytes(analytics?.period_bytes || analytics?.bytes_intercepted || 0)}
            </div>
            <div className="muted small" style={{ fontSize: 11 }}>
              Накопительный трафик с момента запуска
            </div>
          </div>

          {/* Сэкономлено VPS (единый счетчик перехвачено + сэкономлено) */}
          <div
            style={{
              padding: '14px 16px',
              borderRadius: 12,
              background: 'rgba(34, 197, 94, 0.06)',
              border: '1px solid rgba(34, 197, 94, 0.25)',
              display: 'flex',
              flexDirection: 'column',
              gap: 4,
            }}
          >
            <div className="muted small" style={{ fontSize: 11, textTransform: 'uppercase', letterSpacing: 0.5, color: '#4ade80' }}>
              Сэкономлено трафика на VPS
            </div>
            <div style={{ fontSize: 20, fontWeight: 700, color: '#22c55e', fontFamily: 'Consolas, monospace' }}>
              {fmtBytes(analytics?.vps_saved_bytes || analytics?.bytes_intercepted || 0)}
            </div>
            <div className="muted small" style={{ fontSize: 11, color: '#86efac' }}>
              {analytics?.packets_intercepted?.toLocaleString() || 0} пакетов · 100% прямой обход без расхода прокси
            </div>
          </div>

          {/* Пакеты TCP / UDP */}
          <div
            style={{
              padding: '14px 16px',
              borderRadius: 12,
              background: 'rgba(0, 0, 0, 0.25)',
              border: '1px solid rgba(255, 255, 255, 0.05)',
              display: 'flex',
              flexDirection: 'column',
              gap: 4,
            }}
          >
            <div className="muted small" style={{ fontSize: 11, textTransform: 'uppercase', letterSpacing: 0.5 }}>
              TCP / UDP распределение
            </div>
            <div style={{ fontSize: 16, fontWeight: 600, color: '#f1f5f9', display: 'flex', gap: 12 }}>
              <span title="TCP пакеты (YouTube Web, HTTPS SNI)">
                TCP: <b style={{ color: '#38bdf8', fontFamily: 'Consolas, monospace' }}>{analytics?.tcp_packets?.toLocaleString() || 0}</b>
              </span>
              <span title="UDP пакеты (Discord Voice, QUIC)">
                UDP: <b style={{ color: '#a855f7', fontFamily: 'Consolas, monospace' }}>{analytics?.udp_packets?.toLocaleString() || 0}</b>
              </span>
            </div>
            <div className="muted small" style={{ fontSize: 11 }}>
              L7 DPI десинхронизация
            </div>
          </div>

          {/* nfqws2 Ресурсы процесса */}
          <div
            style={{
              padding: '14px 16px',
              borderRadius: 12,
              background: 'rgba(0, 0, 0, 0.25)',
              border: '1px solid rgba(255, 255, 255, 0.05)',
              display: 'flex',
              flexDirection: 'column',
              gap: 4,
            }}
          >
            <div className="muted small" style={{ fontSize: 11, textTransform: 'uppercase', letterSpacing: 0.5 }}>
              Процесс nfqws2
            </div>
            <div style={{ fontSize: 15, fontWeight: 600, color: '#f1f5f9', display: 'flex', gap: 10 }}>
              <span>RAM: <b style={{ color: '#cbd5e1', fontFamily: 'Consolas, monospace' }}>{((analytics?.nfqws_mem_bytes || 0) / (1024 * 1024)).toFixed(1)} МБ</b></span>
              <span>Uptime: <b style={{ color: '#cbd5e1', fontFamily: 'Consolas, monospace' }}>{fmtUptime(analytics?.uptime_seconds)}</b></span>
            </div>
            <div className="muted small" style={{ fontSize: 11 }}>
              {isRunning ? '🟢 Демон активен' : '⚪ Демон остановлен'}
            </div>
          </div>
        </div>
      </section>
  )
}