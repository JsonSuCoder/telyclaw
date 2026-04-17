import { describe, expect, test, vi } from 'vitest';

import {
  DEFAULT_OPENCLAW_SESSION_POLICY_CONFIG,
  OPENCLAW_SESSION_MAINTENANCE,
  OPENCLAW_SESSION_POLICY_STORE_KEY,
  OpenClawSessionKeepAlive,
} from './constants';
import {
  buildOpenClawSessionConfig,
  loadOpenClawSessionPolicyConfig,
  mapKeepAliveToSessionReset,
  normalizeOpenClawSessionPolicyConfig,
  saveOpenClawSessionPolicyConfig,
} from './store';

describe('normalizeOpenClawSessionPolicyConfig', () => {
  test('falls back to default when keepAlive is invalid', () => {
    const config = normalizeOpenClawSessionPolicyConfig({ keepAlive: 'bad-value' });
    expect(config).toEqual(DEFAULT_OPENCLAW_SESSION_POLICY_CONFIG);
  });
});

describe('mapKeepAliveToSessionReset', () => {
  test('maps thirty days to idle reset', () => {
    expect(mapKeepAliveToSessionReset(OpenClawSessionKeepAlive.ThirtyDays)).toEqual({
      mode: 'idle',
      idleMinutes: 43200,
    });
  });
});

describe('buildOpenClawSessionConfig', () => {
  test('uses default policy when omitted', () => {
    expect(buildOpenClawSessionConfig()).toEqual({
      dmScope: 'per-account-channel-peer',
      reset: {
        mode: 'idle',
        idleMinutes: 43200,
      },
      maintenance: OPENCLAW_SESSION_MAINTENANCE,
    });
  });
});

describe('load/save session policy config', () => {
  test('load falls back to default when nothing stored', () => {
    const store = {
      get: vi.fn(() => undefined as undefined),
      set: vi.fn(),
    };

    const result = loadOpenClawSessionPolicyConfig(store);

    expect(store.get).toHaveBeenCalledWith(OPENCLAW_SESSION_POLICY_STORE_KEY);
    expect(result).toEqual(DEFAULT_OPENCLAW_SESSION_POLICY_CONFIG);
  });

  test('save writes normalized configuration', () => {
    const store = {
      get: vi.fn(),
      set: vi.fn(),
    };

    const result = saveOpenClawSessionPolicyConfig(store, { keepAlive: 'bad' });

    expect(store.set).toHaveBeenCalledWith(
      OPENCLAW_SESSION_POLICY_STORE_KEY,
      DEFAULT_OPENCLAW_SESSION_POLICY_CONFIG,
    );
    expect(result).toEqual(DEFAULT_OPENCLAW_SESSION_POLICY_CONFIG);
  });
});
