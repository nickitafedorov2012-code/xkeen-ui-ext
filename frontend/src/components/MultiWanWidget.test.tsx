import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { createRoot } from 'react-dom/client'
import { act } from 'react'
import { MultiWanWidget } from './MultiWanWidget'

vi.mock('../api', () => ({
  apiGet: vi.fn(),
}))

import { apiGet } from '../api'

describe('MultiWanWidget', () => {
  let container: HTMLDivElement | null = null
  let root: ReturnType<typeof createRoot> | null = null

  beforeEach(() => {
    container = document.createElement('div')
    document.body.appendChild(container)
    root = createRoot(container)
    vi.clearAllMocks()
    vi.mocked(apiGet).mockResolvedValue({
      enabled: true,
      mode: 'auto',
      single_ip_subscriptions: false,
      active_count: 2,
      total_rx_mbps: 54.3,
      total_tx_mbps: 12.1,
      wan_marks_spec: '0x10000000:50,0x20000000:50',
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
          rx_mbps: 30.0,
          tx_mbps: 7.0,
          rtt_ms: 24,
          loss_pct: 0,
          manual_weight: 50,
          effective_weight: 50,
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
          state: 'online',
          flapping: false,
          rx_mbps: 24.3,
          tx_mbps: 5.1,
          rtt_ms: 31,
          loss_pct: 0,
          manual_weight: 50,
          effective_weight: 50,
        },
      ],
    } as never)
  })

  afterEach(() => {
    act(() => {
      root?.unmount()
    })
    container?.remove()
    container = null
    root = null
  })

  it('renders aggregate speed, per-channel speeds/pings, and Hot-Swap banner on failover', async () => {
    vi.mocked(apiGet).mockResolvedValueOnce({
      enabled: true,
      mode: 'auto',
      single_ip_subscriptions: false,
      active_count: 1,
      total_rx_mbps: 30.0,
      total_tx_mbps: 7.0,
      wan_marks_spec: '0x10000000:100',
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
          rx_mbps: 30.0,
          tx_mbps: 7.0,
          rtt_ms: 24,
          loss_pct: 0,
          manual_weight: 50,
          effective_weight: 100,
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
          state: 'offline',
          flapping: false,
          rx_mbps: 0.0,
          tx_mbps: 0.0,
          rtt_ms: null,
          loss_pct: 100,
          manual_weight: 50,
          effective_weight: 0,
        },
      ],
    } as never)

    await act(async () => {
      root?.render(<MultiWanWidget onSwitchTab={vi.fn()} />)
      await Promise.resolve()
      await Promise.resolve()
    })

    expect(container?.querySelector('[data-testid="multiwan-dashboard-widget"]')).toBeTruthy()
    expect(container?.textContent).toContain('Активно каналов: 1')
    expect(container?.textContent).toContain('Modem-1 (apcli0): 100% · ↓ 30.0 ↑ 7.0 Мбит/с · 24 мс')
    expect(container?.querySelector('[data-testid="multiwan-hotswap-banner"]')?.textContent).toContain('Hot-Swap активен')
  })
})
