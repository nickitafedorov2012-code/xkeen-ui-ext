import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { FreeSubCard } from './FreeSubCard'
import * as api from '../../api'

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })

vi.mock('../../api', () => ({
  apiGet: vi.fn(),
  apiPost: vi.fn(),
}))

describe('FreeSubCard', () => {
  let container: HTMLDivElement

  beforeEach(() => {
    vi.clearAllMocks()
    container = document.createElement('div')
    document.body.appendChild(container)
  })

  afterEach(() => {
    container.remove()
  })

  it('switches mode between VPN and Whitelist and triggers 1-click connect', async () => {
    const mockGet = vi.mocked(api.apiGet)
    const mockPost = vi.mocked(api.apiPost)

    mockGet.mockResolvedValue({
      status: {
        running: false,
        mode: 'vpn',
        stage: 'idle',
        stage_label: '',
        ttff_ms: null,
        stats: {
          total_raw: 0,
          rejected_invalid: 0,
          rejected_dns: 0,
          deduplicated: 0,
          tcp_checked: 0,
          tcp_passed: 0,
          tcp_retried: 0,
          l7_checked: 0,
          l7_passed: 0,
          working_count: 0,
        },
        active_node: null,
        preserved_active: false,
        source_used: null,
        error: null,
        updated_at: null,
      },
    })
    mockPost.mockResolvedValue({ started: true, mode: 'whitelist' })

    const root = createRoot(container)
    try {
      await act(async () => {
        root.render(<FreeSubCard notify={vi.fn()} />)
      })

      const wlBtn = container.querySelector('[data-testid="free-sub-mode-whitelist"]') as HTMLButtonElement
      expect(wlBtn).not.toBeNull()
      await act(async () => {
        wlBtn.click()
      })

      const connectBtn = container.querySelector('[data-testid="free-sub-connect-btn"]') as HTMLButtonElement
      expect(connectBtn).not.toBeNull()
      await act(async () => {
        connectBtn.click()
      })

      expect(mockPost).toHaveBeenCalledWith('providers/free-sub/connect', { mode: 'whitelist' })
    } finally {
      await act(async () => {
        root.unmount()
      })
    }
  })

  it('displays stage progress, TTFF and diagnostics from status', async () => {
    const mockGet = vi.mocked(api.apiGet)
    mockGet.mockResolvedValue({
      status: {
        running: false,
        mode: 'vpn',
        stage: 'connected',
        stage_label: 'Подключено (VLESS-RU-1) за 412 мс',
        ttff_ms: 412,
        stats: {
          total_raw: 120,
          rejected_invalid: 4,
          rejected_dns: 3,
          deduplicated: 15,
          tcp_checked: 40,
          tcp_passed: 18,
          tcp_retried: 0,
          l7_checked: 6,
          l7_passed: 1,
          working_count: 1,
        },
        active_node: 'VLESS-RU-1',
        preserved_active: true,
        source_used: 'GitVerse RU Mirror',
        error: null,
        updated_at: 1700000000,
      },
    })

    const root = createRoot(container)
    try {
      await act(async () => {
        root.render(<FreeSubCard notify={vi.fn()} />)
      })

      expect(container.querySelector('[data-testid="free-sub-ttff"]')?.textContent).toContain('412 мс')
      expect(container.querySelector('[data-testid="free-sub-stage-label"]')?.textContent).toContain('Активная сессия сохранена')
      const diag = container.querySelector('[data-testid="free-sub-diagnostics"]')?.textContent ?? ''
      expect(diag).toContain('Отсеяно DNS/IP: 3')
      expect(diag).toContain('TCP: 18/40')
      expect(diag).toContain('L7: 1/6')
    } finally {
      await act(async () => {
        root.unmount()
      })
    }
  })
})
