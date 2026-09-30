// Keep the server CSP free of unsafe-inline and build-dependent hashes.
import { readFile, writeFile, readdir } from 'node:fs/promises';
import { join } from 'node:path';
import { createHash } from 'node:crypto';
async function visit(dir) {
  for (const item of await readdir(dir, { withFileTypes: true })) {
    const file = join(dir, item.name);
    if (item.isDirectory()) await visit(file);
    else if (item.name.endsWith('.html')) {
      const original = await readFile(file, 'utf8');
      const scripts = [...original.matchAll(/<script>([\s\S]*?)<\/script>/g)];
      let html = original;
      for (const [tag, script] of scripts) {
        const name = `bootstrap-${createHash('sha256').update(script).digest('hex').slice(0, 16)}.js`;
        await writeFile(join('build', '_app', name), script);
        html = html.replace(tag, `<script src="/_app/${name}"></script>`);
      }
      await writeFile(file, html);
    }
  }
}
await visit('build');

await writeFile('build/index.html', await readFile('build/200.html'));
