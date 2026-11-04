import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

// Fixtures HTTP independentes do Vite usam os mesmos imports e fontes locais.
export async function readCss(file) {
  const location = file instanceof URL ? fileURLToPath(file) : file;
  let css = await readFile(location, 'utf8');
  for (const match of [...css.matchAll(/@import\s+"([^"]+)";/g)]) {
    css = css.replace(match[0], await readCss(path.resolve(path.dirname(location), match[1])));
  }
  for (const match of [...css.matchAll(/url\("([^"\n]+\.woff2)"\)/g)]) {
    const bytes = await readFile(path.resolve(path.dirname(location), match[1]));
    css = css.replace(match[0], `url("data:font/woff2;base64,${bytes.toString('base64')}")`);
  }
  return css;
}
