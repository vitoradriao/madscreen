import assert from 'node:assert/strict';
import { test } from 'node:test';
import { render } from './third-party-notices.mjs';

test('avisos listam licencas declaradas em ordem estavel e afirmam GPL-3.0-or-later', () => {
  const text = render({
    rust: new Map([['b@1', { name: 'b', version: '1', license: 'MIT OR Apache-2.0' }], ['a@2', { name: 'a', version: '2', license: 'MPL-2.0' }]]),
    npm: new Map([['viewer', new Map([['x@1', { name: 'x', version: '1', license: 'MIT|X', dev: true }]])]]),
  });
  assert.match(text, /GNU GPL v3 ou posterior/);
  assert.ok(text.indexOf('| a | 2 |') < text.indexOf('| b | 1 |'));
  assert.match(text, /\| x \| 1 \| MIT\/X \| sim \|/);
  assert.match(text, /não aprova patentes/);
  assert.equal(render({ rust: new Map(), npm: new Map() }), render({ rust: new Map(), npm: new Map() }));
});
