import { describe, it, expect } from 'vitest';
import { parseCsqttLink } from './csqttLink';

describe('parseCsqttLink', () => {
  it('parses valid v2 csqtt link', () => {
    const link = 'csqtt://connect?v=2&host=203.0.113.88&peer=46000&password=dummy_secret_pass';
    const profile = parseCsqttLink(link);
    expect(profile).not.toBeNull();
    expect(profile?.host).toBe('203.0.113.88');
    expect(profile?.port).toBe(46000);
    expect(profile?.password).toBe('dummy_secret_pass');
  });

  it('parses glued v2 link without &', () => {
    const link = 'csqtt://connect?v=2host=203.0.113.88peer=46000password=dummy_secret_pass';
    const profile = parseCsqttLink(link);
    expect(profile).not.toBeNull();
    expect(profile?.host).toBe('203.0.113.88');
    expect(profile?.port).toBe(46000);
    expect(profile?.password).toBe('dummy_secret_pass');
  });

  it('parses partially glued link', () => {
    const link = 'csqtt://connect?v=2host=1.2.3.4&peer=46000password=pw';
    const profile = parseCsqttLink(link);
    expect(profile).not.toBeNull();
    expect(profile?.host).toBe('1.2.3.4');
    expect(profile?.port).toBe(46000);
    expect(profile?.password).toBe('pw');
  });

  it('parses hashes from v2 link', () => {
    const link = 'csqtt://connect?v=2&host=10.0.0.1&peer=443&password=p&hashes=abc123,def456';
    const profile = parseCsqttLink(link);
    expect(profile).not.toBeNull();
    expect(profile?.host).toBe('10.0.0.1');
    expect(profile?.port).toBe(443);
    expect(profile?.password).toBe('p');
    expect(profile?.hashes).toEqual(['abc123', 'def456']);
  });

  it('parses legacy csqtt://pass@host:port', () => {
    const link = 'csqtt://secret@203.0.113.7:46000';
    const profile = parseCsqttLink(link);
    expect(profile).not.toBeNull();
    expect(profile?.host).toBe('203.0.113.7');
    expect(profile?.port).toBe(46000);
    expect(profile?.password).toBe('secret');
  });

  it('parses legacy csqtt with URL-encoded password', () => {
    const link = 'csqtt://my%40pass@example.com:1000';
    const profile = parseCsqttLink(link);
    expect(profile).not.toBeNull();
    expect(profile?.host).toBe('example.com');
    expect(profile?.port).toBe(1000);
    expect(profile?.password).toBe('my@pass');
  });

  it('parses IPv6 address in brackets', () => {
    const link = 'csqtt://s@[2001:db8::1]:46000';
    const profile = parseCsqttLink(link);
    expect(profile).not.toBeNull();
    expect(profile?.host).toBe('2001:db8::1');
    expect(profile?.port).toBe(46000);
    expect(profile?.password).toBe('s');
  });

  it('returns null for invalid scheme or missing fields', () => {
    expect(parseCsqttLink('https://example.com')).toBeNull();
    expect(parseCsqttLink('csqtt://hostonly')).toBeNull();
    expect(parseCsqttLink('csqtt://connect?v=2&host=h&peer=abc')).toBeNull();
  });
});
