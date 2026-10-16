import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname } from 'node:path';

const ALPHABET = 'abcdefghjkmnpqrstuvwxyz23456789';
const DEFAULT_TTL = 30 * 86_400_000;

export class Store {
  constructor(file, { now = Date.now, random = Math.random } = {}) {
    this.file = file;
    this.now = now;
    this.random = random;
    this.links = new Map();
    this.load();
  }

  load() {
    if (!this.file) return;
    try {
      for (const [code, link] of Object.entries(JSON.parse(readFileSync(this.file, 'utf8')))) this.links.set(code, link);
    } catch {
      // No file yet: an empty store.
    }
  }

  save() {
    if (!this.file) return;
    mkdirSync(dirname(this.file), { recursive: true });
    writeFileSync(this.file, JSON.stringify(Object.fromEntries(this.links), null, 2));
  }

  code() {
    let code = '';
    do {
      code = '';
      for (let i = 0; i < 5; i++) code += ALPHABET[Math.floor(this.random() * ALPHABET.length)];
    } while (this.links.has(code));
    return code;
  }

  create(url, ttl = DEFAULT_TTL) {
    const code = this.code();
    const link = { url, created: this.now(), expires: this.now() + ttl };
    this.links.set(code, link);
    this.save();
    return { code, ...link };
  }

  resolve(code) {
    const link = this.links.get(code);
    if (!link) return null;
    if (link.expires <= this.now()) {
      this.links.delete(code);
      this.save();
      return null;
    }
    return link;
  }

  purge() {
    let gone = 0;
    for (const [code, link] of this.links) {
      if (link.expires <= this.now()) {
        this.links.delete(code);
        gone++;
      }
    }
    if (gone) this.save();
    return gone;
  }
}
