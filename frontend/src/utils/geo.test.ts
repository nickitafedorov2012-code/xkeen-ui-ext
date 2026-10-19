import { describe, it, expect } from 'vitest'
import { detectServerCountry, countryCodeToFlag, flagToCountryCode } from './geo'

describe('Geo utility — detectServerCountry and flags', () => {
  it('converts ISO country code to flag emoji and vice versa', () => {
    expect(countryCodeToFlag('DE')).toBe('🇩🇪')
    expect(countryCodeToFlag('NL')).toBe('🇳🇱')
    expect(countryCodeToFlag('GB')).toBe('🇬🇧')
    expect(countryCodeToFlag('US')).toBe('🇺🇸')

    expect(flagToCountryCode('🇩🇪')).toBe('DE')
    expect(flagToCountryCode('🇳🇱')).toBe('NL')
    expect(flagToCountryCode('🇬🇧')).toBe('GB')
    expect(flagToCountryCode('🇺🇸')).toBe('US')
  })

  it('detects country from existing emoji flags in server name', () => {
    const s1 = detectServerCountry('🇩🇪 Обход [Platinum] - Германия')
    expect(s1).not.toBeNull()
    expect(s1?.code).toBe('DE')
    expect(s1?.flag).toBe('🇩🇪')

    const s2 = detectServerCountry('🇳🇱 Обход [Cooper] - Нидерланды')
    expect(s2?.code).toBe('NL')
    expect(s2?.flag).toBe('🇳🇱')

    const s3 = detectServerCountry(' 🇵🇱 ⚡ Польша')
    expect(s3?.code).toBe('PL')
    expect(s3?.flag).toBe('🇵🇱')
  })

  it('detects country from prefixes like ObsV-gb_real_dir and ObsV-ne_fwd', () => {
    const s1 = detectServerCountry('ObsV-gb_real_dir')
    expect(s1?.code).toBe('GB')
    expect(s1?.flag).toBe('🇬🇧')

    const s2 = detectServerCountry('ObsV-de_real')
    expect(s2?.code).toBe('DE')
    expect(s2?.flag).toBe('🇩🇪')

    const s3 = detectServerCountry('ObsV-ne_fwd')
    expect(s3?.code).toBe('NL')
    expect(s3?.flag).toBe('🇳🇱')
  })

  it('detects country from Cyrillic names without emoji flags', () => {
    const s1 = detectServerCountry('Грузия *')
    expect(s1?.code).toBe('GE')
    expect(s1?.flag).toBe('🇬🇪')

    const s2 = detectServerCountry('Финляндия Основной')
    expect(s2?.code).toBe('FI')
    expect(s2?.flag).toBe('🇫🇮')

    const s3 = detectServerCountry('Франция Париж #2')
    expect(s3?.code).toBe('FR')
    expect(s3?.flag).toBe('🇫🇷')
  })

  it('returns null for unknown or generic names', () => {
    expect(detectServerCountry('Direct')).toBeNull()
    expect(detectServerCountry('Fallback')).toBeNull()
    expect(detectServerCountry('')).toBeNull()
  })
})
