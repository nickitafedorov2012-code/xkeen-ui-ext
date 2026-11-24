import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { createRoot } from 'react-dom/client'
import { act } from 'react'
import { MultiWanCard } from './MultiWanCard'

vi.mock('../../api', () => ({
  apiGet: vi.fn(),
  apiPost: vi.fn(),
}))

import { apiGet, apiPost } from '../../api'

const sampleStatus = {
  enabled: true,
  mode: 'manual',
  single_ip_subscriptions: true,
  active_count: 2,
  total_rx_mbps: 42.5,
  total_tx_mbps: 11.2,
  wan_marks_spec: '0x10000000:60@192.168.10.22,0x20000000:40@192.168.2.52',
  last_reconcile_ts: 1700000000,
  last_error: null,
  channels: [
    {
      rci_id: 'WifiMaster0/WifiStation0',
      name: 'Modem-1',
      kind: 'wisp',
      linux_iface: 'apcli0',
      ipv4: '192.168.10.22',
      gateway: '192.168.10.1',
      slot: 0,
      table_id: 180,
      fwmark_hex: '0x10000000',
      enabled: true,
      state: 'online',
      flapping: false,
      rx_mbps: 25.0,
      tx_mbps: 6.0,
      rtt_ms: 28,
      loss_pct: 0,
      manual_weight: 60,
      effective_weight: 60,
    },
    {
      rci_id: 'WifiMaster1/WifiStation0',
      name: 'Modem-2',
      kind: 'wisp',
      linux_iface: 'apcli1',
      ipv4: '192.168.2.52',
      gateway: '192.168.2.1',
      slot: 1,
      table_id: 181,
      fwmark_hex: '0x20000000',
      enabled: true,
      state: 'standby',
      flapping: false,
      rx_mbps: 17.5,
      tx_mbps: 5.2,
      rtt_ms: 35,
      loss_pct: 0,
      manual_weight: 0,
      effective_weight: 0,
    },
  ],
}

describe('MultiWanCard', () => {
  let container: HTMLDivElement | null = null
  let root: ReturnType<typeof createRoot> | null = null

  beforeEach(() => {
    container = document.createElement('div')
    document.body.appendChild(container)
    root = createRoot(container)
    vi.clearAllMocks()
    vi.mocked(apiGet).mockResolvedValue(sampleStatus as never)
    vi.mocked(apiPost).mockResolvedValue(sampleStatus as never)
  })

  afterEach(() => {
    act(() => {
      root?.unmount()
    })
    container?.remove()
    container = null
    root = null
  })

  it('renders channels, weights, standby badge, collision/flapping warnings, and toggles mode', async () => {
    vi.mocked(apiGet).mockResolvedValueOnce({
      ...sampleStatus,
      gateway_collision_warning: 'Обнаружена коллизия адресов модемов: совпадает шлюз 192.168.8.1',
      channels: [
        sampleStatus.channels[0],
        {
          ...sampleStatus.channels[1],
          state: 'quarantine',
          flapping: true,
        },
      ],
    } as never)

    const notify = vi.fn()
    await act(async () => {
      root?.render(<MultiWanCard notify={notify} />)
      await Promise.resolve()
      await Promise.resolve()
    })

    expect(container?.querySelector('[data-testid="multiwan-channel-apcli0"]')).toBeTruthy()
    expect(container?.querySelector('[data-testid="multiwan-channel-apcli1"]')).toBeTruthy()
    expect(container?.textContent).toContain('⚡ Автоматически')
    expect(container?.textContent).toContain('🎛️ Вручную (%)')
    expect(container?.textContent).toContain('🟢 Активен')
    expect(container?.textContent).toContain('🟡 Нестабилен')
    expect(container?.textContent).toContain('🛡️ Защита платных подписок от бана за 2 IP')
    expect(container?.textContent).toContain('Доля: 60%')
    expect(container?.querySelector('[data-testid="multiwan-collision-warning"]')?.textContent).toContain('192.168.8.1')
    expect(container?.querySelector('[data-testid="multiwan-flapping-warning-apcli1"]')?.textContent).toContain(
      'Канал часто обрывается — проверьте питание USB-хаба или уровень сигнала'
    )

    const autoBtn = container?.querySelector('[data-testid="multiwan-mode-auto"]') as HTMLButtonElement
    await act(async () => {
      autoBtn.click()
      await Promise.resolve()
    })
    expect(apiPost).toHaveBeenCalledWith('multiwan/config', { mode: 'auto' })
  })
})
