import { useCallback, useEffect, useState } from 'react'
import { apiGet, apiPost, apiPut } from '../api'
import type { AppSettings, ServerInfo } from '../types'

interface Props {
  notify: (msg: string, isError?: boolean) => void
}

export default function Settings({ notify }: Props) {
  const [settings, setSettings] = useState<AppSettings | null>(null)
  const [servers, setServers] = useState<ServerInfo[]>([])
  const [saving, setSaving] = useState(false)
  const [directDomains, setDirectDomains] = useState('')
  const [forceDomains, setForceDomains] = useState('')
  const [savingDomains, setSavingDomains] = useState(false)

  useEffect(() => {
    apiGet<AppSettings>('settings').then(setSettings).catch((e) => notify(e instanceof Error ? e.message : 'Ошибка', true))
    apiGet<{ servers: ServerInfo[] }>('servers')
      .then((d) => setServers(d.servers))
      .catch(() => {})
    apiGet<{ direct: string[]; force: string[] }>('domains')
      .then((d) => {
        setDirectDomains(d.direct.join('\n'))
        setForceDomains(d.force.join('\n'))
      })
      .catch(() => {})
  }, [notify])

  if (!settings) return <section className="card"><p className="muted">Загрузка…</p></section>

  const patch = (fn: (s: AppSettings) => void) => {
    const copy: AppSettings = JSON.parse(JSON.stringify(settings))
    fn(copy)
    setSettings(copy)
  }

  const save = async () => {
    setSaving(true)
    try {
      await apiPut('settings', settings)
      notify('Настройки сохранены')
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
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка проверки', true)
    }
  }

  const saveDomains = async () => {
    setSavingDomains(true)
    try {
      const data = await apiPost<{ direct: number; force: number }>('domains', {
        direct: directDomains.split('\n'),
        force: forceDomains.split('\n'),
      })
      notify(`Домены сохранены: напрямую ${data.direct}, через прокси ${data.force}`)
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка сохранения доменов', true)
    } finally {
      setSavingDomains(false)
    }
  }

  // --- Сервис XKeen ---
  const [svcBusy, setSvcBusy] = useState('')
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

  // --- Бэкапы ---
  const [backups, setBackups] = useState<string[]>([])
  const [backupDir, setBackupDir] = useState('')
  const [backupBusy, setBackupBusy] = useState(false)

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
        <h2>Failover</h2>
        <label className="row">
          <input
            type="checkbox"
            checked={settings.failover.enabled}
            onChange={(e) => patch((s) => (s.failover.enabled = e.target.checked))}
          />
          Включить автоматический failover
        </label>
        <label className="row"><span>Порог пинга, мс</span>
          <input className="input" type="number" min={50} max={5000} value={settings.failover.ping_threshold_ms}
            onChange={(e) => patch((s) => (s.failover.ping_threshold_ms = Number(e.target.value) || 300))} />
        </label>
        <label className="row"><span>Приоритетный сервер</span>
          <select className="select" value={settings.failover.priority_server}
            onChange={(e) => patch((s) => (s.failover.priority_server = e.target.value))}>
            <option value="">— не задан —</option>
            {servers.map((s) => <option key={s.id} value={s.id}>{s.name}</option>)}
          </select>
        </label>
        <label className="row">
          <input type="checkbox" checked={settings.failover.auto_restore_priority}
            onChange={(e) => patch((s) => (s.failover.auto_restore_priority = e.target.checked))} />
          Возвращаться на приоритетный при восстановлении
        </label>
        <label className="row"><span>Интервал проверки, сек</span>
          <input className="input" type="number" min={15} max={3600} value={settings.failover.interval_secs}
            onChange={(e) => patch((s) => (s.failover.interval_secs = Number(e.target.value) || 60))} />
        </label>
        <button className="btn" onClick={testCheck}>🔍 Тестовая проверка сейчас</button>
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
        <button className="btn primary" onClick={save} disabled={saving}>
          {saving ? 'Сохранение…' : '💾 Сохранить настройки'}
        </button>
        <p className="muted small">Конфиг хранится в /opt/etc/xkeen-route/config.json (путь — на дашборде).</p>
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
        <p className="muted small">По одному домену в строке. Правила вставляются в начало rules: (DOMAIN-SUFFIX) и имеют приоритет. Применяется сразу при сохранении.</p>
        <label className="row" style={{ flexDirection: 'column', alignItems: 'stretch' }}>
          <span>⏭ Напрямую (мимо прокси → DIRECT)</span>
          <textarea
            className="input"
            rows={7}
            placeholder={'example.com\nlocal-service.net\nw3.org'}
            value={directDomains}
            onChange={(e) => setDirectDomains(e.target.value)}
            style={{ fontFamily: 'Consolas, monospace', fontSize: 12.5, resize: 'vertical' }}
          />
        </label>
        <label className="row" style={{ flexDirection: 'column', alignItems: 'stretch' }}>
          <span>🔒 Принудительно через прокси (→ PROXY)</span>
          <textarea
            className="input"
            rows={7}
            placeholder={'openai.com\nyoutube.com\ngithub.com'}
            value={forceDomains}
            onChange={(e) => setForceDomains(e.target.value)}
            style={{ fontFamily: 'Consolas, monospace', fontSize: 12.5, resize: 'vertical' }}
          />
        </label>
        <button className="btn primary" onClick={saveDomains} disabled={savingDomains}>
          {savingDomains ? 'Применение…' : '🌐 Применить домены'}
        </button>
      </section>
    </div>
  )
}

