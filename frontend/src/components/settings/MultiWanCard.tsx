import { useCallback, useEffect, useState } from 'react'
import { apiGet, apiPost } from '../../api'
import { MultiWanChannelConfig, MultiWanStatus, MultiWanWeightMode } from '../../types'

interface Props {
  notify: (msg: string, ok?: boolean) => void
}

const KIND_ICON: Record<string, string> = {
  modem: '📶',
  wisp: '📡',
  ethernet: '🔌',
}

const KIND_LABEL: Record<string, string> = {
  modem: 'USB-модем',
  wisp: 'WISP Wi-Fi',
  ethernet: 'Ethernet',
}

function stateBadge(state: string, flapping: boolean): { text: string; color: string; bg: string } {
  if (flapping || state === 'quarantine') {
    return { text: '🟡 Нестабилен', color: '#fbbf24', bg: 'rgba(251, 191, 36, 0.14)' }
  }
  switch (state) {
    case 'online':
      return { text: '🟢 Активен', color: '#34d399', bg: 'rgba(52, 211, 153, 0.14)' }
    case 'standby':
      return { text: '🟡 Резерв', color: '#fbbf24', bg: 'rgba(251, 191, 36, 0.14)' }
    default:
      return { text: '🔴 Нет интернета', color: '#f87171', bg: 'rgba(248, 113, 113, 0.14)' }
  }
}

