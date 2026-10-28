import assert from 'node:assert/strict';
import test from 'node:test';
import { releaseNotes } from './release-notes.mjs';

const changelog = `# Changelog

## [Unreleased]
- Work in progress.

## [0.2.0] - 2026-10-23
### Improvements
- Studio menus.
- Keep \`code\` and $expressions unchanged.

## [0.1.0] - 2026-10-20
- Initial release.
`;

test('extracts only the exact version, preserving Markdown', () => {
  assert.equal(releaseNotes(changelog, '0.2.0'), '### Improvements\n- Studio menus.\n- Keep `code` and $expressions unchanged.');
  assert.equal(releaseNotes(changelog.replaceAll('\n', '\r\n'), '0.1.0'), '- Initial release.');
});

test('refuses missing, duplicate and empty release sections', () => {
  assert.throws(() => releaseNotes(changelog, '0.2'), /exactly one/);
  assert.throws(() => releaseNotes(changelog + '\n## [0.2.0] - 2026-10-23\n- Duplicate.', '0.2.0'), /exactly one/);
  assert.throws(() => releaseNotes('## [0.2.0] - 2026-10-23\n<!-- pending -->\n## [0.1.0] - 2026-10-20\n- Old.', '0.2.0'), /no release notes/);
});

test('supports prerelease versions without matching another release', () => {
  assert.equal(releaseNotes('## [0.3.0-beta.1] - 2026-10-23\n- Preview.\n## [0.3.0] - 2026-10-23\n- Stable.', '0.3.0-beta.1'), '- Preview.');
});
