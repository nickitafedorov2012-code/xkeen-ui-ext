import type { ChangeEvent, RefObject } from 'react'
import type { AppSettings, DnsEnhancedMode, StatusInfo } from '../../types'
import NumberInput from '../NumberInput'

export interface GeoInfo {
  geoip: { size: number; updated_at: string }
  geosite: { size: number; updated_at: string }
}

interface SystemSettingsCardsProps {
  settings: AppSettings
  status?: StatusInfo | null
  saving: boolean
  patch: (fn: (s: AppSettings) => void) => void
  save: () => void
  adblockEnabled: boolean
  adblockBusy: boolean
  handleToggleAdblock: (enabled: boolean) => void
  svc: (action: string) => void
  svcBusy: string
  dnsMode: DnsEnhancedMode
  dnsModeBusy: boolean
  handleSetDnsMode: (mode: DnsEnhancedMode) => void
  geoInfo: GeoInfo | null
  geoUpdating: boolean
  handleUpdateGeo: () => void
  backups: string[]
  backupDir: string
  backupBusy: boolean
  backupFileRef: RefObject<HTMLInputElement>
  createBackup: () => void
  handleImportBackup: (e: ChangeEvent<HTMLInputElement>) => void
  restoreBackup: (name: string) => void
  deleteBackup: (name: string) => void
}

