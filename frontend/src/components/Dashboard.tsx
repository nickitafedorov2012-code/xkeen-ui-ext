import { useCallback, useEffect, useState } from 'react'
import { apiGet, apiPost, apiPut } from '../api'
import { pingClass, type FailoverEventInfo, type StatusInfo, type GoogleGeoStatus } from '../types'

interface Props {
  status: StatusInfo | null
  notify: (msg: string, isError?: boolean) => void
  refresh?: () => void
  onSwitchTab?: (tab: 'dashboard' | 'servers' | 'devices' | 'settings' | 'help') => void
}

interface GroupedEvent {
  time: string
  message: string
  switched: boolean
  count: number
  type: 'ok' | 'warn' | 'err' | 'switch'
}

function groupEvents(rawEvents: FailoverEventInfo[]): GroupedEvent[] {
  if (!rawEvents || rawEvents.length === 0) return []

  const classify = (e: FailoverEventInfo): 'ok' | 'warn' | 'err' | 'switch' => {
    if (e.switched) return 'switch'
    const m = e.message.toLowerCase()
    if (
      m.includes('ошибка') ||
      m.includes('недоступен') ||
      m.includes('таймаут') ||
      m.includes('failed') ||
      m.includes('timeout')
    )
      return 'err'
    if (m.includes('превышен') || m.includes('высокий') || m.includes('задержк') || m.includes('резерв'))
      return 'warn'
    return 'ok'
  }

  const normMsg = (msg: string) => {
    return msg
      .replace(/пинг\s*\d+\s*мс/gi, '')
      .replace(/\(\d+\s*мс\)/gi, '')
      .replace(/\(в норме[^)]*\)/gi, '')
      .replace(/\s+/g, ' ')
      .trim()
  }

  const grouped: GroupedEvent[] = []
  for (const ev of rawEvents) {
    const t = classify(ev)
    const norm = normMsg(ev.message)
    const last = grouped[grouped.length - 1]
    if (last && normMsg(last.message) === norm && last.type === t) {
      last.count += 1
      last.time = ev.time
      last.message = ev.message
    } else {
      grouped.push({
        time: ev.time,
        message: ev.message,
        switched: ev.switched,
        count: 1,
        type: t,
      })
    }
  }
  return grouped
}

