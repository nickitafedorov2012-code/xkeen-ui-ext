import React, { useCallback, useEffect, useRef, useState } from 'react'
import { apiGet, apiPost } from '../../api'
import type { FreeSubMode, FreeSubPoolInfo, FreeSubStatus } from '../../types'
import { formatQuotaBytes } from './serversHelpers'

interface Props {
  mode: FreeSubMode
  notify?: (msg: string, isError?: boolean) => void
  onUpdated: () => void
  open: boolean
  freeSubStatus?: FreeSubStatus | null
  onJobStarted?: (mode: FreeSubMode) => void
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

export function FreeSubPoolSubmenu({ mode, notify, onUpdated, open, freeSubStatus, onJobStarted }: Props) {
  const [pool, setPool] = useState<FreeSubPoolInfo | null>(null)
  const [loading, setLoading] = useState(false)
  const [busy, setBusy] = useState<string | null>(null)
  const prevRunningRef = useRef(false)
  const prevWorkingRef = useRef<number | undefined>(undefined)

  const loadPool = useCallback(async (silent = false) => {
    if (!silent) setLoading(true)
    try {
      const data = await apiGet<FreeSubPoolInfo>(`providers/free-sub/pool?mode=${mode}`)
      setPool(data)
    } catch (e) {
      if (!silent) {
        notify?.(`Ошибка загрузки пула: ${e instanceof Error ? e.message : String(e)}`, true)
      }
    } finally {
      if (!silent) setLoading(false)
    }
  }, [mode, notify])

  useEffect(() => {
    if (!open) return
    void loadPool(false)
    const timer = window.setInterval(() => {
      void loadPool(true)
    }, 3000)
    return () => window.clearInterval(timer)
  }, [open, loadPool])

  useEffect(() => {
    const isThisModeRunning = Boolean(freeSubStatus?.running && freeSubStatus.mode === mode)
    const curWorking = freeSubStatus?.mode === mode ? freeSubStatus.stats?.working_count : undefined
    const finished = prevRunningRef.current && !isThisModeRunning
    const countChanged =
      curWorking !== undefined && prevWorkingRef.current !== undefined && curWorking !== prevWorkingRef.current
    prevRunningRef.current = isThisModeRunning
    prevWorkingRef.current = curWorking
    if (open && (finished || countChanged)) {
      void loadPool()
    }
  }, [open, freeSubStatus, mode, loadPool])

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
          disabled={busy !== null || !pool || pool.pool_total < 2}
          title={pool && pool.pool_total < 2 ? 'В пуле меньше 2 узлов — нажмите «Добрать ещё»' : 'Предыдущий узел пула'}
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
          disabled={busy !== null || !pool || pool.pool_total < 2}
          title={pool && pool.pool_total < 2 ? 'В пуле меньше 2 узлов — нажмите «Добрать ещё»' : 'Следующий узел пула'}
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
              const res = await apiPost<{ active_node: string | null; refill_started?: boolean; message?: string }>(
                'providers/free-sub/blacklist-node',
                { mode },
              )
              if (res.refill_started) {
                onJobStarted?.(mode)
                notify?.(res.message || 'Пул исчерпан — запущен автоподбор нового рабочего узла…')
              } else {
                notify?.(
                  res.active_node
                    ? `Узел в ЧС (24ч), активен: ${res.active_node}`
                    : res.message || 'Узел добавлен в ЧС (24ч), пул пуст'
                )
              }
            })
          }
        >
          🚫 Плохой узел → следующий
        </button>

        <button
          type="button"
          className="btn sm ghost"
          data-testid={`free-sub-refill-${mode}`}
          disabled={busy !== null || Boolean(freeSubStatus?.running && freeSubStatus.mode === mode)}
          onClick={() =>
            void runAction('refill', async () => {
              await apiPost('providers/free-sub/refill', { mode })
              onJobStarted?.(mode)
              notify?.('Запущен фоновый добор узлов без обрыва сессии')
            })
          }
        >
          🔄 Добрать ещё
        </button>

        {pool && pool.pool_total === 0 && !(freeSubStatus?.running && freeSubStatus.mode === mode) && (
          <button
            type="button"
            className="btn sm primary"
            data-testid={`free-sub-connect-empty-${mode}`}
            disabled={busy !== null || Boolean(freeSubStatus?.running)}
            onClick={() =>
              void runAction('connect-empty', async () => {
                await apiPost('providers/free-sub/connect', { mode })
                onJobStarted?.(mode)
                notify?.('Запущен поиск и подключение рабочего узла…')
              })
            }
          >
            🔍 Подобрать рабочий узел
          </button>
        )}
      </div>

      {freeSubStatus?.running && freeSubStatus.mode === mode && (
        <div
          data-testid={`free-sub-submenu-progress-${mode}`}
          style={{
            fontSize: 11,
            color: '#7dd3fc',
            padding: '5px 8px',
            borderRadius: 6,
            background: 'rgba(56, 189, 248, 0.08)',
            border: '1px solid rgba(56, 189, 248, 0.25)',
            display: 'flex',
            flexDirection: 'column',
            gap: 2,
          }}
        >
          <div>⏳ {freeSubStatus.stage_label || 'Идёт добор узлов…'}</div>
          <div style={{ fontSize: 10, opacity: 0.85 }}>
            TCP: {freeSubStatus.stats.tcp_passed}/{freeSubStatus.stats.tcp_checked} · L7: {freeSubStatus.stats.l7_passed}/{freeSubStatus.stats.l7_checked} · Рабочих: {freeSubStatus.stats.working_count}
          </div>
        </div>
      )}

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
                {rec.end_reason !== 'Активен' && rec.node_name !== pool.active_node && (
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
