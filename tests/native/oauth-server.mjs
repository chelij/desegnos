import assert from 'node:assert/strict';
import { createHash, generateKeyPairSync, randomUUID, sign } from 'node:crypto';

const scopes = 'openid profile email offline_access resource.invoke chatgpt.tokens.use.direct';
const resource = 'https://api.openai.com/v1';

export function oauthFixture(base, registerSecret, errors) {
  const { privateKey, publicKey } = generateKeyPairSync('rsa', { modulusLength: 2048 });
  const jwk = { ...publicKey.export({ format: 'jwk' }), kid: 'desegnos-native-oauth-key', alg: 'RS256', use: 'sig', key_ops: ['verify'] };
  const codes = new Map(); const access = new Map(); const refresh = new Map(); const registrations = new Map();
  const records = []; const callbacks = new Set();
  let mode = { kind: 'complete', profile: 'first', expiresIn: 3600 };
  let refreshHook = null;
  const secret = prefix => { const value = `${prefix}-${randomUUID()}`; registerSecret(value); return value; };
  const json = (response, status, value) => response.writeHead(status, { 'content-type': 'application/json', connection: 'close' }).end(JSON.stringify(value));
  const jwt = claims => {
    const header = Buffer.from(JSON.stringify({ alg: 'RS256', kid: jwk.kid, typ: 'JWT' })).toString('base64url');
    const payload = Buffer.from(JSON.stringify(claims)).toString('base64url');
    const input = `${header}.${payload}`;
    const token = `${input}.${sign('RSA-SHA256', Buffer.from(input), privateKey).toString('base64url')}`;
    registerSecret(token); return token;
  };
  function tokens(registration) {
    const now = Math.floor(Date.now() / 1000);
    const accessToken = secret('synthetic-chatgpt-access');
    const refreshToken = secret('synthetic-chatgpt-refresh');
    access.set(accessToken, registration); refresh.set(refreshToken, registration);
    return { access_token: accessToken, refresh_token: refreshToken, token_type: 'Bearer', expires_in: registration.expiresIn,
      earliest_refresh_at: now,
      scope: registration.permission ? scopes : scopes.replace(' chatgpt.tokens.use.direct', ''),
      id_token: jwt({ iss: base(), aud: registration.clientId, sub: `fixture-user-${registration.profile}`,
        email: 'same-email@desegnos.invalid', iat: now, exp: now + 3600, nonce: registration.nonce }) };
  }
  async function handle(request, response) {
    const url = new URL(request.url, base());
    const path = url.pathname;
    if (!['/.well-known/openid-configuration', '/.well-known/jwks.json', '/api/accounts/authorize', '/api/accounts/oauth/token', '/api/accounts/oauth/revoke'].includes(path)) return false;
    try {
      if (request.method === 'GET' && path === '/.well-known/openid-configuration') {
        records.push({ method: 'GET', path });
        json(response, 200, { issuer: base(), authorization_endpoint: `${base()}/api/accounts/authorize`,
          token_endpoint: `${base()}/api/accounts/oauth/token`, jwks_uri: `${base()}/.well-known/jwks.json`, revocation_endpoint: `${base()}/api/accounts/oauth/revoke` });
      } else if (request.method === 'GET' && path === '/.well-known/jwks.json') {
        records.push({ method: 'GET', path }); json(response, 200, { keys: [jwk] });
      } else if (request.method === 'GET' && path === '/api/accounts/authorize') {
        const params = url.searchParams;
        assert.equal(params.get('response_type'), 'code'); assert.equal(params.get('resource'), resource);
        assert.equal(params.get('scope'), scopes); assert.equal(params.get('code_challenge_method'), 'S256');
        assert.match(params.get('ext_agent_host_id'), /^urn:uuid:[a-f0-9-]{36}$/);
        assert.match(params.get('code_challenge'), /^[A-Za-z0-9_-]{43}$/);
        for (const name of ['state', 'nonce', 'id_token_hint']) if (params.get(name)) registerSecret(params.get(name));
        const redirect = new URL(params.get('redirect_uri'));
        assert.equal(redirect.protocol, 'http:'); assert.equal(redirect.hostname, '127.0.0.1');
        assert.ok(redirect.port); assert.equal(redirect.pathname, '/auth/callback');
        assert.equal(redirect.search, ''); assert.equal(redirect.hash, '');
        const selected = structuredClone(mode);
        const initial = params.get('client_id') === 'dynamic_agent_client';
        if (initial) assert.equal(params.get('agent_name_hint'), 'Desegnos');
        const clientId = initial ? `desegnos-fixture-client-${randomUUID()}` : params.get('client_id');
        const previous = registrations.get(clientId);
        if (!initial) assert.ok(previous, 'Returning sign-in did not reuse an issued registration');
        const registration = { clientId, profile: initial ? selected.profile : previous.profile,
          hostId: params.get('ext_agent_host_id'), nonce: params.get('nonce'), permission: selected.kind !== 'no-permission', expiresIn: selected.expiresIn };
        if (previous) assert.equal(registration.hostId, previous.hostId);
        registrations.set(clientId, registration);
        const code = secret('synthetic-oauth-code');
        codes.set(code, { registration, redirect: redirect.href, challenge: params.get('code_challenge') });
        records.push({ method: 'GET', path, initial, clientId, hostId: registration.hostId,
          profile: registration.profile, kind: selected.kind, idTokenHint: params.has('id_token_hint'), callbackPath: redirect.pathname });
        response.writeHead(200, { 'content-type': 'text/plain', connection: 'close' }).end('Isolated native OAuth browser fixture accepted.');
        if (selected.kind !== 'pending') {
          redirect.searchParams.set('state', params.get('state')); redirect.searchParams.set('code', code);
          if (initial) redirect.searchParams.set('client_id', clientId);
          const callback = fetch(redirect, { signal: AbortSignal.timeout(5000) })
            .then(async result => { assert.equal(result.status, 200); await result.text(); })
            .catch(error => { errors.push(`Isolated OAuth callback failed: ${error.name}`); })
            .finally(() => callbacks.delete(callback));
          callbacks.add(callback);
        }
      } else if (request.method === 'POST' && path === '/api/accounts/oauth/token') {
        const chunks = []; let size = 0;
        for await (const chunk of request) { size += chunk.length; assert.ok(size <= 128 * 1024); chunks.push(chunk); }
        const form = new URLSearchParams(Buffer.concat(chunks).toString('utf8'));
        assert.equal(form.get('resource'), resource); assert.equal(form.has('scope'), false);
        if (form.get('grant_type') === 'authorization_code') {
          const pending = codes.get(form.get('code')); assert.ok(pending, 'Unknown or reused authorization code');
          codes.delete(form.get('code'));
          assert.equal(form.get('client_id'), pending.registration.clientId);
          assert.equal(form.get('redirect_uri'), pending.redirect);
          const verifier = form.get('code_verifier'); registerSecret(verifier);
          assert.equal(createHash('sha256').update(verifier).digest('base64url'), pending.challenge);
          records.push({ method: 'POST', path, grantType: 'authorization_code', clientId: pending.registration.clientId,
            pkceValidated: true, redirectValidated: true, dynamicClientExcluded: true });
          json(response, 200, tokens(pending.registration));
        } else {
          assert.equal(form.get('grant_type'), 'refresh_token');
          const previous = refresh.get(form.get('refresh_token'));
          assert.ok(previous, 'Unknown or reused refresh token'); assert.equal(form.get('client_id'), previous.clientId);
          refresh.delete(form.get('refresh_token'));
          previous.expiresIn = 3600;
          records.push({ method: 'POST', path, grantType: 'refresh_token', clientId: previous.clientId, replacement: true });
          if (refreshHook) await refreshHook();
          json(response, 200, tokens(previous));
        }
      } else if (request.method === 'POST' && path === '/api/accounts/oauth/revoke') {
        const chunks = []; for await (const chunk of request) chunks.push(chunk);
        const form = new URLSearchParams(Buffer.concat(chunks).toString('utf8'));
        assert.equal(form.get('token_type_hint'), 'refresh_token');
        const previous = refresh.get(form.get('token'));
        if (previous) { assert.equal(form.get('client_id'), previous.clientId); refresh.delete(form.get('token')); }
        for (const [token, identity] of access) if (identity.clientId === form.get('client_id')) access.delete(token);
        records.push({ method: 'POST', path, clientId: form.get('client_id'), confirmed: true });
        response.writeHead(200, { connection: 'close' }).end();
      } else { throw new Error('Unexpected OAuth method'); }
    } catch (error) {
      errors.push(`OAuth fixture contract failed: ${error.message}`);
      if (!response.headersSent) json(response, 400, { error: 'fixture_contract_failure' }); else response.end();
    }
    return true;
  }
  return {
    handle,
    authenticate(header) { return typeof header === 'string' && header.startsWith('Bearer ') ? access.get(header.slice(7)) ?? null : null; },
    setMode(kind, profile = 'first', options = {}) { assert.ok(['complete', 'pending', 'no-permission'].includes(kind)); mode = { kind, profile, expiresIn: options.expiresIn ?? 3600 }; },
    records: () => structuredClone(records),
    setRefreshHook(hook) { refreshHook = hook; },
    async settle() { await Promise.allSettled([...callbacks]); },
  };
}
