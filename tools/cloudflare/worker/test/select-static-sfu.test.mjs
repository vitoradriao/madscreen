import assert from 'node:assert/strict';
import { test } from 'node:test';
import { parseSelection, probeLiveKit } from '../select-static-sfu.mjs';

const config = { LIVEKIT_URL: 'wss://media.example.com', LIVEKIT_API_KEY: 'fixture-key', LIVEKIT_API_SECRET: 'fixture-secret' };

test('selecao exige provedor conhecido e aplicacao explicita', () => {
  assert.deepEqual(parseSelection(['livekit']), { provider: 'livekit', apply: false });
  assert.deepEqual(parseSelection(['cloudflare', '--apply']), { provider: 'cloudflare', apply: true });
  for (const args of [[], ['other'], ['livekit', '--force'], ['livekit', '--apply', '--apply']]) assert.throws(() => parseSelection(args));
});

test('preflight usa HTTPS validado e limpa somente sua sala', async () => {
  const calls = [];
  await probeLiveKit(config, async (url, options) => {
    const body = JSON.parse(options.body);
    calls.push({ url, options, body });
    return Response.json(body.name ? { name: body.name } : {});
  });
  assert.equal(calls.length, 2);
  assert.equal(calls[0].url.href, 'https://media.example.com/twirp/livekit.RoomService/CreateRoom');
  assert.equal(calls[1].url.pathname, '/twirp/livekit.RoomService/DeleteRoom');
  assert.equal(calls[1].body.room, calls[0].body.name);
  assert.equal(calls[0].body.empty_timeout, 10);
  assert.equal(calls[0].options.redirect, 'error');
  const jwt = calls[0].options.headers.Authorization.split(' ')[1];
  const claims = JSON.parse(Buffer.from(jwt.split('.')[1], 'base64url'));
  assert.equal(claims.video.room, calls[0].body.name);
  assert.equal(claims.exp - claims.nbf, 65);
  assert.equal(claims.video.roomList, undefined);
});

test('falha de criacao nao tenta apagar salas e erro nao inclui credenciais', async () => {
  let calls = 0;
  await assert.rejects(() => probeLiveKit(config, async () => {
    calls++; return new Response('fixture-secret upstream', { status: 503 });
  }), error => !error.message.includes('fixture-secret') && error.message.includes('503'));
  assert.equal(calls, 1);
});

test('falha de limpeza e de rede impedem aprovar o preflight', async () => {
  let calls = 0;
  await assert.rejects(() => probeLiveKit(config, async (_url, options) => {
    return ++calls === 1 ? Response.json({ name: JSON.parse(options.body).name }) : new Response('', { status: 500 });
  }), /DeleteRoom/);
  await assert.rejects(() => probeLiveKit(config, async () => { throw Error('fixture-secret'); }),
    error => !error.message.includes('fixture-secret'));
});

test('resposta com outra sala nao aprova e nunca apaga a sala retornada pelo servidor', async () => {
  const calls = [];
  await assert.rejects(() => probeLiveKit(config, async (_url, options) => {
    calls.push(JSON.parse(options.body));
    return Response.json(calls.length === 1 ? { name: 'sala-de-terceiro' } : {});
  }), /nao confirmou/);
  assert.equal(calls.length, 2);
  assert.equal(calls[1].room, calls[0].name);
  assert.notEqual(calls[1].room, 'sala-de-terceiro');
});
