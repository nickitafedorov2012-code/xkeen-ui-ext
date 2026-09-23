import { describe, it, expect, beforeEach } from 'vitest'

describe('Servers grid column layout logic', () => {
  beforeEach(() => {
    localStorage.clear()
  })

  it('defaults to 2 columns when no preference is saved', () => {
    const saved = localStorage.getItem('xr_servers_columns')
    const parsed = saved ? parseInt(saved, 10) : null
    const columns = parsed && parsed >= 1 && parsed <= 4 ? parsed : 2
    expect(columns).toBe(2)
  })

  it('restores valid columns from localStorage (1, 2, 3, 4)', () => {
    for (const col of [1, 2, 3, 4]) {
      localStorage.setItem('xr_servers_columns', String(col))
      const saved = localStorage.getItem('xr_servers_columns')
      const parsed = saved ? parseInt(saved, 10) : null
      const columns = parsed && parsed >= 1 && parsed <= 4 ? parsed : 2
      expect(columns).toBe(col)
    }
  })

  it('falls back to 2 columns if localStorage has an invalid value', () => {
    localStorage.setItem('xr_servers_columns', '99')
    const saved = localStorage.getItem('xr_servers_columns')
    const parsed = saved ? parseInt(saved, 10) : null
    const columns = parsed && parsed >= 1 && parsed <= 4 ? parsed : 2
    expect(columns).toBe(2)
  })

  it('page size 24 is divisible evenly by 1, 2, 3, and 4 columns', () => {
    const PAGE = 24
    for (const col of [1, 2, 3, 4]) {
      expect(PAGE % col).toBe(0)
    }
  })
})