export default function Dashboard({ status, notify, refresh, onSwitchTab }: Props) {
  const [events, setEvents] = useState<FailoverEventInfo[]>([])
  const [checking, setChecking] = useState(false)
  const [togglingFailover, setTogglingFailover] = useState(false)
  // История пинга активного сервера (для sparkline): {значение, было ли измерение}
  const [pingHistory, setPingHistory] = useState<{ ms: number; ok: boolean }[]>([])

  useEffect(() => {
    const ping = status?.active_server?.ping_ms
    if (ping === undefined) return
    setPingHistory((prev) => {
      const next = [...prev, { ms: ping, ok: ping > 0 }].slice(-40)
      return next
    })
  }, [status?.active_server?.ping_ms, status?.active_server?.name])

  const [googleGeo, setGoogleGeo] = useState<GoogleGeoStatus | null>(null)
  const [googleGeoLoading, setGoogleGeoLoading] = useState(false)

  useEffect(() => {
    setGoogleGeo(null)
  }, [status?.active_server?.name])

  const runGoogleCheck = useCallback(async () => {
    setGoogleGeoLoading(true)
    try {
      const res = await apiGet<GoogleGeoStatus>('servers/google-check')
      setGoogleGeo(res)
      if (res.is_clean) {
        notify(`Google AI: Сервер чистый (${res.google_lang})`)
      } else {
        notify(`Google AI: Сервер определен как ${res.google_lang}`, true)
      }
    } catch (e: any) {
      notify(`Ошибка проверки Google: ${e.message}`, true)
    } finally {
      setGoogleGeoLoading(false)
    }
  }, [notify])

  const loadEvents = useCallback(async () => {
    try {
      const data = await apiGet<{ events: FailoverEventInfo[] }>('failover/events')
      setEvents(data.events)
    } catch {
      /* ignore */
    }
  }, [])

  useEffect(() => {
    loadEvents()
    const t = setInterval(loadEvents, 10000)
    return () => clearInterval(t)
  }, [loadEvents])

  const runCheck = async () => {
    setChecking(true)
    try {
      const data = await apiPost<{ message: string }>('failover/check')
      notify(data.message)
      loadEvents()
      refresh?.()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка проверки', true)
    } finally {
      setChecking(false)
    }
  }

  const toggleFailover = async (enabled: boolean) => {
    setTogglingFailover(true)
    try {
      await apiPut('settings', { failover: { enabled } })
      notify(enabled ? 'Failover включён' : 'Failover выключен')
      refresh?.()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка переключения failover', true)
    } finally {
      setTogglingFailover(false)
    }
  }

  const f = status?.failover
  const groupedEvents = groupEvents(events)
  const mihomoVer = status?.mihomo_version || 'v1.19.29'

  const okPings = pingHistory.filter((p) => p.ok).map((p) => p.ms)
  const minPing = okPings.length > 0 ? okPings.reduce((a, b) => Math.min(a, b)) : 0
  const maxPing = okPings.length > 0 ? okPings.reduce((a, b) => Math.max(a, b)) : 0

  return (
    <div>
      {/* 1.1 Компактная статусная полоска роутера и ядра вместо двух статичных карточек */}
      <div className="dash-info-strip">
        <div className="dash-info-group">
          <span className="dash-info-badge" title="Информация об интернет-центре Keenetic">
            <span>🌐 Роутер:</span>
            <b>{status?.router?.model || 'Keenetic'}</b>
            <span className="dash-info-sep">·</span>
            <span>KeeneticOS {status?.router?.version || '—'}</span>
            <span className="dash-info-sep">·</span>
            <span className="muted">RCI {status?.rci ? `${status.rci.host}:${status.rci.port}` : '127.0.0.1:79'}</span>
          </span>
        </div>

        <div className="dash-info-group">
          <span className="dash-info-badge" title="Mihomo core proxy engine">
            <span>⚙️ Ядро:</span>
            <b>Mihomo {mihomoVer}</b>
            <span className="dash-info-sep">·</span>
            <span className="muted">API {status?.mihomo ? `${status.mihomo.host}:${status.mihomo.port}` : '127.0.0.1:9090'}</span>
          </span>
        </div>
      </div>

      {/* Оперативные виджеты */}
      <div className="grid2">
        {/* КАРТОЧКА 1: Активный сервер */}
        <section className="card active-server-card">
          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 12 }}>
            <h2 style={{ margin: 0 }}>🛰 Активный сервер</h2>
            <span className="badge badge-online" style={{ color: '#22c55e' }}>
              🟢 В сети
            </span>
          </div>

          {status?.active_server ? (
            <div className="active-server-hero">
              {/* Верхняя идентичность: флаг, имя, протокол, подписка */}
              <div className="active-server-top">
                <div className="active-server-identity">
                  <span className="active-server-flag">
                    {getCountryFlag(status.active_server.name)}
                  </span>
                  <div className="active-server-title">
                    <span className="active-server-name">{status.active_server.name}</span>
                    <div className="active-server-badges">
                      {status.active_server.protocol && (
                        <span className="badge" style={{ fontSize: 10, padding: '1px 6px' }}>
                          {status.active_server.protocol}
                        </span>
                      )}
                      {status.active_server.provider && (
                        <span className="tag-provider" style={{ cursor: 'default', fontSize: 11, padding: '1px 7px' }}>
                          📁 {status.active_server.provider_name || status.active_server.provider}
                        </span>
                      )}
                    </div>
                  </div>
                </div>

                <div style={{ textAlign: 'right' }}>
                  <div className={'ping ' + pingClass(status.active_server.ping_ms)} style={{ fontSize: 15, fontWeight: 700 }}>
                    {status.active_server.ping_ms > 0 ? `${status.active_server.ping_ms} мс` : '—'}
                  </div>
                  <div className="muted small" style={{ fontSize: 11 }}>
                    {status.active_server.ping_ms > 0 && status.active_server.ping_ms <= 100
                      ? 'отличный отклик'
                      : status.active_server.ping_ms > 100 && status.active_server.ping_ms <= 250
                      ? 'хороший отклик'
                      : status.active_server.ping_ms > 250
                      ? 'высокая задержка'
                      : 'проверка…'}
                  </div>
                </div>
              </div>

              {/* Таблица параметров в едином стиле с соседней карточкой */}
              <ul className="kv" style={{ margin: '4px 0 8px' }}>
                <li>
                  <span>Хост и порт</span>
                  <b className="mono" style={{ fontSize: 12 }}>
                    {status.active_server.host ? `${status.active_server.host}${status.active_server.port ? `:${status.active_server.port}` : ''}` : '—'}
                  </b>
                </li>
                <li>
                  <span>Роль в Failover</span>
                  <b>
                    {f?.priority_server && f.priority_server === status.active_server.name ? (
                      <span style={{ color: '#22c55e' }}>★ Основной (приоритетный)</span>
                    ) : f?.priority_chain && f.priority_chain.indexOf(status.active_server.id) === 0 ? (
                      <span style={{ color: '#22c55e' }}>★ Основной (в цепочке)</span>
                    ) : f?.priority_chain && f.priority_chain.indexOf(status.active_server.id) > 0 ? (
                      <span style={{ color: '#eab308' }}>⚡ Резервный (РЕЗ{f.priority_chain.indexOf(status.active_server.id)})</span>
                    ) : (
                      <span style={{ color: 'var(--muted)' }}>Обычный сервер</span>
                    )}
                  </b>
                </li>
                <li>
                  <span>Google AI & Flow</span>
                  <b>
                    {googleGeoLoading ? (
                      <span className="muted" style={{ fontSize: 12 }}>⏳ Проверка…</span>
                    ) : googleGeo ? (
                      <span
                        style={{
                          cursor: 'pointer',
                          color: googleGeo.is_clean ? '#22c55e' : '#ef4444',
                          fontSize: 12,
                          fontWeight: 600,
                        }}
                        onClick={runGoogleCheck}
                        title={googleGeo.message}
                      >
                        {googleGeo.is_clean ? `🟢 Чистый (${googleGeo.google_lang})` : `🔴 Flagged RU (${googleGeo.google_lang})`}
                      </span>
                    ) : (
                      <button
                        className="btn btn-secondary btn-sm"
                        style={{ padding: '1px 8px', fontSize: 11 }}
                        onClick={runGoogleCheck}
                      >
                        Проверить гео
                      </button>
                    )}
                  </b>
                </li>
                <li>
                  <span>Режим трафика</span>
                  <b>Rule (авто-маршрутизация)</b>
                </li>
              </ul>

              {googleGeo && !googleGeo.is_clean && (
                <div style={{ margin: '6px 0 10px', padding: '8px 10px', background: 'rgba(239, 68, 68, 0.1)', border: '1px solid rgba(239, 68, 68, 0.3)', borderRadius: 6, fontSize: 12, color: '#f87171', lineHeight: 1.4 }}>
                  ⚠️ <b>Google определяет этот узел как РФ ({googleGeo.google_lang}).</b><br />
                  Доступ к Google Flow и Gemini Labs будет заблокирован. Рекомендуется переключиться на чистый узел США (например, Вашингтон).
                </div>
              )}

              {/* График стабильности задержки */}
              {pingHistory.length >= 2 && (
                <div className="active-server-sparkline-box">
                  <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 2 }}>
                    <span className="muted small" style={{ fontSize: 11 }}>Стабильность пинга ({pingHistory.length} точек):</span>
                    <span className="muted small" style={{ fontSize: 11, fontFamily: 'monospace' }}>
                      мин: {minPing} мс / макс: {maxPing} мс
                    </span>
                  </div>
                  <PingSparkline data={pingHistory} />
                </div>
              )}

              {/* Кнопки перехода и действий */}
              <div style={{ display: 'flex', gap: 8, marginTop: 4 }}>
                {onSwitchTab && (
                  <button
                    type="button"
                    className="btn ghost sm"
                    onClick={() => onSwitchTab('servers')}
                    style={{ flex: 1 }}
                    title="Выбрать другой сервер из списка"
                  >
                    🔌 Сменить сервер →
                  </button>
                )}
                <button
                  type="button"
                  className="btn ghost sm"
                  onClick={runCheck}
                  disabled={checking}
                  title="Измерить текущую задержку"
                >
                  ⚡ Проверить пинг
                </button>
              </div>
            </div>
          ) : (
            <p className="muted">Сервер не выбран или Mihomo не запущен.</p>
          )}
        </section>

        {/* КАРТОЧКА 2: Failover статус и управление */}
        <section className="card">
          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 12 }}>
            <h2 style={{ margin: 0 }}>⚡ Failover контроль</h2>
            <span className={`badge ${f?.enabled ? 'badge-online' : ''}`} style={{ color: f?.enabled ? '#22c55e' : 'var(--muted)' }}>
              {f?.enabled ? '🟢 включён' : '⚪ выключен'}
            </span>
          </div>
          <ul className="kv">
            <li>
              <span>Автоматический мониторинг</span>
              <div style={{ display: 'inline-flex', alignItems: 'center', gap: 10 }}>
                <label className="switch" title={f?.enabled ? 'Выключить failover' : 'Включить failover'}>
                  <input
                    type="checkbox"
                    checked={f?.enabled ?? false}
                    disabled={togglingFailover}
                    onChange={(e) => toggleFailover(e.target.checked)}
                  />
                  <span className="slider" />
                </label>
                <b style={{ color: f?.enabled ? '#22c55e' : 'var(--muted)', fontSize: 13 }}>
                  {togglingFailover ? 'Сохранение…' : f?.enabled ? 'Включён' : 'Выключен'}
                </b>
              </div>
            </li>
            <li><span>Порог пинга</span><b>{f ? `${f.ping_threshold_ms} мс` : '—'}</b></li>
            <li>
              <span>Приоритетный</span>
              <b>
                {f?.priority_server || 'не задан'}
                {f?.priority_chain &&
                f.priority_chain.length > 0 &&
                status?.active_server?.id &&
                f.priority_chain.indexOf(status.active_server.id) > 0 ? (
                  <span className="muted small" style={{ marginLeft: 6, fontWeight: 'normal' }}>
                    (ожидает восстановления)
                  </span>
                ) : null}
              </b>
            </li>
            {(f?.priority_chain?.length ?? 0) > 0 &&
              status?.active_server?.id &&
              f!.priority_chain!.indexOf(status.active_server.id) >= 0 && (
                <li>
                  <span>Позиция в цепочке</span>
                  <b>
                    {f!.priority_chain!.indexOf(status.active_server.id) === 0 ? (
                      <span style={{ color: '#22c55e' }}>ОСН (основной)</span>
                    ) : (
                      <span style={{ color: '#eab308' }}>
                        РЕЗ{f!.priority_chain!.indexOf(status.active_server.id)} (резервный)
                      </span>
                    )}
                  </b>
                </li>
              )}
            {(f?.priority_chain?.length ?? 0) > 1 && (
              <li><span>Резервы</span><b>{f!.priority_chain!.slice(1).length} сервер(ов)</b></li>
            )}
            <li><span>Интервал проверки</span><b>{f ? `${f.interval_secs} с` : '—'}</b></li>
          </ul>
          <div style={{ display: 'flex', gap: 8, marginTop: 12 }}>
            <button className="btn primary" onClick={runCheck} disabled={checking} style={{ flex: 1 }}>
              {checking ? 'Проверка…' : '🔍 Проверить сейчас'}
            </button>
            {onSwitchTab && (
              <button
                type="button"
                className="btn ghost"
                onClick={() => onSwitchTab('settings')}
                title="Настроить порог пинга и интервал"
              >
                ⚙️
              </button>
            )}
          </div>
        </section>
      </div>

      {/* КАРТОЧКА 3: Блок событий с группировкой и цветовой разметкой */}
      <section className="card" style={{ marginTop: 14 }}>
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 10 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <h2 style={{ margin: 0 }}>📋 Журнал событий Failover</h2>
            {groupedEvents.length > 0 && (
              <span className="badge" style={{ fontSize: 11, background: 'rgba(255,255,255,0.06)' }}>
                {groupedEvents.length} {groupedEvents.length === 1 ? 'запись' : 'записей'}
              </span>
            )}
          </div>
          {onSwitchTab && (
            <button
              type="button"
              className="btn sm ghost"
              onClick={() => onSwitchTab('settings')}
              title="Открыть системный лог и настройки"
            >
              Системный журнал →
            </button>
          )}
        </div>

        {groupedEvents.length === 0 ? (
          <div style={{ padding: '16px 20px', background: 'rgba(34, 197, 94, 0.04)', border: '1px solid rgba(34, 197, 94, 0.15)', borderRadius: 8, display: 'flex', alignItems: 'center', gap: 12 }}>
            <span style={{ fontSize: 22 }}>🟢</span>
            <div>
              <div style={{ fontWeight: 600, color: '#f8fafc', fontSize: 13 }}>Сбоев и переключений не зафиксировано</div>
              <div className="muted small">Автоматический мониторинг активен. Все серверы работают в штатном режиме.</div>
            </div>
          </div>
        ) : (
          <div className="events-container">
            {groupedEvents.map((e, i) => (
              <div key={i} className={`event-entry ev-${e.type}`}>
                <span className="ev-type-icon">
                  {e.type === 'switch' ? '🔵' : e.type === 'err' ? '🔴' : e.type === 'warn' ? '🟡' : '🟢'}
                </span>
                <span className="ev-time-pill">{e.time}</span>
                <span className="ev-msg-text" title={e.message}>{e.message}</span>
                {e.count > 1 && (
                  <span className="ev-repeat-badge" title={`Повторено ${e.count} раз(а) подряд`}>
                    ×{e.count}
                  </span>
                )}
              </div>
            ))}
          </div>
        )}
      </section>
    </div>
  )
}

