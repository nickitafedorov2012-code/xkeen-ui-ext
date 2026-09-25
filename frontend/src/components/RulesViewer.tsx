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

  const runDomainTest = async (domainToTest: string, ipToTest?: string) => {
    const d = domainToTest.trim()
    if (!d) return
    setTestDomain(d)
    setTesting(true)
    try {
      const res = await apiPost<RuleTestResult>('rules/test', {
        domain: d,
        source_ip: (ipToTest !== undefined ? ipToTest : testIp).trim() || undefined,
      })
      setTestResult(res)
    } catch (e: any) {
      notify('Ошибка тестирования: ' + e.message, true)
    } finally {
      setTesting(false)
    }
  }

  const handleTestMatch = async (e: React.FormEvent) => {
    e.preventDefault()
    await runDomainTest(testDomain, testIp)
  }

  const QUICK_DOMAINS = [
    { label: '🎬 YouTube', domain: 'youtube.com' },
    { label: '💬 Discord', domain: 'discord.com' },
    { label: '✨ Gemini Labs', domain: 'aistudio.google.com' },
    { label: '🤖 ChatGPT', domain: 'chatgpt.com' },
    { label: '✈️ Telegram', domain: 'telegram.org' },
    { label: '⚡ 2ip.ru', domain: '2ip.ru' },
    { label: '🐙 GitHub', domain: 'github.com' },
  ]

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
      </div>

      {/* Интерактивный симулятор «Куда пойдёт?» */}
      <section
        className="card simulator-card"
        style={{
          background: 'linear-gradient(180deg, rgba(30, 41, 59, 0.4) 0%, rgba(15, 23, 42, 0.4) 100%)',
          border: '1px solid rgba(56, 189, 248, 0.25)',
          borderRadius: 12,
          padding: '18px 20px',
          boxShadow: '0 4px 20px rgba(0, 0, 0, 0.2)',
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 12, flexWrap: 'wrap', gap: 8 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
            <span style={{ fontSize: 22 }}>🎯</span>
            <div>
              <h3 style={{ margin: 0, fontSize: 16, fontWeight: 600 }}>Интерактивный симулятор маршрутизации</h3>
              <p className="muted small" style={{ margin: 0 }}>
                Эмулирует прохождение запроса через цепочку правил Mihomo и перехват Zapret DPI в реальном времени.
              </p>
            </div>
          </div>
          <span
            style={{
              fontSize: 11,
              background: 'rgba(56, 189, 248, 0.15)',
              color: '#38bdf8',
              border: '1px solid rgba(56, 189, 248, 0.3)',
              padding: '2px 8px',
              borderRadius: 12,
              fontWeight: 600,
            }}
          >
            ⚡ Быстрая симуляция
          </span>
        </div>

        {/* Быстрые пресеты популярных доменов */}
        <div style={{ display: 'flex', alignItems: 'center', gap: 6, flexWrap: 'wrap', marginBottom: 14 }}>
          <span style={{ fontSize: 11, color: 'var(--muted)', fontWeight: 500, marginRight: 2 }}>Тест в 1 клик:</span>
          {QUICK_DOMAINS.map((q) => (
            <button
              key={q.domain}
              type="button"
              className="btn sm ghost"
              onClick={() => runDomainTest(q.domain)}
              disabled={testing}
              style={{
                fontSize: 11.5,
                padding: '3px 10px',
                borderRadius: 20,
                background: testDomain === q.domain ? 'rgba(56, 189, 248, 0.2)' : 'rgba(255, 255, 255, 0.04)',
                borderColor: testDomain === q.domain ? 'rgba(56, 189, 248, 0.5)' : 'rgba(255, 255, 255, 0.08)',
                color: testDomain === q.domain ? '#38bdf8' : 'var(--text)',
              }}
            >
              {q.label}
            </button>
          ))}
        </div>

        <form onSubmit={handleTestMatch} style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
          <div style={{ display: 'flex', gap: 10, flexWrap: 'wrap', alignItems: 'flex-end' }}>
            <div style={{ flex: '1 1 240px', display: 'flex', flexDirection: 'column', gap: 4 }}>
              <label style={{ fontSize: 12, fontWeight: 500, color: 'var(--muted)' }}>Домен или хост:</label>
              <input
                type="text"
                className="input sm"
                placeholder="например: youtube.com, 2ip.ru, chatgpt.com"
                value={testDomain}
                onChange={(e) => setTestDomain(e.target.value)}
                style={{ width: '100%', fontFamily: 'Consolas, monospace', fontSize: 13 }}
                required
              />
            </div>

            <div style={{ flex: '1 1 180px', display: 'flex', flexDirection: 'column', gap: 4 }}>
              <label style={{ fontSize: 12, fontWeight: 500, color: 'var(--muted)' }}>IP устройства (необязательно):</label>
              <input
                type="text"
                className="input sm"
                placeholder="например: 192.168.1.118"
                value={testIp}
                onChange={(e) => setTestIp(e.target.value)}
                style={{ width: '100%', fontFamily: 'Consolas, monospace', fontSize: 13 }}
              />
            </div>

            <button
              type="submit"
              className="btn sm primary"
              disabled={testing}
              style={{ minWidth: 150, height: 34, fontWeight: 600 }}
            >
              {testing ? '⏳ Анализ маршрута…' : '🚀 Проверить путь'}
            </button>
          </div>
        </form>

        {testResult && (
          <div
            className="simulator-result"
            style={{
              marginTop: 16,
              padding: '14px 16px',
              background: 'rgba(0, 0, 0, 0.3)',
              borderRadius: 10,
              border: '1px solid rgba(255, 255, 255, 0.08)',
              display: 'flex',
              flexDirection: 'column',
              gap: 12,
            }}
          >
            {/* Visual Pipeline Flow */}
            <div
              style={{
                display: 'grid',
                gridTemplateColumns: 'repeat(auto-fit, minmax(130px, 1fr))',
                gap: 10,
                alignItems: 'stretch',
              }}
            >
              {/* Step 1: Client */}
              <div
                style={{
                  background: 'rgba(255, 255, 255, 0.03)',
                  border: '1px solid rgba(255, 255, 255, 0.08)',
                  borderRadius: 8,
                  padding: '10px 12px',
                  display: 'flex',
                  flexDirection: 'column',
                  gap: 4,
                }}
              >
                <span style={{ fontSize: 10.5, textTransform: 'uppercase', letterSpacing: 0.5, color: 'var(--muted)', fontWeight: 600 }}>
                  💻 Клиент
                </span>
                <span style={{ fontSize: 13, fontWeight: 600, fontFamily: 'Consolas, monospace', color: 'var(--text)' }}>
                  {testIp ? testIp : 'Любое устройство'}
                </span>
              </div>

              {/* Step 2: Matched Rule */}
              <div
                style={{
                  background: 'rgba(255, 255, 255, 0.03)',
                  border: '1px solid rgba(255, 255, 255, 0.08)',
                  borderRadius: 8,
                  padding: '10px 12px',
                  display: 'flex',
                  flexDirection: 'column',
                  gap: 4,
                }}
              >
                <span style={{ fontSize: 10.5, textTransform: 'uppercase', letterSpacing: 0.5, color: 'var(--muted)', fontWeight: 600 }}>
                  📜 Правило
                </span>
                <span
                  style={{
                    fontSize: 12,
                    fontWeight: 600,
                    fontFamily: 'Consolas, monospace',
                    color: testResult.matched_rule.includes('DIRECT') ? '#10b981' : testResult.matched_rule.includes('MATCH') ? '#f59e0b' : '#38bdf8',
                    overflow: 'hidden',
                    textOverflow: 'ellipsis',
                    whiteSpace: 'nowrap',
                  }}
                  title={testResult.matched_rule}
                >
                  {testResult.matched_rule}
                </span>
              </div>

              {/* Step 3: Target Group */}
              <div
                style={{
                  background: 'rgba(255, 255, 255, 0.03)',
                  border: '1px solid rgba(255, 255, 255, 0.08)',
                  borderRadius: 8,
                  padding: '10px 12px',
                  display: 'flex',
                  flexDirection: 'column',
                  gap: 4,
                }}
              >
                <span style={{ fontSize: 10.5, textTransform: 'uppercase', letterSpacing: 0.5, color: 'var(--muted)', fontWeight: 600 }}>
                  🎯 Группа / Маршрут
                </span>
                <div>
                  <span
                    style={{
                      display: 'inline-block',
                      fontSize: 12,
                      fontWeight: 700,
                      padding: '2px 8px',
                      borderRadius: 6,
                      background:
                        testResult.target_group === 'DIRECT'
                          ? 'rgba(16, 185, 129, 0.2)'
                          : testResult.target_group === 'REJECT'
                          ? 'rgba(239, 68, 68, 0.2)'
                          : 'rgba(56, 189, 248, 0.2)',
                      color:
                        testResult.target_group === 'DIRECT'
                          ? '#34d399'
                          : testResult.target_group === 'REJECT'
                          ? '#f87171'
                          : '#38bdf8',
                    }}
                  >
                    {testResult.target_group}
                  </span>
                </div>
              </div>

              {/* Step 4: Resolved Server / Outbound */}
              <div
                style={{
                  background: 'rgba(255, 255, 255, 0.03)',
                  border: '1px solid rgba(255, 255, 255, 0.08)',
                  borderRadius: 8,
                  padding: '10px 12px',
                  display: 'flex',
                  flexDirection: 'column',
                  gap: 4,
                }}
              >
                <span style={{ fontSize: 10.5, textTransform: 'uppercase', letterSpacing: 0.5, color: 'var(--muted)', fontWeight: 600 }}>
                  🚀 Выходной узел
                </span>
                <span
                  style={{
                    fontSize: 13,
                    fontWeight: 600,
                    color:
                      testResult.target_group === 'DIRECT'
                        ? '#34d399'
                        : testResult.target_group === 'REJECT'
                        ? '#f87171'
                        : '#60a5fa',
                  }}
                >
                  {testResult.resolved_server}
                </span>
              </div>
            </div>

            {/* Verdict Explanation Box */}
            <div
              style={{
                padding: '10px 14px',
                borderRadius: 8,
                fontSize: 12.5,
                lineHeight: 1.5,
                background:
                  testResult.target_group === 'DIRECT'
                    ? 'rgba(16, 185, 129, 0.08)'
                    : testResult.target_group === 'REJECT'
                    ? 'rgba(239, 68, 68, 0.08)'
                    : 'rgba(56, 189, 248, 0.08)',
                border:
                  testResult.target_group === 'DIRECT'
                    ? '1px solid rgba(16, 185, 129, 0.25)'
                    : testResult.target_group === 'REJECT'
                    ? '1px solid rgba(239, 68, 68, 0.25)'
                    : '1px solid rgba(56, 189, 248, 0.25)',
              }}
            >
              💡 <strong>Вердикт маршрутизатора:</strong> {testResult.reason}
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

          <div className="rules-actions-group">
            <button className="btn btn-sm btn-secondary" onClick={loadRules} title="Обновить список правил">
              🔄 Обновить список
            </button>
          </div>

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
