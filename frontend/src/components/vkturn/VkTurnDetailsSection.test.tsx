import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { createRoot } from 'react-dom/client'
import { act } from 'react'
import { VkTurnDetailsSection } from './VkTurnDetailsSection'

vi.mock('../../api', () => ({
  apiGet: vi.fn(),
}))

import { apiGet } from '../../api'

describe('VkTurnDetailsSection', () => {
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
      total_rx_mbps: 10,
      total_tx_mbps: 2,
      wan_marks_spec: '0x10000000:50,0x20000000:50',
      last_reconcile_ts: 1700000000,
      last_error: null,
      channels: [
        {
          rci_id: 'WifiMaster0/WifiStation0',
          name: 'WISP Wi-Fi 2.4 ГГц',
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
          rx_mbps: 5,
          tx_mbps: 1,
          rtt_ms: 20,
          loss_pct: 0,
          manual_weight: 50,
          effective_weight: 50,
        },
        {
          rci_id: 'WifiMaster1/WifiStation0',
          name: 'WISP Wi-Fi 5 ГГц',
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
          rx_mbps: 5,
          tx_mbps: 1,
          rtt_ms: 25,
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

  it('formats wan_streams using human-readable channel names instead of raw hex marks', async () => {
    await act(async () => {
      root?.render(
        <VkTurnDetailsSection
          status={{
            installed: true,
            core_version: 'v2.5.0',
            phase: 'connected',
            connected_streams: 50,
            target_streams: 50,
            bytes_up: 0,
            bytes_down: 0,
            links_count: 1,
            links: [],
            max_links: 5,
            restarts: 0,
            enabled: true,
            peer: '1.2.3.4:46000',
            default_port: 46000,
            has_password: true,
            tunnel_ready: true,
            interface: 'csq0',
            turn_transport: 'udp',
            active_transport: 'udp',
            wan_streams: {
              '0x10000000': 25,
              '0x20000000': 25,
            },
          }}
          phase="connected"
          showPasswordFix={false}
          accordionOpen={true}
          setAccordionOpen={vi.fn()}
          fetchLogs={vi.fn()}
          loadingLogs={false}
          logs={[]}
          maxLinks={5}
          showManualLinksSection={false}
          newLinkInput=""
          setNewLinkInput={vi.fn()}
          updatingLink={false}
          handleUpdateCallLink={vi.fn()}
          handleRemoveLink={vi.fn()}
          editPeerInput=""
          setEditPeerInput={vi.fn()}
          peerEditedRef={{ current: false }}
          updatingPeer={false}
          handleUpdatePeer={vi.fn()}
          newPassword=""
          setNewPassword={vi.fn()}
          updatingPassword={false}
          handleUpdatePassword={vi.fn()}
          actionPending={false}
          targetStreams={50}
          displayedStreamsStr="50"
          rangeStreams={50}
          canSaveStreams={false}
          streamsUpdating={false}
          setDraftStreamsInput={vi.fn()}
          handleSaveStreams={vi.fn()}
          transportUpdating={false}
          handleSaveTurnTransport={vi.fn()}
        />
      )
      await Promise.resolve()
      await Promise.resolve()
    })

    const badge = container?.querySelector('[data-testid="active-transport-badge"]')?.textContent ?? ''
    expect(badge).toContain('Канал 1 (WISP Wi-Fi 2.4 ГГц): 25 пот.')
    expect(badge).toContain('Канал 2 (WISP Wi-Fi 5 ГГц): 25 пот.')
    expect(badge).not.toContain('0x10000000')
    expect(badge).not.toContain('0x20000000')
  })
})