function getCountryFlag(name: string): string {
  const n = name.toLowerCase()
  if (n.includes('германи') || n.includes('germany') || n.includes('de ') || n.includes('[de]')) return '🇩🇪'
  if (n.includes('финлянд') || n.includes('finland') || n.includes('fi ') || n.includes('[fi]')) return '🇫🇮'
  if (n.includes('нидерланд') || n.includes('netherlands') || n.includes('nl ') || n.includes('[nl]')) return '🇳🇱'
  if (n.includes('швеци') || n.includes('sweden') || n.includes('se ') || n.includes('[se]')) return '🇸🇪'
  if (n.includes('сша') || n.includes('usa') || n.includes('united states') || n.includes('us ') || n.includes('[us]')) return '🇺🇸'
  if (n.includes('великобритан') || n.includes('uk ') || n.includes('united kingdom') || n.includes('gb ') || n.includes('[gb]')) return '🇬🇧'
  if (n.includes('франци') || n.includes('france') || n.includes('fr ') || n.includes('[fr]')) return '🇫🇷'
  if (n.includes('польш') || n.includes('poland') || n.includes('pl ') || n.includes('[pl]')) return '🇵🇱'
  if (n.includes('эстони') || n.includes('estonia') || n.includes('ee ') || n.includes('[ee]')) return '🇪🇪'
  if (n.includes('латви') || n.includes('latvia') || n.includes('lv ') || n.includes('[lv]')) return '🇱🇻'
  if (n.includes('литв') || n.includes('lithuania') || n.includes('lt ') || n.includes('[lt]')) return '🇱🇹'
  if (n.includes('турци') || n.includes('turkey') || n.includes('tr ') || n.includes('[tr]')) return '🇹🇷'
  if (n.includes('казахстан') || n.includes('kazakhstan') || n.includes('kz ') || n.includes('[kz]')) return '🇰🇿'
  if (n.includes('япони') || n.includes('japan') || n.includes('jp ') || n.includes('[jp]')) return '🇯🇵'
  if (n.includes('сингапур') || n.includes('singapore') || n.includes('sg ') || n.includes('[sg]')) return '🇸🇬'
  if (n.includes('швейцари') || n.includes('switzerland') || n.includes('ch ') || n.includes('[ch]')) return '🇨🇭'
  if (n.includes('австри') || n.includes('austria') || n.includes('at ') || n.includes('[at]')) return '🇦🇹'
  if (n.includes('чехи') || n.includes('czech') || n.includes('cz ') || n.includes('[cz]')) return '🇨🇿'
  return '🌐'
}

