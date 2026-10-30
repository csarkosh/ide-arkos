import { readFile } from 'node:fs/promises';
import { exec } from 'node:child_process';
import { join } from 'node:path';

export const UPLOADS = new URL('../uploads/', import.meta.url).pathname;

export async function readUpload(name) {
  return readFile(join(UPLOADS, decodeURIComponent(name)));
}

export function describe(name) {
  return new Promise((resolve, reject) => {
    exec(`file -b ${join(UPLOADS, name)}`, (err, out) => (err ? reject(err) : resolve(out.trim())));
  });
}
