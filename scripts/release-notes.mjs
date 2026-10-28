import { appendFileSync, readFileSync } from 'node:fs';
import { randomUUID } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';

export function releaseNotes(changelog, version) {
  const headings = [...changelog.matchAll(/^## (.+)\r?$/gm)];
  const matches = headings.filter(heading =>
    heading[1].trim().match(/^\[([^\]]+)\] - \d{4}-\d{2}-\d{2}$/)?.[1] === version);
  if (matches.length !== 1)
    throw new Error(`CHANGELOG.md must contain exactly one dated section for [${version}].`);
  const heading = matches[0];
  const next = headings[headings.indexOf(heading) + 1];
  const notes = changelog.slice(heading.index + heading[0].length, next?.index).trim();
  if (!notes.replace(/<!--[\s\S]*?-->/g, '').trim())
    throw new Error(`CHANGELOG.md has no release notes for [${version}].`);
  return notes;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const { version } = JSON.parse(readFileSync(new URL('../app/src-tauri/tauri.conf.json', import.meta.url), 'utf8'));
    const notes = releaseNotes(readFileSync(new URL('../CHANGELOG.md', import.meta.url), 'utf8'), version);
    if (process.env.GITHUB_OUTPUT) {
      const delimiter = `NOTES_${randomUUID()}`;
      appendFileSync(process.env.GITHUB_OUTPUT, `notes<<${delimiter}\n${notes}\n${delimiter}\n`);
    }
    console.log(notes);
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
