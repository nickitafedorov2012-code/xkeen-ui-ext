import { useCallback, useEffect, useState } from 'react'
import { apiGet, apiPost } from '../../api'
import type { FreeSubMode, FreeSubStage, FreeSubStatus } from '../../types'

interface FreeSubCardProps {
  notify: (msg: string, isError?: boolean) => void
  onConnected?: () => void
}

const STAGES: { key: FreeSubStage; label: string }[] = [
  { key: 'sources', label: '1. Источники / кэш' },
  { key: 'prefilter', label: '2. DNS + TCP' },
  { key: 'l7', label: '3. L7-проверка' },
  { key: 'connected', label: '4. Подключено' },
]

function stageOrder(stage: FreeSubStage): number {
  switch (stage) {
    case 'sources':
      return 0
    case 'prefilter':
      return 1
    case 'l7':
      return 2
    case 'publishing':
    case 'connected':
      return 3
    default:
      return -1
  }
}

export function FreeSubCard({ notify, onConnected }: FreeSubCardProps) {
  const [mode, setMode] = useState<FreeSubMode>('vpn')
  const [status, setStatus] = useState<FreeSubStatus | null>(null)
  const [submitting, setSubmitting] = useState(false)

  const fetchStatus = useCallback(async () => {
    try {
      const res = await apiGet<{ status: FreeSubStatus }>('providers/free-sub/status')
      if (res?.status) {
        setStatus(res.status)
        return res.status
      }
    } catch {
      // ignore transient poll errors
    }
    return null
  }, [])

  useEffect(() => {
    void fetchStatus()
  }, [fetchStatus])

  useEffect(() => {
    if (!status?.running) return
    const timer = setInterval(async () => {
      const next = await fetchStatus()
      if (next && !next.running) {
        if (next.stage === 'connected') {
          notify(`Подключено: ${next.active_node ?? 'рабочий узел'}${next.ttff_ms ? ` (${next.ttff_ms} мс)` : ''}`)
          onConnected?.()
        } else if (next.stage === 'error' && next.error) {
          notify(next.error, true)
        }
      }
    }, 500)
    return () => clearInterval(timer)
  }, [status?.running, fetchStatus, notify, onConnected])

  const handleConnect = async () => {
    setSubmitting(true)
    try {
      await apiPost('providers/free-sub/connect', { mode })
      await fetchStatus()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка запуска бесплатной подписки', true)
    } finally {
      setSubmitting(false)
    }
  }

  const isRunning = Boolean(status?.running) || submitting
  const activeStageIdx = status ? stageOrder(status.stage) : -1
  const stats = status?.stats

  return (
    <div
      className="card free-sub-card"
      data-testid="free-sub-card"
      style={{
        marginBottom: 12,
        padding: '12px 14px',
        background: mode === 'whitelist'
          ? 'linear-gradient(135deg, rgba(168, 85, 247, 0.08), rgba(56, 189, 248, 0.05))'
          : 'linear-gradient(135deg, rgba(56, 189, 248, 0.08), rgba(34, 197, 94, 0.05))',
        border: mode === 'whitelist'
          ? '1px solid rgba(168, 85, 247, 0.32)'
          : '1px solid rgba(56, 189, 248, 0.28)',
        borderRadius: 10,
      }}
    >
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 10, flexWrap: 'wrap' }}>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
            <b style={{ fontSize: 14 }}>⚡ Бесплатная мульти-подписка в 1 клик</b>
            {status?.ttff_ms != null && (
              <span
                className="badge"
                data-testid="free-sub-ttff"
                style={{
                  fontSize: 11,
                  background: 'rgba(34, 197, 94, 0.16)',
                  color: '#4ade80',
                  border: '1px solid rgba(34, 197, 94, 0.35)',
                }}
              >
                TTFF: {status.ttff_ms} мс
              </span>
            )}
          </div>
          <span className="muted small">
            Автопоиск, DNS/TCP-предфильтр и точечная L7-проверка без ручных настроек
          </span>
        </div>

        <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
          <div
            role="group"
            aria-label="Режим бесплатной подписки"
            style={{
              display: 'inline-flex',
              background: 'rgba(0,0,0,0.22)',
              borderRadius: 8,
              padding: 3,
              border: '1px solid var(--border)',
            }}
          >
            <button
              type="button"
              className={`btn sm ${mode === 'vpn' ? 'primary' : 'ghost'}`}
              data-testid="free-sub-mode-vpn"
              disabled={isRunning}
              onClick={() => setMode('vpn')}
              style={{ borderRadius: 6, fontSize: 12 }}
            >
              🌐 Обычный VPN
            </button>
            <button
              type="button"
              className={`btn sm ${mode === 'whitelist' ? 'primary' : 'ghost'}`}
              data-testid="free-sub-mode-whitelist"
              disabled={isRunning}
              onClick={() => setMode('whitelist')}
              style={{
                borderRadius: 6,
                fontSize: 12,
                background: mode === 'whitelist' ? 'rgba(168, 85, 247, 0.35)' : undefined,
                borderColor: mode === 'whitelist' ? 'rgba(168, 85, 247, 0.6)' : undefined,
              }}
            >
              🟣 Белые списки РФ
            </button>
          </div>

          <button
            type="button"
            className="btn primary"
            data-testid="free-sub-connect-btn"
            disabled={isRunning}
            onClick={handleConnect}
            style={{ whiteSpace: 'nowrap', fontWeight: 600 }}
          >
            {isRunning ? '⏳ Подключение…' : '⚡ Подключить бесплатную мульти-подписку'}
          </button>
        </div>
      </div>

      {status && status.stage !== 'idle' && (
        <div style={{ marginTop: 10, display: 'flex', flexDirection: 'column', gap: 8 }}>
          {/* Прогресс этапов */}
          <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap' }} data-testid="free-sub-stages">
            {STAGES.map((st, idx) => {
              const done = status.stage === 'connected' || activeStageIdx > idx
              const active = status.stage !== 'connected' && status.stage !== 'error' && activeStageIdx === idx
              return (
                <span
                  key={st.key}
                  style={{
                    fontSize: 11,
                    padding: '3px 8px',
                    borderRadius: 6,
                    border: '1px solid',
                    borderColor: done
                      ? 'rgba(34, 197, 94, 0.45)'
                      : active
                      ? 'rgba(56, 189, 248, 0.55)'
                      : 'var(--border)',
                    background: done
                      ? 'rgba(34, 197, 94, 0.14)'
                      : active
                      ? 'rgba(56, 189, 248, 0.14)'
                      : 'rgba(255, 255, 255, 0.03)',
                    color: done ? '#4ade80' : active ? '#38bdf8' : 'var(--muted)',
                    fontWeight: active || done ? 600 : 400,
                  }}
                >
                  {done ? '✓ ' : active ? '● ' : ''}
                  {st.label}
                </span>
              )
            })}
          </div>

          {/* Текущий статус и диагностика */}
          <div
            style={{
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'space-between',
              gap: 8,
              flexWrap: 'wrap',
              fontSize: 12,
            }}
          >
            <span
              data-testid="free-sub-stage-label"
              style={{ color: status.stage === 'error' ? '#f87171' : 'var(--text)' }}
            >
              {status.error ? `⚠️ ${status.error}` : status.stage_label}
              {status.preserved_active ? ' · Активная сессия сохранена' : ''}
            </span>

            {stats && (stats.total_raw > 0 || stats.l7_checked > 0) && (
              <span className="muted small" data-testid="free-sub-diagnostics">
                Кандидатов: {stats.total_raw} · Отсеяно DNS/IP: {stats.rejected_dns} · TCP: {stats.tcp_passed}/{stats.tcp_checked} · L7: {stats.l7_passed}/{stats.l7_checked} · Рабочих: {stats.working_count}
              </span>
            )}
          </div>
        </div>
      )}
    </div>
  )
}
