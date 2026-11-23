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

  it('disables prev/next when pool_total <= 1 and shows refill progress', async () => {
    const mockGet = vi.mocked(api.apiGet)
    mockGet.mockResolvedValue({
      mode: 'whitelist',
      provider_id: 'free_wl',
      active_node: 'OnlyNode',
      pool_index: 1,
      pool_total: 1,
      blacklist_count: 0,
      nodes: [{ name: 'OnlyNode', host: '1.1.1.1', port: 443, ping_ms: 120, is_active: true }],
      history: [],
    })

    const root = createRoot(container)
    try {
      await act(async () => {
        root.render(
          <FreeSubPoolSubmenu
            mode="whitelist"
            notify={vi.fn()}
            onUpdated={vi.fn()}
            open={true}
            freeSubStatus={{
              running: true,
              mode: 'whitelist',
              stage: 'l7',
              stage_label: 'Точечная L7-проверка (8 кандидатов)…',
              ttff_ms: null,
              stats: {
                total_raw: 100,
                rejected_invalid: 0,
                rejected_dns: 0,
                deduplicated: 0,
                tcp_checked: 20,
                tcp_passed: 8,
                tcp_retried: 0,
                l7_checked: 3,
                l7_passed: 1,
                working_count: 1,
              },
              active_node: 'OnlyNode',
              preserved_active: false,
              source_used: 'Yandex Disk',
              error: null,
              updated_at: 1700000000,
            }}
          />
        )
      })

      const prevBtn = container.querySelector('[data-testid="free-sub-prev-whitelist"]') as HTMLButtonElement
      const nextBtn = container.querySelector('[data-testid="free-sub-next-whitelist"]') as HTMLButtonElement
      expect(prevBtn.disabled).toBe(true)
      expect(nextBtn.disabled).toBe(true)
      const progressBanner = container.querySelector('[data-testid="free-sub-submenu-progress-whitelist"]')
      expect(progressBanner?.textContent).toContain('Точечная L7-проверка')
    } finally {
      await act(async () => {
        root.unmount()
      })
    }
  })

  it('awaits activate(s) before polling status and renders live progress in ServerList card', async () => {
    const mockGet = vi.mocked(api.apiGet)
    let resolveActivate!: () => void
    const activate = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          resolveActivate = resolve
        }),
    )

    mockGet.mockResolvedValue({
      status: {
        running: false,
        mode: 'whitelist',
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

    const wlServer = makeFreeSubServer({
      id: 'free_wl',
      name: '🛡️ Бесплатный обход БС (Авто)',
      protocol: 'FREE-WL',
      provider: 'free_wl',
      is_active: false,
      is_whitelist: true,
    })

    const root = createRoot(container)
    try {
      await act(async () => {
        root.render(
          <ServerList
            loading={false}
            columns={1}
            viewMode="detailed"
            filtered={[wlServer]}
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
            activate={activate}
            runSpeedtest={vi.fn()}
            toggleWhitelist={vi.fn()}
            makeServerNormal={vi.fn()}
            addToFailover={vi.fn()}
            notify={vi.fn()}
            onFreeSubChanged={vi.fn()}
          />
        )
      })

      mockGet.mockClear()
      mockGet.mockResolvedValue({
        status: {
          running: true,
          mode: 'whitelist',
          stage: 'l7',
          stage_label: 'Точечная L7-проверка (12 кандидатов)…',
          ttff_ms: null,
          stats: {
            total_raw: 160,
            rejected_invalid: 0,
            rejected_dns: 0,
            deduplicated: 0,
            tcp_checked: 24,
            tcp_passed: 12,
            tcp_retried: 0,
            l7_checked: 4,
            l7_passed: 0,
            working_count: 0,
          },
          active_node: null,
          preserved_active: false,
          source_used: 'Yandex Disk',
          error: null,
          updated_at: 1700000000,
        },
      })
      const connectBtn = container.querySelector('.btn-connect') as HTMLButtonElement
      expect(connectBtn).not.toBeNull()
      expect(connectBtn.disabled).toBe(false)

      await act(async () => {
        connectBtn.click()
      })

      // Пока activate(s) ещё не завершился, статус НЕ должен опрашиваться раньше времени,
      // но оптимистичный прогресс уже отображается!
      expect(activate).toHaveBeenCalledTimes(1)
      expect(mockGet).not.toHaveBeenCalledWith('providers/free-sub/status')
      expect(container.querySelector('[data-testid="free-sub-progress-whitelist"]')?.textContent).toContain(
        'Запуск поиска рабочих узлов',
      )

      await act(async () => {
        resolveActivate()
      })

      expect(mockGet).toHaveBeenCalledWith('providers/free-sub/status')
      const progressText = container.querySelector('[data-testid="free-sub-progress-whitelist"]')?.textContent ?? ''
      expect(progressText).toContain('Точечная L7-проверка (12 кандидатов)…')
      expect(progressText).toContain('TCP: 12/24')
      expect(progressText).toContain('L7: 0/4')
    } finally {
      await act(async () => {
        root.unmount()
      })
    }
  })

  it('shows connect button when pool_total === 0 and triggers providers/free-sub/connect', async () => {
    const mockGet = vi.mocked(api.apiGet)
    const mockPost = vi.mocked(api.apiPost)
    mockGet.mockResolvedValue({
      mode: 'whitelist',
      provider_id: 'free_wl',
      active_node: null,
      pool_index: 0,
      pool_total: 0,
      blacklist_count: 18,
      nodes: [],
      history: [],
    })
    mockPost.mockResolvedValue({ started: true })

    const onJobStarted = vi.fn()
    const root = createRoot(container)
    try {
      await act(async () => {
        root.render(
          <FreeSubPoolSubmenu
            mode="whitelist"
            notify={vi.fn()}
            onUpdated={vi.fn()}
            open={true}
            onJobStarted={onJobStarted}
          />
        )
      })

      const connectEmptyBtn = container.querySelector(
        '[data-testid="free-sub-connect-empty-whitelist"]',
      ) as HTMLButtonElement
      expect(connectEmptyBtn).not.toBeNull()
      await act(async () => {
        connectEmptyBtn.click()
      })
      expect(mockPost).toHaveBeenCalledWith('providers/free-sub/connect', { mode: 'whitelist' })
      expect(onJobStarted).toHaveBeenCalledWith('whitelist')
    } finally {
      await act(async () => {
        root.unmount()
      })
    }
  })
})
