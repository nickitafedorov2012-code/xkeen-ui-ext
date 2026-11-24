import { useEffect, useState } from 'react'
import { apiGet } from '../api'
import { MultiWanStatus } from '../types'

interface Props {
  onSwitchTab?: (tab: 'servers' | 'settings' | 'dashboard' | 'devices' | 'google-ai') => void
}

export function MultiWanWidget({ onSwitchTab }: Props) {
  const [mw, setMw] = useState<MultiWanStatus | null>(null)

  useEffect(() => {
    let active = true
    const load = async () => {
      try {
        const data = await apiGet<MultiWanStatus>('multiwan/status')
        if (active) setMw(data)
      } catch {
        // Игнорируем ошибку опроса на дашборде
      }
    }
    void load()
    const timer = window.setInterval(load, 4000)
    return () => {
      active = false
      window.clearInterval(timer)
    }
  }, [])

  const channels = Array.isArray(mw?.channels) ? mw.channels : []
  if (!mw || (!mw.enabled && channels.length < 2)) {
    return null
  }

  const hotSwapActive =
    mw.enabled &&
    mw.active_count >= 1 &&
    channels.some((ch) => ch.enabled && (ch.state === 'offline' || ch.state === 'quarantine'))

  return (
    <section
      className="card"
      data-testid="multiwan-dashboard-widget"
      style={{ marginBottom: 14, padding: '12px 16px' }}
    >
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 10, flexWrap: 'wrap' }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
          <strong style={{ fontSize: 14 }}>🌐 Multi-WAN</strong>
          <span
            className="badge"
            style={{
              color: mw.enabled ? '#34d399' : 'var(--muted)',
              background: mw.enabled ? 'rgba(52, 211, 153, 0.14)' : 'transparent',
              whiteSpace: 'nowrap',
            }}
          >
            {mw.enabled ? `Активно каналов: ${mw.active_count}` : `Обнаружено каналов: ${channels.length} (выкл)`}
          </span>
          {hotSwapActive && (
            <span
              className="badge"
              data-testid="multiwan-hotswap-banner"
              style={{
                color: '#fbbf24',
                background: 'rgba(251, 191, 36, 0.16)',
                borderColor: 'rgba(251, 191, 36, 0.45)',
                fontWeight: 600,
                whiteSpace: 'nowrap',
              }}
            >
              ⚡ Hot-Swap активен — трафик без обрыва на выжившем канале
            </span>
          )}
          {mw.enabled && (
            <span className="muted" style={{ fontSize: 12, whiteSpace: 'nowrap' }}>
              Суммарно: ↓ {(mw.total_rx_mbps ?? 0).toFixed(1)} · ↑ {(mw.total_tx_mbps ?? 0).toFixed(1)} Мбит/с
            </span>
          )}
        </div>

        {onSwitchTab && (
          <button
            type="button"
            className="btn"
            style={{ fontSize: 12, padding: '4px 10px', whiteSpace: 'nowrap' }}
            onClick={() => onSwitchTab('settings')}
          >
            ⚙️ Настроить
          </button>
        )}
      </div>

      <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap', marginTop: 10 }}>
        {channels.map((ch) => {
          const online = ch.state === 'online'
          const standby = ch.state === 'standby'
          const dot = online ? '🟢' : standby || ch.flapping ? '🟡' : '🔴'
          return (
            <span
              key={ch.rci_id}
              className="badge"
              style={{ fontSize: 12, padding: '4px 10px', whiteSpace: 'nowrap' }}
            >
              {dot} {ch.name} ({ch.linux_iface}): {ch.effective_weight}% · ↓ {ch.rx_mbps.toFixed(1)} ↑ {ch.tx_mbps.toFixed(1)} Мбит/с
              {ch.rtt_ms != null ? ` · ${ch.rtt_ms} мс` : ''}
            </span>
          )
        })}
      </div>
    </section>
  )
}
