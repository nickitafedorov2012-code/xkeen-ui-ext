import { useState, useEffect, useCallback } from 'react'
import { apiGet, apiPost } from '../api'
import type { XboxDnsCheckResponse, XboxDnsStatusResponse, XboxDnsServerInfo } from '../types'

interface XboxDnsProps {
  notify: (msg: string, error?: boolean) => void
}

type GuideDevice = 'xbox' | 'psn' | 'pc' | 'keenetic'

const DEFAULT_SERVERS: XboxDnsServerInfo[] = [
  {
    id: 'xbox-dns-primary',
    name: 'xbox-dns.ru (Основной пул)',
    provider: 'Selectel (Санкт-Петербург, Россия)',
    ips: ['111.88.96.54', '111.88.96.55'],
    ipv6: ['2a00:ab00:1233:26::50'],
    status: 'online',
    is_recommended: true,
    supported_features: ['Xbox Live (0x80a40401)', 'Game Pass', 'xCloud', 'Google AI Studio', 'Gemini'],
  },
  {
    id: 'xbox-dns-alt',
    name: 'xbox-dns.ru (Резервный пул)',
    provider: 'Selectel (Санкт-Петербург, Россия)',
    ips: ['111.88.96.50', '111.88.96.51'],
    ipv6: ['2a00:ab00:1233:26::51'],
    status: 'online',
    is_recommended: false,
    supported_features: ['Xbox Live', 'Game Pass', 'Google AI Studio', 'Gemini'],
  },
]

