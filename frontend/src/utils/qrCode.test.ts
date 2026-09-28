import { describe, it, expect } from 'vitest'
import { generateQrSvg } from './qrCode'

describe('Offline QR Code Generator (SEC-03)', () => {
  it('generates an SVG data URL without any external network calls', () => {
    const result = generateQrSvg('https://example.com', 200)

    // Critical security check: zero external network calls or third-party APIs
    expect(result).not.toContain('api.qrserver.com')
    expect(result).not.toContain('http://api.')
    expect(result).not.toContain('https://api.')

    // Must be a data URI
    expect(result.startsWith('data:image/svg+xml;charset=utf-8,')).toBe(true)

    // Decode and verify SVG structure
    const svg = decodeURIComponent(result.replace('data:image/svg+xml;charset=utf-8,', ''))
    expect(svg).toContain('<svg xmlns="http://www.w3.org/2000/svg"')
    expect(svg).toContain('width="200"')
    expect(svg).toContain('height="200"')
    expect(svg).toContain('<rect width="100%" height="100%" fill="#ffffff"/>')
    expect(svg).toContain('<path d="M')
    expect(svg).toContain('fill="#000000"/>')
    expect(svg).toContain('</svg>')
  })

  it('correctly handles typical proxy protocol URLs (vless, vmess, ss, trojan)', () => {
    const proxyLinks = [
      'vless://550e8400-e29b-41d4-a716-446655440000@198.51.100.1:443?security=reality&sni=example.com&fp=chrome&pbk=xyz123&type=tcp#FastProxy',
      'vmess://ewogICJ2IjogIjIiLAogICJwcyIjogIlByb3h5IiwKICAiYWRkIjogIjE5OC41MS4xMDAuMSIsCiAgInBvcnQiOiA0NDMsCiAgImlkIjogIjU1MGU4NDAwLWUyOWItNDFkNC1hNzE2LTQ0NjY1NTQ0MDAwMCIsCiAgImFpZCI6IDAsCiAgIm5ldCI6ICJ3cyIsCiAgInR5cGUiOiAibm9uZSIKfQ==',
      'ss://YWVzLTI1Ni1nY206cGFzc3dvcmRAMTk4LjUxLjEwMC4xOjgzODg=#Server01',
      'trojan://password123@198.51.100.1:443?security=tls&sni=example.com#TrojanServer',
    ]

    for (const link of proxyLinks) {
      const qr = generateQrSvg(link, 220)
      expect(qr.startsWith('data:image/svg+xml;charset=utf-8,')).toBe(true)
      const svg = decodeURIComponent(qr.replace('data:image/svg+xml;charset=utf-8,', ''))
      expect(svg).toContain('viewBox="0 0 ')
      expect(svg).toContain('width="220"')
      expect(svg).toContain('height="220"')
      expect(svg.length).toBeGreaterThan(100)
    }
  })

  it('respects custom size parameter', () => {
    const qrSmall = generateQrSvg('test', 128)
    const svgSmall = decodeURIComponent(qrSmall.replace('data:image/svg+xml;charset=utf-8,', ''))
    expect(svgSmall).toContain('width="128"')
    expect(svgSmall).toContain('height="128"')

    const qrLarge = generateQrSvg('test', 512)
    const svgLarge = decodeURIComponent(qrLarge.replace('data:image/svg+xml;charset=utf-8,', ''))
    expect(svgLarge).toContain('width="512"')
    expect(svgLarge).toContain('height="512"')
  })

  it('handles empty and unicode strings without throwing', () => {
    expect(() => generateQrSvg('')).not.toThrow()
    expect(() => generateQrSvg('Привет мир! 🚀 测试')).not.toThrow()

    const unicodeQr = generateQrSvg('Привет мир! 🚀')
    expect(unicodeQr.startsWith('data:image/svg+xml;charset=utf-8,')).toBe(true)
  })
})
