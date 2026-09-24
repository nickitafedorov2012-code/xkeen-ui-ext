import { describe, it, expect } from 'vitest'
import { getFlowStatus, getFlowRegion, formatFlowServerName } from './types'

describe('Google Flow keywords and region detection', () => {
  it('identifies US servers correctly', () => {
    const usNodes = [
      '🇺🇸 США Чикаго',
      '🇺🇸 США Вашингтон',
      '🇺🇸 США Майами',
      '🇺🇸 США Лос-Анджелес',
      '🇺🇸 США Сиэтл',
      '🇺🇸 США Атланта',
      '🇺🇸 США Феникс',
      'US-Chicago-VLESS',
      'United States Premium',
      'vless-us-washington',
    ]

    for (const node of usNodes) {
      expect(getFlowStatus(node)).toBe('ok')
      expect(getFlowRegion(node)).toBe('us')
    }
  })

  it('identifies Canada servers correctly', () => {
    const caNodes = [
      '🇨🇦 Канада',
      'Canada-Toronto',
      'CA Fast Proxy',
      '[CA] Montreal Node',
    ]

    for (const node of caNodes) {
      expect(getFlowStatus(node)).toBe('ok')
      expect(getFlowRegion(node)).toBe('ca')
    }
  })

  it('formats Flow server names without duplicate country prefixes', () => {
    expect(formatFlowServerName('🇺🇸 США Лос-Анджелес')).toBe('🇺🇸 [США] Лос-Анджелес')
    expect(formatFlowServerName('США Чикаго')).toBe('🇺🇸 [США] Чикаго')
    expect(formatFlowServerName('Канада')).toBe('🇨🇦 [Канада]')
    expect(formatFlowServerName('🇨🇦 Канада')).toBe('🇨🇦 [Канада]')
    expect(formatFlowServerName('Canada Toronto')).toBe('🇨🇦 [Канада] Toronto')
    expect(formatFlowServerName('🇩🇪 Германия')).toBe('🇩🇪 Германия')
  })

  it('identifies blocked regions correctly', () => {
    const blockedNodes = [
      '🇷🇺 Россия',
      '🇷🇺 Обход [Gold] - Россия',
      '🇫🇮 Финляндия [⚡ Стабильный ]',
      '🇰🇿 Казахстан',
      '🇺🇿 Узбекистан',
      'Мобильный #1 📶 [ ⚡ LTE / 5G ]',
      'Беларусь Минск',
    ]

    for (const node of blockedNodes) {
      expect(getFlowStatus(node)).toBe('blocked')
      expect(getFlowRegion(node)).toBeNull()
    }
  })

  it('returns unknown for neutral foreign regions', () => {
    const neutralNodes = [
      '🇩🇪 Германия',
      '🇳🇱 Нидерланды',
      '🇬🇧 Великобритания',
      '🇯🇵 Япония',
    ]

    for (const node of neutralNodes) {
      expect(getFlowStatus(node)).toBe('unknown')
      expect(getFlowRegion(node)).toBeNull()
    }
  })

  it('handles empty or null-like inputs safely', () => {
    expect(getFlowStatus('')).toBe('unknown')
    expect(getFlowRegion('')).toBeNull()
    expect(formatFlowServerName('')).toBe('')
  })
})