export default function XboxDns({ notify }: XboxDnsProps) {
  const [status, setStatus] = useState<XboxDnsStatusResponse | null>(null)
  const [checkResults, setCheckResults] = useState<XboxDnsCheckResponse | null>(null)
  const [loading, setLoading] = useState(false)
  const [checking, setChecking] = useState(false)
  const [activeGuide, setActiveGuide] = useState<GuideDevice>('xbox')
  const [copiedItem, setCopiedItem] = useState<string | null>(null)

  const loadStatus = useCallback(async () => {
    setLoading(true)
    try {
      const res = await apiGet<XboxDnsStatusResponse>('xbox-dns/status')
      setStatus(res)
    } catch (e: any) {
      notify(e instanceof Error ? e.message : 'Ошибка загрузки статуса Xbox DNS', true)
    } finally {
      setLoading(false)
    }
  }, [notify])

  useEffect(() => {
    loadStatus()
  }, [loadStatus])

  const handleCheckDns = async () => {
    setChecking(true)
    try {
      const res = await apiPost<XboxDnsCheckResponse>('xbox-dns/check', {})
      setCheckResults(res)
      notify('Диагностика DNS-серверов успешно завершена')
    } catch (e: any) {
      notify('Ошибка проверки серверов DNS: ' + (e?.message || e), true)
    } finally {
      setChecking(false)
    }
  }

  const copyToClipboard = (text: string, label: string) => {
    navigator.clipboard?.writeText(text)
    setCopiedItem(label)
    notify(`Скопировано в буфер: ${text}`)
    setTimeout(() => setCopiedItem(null), 2500)
  }

  const servers = status?.servers && status.servers.length > 0 ? status.servers : DEFAULT_SERVERS

  return (
    <div className="tab-pane xbox-dns-page" data-testid="xbox-dns-view">
      {/* 1. ГЛАВНЫЙ СТАТУСНЫЙ HERO-БАННЕР */}
      <section className="card" style={{ marginBottom: 16 }}>
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', flexWrap: 'wrap', gap: 16 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 14 }}>
            <div
              style={{
                width: 52,
                height: 52,
                borderRadius: 14,
                background: 'linear-gradient(135deg, rgba(16, 185, 129, 0.25), rgba(5, 150, 105, 0.45))',
                border: '1px solid rgba(16, 185, 129, 0.4)',
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                fontSize: 26,
              }}
            >
              🎮
            </div>
            <div>
              <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
                <h2 style={{ margin: 0, fontSize: 20, fontWeight: 700 }}>Xbox DNS & SmartDNS Bypass</h2>
                <span
                  className="badge"
                  style={{
                    background: 'rgba(16, 185, 129, 0.15)',
                    color: '#34d399',
                    borderColor: 'rgba(16, 185, 129, 0.35)',
                    fontSize: 12,
                    fontWeight: 600,
                  }}
                >
                  Fix 0x80a40401
                </span>
                <span
                  className="badge"
                  style={{
                    background: 'rgba(56, 189, 248, 0.15)',
                    color: '#38bdf8',
                    borderColor: 'rgba(56, 189, 248, 0.35)',
                    fontSize: 12,
                  }}
                >
                  Selectel SPb
                </span>
                {status?.antigravity_fallback_ready && (
                  <span
                    className="badge"
                    style={{
                      background: 'rgba(16, 185, 129, 0.2)',
                      color: '#10b981',
                      borderColor: '#10b981',
                      fontSize: 12,
                      fontWeight: 600,
                    }}
                  >
                    ✓ Antigravity Resolver
                  </span>
                )}
              </div>
              <p className="muted" style={{ margin: '4px 0 0', fontSize: 13 }}>
                Обход региональной блокировки авторизации Xbox (ошибка 0x80a40401), Xbox Game Pass, PSN и интеграция резервных прокси для Google AI
              </p>
            </div>
          </div>

          <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
            <button
              type="button"
              className="btn btn-primary"
              onClick={handleCheckDns}
              disabled={checking}
              data-testid="xbox-dns-check-btn"
              style={{ display: 'inline-flex', alignItems: 'center', gap: 6 }}
            >
              <span>{checking ? '⏳' : '🩺'}</span>
              <span>{checking ? 'Проверка задержки…' : 'Проверить серверы DNS'}</span>
            </button>

            <a
              href="/api/xbox-dns/fix.cmd"
              download="fix_xbox_dns.cmd"
              className="btn"
              data-testid="xbox-dns-download-cmd"
              style={{
                display: 'inline-flex',
                alignItems: 'center',
                gap: 6,
                textDecoration: 'none',
                background: 'rgba(255, 255, 255, 0.05)',
              }}
              title="Скачать готовый .cmd скрипт для быстрой установки или сброса Xbox DNS в Windows"
            >
              <span>📥</span>
              <span>fix_xbox_dns.cmd</span>
            </a>

            <button
              type="button"
              className="btn"
              onClick={loadStatus}
              disabled={loading}
              title="Обновить данные статуса"
            >
              {loading ? '⏳' : '🔄'}
            </button>
          </div>
        </div>
      </section>

      {/* 2. ПУЛЫ СЕРВЕРОВ XBOX-DNS */}
      <h3 style={{ margin: '0 0 12px 2px', fontSize: 16, display: 'flex', alignItems: 'center', gap: 8 }}>
        <span>📡 Доступные пулы DNS-серверов</span>
        {checkResults && <span className="muted small">(проверено в {checkResults.timestamp})</span>}
      </h3>

      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(320px, 1fr))', gap: 14, marginBottom: 20 }}>
        {servers.map((srv, idx) => {
          const primaryIp = srv.ips[0] || ''
          const secondaryIp = srv.ips[1] || ''
          const ipv6 = srv.ipv6[0] || ''
          const checkRes = checkResults?.results.find((r) => r.ip === primaryIp)
          const testId = srv.is_recommended ? 'xbox-dns-primary-card' : 'xbox-dns-alt-card'

          return (
            <div
              key={srv.id || primaryIp}
              className="card"
              data-testid={testId}
              style={{
                border: srv.is_recommended ? '1px solid rgba(16, 185, 129, 0.3)' : '1px solid var(--border)',
                background: srv.is_recommended ? 'linear-gradient(180deg, rgba(16, 185, 129, 0.04), transparent)' : 'var(--panel)',
                position: 'relative',
              }}
            >
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start', marginBottom: 10 }}>
                <div>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                    <span style={{ fontSize: 18 }}>{srv.is_recommended ? '🟢' : '🛡️'}</span>
                    <b style={{ fontSize: 16, color: '#f8fafc' }}>{srv.name}</b>
                  </div>
                  <div className="muted small" style={{ marginTop: 2 }}>{srv.provider}</div>
                </div>
                {srv.is_recommended ? (
                  <span className="badge" style={{ background: 'rgba(16, 185, 129, 0.2)', color: '#10b981', border: '1px solid #10b981' }}>
                    Рекомендуемый
                  </span>
                ) : (
                  <span className="badge" style={{ background: 'rgba(255,255,255,0.08)', color: '#94a3b8' }}>
                    Резервный
                  </span>
                )}
              </div>

              <div style={{ margin: '14px 0', display: 'flex', flexDirection: 'column', gap: 8 }}>
                {primaryIp && (
                  <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', background: 'rgba(0,0,0,0.2)', padding: '6px 10px', borderRadius: 8 }}>
                    <span className="muted small">Основной DNS (IPv4):</span>
                    <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                      <code style={{ fontSize: 14, color: '#38bdf8', fontWeight: 600 }}>{primaryIp}</code>
                      <button
                        type="button"
                        className="btn"
                        style={{ padding: '2px 8px', fontSize: 11 }}
                        onClick={() => copyToClipboard(primaryIp, `dns1_${idx}`)}
                      >
                        {copiedItem === `dns1_${idx}` ? '✓' : 'Копировать'}
                      </button>
                    </div>
                  </div>
                )}

                {secondaryIp && (
                  <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', background: 'rgba(0,0,0,0.2)', padding: '6px 10px', borderRadius: 8 }}>
                    <span className="muted small">Дополнительный DNS:</span>
                    <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                      <code style={{ fontSize: 14, color: '#38bdf8', fontWeight: 600 }}>{secondaryIp}</code>
                      <button
                        type="button"
                        className="btn"
                        style={{ padding: '2px 8px', fontSize: 11 }}
                        onClick={() => copyToClipboard(secondaryIp, `dns2_${idx}`)}
                      >
                        {copiedItem === `dns2_${idx}` ? '✓' : 'Копировать'}
                      </button>
                    </div>
                  </div>
                )}

                {ipv6 && (
                  <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', background: 'rgba(0,0,0,0.2)', padding: '6px 10px', borderRadius: 8 }}>
                    <span className="muted small">IPv6 DNS:</span>
                    <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                      <code style={{ fontSize: 12, color: '#94a3b8' }}>{ipv6}</code>
                      <button
                        type="button"
                        className="btn"
                        style={{ padding: '2px 8px', fontSize: 11 }}
                        onClick={() => copyToClipboard(ipv6, `ipv6_${idx}`)}
                      >
                        {copiedItem === `ipv6_${idx}` ? '✓' : 'Копировать'}
                      </button>
                    </div>
                  </div>
                )}
              </div>

              {/* Результат проверки задержки */}
              {checkRes && (
                <div
                  style={{
                    marginBottom: 12,
                    padding: '8px 10px',
                    borderRadius: 6,
                    background: checkRes.reachable ? 'rgba(16, 185, 129, 0.1)' : 'rgba(239, 68, 68, 0.1)',
                    border: checkRes.reachable ? '1px solid rgba(16, 185, 129, 0.25)' : '1px solid rgba(239, 68, 68, 0.25)',
                    fontSize: 12.5,
                  }}
                >
                  {checkRes.reachable ? (
                    <span>
                      ⚡ Задержка ответа: <b>{checkRes.latency_ms} мс</b> · Реверс-прокси: <code>{checkRes.resolved_ips[0] || '188.68.214.130'}</code>
                    </span>
                  ) : (
                    <span style={{ color: '#ef4444' }}>
                      ❌ Недоступен: {checkRes.error || 'Таймаут опроса'}
                    </span>
                  )}
                </div>
              )}

              <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6 }}>
                {(srv.supported_features || ['Xbox Live', 'Game Pass', 'Google AI Studio', 'Gemini']).map((tag) => (
                  <span key={tag} className="badge" style={{ fontSize: 11, background: 'rgba(255,255,255,0.05)', color: '#cbd5e1' }}>
                    {tag}
                  </span>
                ))}
              </div>
            </div>
          )
        })}
      </div>

      {/* 3. ИНТЕГРАЦИЯ С ANTIGRAVITY & GOOGLE AI */}
      <section className="card" style={{ marginBottom: 20 }}>
        <div style={{ display: 'flex', alignItems: 'flex-start', gap: 12 }}>
          <span style={{ fontSize: 24 }}>🤖</span>
          <div style={{ flex: 1 }}>
            <h3 style={{ margin: '0 0 6px', fontSize: 16 }}>Интеграция с Google Antigravity & AI Flow</h3>
            <p style={{ margin: '0 0 10px', fontSize: 13.5, lineHeight: 1.6, color: 'var(--text-secondary)' }}>
              Обратные SNI-прокси серверов Xbox-DNS (<code>188.68.214.130</code>, <code>188.68.214.143</code>) успешно пропускают защищенный TLS-трафик не только к консольным сервисам, но и к <b>Google AI Studio</b>, <b>Gemini</b>, <b>generativelanguage.googleapis.com</b>, а также к <b>daily-cloudcode-pa.googleapis.com</b>.
            </p>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap', fontSize: 12.5 }}>
              <span className="badge" style={{ background: 'rgba(16, 185, 129, 0.15)', color: '#34d399', borderColor: '#10b981' }}>
                ✓ Добавлен в кандидаты antigravity.rs
              </span>
              <span className="badge" style={{ background: 'rgba(56, 189, 248, 0.15)', color: '#38bdf8', borderColor: '#38bdf8' }}>
                ✓ Резервный источник подменных IP
              </span>
              <span className="badge" style={{ background: 'rgba(245, 158, 11, 0.15)', color: '#fbbf24', borderColor: '#f59e0b' }}>
                ℹ Приоритет: туннель Mihomo (Канада/США)
              </span>
            </div>
          </div>
        </div>
      </section>

      {/* 4. ИНСТРУКЦИИ ПО НАСТРОЙКЕ УСТРОЙСТВ */}
      <section className="card">
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 14, flexWrap: 'wrap', gap: 8 }}>
          <h3 style={{ margin: 0, fontSize: 16, display: 'flex', alignItems: 'center', gap: 8 }}>
            <span>🛠️ Инструкции по настройке устройств</span>
          </h3>

          <div className="segmented-control" style={{ display: 'flex', gap: 4, background: 'rgba(0,0,0,0.25)', padding: 3, borderRadius: 8 }}>
            <button
              type="button"
              className={`btn btn-sm ${activeGuide === 'xbox' ? 'btn-primary' : ''}`}
              onClick={() => setActiveGuide('xbox')}
            >
              Xbox Series / One
            </button>
            <button
              type="button"
              className={`btn btn-sm ${activeGuide === 'psn' ? 'btn-primary' : ''}`}
              onClick={() => setActiveGuide('psn')}
            >
              PlayStation 5 / 4
            </button>
            <button
              type="button"
              className={`btn btn-sm ${activeGuide === 'pc' ? 'btn-primary' : ''}`}
              onClick={() => setActiveGuide('pc')}
            >
              Windows 10 / 11
            </button>
            <button
              type="button"
              className={`btn btn-sm ${activeGuide === 'keenetic' ? 'btn-primary' : ''}`}
              onClick={() => setActiveGuide('keenetic')}
            >
              Роутер Keenetic
            </button>
          </div>
        </div>

        {/* Руководство: Xbox */}
        {activeGuide === 'xbox' && (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 10, fontSize: 13.5, lineHeight: 1.6 }}>
            <div><b>1.</b> Нажмите кнопку <b>Xbox</b> на геймпаде и откройте <b>«Параметры»</b> &rarr; <b>«Общие»</b> &rarr; <b>«Параметры сети»</b>.</div>
            <div><b>2.</b> Перейдите в <b>«Дополнительные настройки»</b> &rarr; <b>«Параметры DNS»</b> &rarr; выберите <b>«Вручную»</b>.</div>
            <div>
              <b>3.</b> Введите адреса серверов:
              <ul style={{ margin: '6px 0 0 20px', padding: 0 }}>
                <li>Основной IPv4 DNS: <code>111.88.96.54</code></li>
                <li>Дополнительный IPv4 DNS: <code>111.88.96.55</code></li>
              </ul>
            </div>
            <div><b>4.</b> Сохраните настройки и перезагрузите консоль. Ошибка <code>0x80a40401</code> при авторизации в профиль будет устранена.</div>
          </div>
        )}

        {/* Руководство: PlayStation */}
        {activeGuide === 'psn' && (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 10, fontSize: 13.5, lineHeight: 1.6 }}>
            <div><b>1.</b> Откройте <b>«Настройки»</b> &rarr; <b>«Сеть»</b> &rarr; <b>«Установить соединение с интернетом»</b>.</div>
            <div><b>2.</b> Выберите ваше Wi-Fi или проводное подключение &rarr; нажмите <b>«Дополнительные настройки»</b>.</div>
            <div><b>3.</b> Установите <b>«Настройки DNS»</b> в положение <b>«Вручную»</b>.</div>
            <div>
              <b>4.</b> Заполните поля адресов:
              <ul style={{ margin: '6px 0 0 20px', padding: 0 }}>
                <li>Основной DNS: <code>111.88.96.54</code></li>
                <li>Дополнительный DNS: <code>111.88.96.55</code></li>
              </ul>
            </div>
            <div><b>5.</b> Проверьте соединение с интернетом в меню консоли.</div>
          </div>
        )}

        {/* Руководство: Windows PC */}
        {activeGuide === 'pc' && (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 12, fontSize: 13.5, lineHeight: 1.6 }}>
            <div>
              <b>Способ 1 (самый простой):</b> Скачайте файл <a href="/api/xbox-dns/fix.cmd" download="fix_xbox_dns.cmd" style={{ color: '#38bdf8', fontWeight: 600 }}>fix_xbox_dns.cmd</a> и запустите его от имени Администратора. Скрипт в один клик применит или сбросит DNS на всех активных адаптерах.
            </div>

            <div>
              <b>Способ 2 (PowerShell):</b> Откройте PowerShell от Администратора и выполните:
              <pre style={{ background: 'rgba(0,0,0,0.3)', padding: 10, borderRadius: 6, margin: '6px 0', overflowX: 'auto', fontSize: 12.5 }}>
                <code>{`Set-DnsClientServerAddress -InterfaceAlias "Ethernet" -ServerAddresses ('111.88.96.54','111.88.96.55')\nClear-DnsClientCache`}</code>
              </pre>
            </div>

            <div>
              <b>Для возврата на авто-DNS роутера:</b>
              <pre style={{ background: 'rgba(0,0,0,0.3)', padding: 10, borderRadius: 6, margin: '6px 0', overflowX: 'auto', fontSize: 12.5 }}>
                <code>{`Set-DnsClientServerAddress -InterfaceAlias "Ethernet" -ResetServerAddresses\nClear-DnsClientCache`}</code>
              </pre>
            </div>
          </div>
        )}

        {/* Руководство: Keenetic */}
        {activeGuide === 'keenetic' && (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 10, fontSize: 13.5, lineHeight: 1.6 }}>
            <div>
              <b>Централизованный обход на роутере:</b> Чтобы не настраивать консоли вручную, в KeeneticOS можно перенаправить домены авторизации напрямую на реверс-прокси:
            </div>
            <pre style={{ background: 'rgba(0,0,0,0.3)', padding: 10, borderRadius: 6, margin: '6px 0', overflowX: 'auto', fontSize: 12.5 }}>
              <code>{`ip host auth.xboxlive.com 188.68.214.130\nip host xsts.auth.xboxlive.com 188.68.214.130\nip host title.mgt.xboxlive.com 188.68.214.130\nsystem configuration save`}</code>
            </pre>
            <div className="muted small">
              При этом на роутере адрес <code>188.68.214.130</code> автоматически исключается из перехвата VPN/Mihomo через ipset <code>user_exclude</code>.
            </div>
          </div>
        )}
      </section>
    </div>
  )
}
