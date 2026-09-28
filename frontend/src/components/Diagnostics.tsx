import { useEffect, useState } from 'react'
import { apiGet, apiPost } from '../api'

interface DiagnosticCheck {
  id: string
  name: string
  status: 'ok' | 'warn' | 'fail'
  message: string
  latency_ms?: number
}

interface HealthResponse {
  checks: DiagnosticCheck[]
}

interface DnsTestResponse {
  domain: string
  resolved_ips: string[]
  is_poisoned: boolean
  has_private_ip?: boolean
  http_direct_ok: boolean
  http_proxy_ok: boolean
  http_status?: number | null
  error_type?: string | null
  verdict: string
  recommendation: string
}

interface DiagnosticsProps {
  notify: (msg: string, error?: boolean) => void
}

export default function Diagnostics({ notify }: DiagnosticsProps) {
  const [checks, setChecks] = useState<DiagnosticCheck[]>([])
  const [loadingHealth, setLoadingHealth] = useState(true)
  const [lastHealthCheckTime, setLastHealthCheckTime] = useState<Date | null>(null)
  const [healthError, setHealthError] = useState<string | null>(null)

  // Smart DNS
  const [domain, setDomain] = useState('chatgpt.com')
  const [dnsResult, setDnsResult] = useState<DnsTestResponse | null>(null)
  const [testingDns, setTestingDns] = useState(false)
  const [lastDnsTestTime, setLastDnsTestTime] = useState<Date | null>(null)
  const [dnsError, setDnsError] = useState<string | null>(null)

  const runHealthCheck = async () => {
    setLoadingHealth(true)
    setHealthError(null)
    try {
      const res = await apiGet<HealthResponse>('diagnostics/health')
      setChecks(res.checks || [])
      setLastHealthCheckTime(new Date())
      setHealthError(null)
    } catch (e: any) {
      setHealthError(e.message || 'Ошибка выполнения диагностики')
      notify('Ошибка диагностики: ' + e.message, true)
    } finally {
      setLoadingHealth(false)
    }
  }

  useEffect(() => {
    runHealthCheck()
  }, [])

  const handleDnsTest = async (e: React.FormEvent) => {
    e.preventDefault()
    if (!domain.trim()) return
    setTestingDns(true)
    setDnsError(null)
    setDnsResult(null)
    try {
      const res = await apiPost<DnsTestResponse>('diagnostics/dns-test', {
        domain: domain.trim(),
      })
      setDnsResult(res)
      setLastDnsTestTime(new Date())
      setDnsError(null)
    } catch (e: any) {
      setDnsError(e.message || 'Ошибка DNS-теста')
      setDnsResult(null)
      notify('Ошибка DNS-теста: ' + e.message, true)
    } finally {
      setTestingDns(false)
    }
  }

  return (
    <div className="diagnostics-view">
      <div className="section-header">
        <div>
          <h2>🩺 Диагностика и здоровье сети</h2>
          <p className="muted">
            Комплексная проверка доступности ключевых узлов роутера, ядра Mihomo, прямого интернета и интеллектуальный DNS-тест.
          </p>
        </div>
        <button
          className="btn btn-sm btn-primary"
          onClick={runHealthCheck}
          disabled={loadingHealth}
        >
          {loadingHealth ? '⏳ Проверка…' : '🔄 Запустить диагностику'}
        </button>
      </div>

      {healthError && (
        <div className="alert alert-error" style={{ marginBottom: '1rem', padding: '0.75rem 1rem', background: 'rgba(239, 68, 68, 0.1)', border: '1px solid #ef4444', borderRadius: '6px' }}>
          <span>⚠️ <strong>Ошибка диагностики:</strong> {healthError}</span>
          {lastHealthCheckTime && (
            <span className="muted" style={{ marginLeft: '1rem', fontSize: '0.85em' }}>
              (Данные устарели. Последнее обновление: {lastHealthCheckTime.toLocaleTimeString()})
            </span>
          )}
        </div>
      )}
      {!healthError && lastHealthCheckTime && (
        <div className="text-muted" style={{ fontSize: '0.85em', marginBottom: '0.75rem' }}>
          Последнее успешное обновление: {lastHealthCheckTime.toLocaleTimeString()}
        </div>
      )}

      {/* Проверки компонентов */}
      <div className="diagnostics-grid">
        {loadingHealth ? (
          <div className="card loading-placeholder">⏳ Опрос сетевых интерфейсов и служб…</div>
        ) : (
          checks.map((c) => (
            <div key={c.id} className={`diagnostic-card ${c.status}`}>
              <div className="diagnostic-header">
                <span className="diagnostic-status-icon">
                  {c.status === 'ok' ? '🟢' : c.status === 'warn' ? '🟡' : '🔴'}
                </span>
                <span className="diagnostic-name">{c.name}</span>
                {c.latency_ms !== undefined && (
                  <span className="diagnostic-latency">{c.latency_ms} мс</span>
                )}
              </div>
              <div className="diagnostic-message">{c.message}</div>
            </div>
          ))
        )}
      </div>

      {/* Smart DNS Test */}
      <section className="card smart-dns-card">
        <div className="card-header">
          <div className="card-title">
            <span className="card-icon">🧠</span>
            <h3>Smart DNS Test: «Почему не открывается сайт?»</h3>
          </div>
          <span className="badge badge-accent">Авто-вердикт</span>
        </div>
        <p className="muted">
          Мгновенный анализ доступности любого ресурса: резолвится ли IP, нет ли фильтрации DNS (заглушки 127.0.0.1 / AdBlock) и проходит ли прямой TCP/TLS коннект.
        </p>

        <form onSubmit={handleDnsTest} className="smart-dns-form">
          <div className="input-group-horizontal">
            <input
              type="text"
              className="input-text"
              placeholder="Введите домен: rutracker.org, chatgpt.com, instagram.com..."
              value={domain}
              onChange={(e) => setDomain(e.target.value)}
              required
            />
            <button type="submit" className="btn btn-primary" disabled={testingDns}>
              {testingDns ? '⏳ Тестирование…' : '🔍 Проверить домен'}
            </button>
          </div>
        </form>

        {dnsError && (
          <div className="alert alert-error" style={{ marginTop: '1rem', padding: '0.75rem 1rem', background: 'rgba(239, 68, 68, 0.1)', border: '1px solid #ef4444', borderRadius: '6px' }}>
            <span>⚠️ <strong>Ошибка DNS-теста:</strong> {dnsError}</span>
            {lastDnsTestTime && (
              <span className="muted" style={{ marginLeft: '1rem', fontSize: '0.85em' }}>
                (Последняя проверка: {lastDnsTestTime.toLocaleTimeString()})
              </span>
            )}
          </div>
        )}

        {dnsResult && (
          <div className="smart-dns-result">
            <div className="dns-summary-card">
              <div className="dns-domain-heading">
                <h4>Анализ: <code>{dnsResult.domain}</code></h4>
                <div style={{ display: 'flex', gap: '0.5rem', alignItems: 'center' }}>
                  {lastDnsTestTime && (
                    <span className="muted" style={{ fontSize: '0.8em' }}>
                      {lastDnsTestTime.toLocaleTimeString()}
                    </span>
                  )}
                  <span className={`status-pill ${dnsResult.http_direct_ok ? 'pill-ok' : 'pill-warn'}`}>
                    {dnsResult.http_direct_ok ? 'Прямой доступ OK' : 'Прямой доступ заблокирован'}
                  </span>
                </div>
              </div>

              <div className="dns-details-list">
                {dnsResult.error_type && (
                  <div className="dns-detail-row">
                    <span className="detail-label">Тип ошибки соединения:</span>
                    <span className="detail-value text-warn">
                      <code>{dnsResult.error_type}</code>
                    </span>
                  </div>
                )}
                <div className="dns-detail-row">
                  <span className="detail-label">Полученные IP адреса:</span>
                  <span className="detail-value">
                    {dnsResult.resolved_ips.length > 0 ? (
                      dnsResult.resolved_ips.map((ip) => (
                        <code key={ip} className="ip-badge">{ip}</code>
                      ))
                    ) : (
                      <span className="muted">Не удалось получить IP</span>
                    )}
                  </span>
                </div>

                <div className="dns-detail-row">
                  <span className="detail-label">DNS-фильтрация / Адрес-заглушка:</span>
                  <span className={`detail-value ${dnsResult.is_poisoned ? 'val-danger' : 'val-ok'}`}>
                    {dnsResult.is_poisoned ? '❌ Обнаружен адрес-заглушка (127.0.0.1 / 0.0.0.0)' : '✅ Адрес-заглушка не обнаружен'}
                  </span>
                </div>

                <div className="dns-detail-row">
                  <span className="detail-label">Прямое HTTPS подключение:</span>
                  <span className={`detail-value ${dnsResult.http_direct_ok ? 'val-ok' : 'val-danger'}`}>
                    {dnsResult.http_direct_ok ? '✅ Успешно' : '❌ Сброшено или таймаут'}
                  </span>
                </div>
              </div>

              <div className="dns-verdict-box">
                <div className="verdict-title">📋 Вердикт:</div>
                <div className="verdict-body">{dnsResult.verdict}</div>
                <div className="verdict-recom">
                  👉 <strong>Рекомендация:</strong> {dnsResult.recommendation}
                </div>
              </div>
            </div>
          </div>
        )}
      </section>
    </div>
  )
}
