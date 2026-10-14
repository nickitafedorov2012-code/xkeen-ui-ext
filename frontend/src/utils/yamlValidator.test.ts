import { describe, it, expect } from 'vitest'
import { validateYaml } from './yamlValidator'

describe('yamlValidator', () => {
  it('passes on valid simple yaml', () => {
    const yaml = `
port: 7890
socks-port: 7891
mode: rule
rules:
  - DOMAIN-SUFFIX,google.com,PROXY
`
    const res = validateYaml(yaml)
    expect(res.valid).toBe(true)
  })

  it('detects tab characters', () => {
    const yaml = `
port: 7890
\tmode: rule
`
    const res = validateYaml(yaml)
    expect(res.valid).toBe(false)
    expect(res.error).toContain('табуляция')
  })

  it('detects unclosed double quotes', () => {
    const yaml = `
name: "my node
port: 443
`
    const res = validateYaml(yaml)
    expect(res.valid).toBe(false)
    expect(res.error).toContain('двойная кавычка')
  })

  it('detects unclosed brackets', () => {
    const yaml = `
proxies: [
  - name: node1
`
    const res = validateYaml(yaml)
    expect(res.valid).toBe(false)
    expect(res.error).toContain('незакрытая скобка')
  })

  it('detects missing space after colon', () => {
    const yaml = `
key:value
`
    const res = validateYaml(yaml)
    expect(res.valid).toBe(false)
    expect(res.error).toContain('пропущен пробел после двоеточия')
  })

  it('correctly handles doubled single quotes inside strings', () => {
    const yaml = `
name: 'It''s a valid node name'
port: 443
`
    const res = validateYaml(yaml)
    expect(res.valid).toBe(true)
  })

  it('allows URL schemes and IPv6 addresses without false-positive colon errors (FE-11)', () => {
    const yaml = `
proxies:
  - vless://550e8400@198.51.100.1:443?type=tcp
  - tls://example.com:8443
  - hysteria2://pass@example.com:443
server: 2606:4700::1
`
    const res = validateYaml(yaml)
    expect(res.valid).toBe(true)
  })
})
