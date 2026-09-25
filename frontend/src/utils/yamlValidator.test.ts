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
})
