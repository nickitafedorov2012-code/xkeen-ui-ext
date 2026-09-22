import { useCallback, useEffect, useState } from 'react'
import { apiGet, apiPost, apiPut } from '../api'
import { pingClass, type AppSettings, type ServerInfo, type StatusInfo } from '../types'

interface Props {
  notify: (msg: string, isError?: boolean) => void
  status?: StatusInfo | null
  refresh?: () => void
}

export default function Settings({ notify, status, refresh }: Props) {
  const [settings, setSettings] = useState<AppSettings | null>(null)
  const [servers, setServers] = useState<ServerInfo[]>([])
  const [saving, setSaving] = useState(false)
  const [directDomains, setDirectDomains] = useState('')
  const [forceDomains, setForceDomains] = useState('')
  const [autoCdns, setAutoCdns] = useState<string[]>([])
  const [savingDomains, setSavingDomains] = useState(false)
  // --- Сервис XKeen ---
  const [svcBusy, setSvcBusy] = useState('')
  // --- Бэкапы ---
  const [backups, setBackups] = useState<string[]>([])
  const [backupDir, setBackupDir] = useState('')
  const [backupBusy, setBackupBusy] = useState(false)
  // --- Журнал ---
  const [logText, setLogText] = useState('')
  const [logPath, setLogPath] = useState('')
  const [logsBusy, setLogsBusy] = useState(false)
  const [logsAuto, setLogsAuto] = useState(false)
  const [logsLive, setLogsLive] = useState(false)
  // --- Глобальная цепочка приоритетов ---
  const [chainBusy, setChainBusy] = useState(false)
  // --- Обновление панели ---
  interface UpdateInfo { current: string; latest: string; update_available: boolean; notes: string[] }
  const [upd, setUpd] = useState<UpdateInfo | null>(null)
  const [updBusy, setUpdBusy] = useState(false)
  const [updStage, setUpdStage] = useState('')
  const [updError, setUpdError] = useState('')
  const [updChecking, setUpdChecking] = useState(false)
  const [showUpdNotes, setShowUpdNotes] = useState(false)

  useEffect(() => {
    apiGet<AppSettings>('settings').then(setSettings).catch((e) => notify(e instanceof Error ? e.message : 'Ошибка', true))
    apiGet<{ servers: ServerInfo[] }>('servers')
      .then((d) => setServers(d.servers))
      .catch(() => {})
    apiGet<{ direct: string[]; force: string[]; auto_cdns?: string[] }>('domains')
      .then((d) => {
        setDirectDomains(d.direct.join('\n'))
        setForceDomains(d.force.join('\n'))
        if (d.auto_cdns) setAutoCdns(d.auto_cdns)
      })
      .catch(() => {})
  }, [notify])

  const loadBackups = useCallback(async () => {
    try {
      const d = await apiGet<{ backups: { name: string }[]; dir: string }>('backups')
      setBackups(d.backups.map((b) => b.name))
      setBackupDir(d.dir)
    } catch {
      /* бэкапы не критичны для загрузки страницы */
    }
  }, [])

  useEffect(() => {
    loadBackups()
  }, [loadBackups])

  const loadLogs = useCallback(async () => {
    try {
      const d = await apiGet<{ text: string; path: string }>('logs?lines=800')
      setLogText(d.text)
      setLogPath(d.path)
    } catch {
      /* журнал не критичен */
    }
  }, [])

  useEffect(() => {
    loadLogs()
  }, [loadLogs])

  // Проверка новой версии панели.
  const checkUpdate = useCallback(async () => {
    setUpdChecking(true)
    setUpdError('')
    try {
      const data = await apiGet<UpdateInfo>('update/check')
      setUpd(data)
    } catch (e) {
      setUpdError(e instanceof Error ? e.message : 'Ошибка проверки')
    } finally {
      setUpdChecking(false)
    }
  }, [])

  useEffect(() => {
    checkUpdate()
  }, [checkUpdate])

  const doUpdate = async () => {
    if (!confirm(`Установить обновление ${upd?.latest}? Панель перезапустится автоматически.`)) return
    setUpdBusy(true)
    setUpdStage('Загрузка и установка…')
    try {
      await apiPost<{ installed: string; restarting: boolean }>('update/install')
      setUpdStage(`Установлена ${upd?.latest}. Перезапуск панели…`)
      setTimeout(() => location.reload(), 8000)
    } catch (e) {
      setUpdBusy(false)
      setUpdStage('')
      notify(e instanceof Error ? e.message : 'Ошибка обновления', true)
    }
  }

  useEffect(() => {
    if (!logsAuto) return
    const t = setInterval(loadLogs, 5000)
    return () => clearInterval(t)
  }, [logsAuto, loadLogs])

  // Живой режим: WebSocket-поток новых строк журнала.
  useEffect(() => {
    if (!logsLive) return
    let ws: WebSocket | null = null
    let closed = false
    let retry: ReturnType<typeof setTimeout>
    const connect = () => {
      if (closed) return
      const proto = location.protocol === 'https:' ? 'wss' : 'ws'
      ws = new WebSocket(`${proto}://${location.host}/api/logs/ws?lines=300`)
      ws.onmessage = (ev) => {
        setLogText((prev) => {
          const lines = (prev ? prev.split('\n') : []).concat(ev.data as string)
          return lines.slice(-500).join('\n')
        })
      }
      ws.onclose = () => {
        if (!closed) retry = setTimeout(connect, 3000)
      }
    }
    connect()
    return () => {
      closed = true
      clearTimeout(retry)
      ws?.close()
    }
  }, [logsLive])

  if (!settings) return <section className="card"><p className="muted">Загрузка…</p></section>

  const patch = (fn: (s: AppSettings) => void) => {
    const copy: AppSettings = JSON.parse(JSON.stringify(settings))
    fn(copy)
    setSettings(copy)
  }

  const toggleFailoverEnabled = async (enabled: boolean) => {
    patch((s) => (s.failover.enabled = enabled))
    try {
      await apiPut('settings', { failover: { enabled } })
      notify(enabled ? 'Failover включён' : 'Failover выключен')
      refresh?.()
    } catch (e) {
      patch((s) => (s.failover.enabled = !enabled))
      notify(e instanceof Error ? e.message : 'Ошибка переключения failover', true)
    }
  }

  const save = async () => {
    setSaving(true)
    try {
      await apiPut('settings', settings)
      notify('Настройки сохранены')
      refresh?.()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка сохранения', true)
    } finally {
      setSaving(false)
    }
  }

  const testCheck = async () => {
    try {
      const data = await apiPost<{ message: string }>('failover/check')
      notify(data.message)
      refresh?.()
      apiGet<{ servers: ServerInfo[] }>('servers').then((d) => setServers(d.servers)).catch(() => {})
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка проверки', true)
    }
  }

  // --- Глобальная цепочка приоритетов ---
  const chain = settings?.failover.priority_chain ?? (settings?.failover.priority_server ? [settings.failover.priority_server] : [])

  const chainMove = (idx: number, dir: -1 | 1) => {
    const next = [...chain]
    const j = idx + dir
    if (j < 0 || j >= next.length) return
    ;[next[idx], next[j]] = [next[j], next[idx]]
    patch((s) => (s.failover.priority_chain = next))
  }

  const saveFailover = async () => {
    if (!settings) return
    setChainBusy(true)
    try {
      const data = await apiPost<{ message?: string }>('settings/priority', {
        server_ids: chain,
        enabled: settings.failover.enabled,
      })
      await apiPut('settings', {
        failover: {
          enabled: settings.failover.enabled,
          ping_threshold_ms: settings.failover.ping_threshold_ms,
          auto_restore_priority: settings.failover.auto_restore_priority,
          interval_secs: settings.failover.interval_secs,
          priority_chain: chain,
        },
      })
      notify(data.message || 'Настройки Failover сохранены')
      refresh?.()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка сохранения настроек Failover', true)
    } finally {
      setChainBusy(false)
    }
  }

  const addPreset = (domain: string) => {
    const list = forceDomains.split('\n').map((s) => s.trim()).filter(Boolean)
    if (!list.includes(domain)) {
      list.push(domain)
      setForceDomains(list.join('\n'))
      notify(`Добавлен пресет: ${domain}`)
    }
  }

  const saveDomains = async () => {
    setSavingDomains(true)
    try {
      const data = await apiPost<{
        direct: number
        force: number
        auto_cdns?: string[]
        overridden_ips?: number
      }>('domains', {
        direct: directDomains.split('\n'),
        force: forceDomains.split('\n'),
      })
      if (data.auto_cdns) setAutoCdns(data.auto_cdns)
      const cdnMsg = data.auto_cdns && data.auto_cdns.length > 0 ? `, CDN: +${data.auto_cdns.length}` : ''
      const ipMsg = data.overridden_ips ? `, IP: ${data.overridden_ips}` : ''
      notify(`Домены сохранены: напрямую ${data.direct}, через прокси ${data.force}${cdnMsg}${ipMsg}`)
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка сохранения доменов', true)
    } finally {
      setSavingDomains(false)
    }
  }

  // --- Сервис XKeen ---
  const svc = async (action: string) => {
    setSvcBusy(action)
    try {
      const data = await apiPost<{ stdout: string; stderr: string }>('xkeen/service', { action })
      const out = (data.stdout || data.stderr || '').trim()
      notify(`XKeen ${action}: ${out || 'готово'}`)
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка', true)
    } finally {
      setSvcBusy('')
    }
  }

  const clearLogs = async () => {
    setLogsBusy(true)
    try {
      await apiPost('logs/clear')
      setLogText('')
      notify('Журнал очищен')
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка', true)
    } finally {
      setLogsBusy(false)
    }
  }

  const createBackup = async () => {
    setBackupBusy(true)
    try {
      const d = await apiPost<{ name: string }>('backups', {})
      notify(`Бэкап создан: ${d.name}`)
      loadBackups()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка', true)
    } finally {
      setBackupBusy(false)
    }
  }

  const restoreBackup = async (name: string) => {
    if (!confirm(`Восстановить конфиги из ${name}? Текущие config.yaml и config.json будут перезаписаны.`)) return
    setBackupBusy(true)
    try {
      await apiPost('backups/restore', { name })
      notify(`Восстановлено из ${name}`)
      loadBackups()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка', true)
    } finally {
      setBackupBusy(false)
    }
  }

  const deleteBackup = async (name: string) => {
    if (!confirm(`Удалить бэкап ${name}?`)) return
    setBackupBusy(true)
    try {
      await apiPost('backups/delete', { name })
      notify(`Бэкап ${name} удалён`)
      loadBackups()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка', true)
    } finally {
      setBackupBusy(false)
    }
  }

  return (
    <div className="grid2">
      <section className="card">
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 12 }}>
          <h2 style={{ margin: 0 }}>Failover</h2>
          <span className={`badge ${settings.failover.enabled ? 'badge-online' : ''}`}>
            {settings.failover.enabled ? '🟢 включён' : '⚪ выключен'}
          </span>
        </div>
        <label className="row" style={{ justifyContent: 'flex-start', gap: 8, cursor: 'pointer' }}>
          <input
            type="checkbox"
            checked={settings.failover.enabled}
            onChange={(e) => toggleFailoverEnabled(e.target.checked)}
          />
          <b>Включить автоматический failover</b>
        </label>
        <label className="row"><span>Порог пинга, мс</span>
          <input className="input" type="number" min={50} max={5000} value={settings.failover.ping_threshold_ms}
            onChange={(e) => patch((s) => (s.failover.ping_threshold_ms = Number(e.target.value) || 300))} />
        </label>
        <div className="row" style={{ flexDirection: 'column', alignItems: 'stretch', gap: 6 }}>
          <span>Цепочка приоритетов (первый — основной)</span>
          <div className="modal-list" style={{ maxHeight: 260 }}>
            {chain.length === 0 && <p className="muted small" style={{ margin: 0 }}>Цепочка не задана — failover выбирает лучший доступный сервер.</p>}
            {chain.map((id, i) => {
              const sv = servers.find((x) => x.id === id)
              return (
                <div key={id} className="check-row" style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
                  <span className="badge">{i === 0 ? 'ОСН' : `РЕЗ${i}`}</span>
                  <span className="server-name" style={{ flex: 1 }} title={id}>{sv ? sv.name : `${id} (сейчас недоступен)`}</span>
                  {sv && (
                    <span className={'ping ' + pingClass(sv.ping_ms)}>
                      {sv.ping_ms > 0 ? `${sv.ping_ms} мс` : '—'}
                    </span>
                  )}
                  <button className="btn sm ghost" disabled={i === 0} onClick={() => chainMove(i, -1)}>↑</button>
                  <button className="btn sm ghost" disabled={i === chain.length - 1} onClick={() => chainMove(i, 1)}>↓</button>
                  <button
                    className="btn sm ghost"
                    onClick={() => patch((s) => (s.failover.priority_chain = chain.filter((x) => x !== id)))}
                  >
                    ✕
                  </button>
                </div>
              )
            })}
            <select
              className="select"
              value=""
              onChange={(e) => {
                if (!e.target.value) return
                if (!chain.includes(e.target.value)) {
                  patch((s) => (s.failover.priority_chain = [...chain, e.target.value]))
                }
              }}
            >
              <option value="">+ добавить сервер в цепочку…</option>
              {servers
                .filter((s) => !chain.includes(s.id))
                .map((s) => (
                  <option key={s.id} value={s.id}>
                    {s.name} · {s.ping_ms > 0 ? `${s.ping_ms} мс` : '—'}
                  </option>
                ))}
            </select>
          </div>
          {chain.length > 0 && (
            <div style={{ display: 'flex', justifyContent: 'flex-end', marginTop: 6 }}>
              <button className="btn sm ghost" onClick={() => patch((s) => (s.failover.priority_chain = []))}>
                Очистить цепочку
              </button>
            </div>
          )}
        </div>
        <label className="row" style={{ justifyContent: 'flex-start', gap: 8, cursor: 'pointer' }}>
          <input type="checkbox" checked={settings.failover.auto_restore_priority}
            onChange={(e) => patch((s) => (s.failover.auto_restore_priority = e.target.checked))} />
          <span>Возвращаться на приоритетный при восстановлении</span>
        </label>
        <label className="row"><span>Интервал проверки, сек</span>
          <input className="input" type="number" min={15} max={3600} value={settings.failover.interval_secs}
            onChange={(e) => patch((s) => (s.failover.interval_secs = Number(e.target.value) || 60))} />
        </label>
        <div style={{ display: 'flex', gap: 8, justifyContent: 'space-between', alignItems: 'center', marginTop: 10, flexWrap: 'wrap' }}>
          <button className="btn" onClick={testCheck}>🔍 Тестовая проверка сейчас</button>
          <button className="btn primary" disabled={chainBusy} onClick={saveFailover}>
            {chainBusy ? 'Сохранение…' : '💾 Сохранить настройки Failover'}
          </button>
        </div>
      </section>

      <section className="card">
        <h2>RCI (Keenetic)</h2>
        <label className="row"><span>Host</span>
          <input className="input" value={settings.rci.host} onChange={(e) => patch((s) => (s.rci.host = e.target.value))} />
        </label>
        <label className="row"><span>Порт</span>
          <input className="input" type="number" value={settings.rci.port} onChange={(e) => patch((s) => (s.rci.port = Number(e.target.value) || 79))} />
        </label>
        <label className="row"><span>Логин</span>
          <input className="input" value={settings.rci.login} onChange={(e) => patch((s) => (s.rci.login = e.target.value))} />
        </label>
        <label className="row"><span>Пароль (fallback)</span>
          <input className="input" type="password" value={settings.rci.password} onChange={(e) => patch((s) => (s.rci.password = e.target.value))} />
        </label>
        <label className="row"><span>Токен (X-Ndma-Tkn)</span>
          <input className="input" value={settings.rci.token} placeholder="пусто = из /opt/etc/xkeen/xkeen.json"
            onChange={(e) => patch((s) => (s.rci.token = e.target.value))} />
        </label>
        <p className="muted small">Если токен не задан, панель возьмёт его из /opt/etc/xkeen/xkeen.json; иначе — challenge-auth.</p>
      </section>

      <section className="card">
        <h2>Mihomo</h2>
        <label className="row"><span>Host</span>
          <input className="input" value={settings.mihomo.host} onChange={(e) => patch((s) => (s.mihomo.host = e.target.value))} />
        </label>
        <label className="row"><span>Порт</span>
          <input className="input" type="number" value={settings.mihomo.port} onChange={(e) => patch((s) => (s.mihomo.port = Number(e.target.value) || 9090))} />
        </label>
        <label className="row"><span>Secret</span>
          <input className="input" value={settings.mihomo.secret} onChange={(e) => patch((s) => (s.mihomo.secret = e.target.value))} />
        </label>
        <label className="row"><span>Путь к config.yaml</span>
          <input className="input" value={settings.mihomo.config_path} onChange={(e) => patch((s) => (s.mihomo.config_path = e.target.value))} />
        </label>
        <label className="row"><span>Провайдеры групп устройств</span>
          <input
            className="input"
            placeholder="пусто = авто из config.yaml"
            value={(settings.mihomo.device_providers ?? []).join(', ')}
            onChange={(e) => patch((s) => (s.mihomo.device_providers = e.target.value.split(',').map((x) => x.trim()).filter(Boolean)))}
          />
        </label>
        <p className="muted small">Имена proxy-providers, подключаемые к per-device группам (use:). Пусто — берутся все из config.yaml автоматически.</p>
      </section>

      <section className="card">
        <h2>Панель</h2>
        <label className="row"><span>Интервал автообновления, сек</span>
          <input className="input" type="number" min={3} max={300} value={settings.refresh_interval_sec}
            onChange={(e) => patch((s) => (s.refresh_interval_sec = Number(e.target.value) || 10))} />
        </label>
        <label className="row"><span>Уровень логов</span>
          <select className="select" value={settings.logs?.level ?? 'info'}
            onChange={(e) => patch((s) => { s.logs.level = e.target.value })}>
            <option value="info">info (подробно)</option>
            <option value="warn">warn (предупреждения и ошибки)</option>
            <option value="error">error (только ошибки)</option>
          </select>
        </label>
        <label className="row" style={{ justifyContent: 'flex-start', gap: 8, cursor: 'pointer' }}>
          <input type="checkbox" checked={settings.logs?.log_requests ?? true}
            onChange={(e) => patch((s) => (s.logs.log_requests = e.target.checked))} />
          <span>Логировать HTTP-запросы к панели</span>
        </label>
        <label className="row"><span>Удалённый syslog (host:port, UDP)</span>
          <input className="input" placeholder="пусто = не отправлять, напр. 192.168.2.10:514"
            value={settings.logs?.remote_syslog ?? ''}
            onChange={(e) => patch((s) => (s.logs.remote_syslog = e.target.value))} />
        </label>
        <p className="muted small">Syslog начнёт работать после перезапуска панели (restart в разделе «Сервис XKeen» не нужен — перезапуск S99xkeen-route).</p>
        <button className="btn primary" onClick={save} disabled={saving}>
          {saving ? 'Сохранение…' : '💾 Сохранить настройки'}
        </button>
        <p className="muted small">Конфиг хранится в /opt/etc/xkeen-route/config.json (путь — на дашборде).</p>

        <div style={{ marginTop: 14, borderTop: '1px solid var(--border)', paddingTop: 12 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
            <span className="muted small">
              Версия панели: <b>{upd?.current || status?.version || '…'}</b>
            </span>

            {updChecking && <span className="muted small">⏳ Проверка…</span>}

            {!updChecking && upd && !upd.update_available && (
              <span className="muted small">✓ актуальная</span>
            )}

            {!updChecking && upd?.update_available && (
              <>
                <span className="upd-dot" title="Доступна новая версия" />
                <button
                  className="btn upd-glow"
                  disabled={updBusy}
                  onClick={() => setShowUpdNotes((prev) => !prev)}
                >
                  ⬆ Доступна {upd.latest} — {showUpdNotes ? 'скрыть' : 'что нового'}
                </button>
              </>
            )}

            {!updChecking && updError && (
              <span className="small" style={{ color: 'var(--red, #e53935)' }}>
                ⚠ {updError}
              </span>
            )}

            <button
              className="btn sm ghost"
              disabled={updChecking || updBusy}
              onClick={checkUpdate}
              title="Проверить наличие обновлений"
            >
              🔄 Проверить
            </button>
          </div>

          {upd?.update_available && upd.notes.length > 0 && showUpdNotes && (
            <div
              style={{
                margin: '10px 0 0',
                padding: '10px 12px',
                background: 'var(--bg-elem, rgba(255,255,255,.04))',
                borderRadius: 8,
              }}
            >
              <p className="small" style={{ margin: '0 0 6px' }}>
                <b>Что нового в {upd.latest}:</b>
              </p>
              <ul className="small" style={{ margin: 0, paddingLeft: 18 }}>
                {upd.notes.map((n, i) => (
                  <li key={i} style={{ marginBottom: 4 }}>
                    {n}
                  </li>
                ))}
              </ul>
              <button
                className="btn primary upd-glow"
                style={{ marginTop: 10 }}
                disabled={updBusy}
                onClick={doUpdate}
              >
                ⬆ Обновить до {upd.latest}
              </button>
            </div>
          )}
          {updStage && <p className="small" style={{ margin: '8px 0 0' }}>⏳ {updStage}</p>}
        </div>
      </section>

      <section className="card">
        <h2>🖥 Сервис XKeen</h2>
        <p className="muted small">Restart перегенерирует config.yaml — настройки маршрутизации возвращаются к исходным (до любых изменений из панели).</p>
        <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
          <button className="btn" disabled={svcBusy !== ''} onClick={() => svc('status')}>📊 Статус</button>
          <button className="btn" disabled={svcBusy !== ''} onClick={() => svc('start')}>▶ Старт</button>
          <button className="btn" disabled={svcBusy !== ''} onClick={() => svc('restart')}>🔄 Рестарт</button>
          <button className="btn" style={{ borderColor: 'var(--red)', color: 'var(--red)' }} disabled={svcBusy !== ''} onClick={() => { if (confirm('Остановить сервис XKeen? Интернет через прокси пропадёт.')) svc('stop') }}>⏹ Стоп</button>
        </div>
        <label className="row" style={{ marginTop: 10 }}><span>Init-скрипт XKeen</span>
          <input className="input" value={settings.system?.xkeen_init ?? '/opt/etc/init.d/S05xkeen'}
            onChange={(e) => patch((s) => (s.system.xkeen_init = e.target.value))} />
        </label>
      </section>

      <section className="card">
        <h2>💾 Бэкапы</h2>
        <p className="muted small">Снимок config.yaml (Mihomo) + config.json (панель). Каталог: {backupDir || '…'}</p>
        <button className="btn primary" onClick={createBackup} disabled={backupBusy}>
          {backupBusy ? 'Работаю…' : '＋ Создать бэкап'}
        </button>
        <div className="modal-list" style={{ marginTop: 10 }}>
          {backups.length === 0 && <p className="muted small">Бэкапов пока нет.</p>}
          {backups.map((b) => (
            <div key={b} className="check-row" style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
              <span className="server-name" style={{ flex: 1 }}>{b}</span>
              <button className="btn sm" disabled={backupBusy} onClick={() => restoreBackup(b)}>Восстановить</button>
              <button className="btn sm ghost" disabled={backupBusy} onClick={() => deleteBackup(b)}>✕</button>
            </div>
          ))}
        </div>
        <label className="row" style={{ marginTop: 10 }}><span>Каталог бэкапов</span>
          <input className="input" value={settings.system?.backup_dir ?? ''}
            onChange={(e) => patch((s) => (s.system.backup_dir = e.target.value))} />
        </label>
      </section>

      <section className="card">
        <h2>🌐 Домены</h2>
        <p className="muted small">По одному домену в строке. Правила вставляются в начало rules: (DOMAIN-SUFFIX) и имеют приоритет. Сопутствующие CDN и медиа-сервера подтягиваются автоматически.</p>
        <label className="row" style={{ flexDirection: 'column', alignItems: 'stretch' }}>
          <span>⏭ Напрямую (мимо прокси → DIRECT)</span>
          <textarea
            className="input"
            rows={6}
            placeholder={'example.com\nlocal-service.net\nw3.org'}
            value={directDomains}
            onChange={(e) => setDirectDomains(e.target.value)}
            style={{ fontFamily: 'Consolas, monospace', fontSize: 12.5, resize: 'vertical' }}
          />
        </label>
        <div style={{ marginTop: 8 }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 4, flexWrap: 'wrap', gap: 4 }}>
            <span style={{ fontWeight: 500 }}>🔒 Принудительно через прокси (→ PROXY)</span>
            <div style={{ display: 'flex', gap: 4, flexWrap: 'wrap' }}>
              <span className="muted small" style={{ alignSelf: 'center', marginRight: 2 }}>Пресеты:</span>
              <button type="button" className="btn small ghost" onClick={() => addPreset('mysku.club')}>+ Муська</button>
              <button type="button" className="btn small ghost" onClick={() => addPreset('habr.com')}>+ Хабр</button>
              <button type="button" className="btn small ghost" onClick={() => addPreset('rutracker.org')}>+ Rutracker</button>
              <button type="button" className="btn small ghost" onClick={() => addPreset('x.com')}>+ X/Twitter</button>
              <button type="button" className="btn small ghost" onClick={() => addPreset('instagram.com')}>+ Instagram</button>
            </div>
          </div>
          <textarea
            className="input"
            rows={6}
            placeholder={'openai.com\nyoutube.com\ngithub.com'}
            value={forceDomains}
            onChange={(e) => setForceDomains(e.target.value)}
            style={{ width: '100%', boxSizing: 'border-box', fontFamily: 'Consolas, monospace', fontSize: 12.5, resize: 'vertical' }}
          />
          {autoCdns.length > 0 && (
            <div style={{ marginTop: 8, padding: '8px 10px', background: 'rgba(56, 189, 248, 0.08)', border: '1px solid rgba(56, 189, 248, 0.2)', borderRadius: 6, fontSize: 12 }}>
              <div style={{ color: '#38bdf8', fontWeight: 600, marginBottom: 4 }}>
                ⚡ Автоматически подключенные CDN и медиа-сервера ({autoCdns.length}):
              </div>
              <div style={{ display: 'flex', gap: 4, flexWrap: 'wrap' }}>
                {autoCdns.map((cdn) => (
                  <span key={cdn} style={{ fontSize: 11, background: 'rgba(56, 189, 248, 0.15)', color: '#38bdf8', padding: '1px 6px', borderRadius: 4, fontFamily: 'monospace' }}>
                    {cdn}
                  </span>
                ))}
              </div>
            </div>
          )}
        </div>
        <button className="btn primary" style={{ marginTop: 12 }} onClick={saveDomains} disabled={savingDomains}>
          {savingDomains ? 'Применение и поиск CDN…' : '🌐 Применить домены'}
        </button>
      </section>

      <section className="card" style={{ gridColumn: '1 / -1' }}>
        <h2>📄 Журнал (логи)</h2>
        <p className="muted small">Файл: {logPath || '…'} · ротация при 2 МБ (старая копия — .log.old)</p>
        <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap', alignItems: 'center', marginBottom: 10 }}>
          <button className="btn" onClick={loadLogs} disabled={logsBusy}>🔄 Обновить</button>
          <a className="btn" href="/api/logs/download" download>⬇ Скачать</a>
          <button
            className="btn ghost"
            style={{ borderColor: 'var(--red)', color: 'var(--red)' }}
            onClick={() => { if (confirm('Очистить журнал?')) clearLogs() }}
            disabled={logsBusy}
          >🗑 Очистить</button>
          <label className="check" style={{ marginLeft: 'auto' }}>
            <input type="checkbox" checked={logsLive} onChange={(e) => { setLogsLive(e.target.checked); if (e.target.checked) setLogsAuto(false) }} />
            🔴 live (WebSocket)
          </label>
          <label className="check">
            <input type="checkbox" checked={logsAuto} onChange={(e) => { setLogsAuto(e.target.checked); if (e.target.checked) setLogsLive(false) }} />
            автообновление 5 сек
          </label>
        </div>
        <textarea
          className="input"
          rows={16}
          readOnly
          value={logText}
          placeholder="Журнал пуст"
          style={{ width: '100%', boxSizing: 'border-box', fontFamily: 'Consolas, monospace', fontSize: 12, whiteSpace: 'pre', overflow: 'auto' }}
        />
      </section>
    </div>
  )
}