export function MultiWanCard({ notify }: Props) {
  const [mw, setMw] = useState<MultiWanStatus | null>(null)
  const [busy, setBusy] = useState(false)
  const [probing, setProbing] = useState(false)

  const loadStatus = useCallback(async () => {
    try {
      const data = await apiGet<MultiWanStatus>('multiwan/status')
      setMw(data)
    } catch {
      // Игнорируем временный сбой опроса
    }
  }, [])

  useEffect(() => {
    loadStatus()
    const timer = window.setInterval(loadStatus, 4000)
    return () => window.clearInterval(timer)
  }, [loadStatus])

  const saveConfig = async (payload: {
    enabled?: boolean
    mode?: MultiWanWeightMode
    single_ip_subscriptions?: boolean
    channels?: MultiWanChannelConfig[]
  }) => {
    setBusy(true)
    try {
      const updated = await apiPost<MultiWanStatus>('multiwan/config', payload)
      setMw(updated)
      notify('Настройки Multi-WAN применены')
    } catch (err) {
      notify(err instanceof Error ? err.message : 'Ошибка сохранения Multi-WAN', false)
    } finally {
      setBusy(false)
    }
  }

  const runProbe = async () => {
    setProbing(true)
    try {
      const updated = await apiPost<MultiWanStatus>('multiwan/test-probe', {})
      setMw(updated)
      notify('Проверка каналов Multi-WAN завершена')
    } catch (err) {
      notify(err instanceof Error ? err.message : 'Ошибка проверки каналов', false)
    } finally {
      setProbing(false)
    }
  }

  const updateChannel = (rciId: string, patch: Partial<MultiWanChannelConfig>) => {
    if (!mw) return
    const next: MultiWanChannelConfig[] = mw.channels.map((ch) => ({
      rci_id: ch.rci_id,
      weight: ch.rci_id === rciId && patch.weight !== undefined ? patch.weight : ch.manual_weight,
      enabled: ch.rci_id === rciId && patch.enabled !== undefined ? patch.enabled : ch.enabled,
    }))
    void saveConfig({ channels: next })
  }

  const enabled = mw?.enabled ?? false
  const mode = mw?.mode ?? 'auto'
  const singleIp = mw?.single_ip_subscriptions ?? false
  const channels = mw?.channels ?? []

  return (
    <section className="card" data-testid="multiwan-settings-card">
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 12, flexWrap: 'wrap', marginBottom: 12 }}>
        <div>
          <h2 style={{ margin: 0 }}>🌐 Multi-WAN — Суммирование и Хотсвап</h2>
          <div className="muted" style={{ fontSize: 12, marginTop: 4 }}>
            Объединяет скорость нескольких модемов и WISP-каналов для VK-туннеля, Mihomo, Direct и Zapret.
          </div>
        </div>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <button
            type="button"
            className="btn"
            disabled={probing}
            onClick={runProbe}
            data-testid="multiwan-probe-btn"
          >
            {probing ? '⏳ Проверка…' : '🩺 Проверить каналы'}
          </button>
          <button
            type="button"
            className={enabled ? 'btn btn-primary' : 'btn'}
            disabled={busy}
            onClick={() => void saveConfig({ enabled: !enabled })}
            data-testid="multiwan-toggle-btn"
          >
            {enabled ? '🟢 Включено' : '⚪ Выключено'}
          </button>
        </div>
      </div>

      <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap', alignItems: 'center', marginBottom: 14 }}>
        <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
          <span className="muted" style={{ fontSize: 12 }}>Веса каналов:</span>
          <button
            type="button"
            className={mode === 'auto' ? 'btn btn-primary' : 'btn'}
            disabled={busy}
            onClick={() => void saveConfig({ mode: 'auto' })}
            data-testid="multiwan-mode-auto"
          >
            ⚡ Автоматически
          </button>
          <button
            type="button"
            className={mode === 'manual' ? 'btn btn-primary' : 'btn'}
            disabled={busy}
            onClick={() => void saveConfig({ mode: 'manual' })}
            data-testid="multiwan-mode-manual"
          >
            🎛️ Вручную (%)
          </button>
        </div>

        <label style={{ display: 'flex', alignItems: 'center', gap: 8, cursor: 'pointer', fontSize: 13 }}>
          <input
            type="checkbox"
            checked={singleIp}
            disabled={busy}
            onChange={(e) => void saveConfig({ single_ip_subscriptions: e.target.checked })}
            data-testid="multiwan-single-ip-toggle"
          />
          <span>🛡️ Защита платных подписок от бана за 2 IP</span>
        </label>
      </div>

      {mw?.gateway_collision_warning && (
        <div
          data-testid="multiwan-collision-warning"
          style={{
            background: 'rgba(251, 191, 36, 0.14)',
            border: '1px solid rgba(251, 191, 36, 0.45)',
            color: '#fbbf24',
            borderRadius: 8,
            padding: '8px 12px',
            fontSize: 12.5,
            marginBottom: 12,
          }}
        >
          ⚠️ {mw.gateway_collision_warning}
        </div>
      )}

      {mw?.last_error && (
        <div style={{ color: '#f87171', fontSize: 12, marginBottom: 10 }}>
          Ошибка Multi-WAN: {mw.last_error}
        </div>
      )}

      {channels.length === 0 ? (
        <div className="muted" style={{ fontSize: 13, padding: '10px 0' }}>
          Внешние WAN-каналы не обнаружены. Подключите USB-модем, WISP Wi-Fi или Ethernet WAN.
        </div>
      ) : (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
          {channels.map((ch) => {
            const badge = stateBadge(ch.state, ch.flapping)
            return (
              <div
                key={ch.rci_id}
                data-testid={`multiwan-channel-${ch.linux_iface}`}
                style={{
                  border: '1px solid var(--border)',
                  borderRadius: 10,
                  padding: '10px 12px',
                  display: 'flex',
                  flexDirection: 'column',
                  gap: 8,
                  background: 'var(--bg-elevated, rgba(255,255,255,0.02))',
                }}
              >
                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: 8 }}>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
                    <input
                      type="checkbox"
                      checked={ch.enabled}
                      disabled={busy}
                      onChange={(e) => updateChannel(ch.rci_id, { enabled: e.target.checked })}
                      title="Участвует в Multi-WAN"
                    />
                    <span style={{ fontWeight: 600 }}>
                      {KIND_ICON[ch.kind] ?? '🌐'} {ch.name}
                    </span>
                    <span className="muted" style={{ fontSize: 12 }}>
                      {KIND_LABEL[ch.kind] ?? ch.kind} · {ch.linux_iface} · {ch.ipv4}
                    </span>
                  </div>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexShrink: 0 }}>
                    <span
                      className="badge"
                      style={{ color: badge.color, background: badge.bg, borderColor: 'transparent', whiteSpace: 'nowrap' }}
                    >
                      {badge.text}
                    </span>
                    <span className="badge" style={{ fontWeight: 600, whiteSpace: 'nowrap' }}>
                      Доля: {ch.effective_weight}%
                    </span>
                  </div>
                </div>

                {(ch.flapping || ch.state === 'quarantine') && (
                  <div
                    data-testid={`multiwan-flapping-warning-${ch.linux_iface}`}
                    style={{
                      background: 'rgba(251, 191, 36, 0.12)',
                      border: '1px solid rgba(251, 191, 36, 0.35)',
                      color: '#fbbf24',
                      borderRadius: 6,
                      padding: '6px 10px',
                      fontSize: 12,
                    }}
                  >
                    ⚠️ Канал часто обрывается — проверьте питание USB-хаба или уровень сигнала
                  </div>
                )}

                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: 12, fontSize: 12 }}>
                  <span className="muted">
                    ↓ {ch.rx_mbps.toFixed(2)} Мбит/с · ↑ {ch.tx_mbps.toFixed(2)} Мбит/с
                    {ch.rtt_ms != null ? ` · RTT ${ch.rtt_ms} мс` : ''}
                    {ch.loss_pct > 0 ? ` · Потери ${ch.loss_pct}%` : ''}
                  </span>

                  {mode === 'manual' && (
                    <label style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                      <span className="muted">
                        Вес: {ch.manual_weight === 0 ? 'Резерв (0%)' : `${ch.manual_weight}%`}
                      </span>
                      <input
                        type="range"
                        min={0}
                        max={100}
                        step={5}
                        value={ch.manual_weight}
                        disabled={busy}
                        onChange={(e) => updateChannel(ch.rci_id, { weight: Number(e.target.value) })}
                        data-testid={`multiwan-weight-${ch.linux_iface}`}
                      />
                    </label>
                  )}
                </div>
              </div>
            )
          })}
        </div>
      )}
    </section>
  )
}
