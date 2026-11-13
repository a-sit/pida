import { afterEach, describe, expect, it } from 'vitest';
import { hintOs, setWebProfile, webProfile } from './web-profile';

describe('hintOs', () => {
  const before = webProfile();
  afterEach(() => setWebProfile(before));

  it('keeps the host platform in the native apps', () => {
    setWebProfile(null);
    expect(hintOs('linux', true)).toBe('linux');
    expect(hintOs('macos', true)).toBe('macos');
  });

  it('follows the browser in the web variant: ⌘ on a Mac, otherwise the server value', () => {
    setWebProfile({ keystore: false });
    expect(hintOs('linux', true)).toBe('macos');
    expect(hintOs('linux', false)).toBe('linux');
  });
});