export function SystemSettingsCards({
  settings,
  status,
  saving,
  patch,
  save,
  adblockEnabled,
  adblockBusy,
  handleToggleAdblock,
  svc,
  svcBusy,
  dnsMode,
  dnsModeBusy,
  handleSetDnsMode,
  geoInfo,
  geoUpdating,
  handleUpdateGeo,
  backups,
  backupDir,
  backupBusy,
  backupFileRef,
  createBackup,
  handleImportBackup,
  restoreBackup,
  deleteBackup,
}: SystemSettingsCardsProps) {
  return (
    <>
          {/* ADBLOCK */}
          <section className="card">
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 8 }}>
              <h2 style={{ margin: 0 }}>🛡️ Блокировка рекламы (AdBlock)</h2>
              <span className={`badge ${adblockEnabled ? 'badge-online' : ''}`} style={{ color: adblockEnabled ? '#22c55e' : 'var(--muted)' }}>
                {adblockEnabled ? '🟢 Активно на роутере' : '⚪ Отключено'}
              </span>
            </div>
            <p className="muted small">
              Блокирует рекламные баннеры, видеовставки, счетчики трекеров и аналитику для всех устройств в сети без установки расширений в браузеры (правило <code>category-ads-all</code> из базы GeoSite).
            </p>
            <div className="row" style={{ justifyContent: 'space-between', alignItems: 'center' }}>
              <span>Включить AdBlock для всех устройств</span>
              <div style={{ display: 'inline-flex', alignItems: 'center', gap: 10 }}>
                <label className="switch">
                  <input
                    type="checkbox"
                    checked={adblockEnabled}
                    disabled={adblockBusy}
                    onChange={(e) => handleToggleAdblock(e.target.checked)}
                  />
                  <span className="slider" />
                </label>
                <span style={{ fontSize: 13, minWidth: 64, color: adblockEnabled ? 'var(--green)' : 'var(--muted)' }}>
                  {adblockBusy ? '⏳…' : adblockEnabled ? 'Да' : 'Нет'}
                </span>
              </div>
            </div>
          </section>

          {/* RCI (KEENETIC) */}
          <section className="card">
            <h2>🔌 RCI (Keenetic)</h2>
            <label className="row"><span>Host</span>
              <input className="input" value={settings.rci.host} onChange={(e) => patch((s) => (s.rci.host = e.target.value))} />
            </label>
            <label className="row"><span>Порт</span>
              <NumberInput
                min={1}
                max={65535}
                fallback={79}
                value={settings.rci.port}
                onChange={(val) => patch((s) => (s.rci.port = val))}
              />
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

          {/* СЕРВИС XKEEN */}
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

          {/* DNS РЕЖИМ (MIHOMO) */}
          <section className="card">
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 8 }}>
              <h2 style={{ margin: 0 }}>🧭 DNS Режим (Mihomo)</h2>
              <span className="badge" style={{ textTransform: 'uppercase' }}>{dnsMode}</span>
            </div>
            <p className="muted small">Режим обработки DNS-запросов ядром. Изменение режима перезапускает службу DNS.</p>
            <div className="tile-options-grid" style={{ gridTemplateColumns: 'repeat(auto-fit, minmax(220px, 1fr))' }}>
              <label className={`option-tile-card ${dnsMode === 'fake-ip' ? 'active' : ''}`}>
                <div className="option-tile-header">
                  <input
                    type="radio"
                    name="dns_mode_setting"
                    checked={dnsMode === 'fake-ip'}
                    onChange={() => handleSetDnsMode('fake-ip')}
                    disabled={dnsModeBusy}
                  />
                  <span className="option-tile-title">⚡ Fake-IP</span>
                  <span className="badge badge-accent" style={{ fontSize: 10, padding: '1px 5px', marginLeft: 'auto' }}>
                    Рекомендуется
                  </span>
                </div>
                <div className="option-tile-desc">
                  Мгновенный отклик DNS (~1 мс), эффективный обход DPI и блокировок, идеален для стримов, мессенджеров и игр.
                </div>
              </label>

              <label className={`option-tile-card ${dnsMode === 'redir-host' ? 'active' : ''}`}>
                <div className="option-tile-header">
                  <input
                    type="radio"
                    name="dns_mode_setting"
                    checked={dnsMode === 'redir-host'}
                    onChange={() => handleSetDnsMode('redir-host')}
                    disabled={dnsModeBusy}
                  />
                  <span className="option-tile-title">🌐 Redir-Host</span>
                  <span className="badge" style={{ fontSize: 10, padding: '1px 5px', marginLeft: 'auto' }}>
                    Прямой
                  </span>
                </div>
                <div className="option-tile-desc">
                  Классический резолв реальных IP-адресов. Используйте, если требуются локальные домены роутера (.keenetic.io / Home LAN).
                </div>
              </label>
            </div>
          </section>


          {/* GEOIP / GEOSITE */}
          <section className="card">
            <h2>🔄 Базы данных GeoIP и GeoSite</h2>
            <p className="muted small">
              Используются ядром Mihomo для точного определения стран и категорий сайтов (включая списки рекламы AdBlock). Загрузка выполняется через прокси Mihomo для стабильности.
            </p>
            <div className="stats-grid four-col" style={{ marginBottom: 12 }}>
              <div className="stat-card">
                <div className="stat-label">База GeoIP</div>
                <div className="stat-value" style={{ fontSize: 16 }}>
                  {geoInfo?.geoip?.size ? `${(geoInfo.geoip.size / (1024 * 1024)).toFixed(1)} МБ` : '—'}
                </div>
                <div className="muted small">{geoInfo?.geoip?.updated_at || 'Mihomo Core'}</div>
              </div>
              <div className="stat-card">
                <div className="stat-label">База GeoSite</div>
                <div className="stat-value" style={{ fontSize: 16 }}>
                  {geoInfo?.geosite?.size ? `${(geoInfo.geosite.size / (1024 * 1024)).toFixed(1)} МБ` : '—'}
                </div>
                <div className="muted small">{geoInfo?.geosite?.updated_at || 'Meta Rules Dat'}</div>
              </div>
              <div className="stat-card">
                <div className="stat-label">Источник баз</div>
                <div className="stat-value" style={{ fontSize: 14, color: '#38bdf8' }}>
                  MetaCubeX
                </div>
                <div className="muted small">Загрузка через прокси</div>
              </div>
              <div className="stat-card">
                <div className="stat-label">Интеграция</div>
                <div className="stat-value" style={{ fontSize: 14, color: '#22c55e' }}>
                  ✓ Активны
                </div>
                <div className="muted small">Маршрутизация & AdBlock</div>
              </div>
            </div>
            <button
              type="button"
              className="btn primary"
              onClick={handleUpdateGeo}
              disabled={geoUpdating}
            >
              {geoUpdating ? '⏳ Загрузка баз (может занять до 1 мин)…' : '🔄 Обновить GeoIP / GeoSite базы'}
            </button>
          </section>

          {/* MIHOMO */}
          <section className="card">
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 10 }}>
              <h2 style={{ margin: 0 }}>⚙️ Mihomo</h2>
              <button
                type="button"
                className="btn sm primary"
                onClick={() => window.dispatchEvent(new CustomEvent('xr:open-mihomo-modal'))}
                title="Открыть список релизов MetaCubeX и обновить ядро Mihomo"
              >
                🚀 Релизы и обновление ядра
              </button>
            </div>
            <label className="row"><span>Host</span>
              <input className="input" value={settings.mihomo.host} onChange={(e) => patch((s) => (s.mihomo.host = e.target.value))} />
            </label>
            <label className="row"><span>Порт</span>
              <NumberInput
                min={1}
                max={65535}
                fallback={9090}
                value={settings.mihomo.port}
                onChange={(val) => patch((s) => (s.mihomo.port = val))}
              />
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

          {/* БЭКАПЫ */}
          <section className="card">
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 6 }}>
              <h2 style={{ margin: 0 }}>💾 Бэкапы (.xkbak)</h2>
              <div style={{ display: 'flex', gap: 6 }}>
                <button className="btn sm primary" onClick={createBackup} disabled={backupBusy}>
                  {backupBusy ? 'Создание…' : '＋ Создать бэкап'}
                </button>
                <button className="btn sm" onClick={() => backupFileRef.current?.click()} disabled={backupBusy} title="Загрузить архив бэкапа с компьютера">
                  📤 Загрузить архив
                </button>
                <input
                  ref={backupFileRef}
                  type="file"
                  accept=".xkbak,.tar.gz,.tar,.zip"
                  style={{ display: 'none' }}
                  onChange={handleImportBackup}
                />
              </div>
            </div>
            <p className="muted small">Снимок config.yaml (Mihomo) + config.json (панель). Каталог: {backupDir || '…'}</p>
            <div className="modal-list" style={{ marginTop: 10 }}>
              {backups.length === 0 && <p className="muted small">Бэкапов пока нет.</p>}
              {backups.map((b) => (
                <div key={b} className="check-row" style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
                  <span className="server-name" style={{ flex: 1 }}>{b}</span>
                  <button className="btn sm" disabled={backupBusy} onClick={() => restoreBackup(b)}>Восстановить</button>
                  <a className="btn sm ghost" href={`/api/backups/export/${encodeURIComponent(b)}`} download title="Скачать архив бэкапа (.xkbak)">
                    ⬇ .xkbak
                  </a>
                  <button className="btn sm ghost" disabled={backupBusy} onClick={() => deleteBackup(b)}>✕</button>
                </div>
              ))}
            </div>
            <label className="row" style={{ marginTop: 10 }}><span>Каталог бэкапов</span>
              <input className="input" value={settings.system?.backup_dir ?? ''}
                onChange={(e) => patch((s) => (s.system.backup_dir = e.target.value))} />
            </label>
          </section>

          {/* ПАНЕЛЬ */}
          <section className="card">
            <h2>🖥 Панель</h2>
            <label className="row"><span>Интервал автообновления, сек</span>
              <NumberInput
                min={3}
                max={300}
                step={1}
                fallback={10}
                value={settings.refresh_interval_sec}
                onChange={(val) => patch((s) => (s.refresh_interval_sec = val))}
              />
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
              <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 12, flexWrap: 'wrap' }}>
                <span className="muted small">
                  Версия панели: <b style={{ color: 'var(--text-bright)' }}>{status?.version || '…'}</b>
                </span>
                <button
                  type="button"
                  className="btn sm"
                  onClick={() => window.dispatchEvent(new CustomEvent('xr:open-update-modal'))}
                  title="Открыть окно управления версией и обновления панели"
                >
                  🚀 Меню обновления панели
                </button>
              </div>
            </div>
          </section>
    </>
  )
}