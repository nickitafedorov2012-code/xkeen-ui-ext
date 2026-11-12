export type TabId =
  | 'dashboard'
  | 'servers'
  | 'devices'
  | 'whitelist'
  | 'zapret'
  | 'vkturn'
  | 'gaming'
  | 'xbox-dns'
  | 'google-ai'
  | 'connections'
  | 'rules'
  | 'diagnostics'
  | 'settings'
  | 'adguard'
  | 'help'
  | 'antigravity'

const VALID_TABS: ReadonlySet<string> = new Set<TabId>([
  'dashboard',
  'servers',
  'devices',
  'whitelist',
  'zapret',
  'vkturn',
  'gaming',
  'xbox-dns',
  'google-ai',
  'connections',
  'rules',
  'diagnostics',
  'settings',
  'adguard',
  'help',
  'antigravity',
])

export function tabFromHash(hash: string): TabId | null {
  const h = hash.replace(/^#/, '')
  if (!h) return null
  if (h === 'antigravity') return 'google-ai'
  if (h === 'xbox-dns') return 'gaming'
  if (h === 'bs' || h === 'whitelists') return 'whitelist'
  if (VALID_TABS.has(h)) return h as TabId
  return null
}
