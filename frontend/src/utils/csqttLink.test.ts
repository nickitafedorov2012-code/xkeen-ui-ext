import { describe, it, expect } from 'vitest';
import { parseCsqttLink } from './csqttLink';

describe('parseCsqttLink', () => {
  it('rejects profile hosts the backend cannot use', () => {
    for (const host of ['localhost', '127.0.0.1', '0.0.0.0', 'example.com%2Fwrong', 'bad%20host']) {
      expect(parseCsqttLink(`csqtt://connect?v=2&host=${host}&peer=46000&password=p`)).toBeNull();
    }
  });
  it('rejects duplicate, short, empty or excessive reference hashes', () => {
    const prefix = 'csqtt://connect?v=2&host=example.com&peer=46000&password=p&hashes=';
    expect(parseCsqttLink(prefix + 'short')).toBeNull();
    expect(parseCsqttLink(prefix + 'abcdefghijklmnop+abcdefghijklmnop')).toBeNull();
    expect(parseCsqttLink(prefix + 'abcdefghijklmnop+')).toBeNull();
    expect(parseCsqttLink(prefix + Array.from({ length: 7 }, (_, i) => `abcdefghijklmnop${i}`).join('+'))).toBeNull();
  });
  it('rejects an ambiguous glued profile instead of truncating its password', () => {
    expect(parseCsqttLink('csqtt://connect?v=2host=example.compeer=46000password=abchashes=abcdefghijklmnop')).toBeNull();
  });
  it('rejects unsupported versions and repeated or conflicting parameters', () => {
    expect(parseCsqttLink('csqtt://connect?v=3&host=example.com&peer=46000&password=p')).toBeNull();
    expect(parseCsqttLink('csqtt://connect?v=2&host=example.com&host=other.example&peer=46000&password=p')).toBeNull();
    expect(parseCsqttLink('csqtt://connect?v=2&host=example.com&peer=46000&password=p&token=t&hashes=abcdefghijklmnop')).toBeNull();
    expect(parseCsqttLink('csqtt://connect?v=2&host=example.com&peer=46000&password=p&power=20&workers=30')).toBeNull();
  });
  it('preserves profile token and worker limit', () => {
    const profile = parseCsqttLink('csqtt://connect?v=2&host=example.com&peer=46000&password=p&token=synthetic-token&workers=27');
    expect(profile).toMatchObject({ token: 'synthetic-token', power: 27 });
  });
  it('splits reference hashes before decoding their components', () => {
    const profile = parseCsqttLink('csqtt://connect?v=2&host=example.com&peer=46000&password=p&hashes=abcdefghijklmnop+qrstuvwxyzABCDEF');
    expect(profile?.hashes).toEqual(['abcdefghijklmnop', 'qrstuvwxyzABCDEF']);
    const encoded = parseCsqttLink('csqtt://connect?v=2&host=example.com&peer=46000&password=p&hashes=abcdefghijklmnop%2Bqrst');
    expect(encoded).toBeNull(); // encoded plus is data, and is not accepted by the backend as a hash
  });
  it('rejects malformed and out-of-range profile ports', () => {
    expect(parseCsqttLink('csqtt://connect?v=2&host=example.com&peer=46000oops&password=p')).toBeNull();
    expect(parseCsqttLink('csqtt://p@example.com:99999')).toBeNull();
    expect(parseCsqttLink('csqtt://p@example.com:46000oops')).toBeNull();
  });
  it('preserves literal plus and parameter-like text in a v2 password', () => {
    for (const password of ['abchost=xyz', 'abc+xyz', 'abc%2Bxyz']) {
      const profile = parseCsqttLink(`csqtt://connect?v=2&host=example.com&peer=46000&password=${password}`);
      expect(profile?.password).toBe(password === 'abc%2Bxyz' ? 'abc+xyz' : password);
    }
  });
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
    const link = 'csqtt://connect?v=2&host=10.0.0.1&peer=443&password=p&hashes=abcdefghijklmnop,qrstuvwxyzABCDEF';
    const profile = parseCsqttLink(link);
    expect(profile).not.toBeNull();
    expect(profile?.host).toBe('10.0.0.1');
    expect(profile?.port).toBe(443);
    expect(profile?.password).toBe('p');
    expect(profile?.hashes).toEqual(['abcdefghijklmnop', 'qrstuvwxyzABCDEF']);
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
