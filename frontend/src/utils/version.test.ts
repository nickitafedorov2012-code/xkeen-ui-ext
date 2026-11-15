import { describe, expect, it } from 'vitest'
import { compareVersions, isUpdateAvailable } from './version'

describe('version.ts — SemVer & Beta channel comparison', () => {
  it('orders beta.1 < beta.2 < beta.10 < release and handles alpha/rc without dots', () => {
    expect(compareVersions('1.17.0-alpha.1', '1.17.0-beta.1')).toBe(-1)
    expect(compareVersions('1.17.0-beta.1', '1.17.0-beta.2')).toBe(-1)
    expect(compareVersions('1.17.0-beta.2', '1.17.0-beta.10')).toBe(-1)
    expect(compareVersions('1.17.0-beta.10', '1.17.0-beta.2')).toBe(1)
    expect(compareVersions('1.17.0-rc2', '1.17.0-rc10')).toBe(-1)
    expect(compareVersions('1.17.0-beta.10', '1.17.0-rc1')).toBe(-1)
    expect(compareVersions('1.17.0-beta.10', '1.17.0')).toBe(-1)
    expect(compareVersions('1.17.0', '1.17.0-beta.10')).toBe(1)
    expect(compareVersions('1.16.18', '1.16.18.0')).toBe(0)
  })

  it('allows updates within same base version from beta to newer beta or release', () => {
    expect(isUpdateAvailable('1.17.0-beta.1', '1.17.0-beta.2')).toBe(true)
    expect(isUpdateAvailable('1.17.0-beta.2', '1.17.0-beta.10')).toBe(true)
    expect(isUpdateAvailable('1.17.0-beta.1', '1.17.0')).toBe(true)
    expect(isUpdateAvailable('1.17.0-beta.2', '1.17.0-beta.1')).toBe(false)
    expect(isUpdateAvailable('1.17.0', '1.17.0-beta.10')).toBe(false)
  })
})
