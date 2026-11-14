import { describe, it, expect } from 'vitest'
import { kbpsToMbpsInput, mbpsInputToKbps } from './utils/deviceSpeed'

describe('Device speed calculation and unit conversions', () => {
  it('converts kbps to mbps for display', () => {
    expect(kbpsToMbpsInput(undefined)).toBe('')
    expect(kbpsToMbpsInput(0)).toBe('')
    expect(kbpsToMbpsInput(1024)).toBe('1')
    expect(kbpsToMbpsInput(10240)).toBe('10')
    expect(kbpsToMbpsInput(30720)).toBe('30')
    expect(kbpsToMbpsInput(102400)).toBe('100')
    expect(kbpsToMbpsInput(512)).toBe('0.5')
    expect(kbpsToMbpsInput(25600)).toBe('25')
  })

  it('converts user entered mbps string to kbps for API', () => {
    expect(mbpsInputToKbps('')).toBe(0)
    expect(mbpsInputToKbps('0')).toBe(0)
    expect(mbpsInputToKbps('10')).toBe(10240)
    expect(mbpsInputToKbps('35')).toBe(35840)
    expect(mbpsInputToKbps('0.5')).toBe(512)
    expect(mbpsInputToKbps('1,5')).toBe(1536)
    expect(mbpsInputToKbps('100')).toBe(102400)
    expect(mbpsInputToKbps('invalid')).toBe(0)
  })
})
