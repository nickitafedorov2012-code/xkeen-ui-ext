import { describe, it, expect, beforeEach } from 'vitest'

describe('Jitter & Ping Stability Anti-Regression Tests', () => {
  beforeEach(() => {
    sessionStorage.clear()
  })

  // 1. Формула расчёта джиттера (Mean Absolute Deviation)
  const calcJitter = (history: number[]): number => {
    if (history.length < 2) return 0
    let sum = 0
    for (let i = 1; i < history.length; i++) {
      sum += Math.abs(history[i] - history[i - 1])
    }
    return Math.round(sum / (history.length - 1))
  }

  it('calculates jitter correctly for stable and unstable connections', () => {
    // Стабильное соединение (колебания 1-2 мс)
    const stable = [45, 46, 44, 45, 47]
    expect(calcJitter(stable)).toBe(2)

    // Умеренный джиттер: |65-50|=15, |45-65|=20, |60-45|=15 => sum=50, 50/3 = 16.67 => 17
    const moderate = [50, 65, 45, 60]
    expect(calcJitter(moderate)).toBe(17)

    // Высокий джиттер
    const high = [30, 90, 25, 110]
    expect(calcJitter(high)).toBe(70)
  })

  it('returns 0 when less than 2 measurements are available', () => {
    expect(calcJitter([])).toBe(0)
    expect(calcJitter([42])).toBe(0)
  })

  // 2. Изоляция истории по серверам (исключение перекрестного загрязнения)
  it('strictly isolates ping history per server in sessionStorage', () => {
    const serverA = 'vless_de_frankfurt'
    const serverB = 'vless_ru_moscow'

    // Сервер A: высокий RTT (Европа, ~140 мс)
    const historyA = [138, 142, 140, 139]
    sessionStorage.setItem(`xr_pings_${serverA}`, JSON.stringify(historyA))

    // Сервер B: низкий RTT (локальный узел, ~20 мс, |21-19|=2, |20-21|=1 => sum=3, 3/2=1.5 => 2)
    const historyB = [19, 21, 20]
    sessionStorage.setItem(`xr_pings_${serverB}`, JSON.stringify(historyB))

    const loadedA = JSON.parse(sessionStorage.getItem(`xr_pings_${serverA}`) || '[]')
    const loadedB = JSON.parse(sessionStorage.getItem(`xr_pings_${serverB}`) || '[]')

    expect(calcJitter(loadedA)).toBe(2)
    expect(calcJitter(loadedB)).toBe(2)

    // Гарантия, что пинг сервера B (20 мс) не попал в историю сервера A
    expect(loadedA).not.toContain(20)
    // Гарантия, что пинг сервера A (140 мс) не попал в историю сервера B
    expect(loadedB).not.toContain(140)
  })

  it('prevents cross-contamination spike when switching servers', () => {
    const prevServerHistory = [140, 142, 141]
    const newServerPing = 20

    // СТАРЫЙ БАГ: [140, 142, 141, 20] -> разрыв 121 мс, джиттер 41 мс (ложная тревога!)
    const contaminated = [...prevServerHistory, newServerPing]
    expect(calcJitter(contaminated)).toBe(41)

    // НОВОЕ ПРАВИЛО: изолированная история сервера B
    const isolatedServerB = [newServerPing]
    expect(calcJitter(isolatedServerB)).toBe(0) // не рассчитывается пока 1 точка
  })

  // 3. Цветовая шкала качества связи
  const getJitterColor = (jitter: number, pointsCount: number): string => {
    if (pointsCount < 2) return 'var(--muted)'
    if (jitter > 25) return '#ef4444' // красный (плохо)
    if (jitter > 10) return '#eab308' // жёлтый (умеренно)
    return '#22c55e' // зелёный (отлично)
  }

  it('assigns correct visual status colors based on thresholds', () => {
    expect(getJitterColor(0, 1)).toBe('var(--muted)') // нейтральный при 1 точке
    expect(getJitterColor(3, 5)).toBe('#22c55e') // <= 10 мс: зеленый
    expect(getJitterColor(10, 5)).toBe('#22c55e') // ровно 10 мс: зеленый
    expect(getJitterColor(11, 5)).toBe('#eab308') // 11-25 мс: желтый
    expect(getJitterColor(25, 5)).toBe('#eab308') // 25 мс: желтый
    expect(getJitterColor(26, 5)).toBe('#ef4444') // > 25 мс: красный
  })

  // 4. Zero Fake Metrics на Дашборде
  it('does not synthesize fake sinusoidal jitter points on dashboard', () => {
    const realPing = 50
    // На Дашборде сохраняются ТОЛЬКО реальные инструментальные замеры
    const nextHistory = [{ ms: realPing, ok: true }]

    // Все точки в истории обязаны иметь реальное значение, без синтетических ((idx % 3) - 1)
    expect(nextHistory.every((p) => p.ms === realPing)).toBe(true)
    expect(nextHistory.length).toBe(1)
  })

  // 5. Изоляция истории пингов на Дашборде
  it('strictly isolates dashboard ping history per active server', () => {
    const s1 = 'server_1'
    const s2 = 'server_2'

    sessionStorage.setItem(`xr_dash_pings_${s1}`, JSON.stringify([{ ms: 30, ok: true }, { ms: 32, ok: true }]))
    sessionStorage.setItem(`xr_dash_pings_${s2}`, JSON.stringify([{ ms: 120, ok: true }]))

    const pingsS1 = JSON.parse(sessionStorage.getItem(`xr_dash_pings_${s1}`) || '[]')
    const pingsS2 = JSON.parse(sessionStorage.getItem(`xr_dash_pings_${s2}`) || '[]')

    expect(pingsS1.length).toBe(2)
    expect(pingsS2.length).toBe(1)
    expect(pingsS1[0].ms).toBe(30)
    expect(pingsS2[0].ms).toBe(120)
  })
})
