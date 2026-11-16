import React, { useCallback, useEffect, useState } from 'react'
import { apiGet, apiPost } from '../../api'
import type { FreeSubMode, FreeSubPoolInfo } from '../../types'
import { formatQuotaBytes } from './serversHelpers'

interface Props {
  mode: FreeSubMode
  notify?: (msg: string, isError?: boolean) => void
  onUpdated: () => void
  open: boolean
}

function formatDuration(secs: number): string {
  if (secs < 60) return `${secs}с`
  const mins = Math.floor(secs / 60)
  const rem = secs % 60
  if (mins < 60) return `${mins}м ${rem}с`
  const hrs = Math.floor(mins / 60)
  return `${hrs}ч ${mins % 60}м`
}

function reasonStyle(reason: string): React.CSSProperties {
  if (reason === 'Активен') {
    return { background: 'rgba(16,185,129,0.16)', color: '#34d399', borderColor: 'rgba(16,185,129,0.4)' }
  }
  if (reason === 'Обрыв') {
    return { background: 'rgba(239,68,68,0.16)', color: '#f87171', borderColor: 'rgba(239,68,68,0.4)' }
  }
  return { background: 'rgba(148,163,184,0.12)', color: 'var(--muted)', borderColor: 'rgba(148,163,184,0.28)' }
}

