import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { FreeSubPoolSubmenu } from './FreeSubPoolSubmenu'
import { ServerList } from './ServerList'
import * as api from '../../api'
import type { ServerInfo } from '../../types'

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })

vi.mock('../../api', () => ({
  apiGet: vi.fn(),
  apiPost: vi.fn(),
}))

function makeFreeSubServer(overrides: Partial<ServerInfo> = {}): ServerInfo {
  return {
    id: 'free_vpn',
    name: '🆓 Бесплатный VPN (Авто)',
    protocol: 'FREE-VPN',
    host: '1.2.3.4',
    port: 443,
    ping_ms: 85,
    is_active: true,
    is_priority: false,
    is_whitelist: false,
    provider: 'free_vpn',
    now_node: '🇩🇪 Germany — #1',
    ...overrides,
  }
}

describe('FreeSubPoolSubmenu & ServerList free-sub cards', () => {
  let container: HTMLDivElement

  beforeEach(() => {
    vi.clearAllMocks()
    container = document.createElement('div')
    document.body.appendChild(container)
  })

  afterEach(() => {
    container.remove()
  })

  it('loads pool and history when opened and triggers prev/next/blacklist/refill/reconnect', async () => {
    const mockGet = vi.mocked(api.apiGet)
    const mockPost = vi.mocked(api.apiPost)

    mockGet.mockResolvedValue({
      mode: 'vpn',
      provider_id: 'free_vpn',
      active_node: '🇩🇪 Germany — #2',
      pool_index: 1,
      pool_total: 5,
      blacklist_count: 1,
      nodes: [
        { name: '🇩🇪 Germany — #1', host: '1.1.1.1', port: 443, ping_ms: 90, is_active: false },
        { name: '🇩🇪 Germany — #2', host: '2.2.2.2', port: 443, ping_ms: 70, is_active: true },
      ],
      history: [
        {
          node_name: '🇩🇪 Germany — #2',
          host: '2.2.2.2',
          port: 443,
          started_at: 1700000100,
          ended_at: null,
          duration_secs: 45,
          down_bytes: 2048000,
          up_bytes: 128000,
          end_reason: 'Активен',
        },
        {
          node_name: '🇫🇷 France — #1',
          host: '3.3.3.3',
          port: 443,
          started_at: 1700000000,
          ended_at: 1700000090,
          duration_secs: 90,
          down_bytes: 1048576,
          up_bytes: 64000,
          end_reason: 'Обрыв',
        },
      ],
    })
    mockPost.mockResolvedValue({ active_node: '🇩🇪 Germany — #1', started: true })

    const onUpdated = vi.fn()
    const notify = vi.fn()
    const root = createRoot(container)
    try {
      await act(async () => {
        root.render(<FreeSubPoolSubmenu mode="vpn" notify={notify} onUpdated={onUpdated} open={true} />)
      })

      expect(mockGet).toHaveBeenCalledWith('providers/free-sub/pool?mode=vpn')
      expect(container.textContent).toContain('Пул узлов (1 / 5)')
      expect(container.textContent).toContain('🚫 ЧС: 1')
      expect(container.textContent).toContain('🇫🇷 France — #1')
      expect(container.textContent).toContain('Обрыв')

      await act(async () => {
        ;(container.querySelector('[data-testid="free-sub-prev-vpn"]') as HTMLButtonElement).click()
      })
      expect(mockPost).toHaveBeenCalledWith('providers/free-sub/switch-node', { mode: 'vpn', direction: 'prev' })

      await act(async () => {
        ;(container.querySelector('[data-testid="free-sub-next-vpn"]') as HTMLButtonElement).click()
      })
      expect(mockPost).toHaveBeenCalledWith('providers/free-sub/switch-node', { mode: 'vpn', direction: 'next' })

      await act(async () => {
        ;(container.querySelector('[data-testid="free-sub-blacklist-vpn"]') as HTMLButtonElement).click()
      })
      expect(mockPost).toHaveBeenCalledWith('providers/free-sub/blacklist-node', { mode: 'vpn' })

      await act(async () => {
        ;(container.querySelector('[data-testid="free-sub-refill-vpn"]') as HTMLButtonElement).click()
      })
      expect(mockPost).toHaveBeenCalledWith('providers/free-sub/refill', { mode: 'vpn' })

      await act(async () => {
        ;(container.querySelector('[data-testid="free-sub-reconnect-1"]') as HTMLButtonElement).click()
      })
      expect(mockPost).toHaveBeenCalledWith('providers/free-sub/switch-node', {
        mode: 'vpn',
        node_name: '🇫🇷 France — #1',
      })
      expect(onUpdated).toHaveBeenCalled()
    } finally {
      await act(async () => {
        root.unmount()
      })
    }
  })

  it('renders Disconnect, Remove and Pool Submenu toggle on virtual free-sub cards in ServerList', async () => {
    const mockGet = vi.mocked(api.apiGet)
    const mockPost = vi.mocked(api.apiPost)
    mockGet.mockResolvedValue({
      mode: 'vpn',
      provider_id: 'free_vpn',
      active_node: '🇩🇪 Germany — #1',
      pool_index: 0,
      pool_total: 3,
      blacklist_count: 0,
      nodes: [],
      history: [],
    })
    mockPost.mockResolvedValue({ disconnected: true, restored_server: 'my_vless', removed: true })

    const onFreeSubChanged = vi.fn()
    const notify = vi.fn()
    const root = createRoot(container)
    try {
      await act(async () => {
        root.render(
          <ServerList
            loading={false}
            columns={1}
            viewMode="detailed"
            filtered={[makeFreeSubServer()]}
            limit={20}
            pageSize={20}
            setLimit={vi.fn()}
            providerColorMap={new Map()}
            failoverChain={[]}
            ignored={new Set()}
            speedResults={{}}
            speedtestingId={null}
            editingAliasId={null}
            aliasInput=""
            aliasSaving={false}
            setEditingAliasId={vi.fn()}
            setAliasInput={vi.fn()}
            setShareServer={vi.fn()}
            openProvidersModal={vi.fn()}
            saveAlias={vi.fn()}
            saveAliasWithVal={vi.fn()}
            activate={vi.fn()}
            runSpeedtest={vi.fn()}
            toggleWhitelist={vi.fn()}
            makeServerNormal={vi.fn()}
            addToFailover={vi.fn()}
            notify={notify}
            onFreeSubChanged={onFreeSubChanged}
          />
        )
      })

      const disconnectBtn = container.querySelector('[data-testid="free-sub-disconnect-vpn"]') as HTMLButtonElement
      expect(disconnectBtn).not.toBeNull()
      await act(async () => {
        disconnectBtn.click()
      })
      expect(mockPost).toHaveBeenCalledWith('providers/free-sub/disconnect', { mode: 'vpn' })

      const toggleBtn = container.querySelector('[data-testid="free-sub-toggle-vpn"]') as HTMLButtonElement
      expect(toggleBtn).not.toBeNull()
      await act(async () => {
        toggleBtn.click()
      })
      expect(container.querySelector('[data-testid="free-sub-submenu-vpn"]')).not.toBeNull()

      const removeBtn = container.querySelector('[data-testid="free-sub-remove-vpn"]') as HTMLButtonElement
      expect(removeBtn).not.toBeNull()
      await act(async () => {
        removeBtn.click()
      })
      expect(mockPost).toHaveBeenCalledWith('providers/free-sub/remove', { mode: 'vpn' })
    } finally {
      await act(async () => {
        root.unmount()
      })
    }
  })
})
