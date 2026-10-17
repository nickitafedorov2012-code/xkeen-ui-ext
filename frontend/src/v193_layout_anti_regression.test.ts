import { describe, it, expect } from 'vitest'
import fs from 'fs'
import path from 'path'

describe('v1.9.3 Anti-Regression: Layout, Telemetry & Direction Bug Fixes', () => {
  it('ensures table.devices-table column widths allocate proper space to avoid cramped Zapret/Gaming buttons', () => {
    const cssPath = path.resolve(__dirname, 'styles.css')
    const cssContent = fs.readFileSync(cssPath, 'utf8')

    // Column 2 (Device name) must not be auto-stretched indefinitely
    expect(cssContent).toMatch(/table\.devices-table\s+th:nth-child\(2\)\s*\{\s*width:\s*190px;?\s*\}/)

    // Column 8 (Zapret) must have >= 110px
    expect(cssContent).toMatch(/table\.devices-table\s+th:nth-child\(8\)\s*\{\s*width:\s*112px;/)

    // Column 9 (Gaming) must have >= 105px
    expect(cssContent).toMatch(/table\.devices-table\s+th:nth-child\(9\)\s*\{\s*width:\s*105px;/)

    // Column 10 (Gear/actions) must have >= 70px
    expect(cssContent).toMatch(/table\.devices-table\s+th:nth-child\(10\)\s*\{\s*width:\s*72px;/)
  })

  it('ensures servers-main-content has overflow-x hidden and max-width 100% to prevent horizontal scrollbars', () => {
    const cssPath = path.resolve(__dirname, 'styles.css')
    const cssContent = fs.readFileSync(cssPath, 'utf8')

    expect(cssContent).toMatch(/\.servers-main-content\s*\{[^}]*overflow-x:\s*hidden/s)
    expect(cssContent).toMatch(/\.servers-main-content\s*\{[^}]*max-width:\s*100%/s)
  })

  it('ensures 4-column mode (.cols-4) disables text labels to maintain compact icon-only buttons', () => {
    const cssPath = path.resolve(__dirname, 'styles.css')
    const cssContent = fs.readFileSync(cssPath, 'utf8')

    expect(cssContent).toMatch(/\.cols-4\s+\.btn-label-short\s*\{\s*display:\s*none\s*!important;\s*\}/)
    expect(cssContent).toMatch(/\.cols-4\s+\.server-card-action-primary,\s*\.cols-4\s+\.server-card-action-secondary\s*\{\s*flex-shrink:\s*1;\s*\}/)
  })

  it('ensures Default (PROXY) is renamed to 🌐 Общий активный узел across DeviceRow, Devices, and DeviceRoutingModal', () => {
    const deviceRowPath = path.resolve(__dirname, 'components/DeviceRow.tsx')
    const deviceRowContent = fs.readFileSync(deviceRowPath, 'utf8')
    expect(deviceRowContent).toContain('<option value="default">🌐 Общий активный узел</option>')
    expect(deviceRowContent).not.toContain('<option value="default">Default (PROXY)</option>')

    const devicesPath = path.resolve(__dirname, 'components/Devices.tsx')
    const devicesContent = fs.readFileSync(devicesPath, 'utf8')
    expect(devicesContent).toContain('🌐 Общий активный узел')
    expect(devicesContent).not.toContain('Default (PROXY)')

    const routingModalPath = path.resolve(__dirname, 'components/DeviceRoutingModal.tsx')
    const routingModalContent = fs.readFileSync(routingModalPath, 'utf8')
    expect(routingModalContent).toContain('<option value="PROXY">🌐 Общий активный узел</option>')
    expect(routingModalContent).toContain('Цепочка пуста — устройство использует общий активный узел.')
  })

  it('ensures TrafficGraph hides Proxy badge during active Whitelist mode to prevent duplicate metrics', () => {
    const graphPath = path.resolve(__dirname, 'components/TrafficGraph.tsx')
    const graphContent = fs.readFileSync(graphPath, 'utf8')

    expect(graphContent).toContain('{!isWhitelistActive && (')
    expect(graphContent).toContain('className="traffic-badge proxy"')
  })

  it('ensures Settings priority chain rows use flexWrap nowrap to prevent button dropping to second line', () => {
    const settingsPath = path.resolve(__dirname, 'components/Settings.tsx')
    const settingsContent = fs.readFileSync(settingsPath, 'utf8')

    expect(settingsContent).toContain("flexWrap: 'nowrap'")
    expect(settingsContent).toContain("textOverflow: 'ellipsis'")
  })

  it('ensures Servers.tsx completely hides ignored/banned servers from the main filtered pool', () => {
    const serversPath = path.resolve(__dirname, 'components/Servers.tsx')
    const serversContent = fs.readFileSync(serversPath, 'utf8')

    expect(serversContent).toContain('if (ignored.has(s.id))')
    expect(serversContent).toContain('return false')
  })

  it('ensures Active Node Card displays live telemetry (traffic & sparkline & failover role badge)', () => {
    const serversPath = path.resolve(__dirname, 'components/Servers.tsx')
    const serversContent = fs.readFileSync(serversPath, 'utf8')

    expect(serversContent).toContain('active-node-telemetry')
    expect(serversContent).toContain('Live-трафик:')
    expect(serversContent).toContain('👑 Основной сервер')
    expect(serversContent).toContain('джиттер:')
  })

  it('ensures PROXY and pool groups are filtered from individual device server dropdowns across Devices, DeviceRow, and DeviceRoutingModal', () => {
    const devicesPath = path.resolve(__dirname, 'components/Devices.tsx')
    const devicesContent = fs.readFileSync(devicesPath, 'utf8')
    expect(devicesContent).toContain('const individualServers = useMemo')
    expect(devicesContent).toContain("s.id !== 'PROXY'")

    const deviceRowPath = path.resolve(__dirname, 'components/DeviceRow.tsx')
    const deviceRowContent = fs.readFileSync(deviceRowPath, 'utf8')
    expect(deviceRowContent).toContain("assigned !== 'PROXY'")
    expect(deviceRowContent).toContain("s.id !== 'PROXY'")

    const routingModalPath = path.resolve(__dirname, 'components/DeviceRoutingModal.tsx')
    const routingModalContent = fs.readFileSync(routingModalPath, 'utf8')
    expect(routingModalContent).toContain("!s.is_pool && s.id !== 'PROXY'")
  })

  it('ensures Antigravity.tsx filters PROXY and pool groups from Google Flow server selection and has responsive layout without overflow', () => {
    const agPath = path.resolve(__dirname, 'components/Antigravity.tsx')
    const agContent = fs.readFileSync(agPath, 'utf8')
    expect(agContent).toContain("s.id !== 'PROXY'")
    expect(agContent).toContain("flow_server !== 'PROXY'")
    expect(agContent).toContain("🎯 Выделенный узел Google Flow & AI (только AI)")

    const cssPath = path.resolve(__dirname, 'styles.css')
    const cssContent = fs.readFileSync(cssPath, 'utf8')
    expect(cssContent).toMatch(/\.antigravity-page\s*\{[^}]*overflow-x:\s*hidden/s)
    expect(cssContent).toMatch(/\.antigravity-page\s*\{[^}]*max-width:\s*100%/s)
  })

  it('ensures Gaming.tsx renames PROXY to 🌐 Общий активный узел and filters PROXY from custom servers', () => {
    const gamingPath = path.resolve(__dirname, 'components/Gaming.tsx')
    const gamingContent = fs.readFileSync(gamingPath, 'utf8')
    expect(gamingContent).toContain('<option value="PROXY">🌐 Общий активный узел</option>')
    expect(gamingContent).not.toContain('<option value="PROXY">🔒 PROXY (Основной туннель)</option>')
  })
})
