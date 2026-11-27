// Cliente mínimo do MCP HTTP local do OpenPencil (openpencil-mcp-http), usado por capture-redesign.mjs.
// O token vem do mcp.json que o servidor grava e nunca é impresso.
// shortcut: lê o mcp.json do app no Windows; em outro SO, aponte MADSCREEN_OPENPENCIL_MCP_JSON.
import { readFileSync } from 'node:fs';
const info = JSON.parse(readFileSync(process.env.MADSCREEN_OPENPENCIL_MCP_JSON || process.env.LOCALAPPDATA + '/OpenPencil/mcp.json', 'utf8'));
const endpoint = `http://127.0.0.1:${info.httpPort}/mcp`;
let sid, seq = 0;
async function post(body) {
  const res = await fetch(endpoint, { method: 'POST', headers: { 'Content-Type': 'application/json', Accept: 'application/json, text/event-stream', ...(info.clientToken ? { Authorization: 'Bearer ' + info.clientToken } : {}), ...(sid ? { 'mcp-session-id': sid } : {}) }, body: JSON.stringify(body) });
  sid ??= res.headers.get('mcp-session-id');
  if (res.status >= 400) throw Error(`OpenPencil HTTP ${res.status}: ${(await res.text()).slice(0, 300)}`);
  return res.text();
}
async function rpc(method, params) {
  const id = ++seq;
  const text = await post({ jsonrpc: '2.0', id, method, params });
  const msgs = text.split('\n').filter(l => l.startsWith('data:')).map(l => JSON.parse(l.slice(5)));
  const msg = msgs.find(m => m.id === id) ?? (text.trim().startsWith('{') ? JSON.parse(text) : null);
  if (!msg) throw Error('Resposta vazia do OpenPencil: ' + text.slice(0, 300));
  if (msg.error) throw Error(JSON.stringify(msg.error));
  return msg.result;
}
export async function connect() {
  await rpc('initialize', { protocolVersion: '2025-06-18', capabilities: {}, clientInfo: { name: 'madscreen-capture', version: '1' } });
  await post({ jsonrpc: '2.0', method: 'notifications/initialized' });
}
// O servidor guarda no máximo 10 sessões: encerre a sua ao terminar.
export async function close() {
  if (sid) await fetch(endpoint, { method: 'DELETE', headers: { ...(info.clientToken ? { Authorization: 'Bearer ' + info.clientToken } : {}), 'mcp-session-id': sid } }).catch(() => {});
  sid = undefined;
}
export async function tool(name, args) {
  const r = await rpc('tools/call', { name, arguments: args });
  const parts = (r.content ?? []).map(c => c.text ?? '');
  if (r.isError) throw Error(name + ': ' + parts.join('\n').slice(0, 800));
  if (r.structuredContent) return r.structuredContent;
  try { return JSON.parse(parts.at(-1)); } catch { return parts.at(-1); }
}
