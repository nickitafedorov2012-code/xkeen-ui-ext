import { describe, expect, it } from 'vitest'
import { isNewerMihomoVersion } from './MihomoCoreModal'

describe('isNewerMihomoVersion (R52)', () => {
  it('detects genuine newer stable versions', () => {
    expect(isNewerMihomoVersion('v1.19.2', 'v1.19.1')).toBe(true)
    expect(isNewerMihomoVersion('v1.20.0', 'Mihomo Meta v1.19.9')).toBe(true)
  })

  it('never offers a downgrade when current > latest', () => {
    expect(isNewerMihomoVersion('v1.19.2', 'v1.20.0')).toBe(false)
    expect(isNewerMihomoVersion('v1.19.2', 'v1.19.2')).toBe(false)
  })

  it('handles alpha vs stable channels cleanly', () => {
    expect(isNewerMihomoVersion('v1.19.2', 'v1.19.2-alpha')).toBe(true)
    expect(isNewerMihomoVersion('v1.19.2-alpha', 'v1.19.2')).toBe(false)
    expect(isNewerMihomoVersion('Prerelease-Alpha', 'v1.19.2')).toBe(false)
  })
})