/// Мини-график пинга (SVG sparkline): последняя точка справа, провалы — красные.
function PingSparkline({ data }: { data: { ms: number; ok: boolean }[] }) {
  const W = 280
  const H = 40
  const okVals = data.filter((d) => d.ok).map((d) => d.ms)
  const maxVal = okVals.length > 0 ? okVals.reduce((a, b) => Math.max(a, b)) : 100
  const minVal = okVals.length > 0 ? okVals.reduce((a, b) => Math.min(a, b)) : 0
  const max = Math.max(100, maxVal) * 1.15
  const min = Math.max(0, minVal * 0.85)
  const range = max - min || 1
  const step = data.length > 1 ? W / (data.length - 1) : W
  const y = (d: { ms: number; ok: boolean }) =>
    d.ok ? H - 4 - ((d.ms - min) / range) * (H - 8) : H - 2
  const pts = data.map((d, i) => `${(i * step).toFixed(1)},${y(d).toFixed(1)}`).join(' ')
  const areaPts = `0,${H} ` + pts + ` ${W},${H}`
  const last = data[data.length - 1]
  return (
    <svg width="100%" height={H} viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" style={{ display: 'block' }}>
      <defs>
        <linearGradient id="dashSparkGrad" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor="#22c55e" stopOpacity="0.25" />
          <stop offset="100%" stopColor="#22c55e" stopOpacity="0.0" />
        </linearGradient>
      </defs>
      <polygon points={areaPts} fill="url(#dashSparkGrad)" />
      <polyline points={pts} fill="none" stroke="#22c55e" strokeWidth="1.8" />
      {data.map((d, i) =>
        d.ok ? null : (
          <circle key={i} cx={i * step} cy={y(d)} r="2.6" fill="var(--red, #e5484d)" />
        ),
      )}
      <circle cx={(data.length - 1) * step} cy={y(last)} r="3" fill="#22c55e" />
    </svg>
  )
}