export function FreeSubPoolSubmenu({ mode, notify, onUpdated, open }: Props) {
  const [pool, setPool] = useState<FreeSubPoolInfo | null>(null)
  const [loading, setLoading] = useState(false)
  const [busy, setBusy] = useState<string | null>(null)

  const loadPool = useCallback(async () => {
    setLoading(true)
    try {
      const data = await apiGet<FreeSubPoolInfo>(`providers/free-sub/pool?mode=${mode}`)
      setPool(data)
    } catch (e) {
      notify?.(`Ошибка загрузки пула: ${e instanceof Error ? e.message : String(e)}`, true)
    } finally {
      setLoading(false)
    }
  }, [mode, notify])

  useEffect(() => {
    if (open) {
      void loadPool()
    }
  }, [open, loadPool])

  if (!open) return null

  const runAction = async (key: string, fn: () => Promise<void>) => {
    setBusy(key)
    try {
      await fn()
      await loadPool()
      onUpdated()
    } catch (e) {
      notify?.(e instanceof Error ? e.message : String(e), true)
    } finally {
      setBusy(null)
    }
  }

  const counterText =
    pool && pool.pool_total > 0
      ? `${pool.pool_index != null && pool.pool_index > 0 ? pool.pool_index : '—'} / ${pool.pool_total}`
      : '0 / 0'

  return (
    <div
      data-testid={`free-sub-submenu-${mode}`}
      style={{
        marginTop: 10,
        padding: '10px 12px',
        borderRadius: 10,
        background: 'rgba(15, 23, 42, 0.55)',
        border: '1px solid rgba(56, 189, 248, 0.22)',
        display: 'flex',
        flexDirection: 'column',
        gap: 10,
      }}
    >
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', flexWrap: 'wrap', gap: 8 }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
          <span style={{ fontSize: 12, fontWeight: 700, color: 'var(--text)' }}>
            Пул узлов ({counterText})
          </span>
          {pool?.active_node && (
            <span className="badge" style={{ fontSize: 11 }}>
              {pool.active_node}
            </span>
          )}
          {pool && pool.blacklist_count > 0 && (
            <span
              className="badge"
              style={{
                fontSize: 10,
                background: 'rgba(239,68,68,0.14)',
                color: '#fca5a5',
                borderColor: 'rgba(239,68,68,0.35)',
              }}
              title="Узлы в чёрном списке на 24 часа"
            >
              🚫 ЧС: {pool.blacklist_count}
            </span>
          )}
        </div>
        {loading && <span style={{ fontSize: 11, color: 'var(--muted)' }}>Обновление…</span>}
      </div>

      <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6 }}>
        <button
          type="button"
          className="btn sm ghost"
          data-testid={`free-sub-prev-${mode}`}
          disabled={busy !== null || !pool || pool.pool_total === 0}
          onClick={() =>
            void runAction('prev', async () => {
              const res = await apiPost<{ active_node: string }>('providers/free-sub/switch-node', {
                mode,
                direction: 'prev',
              })
              notify?.(`Переключено на: ${res.active_node}`)
            })
          }
        >
          ◀ Предыдущий
        </button>

        <button
          type="button"
          className="btn sm ghost"
          data-testid={`free-sub-next-${mode}`}
          disabled={busy !== null || !pool || pool.pool_total === 0}
          onClick={() =>
            void runAction('next', async () => {
              const res = await apiPost<{ active_node: string }>('providers/free-sub/switch-node', {
                mode,
                direction: 'next',
              })
              notify?.(`Переключено на: ${res.active_node}`)
            })
          }
        >
          Следующий ▶
        </button>

        <button
          type="button"
          className="btn sm ghost"
          data-testid={`free-sub-blacklist-${mode}`}
          disabled={busy !== null || !pool || pool.pool_total === 0}
          onClick={() =>
            void runAction('blacklist', async () => {
              const res = await apiPost<{ active_node: string | null }>('providers/free-sub/blacklist-node', {
                mode,
              })
              notify?.(
                res.active_node
                  ? `Узел в ЧС (24ч), активен: ${res.active_node}`
                  : 'Узел добавлен в ЧС (24ч), пул пуст'
              )
            })
          }
        >
          🚫 Плохой узел → следующий
        </button>

        <button
          type="button"
          className="btn sm ghost"
          data-testid={`free-sub-refill-${mode}`}
          disabled={busy !== null}
          onClick={() =>
            void runAction('refill', async () => {
              await apiPost('providers/free-sub/refill', { mode })
              notify?.('Запущен фоновый добор узлов без обрыва сессии')
            })
          }
        >
          🔄 Добрать ещё
        </button>
      </div>

      <div>
        <div style={{ fontSize: 11, fontWeight: 700, color: 'var(--muted)', marginBottom: 6 }}>
          История сессий и трафика
        </div>
        {!pool || pool.history.length === 0 ? (
          <div style={{ fontSize: 11, color: 'var(--muted)' }}>
            История пока пуста — подключите подписку, чтобы начать учёт сессий.
          </div>
        ) : (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 5, maxHeight: 220, overflowY: 'auto' }}>
            {pool.history.slice(0, 20).map((rec, idx) => (
              <div
                key={`${rec.node_name}-${rec.started_at}-${idx}`}
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'space-between',
                  gap: 8,
                  padding: '6px 8px',
                  borderRadius: 8,
                  background: 'rgba(255,255,255,0.03)',
                  border: '1px solid rgba(255,255,255,0.06)',
                  fontSize: 11,
                  flexWrap: 'wrap',
                }}
              >
                <div style={{ display: 'flex', flexDirection: 'column', gap: 2, minWidth: 0, flex: '1 1 180px' }}>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 6, flexWrap: 'wrap' }}>
                    <strong style={{ color: 'var(--text)' }}>{rec.node_name}</strong>
                    <span className="badge" style={{ fontSize: 10, ...reasonStyle(rec.end_reason) }}>
                      {rec.end_reason}
                    </span>
                  </div>
                  <div style={{ color: 'var(--muted)', fontSize: 10 }}>
                    {rec.host}:{rec.port} · ⏱ {formatDuration(rec.duration_secs)} · ↓ {formatQuotaBytes(rec.down_bytes)} / ↑ {formatQuotaBytes(rec.up_bytes)}
                  </div>
                </div>
                {rec.end_reason !== 'Активен' && (
                  <button
                    type="button"
                    className="btn sm ghost"
                    data-testid={`free-sub-reconnect-${idx}`}
                    disabled={busy !== null}
                    onClick={() =>
                      void runAction(`reconnect-${idx}`, async () => {
                        const res = await apiPost<{ active_node: string }>('providers/free-sub/switch-node', {
                          mode,
                          node_name: rec.node_name,
                        })
                        notify?.(`Подключено снова: ${res.active_node}`)
                      })
                    }
                    style={{ fontSize: 11, padding: '3px 8px' }}
                  >
                    Подключить снова
                  </button>
                )}
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  )
}
