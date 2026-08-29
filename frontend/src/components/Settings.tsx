import { useEffect, useState } from 'react'
import { apiGet, apiPost, apiPut } from '../api'
import type { AppSettings, ServerInfo } from '../types'

interface Props {
  notify: (msg: string, isError?: boolean) => void
}

export default function Settings({ notify }: Props) {
  const [settings, setSettings] = useState<AppSettings | null>(null)
  const [servers, setServers] = useState<ServerInfo[]>([])
  const [saving, setSaving] = useState(false)

  useEffect(() => {
    apiGet<AppSettings>('settings').then(setSettings).catch((e) => notify(e instanceof Error ? e.message : 'Ошибка', true))
    apiGet<{ servers: ServerInfo[] }>('servers')
      .then((d) => setServers(d.servers))
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
    </div>
  )
}

