import { useEffect, useState } from 'react'
import { apiGet, apiPost } from '../api'

interface RuleItem {
  type: string
  payload: string
  proxy: string
}

interface RuleTestResult {
  matched_rule: string
  rule_type: string
  target_group: string
  resolved_server: string
  reason: string
}

interface RulesViewerProps {
  notify: (msg: string, error?: boolean) => void
}

export default function RulesViewer({ notify }: RulesViewerProps) {
  const [rules, setRules] = useState<RuleItem[]>([])
  const [loading, setLoading] = useState(true)
  const [search, setSearch] = useState('')
  const [filterType, setFilterType] = useState('all')

  // Симулятор «Куда пойдёт?»
  const [testDomain, setTestDomain] = useState('youtube.com')
  const [testIp, setTestIp] = useState('')
  const [testResult, setTestResult] = useState<RuleTestResult | null>(null)
  const [testing, setTesting] = useState(false)

  const loadRules = async () => {
    setLoading(true)
    try {
      const res = await apiGet<{ rules: RuleItem[] }>('rules')
      setRules(res.rules || [])
    } catch (e: any) {
      notify('Ошибка загрузки правил: ' + e.message, true)
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    loadRules()
  }, [])

  const handleTestMatch = async (e: React.FormEvent) => {
    e.preventDefault()
    if (!testDomain.trim()) return
    setTesting(true)
    try {
      const res = await apiPost<RuleTestResult>('rules/test', {
        domain: testDomain.trim(),
        source_ip: testIp.trim() || undefined,
      })
      setTestResult(res)
    } catch (e: any) {
      notify('Ошибка тестирования: ' + e.message, true)
    } finally {
      setTesting(false)
    }
  }

  // Фильтрация правил
  const filtered = rules.filter((r) => {
    if (filterType !== 'all' && r.type.toLowerCase() !== filterType.toLowerCase()) return false
    if (!search.trim()) return true
    const q = search.toLowerCase()
    return (
      r.payload.toLowerCase().includes(q) ||
      r.proxy.toLowerCase().includes(q) ||
      r.type.toLowerCase().includes(q)
    )
  })

  return (
    <div className="rules-view">
      <div className="section-header">
        <div>
          <h2>📋 Правила маршрутизации</h2>
          <p className="muted">
            Активные правила ядра Mihomo: доменные списки, AdBlock, маршрутизация по устройствам и финальный выбор прокси.
          </p>
        </div>
        <button className="btn btn-sm btn-secondary" onClick={loadRules}>
          🔄 Обновить список
        </button>
      </div>

      {/* Интерактивный симулятор «Куда пойдёт?» */}
      <section className="card simulator-card">
        <div className="card-header">
          <div className="card-title">
            <span className="card-icon">🎯</span>
            <h3>Интерактивный симулятор: «Куда пойдёт трафик?»</h3>
          </div>
          <span className="badge badge-accent">Проверка в 1 клик</span>
        </div>
        <p className="muted">
          Введите домен и (опционально) IP-адрес домашнего устройства, чтобы узнать, по какому именно правилу пойдет запрос и на какой сервер/узел он выйдет.
        </p>

        <form onSubmit={handleTestMatch} className="simulator-form">
          <div className="simulator-inputs">
            <div className="input-group">
              <label>Домен или хост:</label>
              <input
                type="text"
                className="input-text"
                placeholder="например: youtube.com, 2ip.ru, chatgpt.com"
                value={testDomain}
                onChange={(e) => setTestDomain(e.target.value)}
                required
              />
            </div>

            <div className="input-group">
              <label>IP устройства (необязательно):</label>
              <input
                type="text"
                className="input-text"
                placeholder="например: 192.168.1.118"
                value={testIp}
                onChange={(e) => setTestIp(e.target.value)}
              />
            </div>

            <button type="submit" className="btn btn-primary" disabled={testing}>
              {testing ? '⏳ Анализ…' : '🚀 Проверить маршрут'}
            </button>
          </div>
        </form>

        {testResult && (
          <div className="simulator-result">
            <div className="result-flow">
              <div className="flow-step client-step">
                <span className="step-label">Клиент</span>
                <span className="step-val">{testIp ? testIp : 'Любое устройство'}</span>
              </div>
              <div className="flow-arrow">➔</div>
              <div className="flow-step rule-step">
                <span className="step-label">Правило</span>
                <span className="step-val">{testResult.matched_rule}</span>
              </div>
              <div className="flow-arrow">➔</div>
              <div className="flow-step group-step">
                <span className="step-label">Группа / Цель</span>
                <span className="step-val">{testResult.target_group}</span>
              </div>
              <div className="flow-arrow">➔</div>
              <div className="flow-step server-step">
                <span className="step-label">Выход</span>
                <span className={`step-val ${testResult.target_group === 'REJECT' ? 'step-reject' : 'step-proxy'}`}>
                  {testResult.resolved_server}
                </span>
              </div>
            </div>

            <div className="result-explanation">
              💡 <strong>Вердикт:</strong> {testResult.reason}
            </div>
          </div>
        )}
      </section>

      {/* Панель фильтрации правил */}
      <div className="card toolbar-card">
        <div className="rules-filters">
          <input
            type="text"
            className="input-text rules-search"
            placeholder="🔍 Поиск правил по шаблону, домену или группе назначения…"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />

          <div className="rules-types">
            {['all', 'geosite', 'domain-suffix', 'src-ip-cidr', 'match'].map((t) => (
              <button
                key={t}
                type="button"
                className={`btn btn-sm ${filterType === t ? 'btn-primary' : 'btn-secondary'}`}
                onClick={() => setFilterType(t)}
              >
                {t === 'all' ? `Все (${rules.length})` : t.toUpperCase()}
              </button>
            ))}
          </div>
        </div>
      </div>

      {/* Список правил */}
      {loading ? (
        <div className="card loading-placeholder">⏳ Загрузка правил ядра…</div>
      ) : filtered.length === 0 ? (
        <div className="card empty-placeholder">Правила не найдены</div>
      ) : (
        <div className="card table-card">
          <div className="table-responsive">
            <table className="rules-table">
              <thead>
                <tr>
                  <th style={{ width: '60px' }}>#</th>
                  <th>Тип правила</th>
                  <th>Шаблон / Параметр (Payload)</th>
                  <th>Группа назначения (Proxy)</th>
                </tr>
              </thead>
              <tbody>
                {filtered.map((r, i) => (
                  <tr key={i}>
                    <td className="muted">{i + 1}</td>
                    <td>
                      <span className={`rule-type-badge ${r.type.toLowerCase().replace(/[^a-z0-9]/g, '-')}`}>
                        {r.type}
                      </span>
                    </td>
                    <td className="payload-cell">
                      <code>{r.payload || '—'}</code>
                    </td>
                    <td>
                      <span className={`proxy-target-badge ${r.proxy === 'REJECT' ? 'reject' : r.proxy === 'DIRECT' ? 'direct' : 'proxy'}`}>
                        {r.proxy}
                      </span>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}
    </div>
  )
}
