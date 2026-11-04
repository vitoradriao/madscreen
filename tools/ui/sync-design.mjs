// Mantém os tokens/componentes publicados pelo design como fontes de verdade.
import { readFile, writeFile, copyFile } from 'node:fs/promises';
const root = new URL('../../', import.meta.url);
const read = file => readFile(new URL(file, root), 'utf8');
await writeFile(new URL('viewer/shared/design-tokens.css', root),
  (await read('docs/design/tokens.css')).replace('../../apps/madscreen-app/public/fonts/inter.woff2', './fonts/inter.woff2'));
await copyFile(new URL('docs/design/components/bundle.css', root), new URL('viewer/shared/design-components.css', root));
await copyFile(new URL('apps/madscreen-app/public/fonts/inter.woff2', root), new URL('viewer/shared/fonts/inter.woff2', root));
console.log('Tokens, componentes e Inter sincronizados com docs/design/.');
