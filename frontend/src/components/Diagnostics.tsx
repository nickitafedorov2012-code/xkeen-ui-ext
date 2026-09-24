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
  http_direct_ok: boolean
  http_proxy_ok: boolean
  verdict: string
  recommendation: string
}

interface DiagnosticsProps {
  notify: (msg: string, error?: boolean) => void
}

export default function Diagnostics({ notify }: DiagnosticsProps) {
  const [checks, setChecks] = useState<DiagnosticCheck[]>([])
  const [loadingHealth, setLoadingHealth] = useState(true)

  // Smart DNS
  const [domain, setDomain] = useState('chatgpt.com')
  const [dnsResult, setDnsResult] = useState<DnsTestResponse | null>(null)
  const [testingDns, setTestingDns] = useState(false)

  const runHealthCheck = async () => {
    setLoadingHealth(true)
    try {
      const res = await apiGet<HealthResponse>('diagnostics/health')
      setChecks(res.checks || [])
    } catch (e: any) {
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
    try {
      const res = await apiPost<DnsTestResponse>('diagnostics/dns-test', {
        domain: domain.trim(),
      })
      setDnsResult(res)
    } catch (e: any) {
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
          Мгновенный анализ доступности любого ресурса: резолвится ли IP, нет ли подмены DNS провайдером (РКН-заглушки) и проходит ли прямой TCP/TLS коннект.
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

        {dnsResult && (
          <div className="smart-dns-result">
            <div className="dns-summary-card">
              <div className="dns-domain-heading">
                <h4>Анализ: <code>{dnsResult.domain}</code></h4>
                <span className={`status-pill ${dnsResult.http_direct_ok ? 'pill-ok' : 'pill-warn'}`}>
                  {dnsResult.http_direct_ok ? 'Прямой доступ OK' : 'Прямой доступ заблокирован'}
                </span>
              </div>

              <div className="dns-details-list">
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
                  <span className="detail-label">DNS Spoofing (Заглушка РКН):</span>
                  <span className={`detail-value ${dnsResult.is_poisoned ? 'val-danger' : 'val-ok'}`}>
                    {dnsResult.is_poisoned ? '❌ Обнаружена подмена адреса' : '✅ Подмена не обнаружена'}
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
