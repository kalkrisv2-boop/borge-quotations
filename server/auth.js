// Auth Implementation Module — ESM (matches package.json "type": "module")
// Corrected during Phase 1.1 Architect review — see PROJECT_BASELINE.md Session 4.
import crypto from 'node:crypto';

/**
 * Hash a password with PBKDF2-SHA512.
 * Iteration count raised from the originally delivered 10,000 to 100,000 —
 * 10k is below current baseline security guidance for PBKDF2 in 2026.
 */
export function hashPassword(password, salt = crypto.randomBytes(16).toString('hex')) {
  const hash = crypto.pbkdf2Sync(password, salt, 100000, 64, 'sha512').toString('hex');
  return `${salt}$${hash}`;
}

/**
 * Verify a password against a stored "salt$hash" string.
 * Uses timingSafeEqual instead of === to avoid timing side-channel leakage.
 */
export function verifyPassword(password, storedHash) {
  const [salt, originalHash] = storedHash.split('$');
  if (!salt || !originalHash) return false;
  const candidateHash = crypto.pbkdf2Sync(password, salt, 100000, 64, 'sha512').toString('hex');
  const a = Buffer.from(candidateHash, 'hex');
  const b = Buffer.from(originalHash, 'hex');
  if (a.length !== b.length) return false;
  return crypto.timingSafeEqual(a, b);
}

/**
 * Issue an 8-hour session token (JWT-shaped, HMAC-SHA256 signed).
 */
export function generateSessionToken(user, secretKey) {
  const header = base64url(JSON.stringify({ alg: 'HS256', typ: 'JWT' }));
  const payload = base64url(JSON.stringify({
    sub: user.id,
    tenant_id: user.tenant_id,
    role: user.role,
    iat: Math.floor(Date.now() / 1000),
    exp: Math.floor(Date.now() / 1000) + (8 * 3600)
  }));
  const signature = sign(`${header}.${payload}`, secretKey);
  return `${header}.${payload}.${signature}`;
}

/**
 * Verify a session token's signature and expiry, returning the decoded
 * payload on success or null on any failure (bad signature, malformed,
 * expired). This function was missing from the original delivery — a
 * login flow needs both issuance (above) and verification (this) to be
 * a complete auth module; only issuance was provided.
 */
export function verifySessionToken(token, secretKey) {
  if (typeof token !== 'string') return null;
  const parts = token.split('.');
  if (parts.length !== 3) return null;
  const [header, payload, signature] = parts;

  const expectedSig = sign(`${header}.${payload}`, secretKey);
  const a = Buffer.from(signature);
  const b = Buffer.from(expectedSig);
  if (a.length !== b.length || !crypto.timingSafeEqual(a, b)) return null;

  let decoded;
  try {
    decoded = JSON.parse(Buffer.from(payload, 'base64url').toString('utf8'));
  } catch {
    return null;
  }

  if (typeof decoded.exp !== 'number' || Math.floor(Date.now() / 1000) >= decoded.exp) {
    return null; // expired
  }

  return decoded; // { sub, tenant_id, role, iat, exp }
}

// --- internal helpers ---

function base64url(str) {
  return Buffer.from(str).toString('base64url');
}

function sign(data, secretKey) {
  return crypto.createHmac('sha256', secretKey).update(data).digest('base64url');
}
