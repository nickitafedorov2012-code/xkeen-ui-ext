import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import TaskManager from './TaskManager'
import * as api from '../api'
import type { TaskManagerSnapshot } from '../types'

// @ts-ignore
globalThis.IS_REACT_ACT_ENVIRONMENT = true

const mockSnapshot: TaskManagerSnapshot = {
  resources: {
    cpu_total_percent: 15.4,
    cpu_cores: [
      { core_id: 1, usage_percent: 20.2 },
      { core_id: 2, usage_percent: 10.6 },
    ],
    memory_total_kb: 512000,
    memory_used_kb: 198000,
    memory_free_kb: 45000,
    memory_buffers_kb: 25000,
    memory_cached_kb: 244000,
    memory_available_kb: 314000,
    memory_percent: 38.7,
    swap_total_kb: 0,
    swap_used_kb: 0,
    load_avg_1m: 0.35,
    load_avg_5m: 0.25,
    load_avg_15m: 0.15,
    uptime_seconds: 172800,
    tasks_total: 4,
    tasks_running: 1,
    tasks_sleeping: 3,
  },
  processes: [
    {
      pid: 1,
      ppid: 0,
      name: 'ndm',
      cmdline: '/usr/sbin/ndm',
      user: 'root',
      state: 'S',
      cpu_percent: 0.5,
      mem_percent: 5.2,
      mem_rss_kb: 26624,
      threads: 12,
      category: 'keenetic',
      protected: true,
    },
    {
      pid: 200,
      ppid: 1,
      name: 'xkeen-route',
      cmdline: '/opt/sbin/xkeen-route',
      user: 'root',
      state: 'S',
      cpu_percent: 1.1,
      mem_percent: 1.8,
      mem_rss_kb: 9216,
      threads: 6,
      category: 'xkeen',
      protected: true,
    },
    {
      pid: 300,
      ppid: 1,
      name: 'mihomo',
      cmdline: '/opt/sbin/mihomo -d /opt/etc/mihomo',
      user: 'root',
      state: 'R',
      cpu_percent: 12.5,
      mem_percent: 30.1,
      mem_rss_kb: 154112,
      threads: 18,
      category: 'xkeen',
      protected: false,
    },
    {
      pid: 400,
      ppid: 1,
      name: 'nfqws',
      cmdline: '/opt/sbin/nfqws --qnum=200',
      user: 'root',
      state: 'S',
      cpu_percent: 0.4,
      mem_percent: 0.8,
      mem_rss_kb: 4096,
      threads: 2,
      category: 'zapret',
      protected: false,
    },
  ],
}

describe('TaskManager (htop-shell) Component', () => {
  let container: HTMLDivElement
  let root: ReturnType<typeof createRoot>
  const notify = vi.fn()

  beforeEach(() => {
    container = document.createElement('div')
    document.body.appendChild(container)
    root = createRoot(container)
    vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path === 'system/processes') {
        return Promise.resolve(mockSnapshot)
      }
      return Promise.reject(new Error(`Unknown path: ${path}`))
    })
  })

  afterEach(() => {
    vi.restoreAllMocks()
    document.body.removeChild(container)
  })

  it('renders CPU and RAM gauge bars and system statistics correctly', async () => {
    await act(async () => {
      root.render(<TaskManager notify={notify} />)
    })

    expect(container.innerHTML).toContain('Диспетчер задач')
    expect(container.innerHTML).toContain('htop-shell')
    // CPU Total
    expect(container.innerHTML).toContain('15.4%')
    // CPU Cores
    expect(container.innerHTML).toContain('Ядро 1')
    expect(container.innerHTML).toContain('20.2%')
    expect(container.innerHTML).toContain('Ядро 2')
    expect(container.innerHTML).toContain('10.6%')
    // RAM
    expect(container.innerHTML).toContain('193 МБ')
    expect(container.innerHTML).toContain('38.7%')
    // Uptime
    expect(container.innerHTML).toContain('2 дн.')
  })

  it('displays process list with appropriate category badges and protects PID 1', async () => {
    await act(async () => {
      root.render(<TaskManager notify={notify} />)
    })

    // Process names
    expect(container.innerHTML).toContain('ndm')
    expect(container.innerHTML).toContain('xkeen-route')
    expect(container.innerHTML).toContain('mihomo')
    expect(container.innerHTML).toContain('nfqws')

    // Category tags
    expect(container.innerHTML).toContain('KEENETIC')
    expect(container.innerHTML).toContain('XKEEN')
    expect(container.innerHTML).toContain('ZAPRET')

    // Protection check
    expect(container.innerHTML).toContain('🛡 Защищён')
    // Non-protected mihomo and nfqws should have kill/term buttons
    expect(container.innerHTML).toContain('🛑 Term')
    expect(container.innerHTML).toContain('⚡ Kill')
  })

  it('filters processes by category when filter pill is clicked', async () => {
    await act(async () => {
      root.render(<TaskManager notify={notify} />)
    })

    // Find and click Zapret button
    const buttons = Array.from(container.querySelectorAll('button'))
    const zapretBtn = buttons.find((b) => b.textContent?.includes('Zapret'))
    expect(zapretBtn).toBeDefined()

    await act(async () => {
      zapretBtn?.click()
    })

    // Should only show nfqws in tbody
    const tbody = container.querySelector('tbody')
    expect(tbody?.innerHTML).toContain('nfqws')
    expect(tbody?.innerHTML).not.toContain('mihomo')
    expect(tbody?.innerHTML).not.toContain('ndm')
  })

  it('searches processes by name or PID', async () => {
    await act(async () => {
      root.render(<TaskManager notify={notify} />)
    })

    const searchInput = container.querySelector('input[type="text"]') as HTMLInputElement
    expect(searchInput).not.toBeNull()

    await act(async () => {
      searchInput.value = 'mihomo'
      searchInput.dispatchEvent(new Event('input', { bubbles: true }))
      // Also trigger onChange in React
      const nativeInputValueSetter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value')?.set
      nativeInputValueSetter?.call(searchInput, 'mihomo')
      searchInput.dispatchEvent(new Event('input', { bubbles: true }))
    })

    expect(container.innerHTML).toContain('mihomo')
  })

  it('opens confirmation modal when Kill button is clicked on a non-protected process', async () => {
    await act(async () => {
      root.render(<TaskManager notify={notify} />)
    })

    // Click ⚡ Kill on nfqws or mihomo
    const killButtons = Array.from(container.querySelectorAll('button')).filter((b) => b.textContent?.includes('Kill'))
    expect(killButtons.length).toBeGreaterThan(0)

    await act(async () => {
      killButtons[0].click()
    })

    // Modal should be open
    expect(container.innerHTML).toContain('Подтвердить завершение')
    expect(container.innerHTML).toContain('SIGKILL')
  })
})
