[Português](self-hosting.md)

# Run your own server

The app needs a **signaling Worker** (accounts, servers, channels, invitations, presence and the SFU control
plane). You can host your own on Cloudflare and, optionally, a backup LiveKit. This guide lists what you need;
route and login details are in [Authentication and channels](cloudflare-auth.md), and SFU operation is in
[SFU](sfu.md).

> The secrets below stay **only in the Worker**. Never put them in Git, the front end, command-line arguments
> or logs. The repository's `wrangler.toml` contains no IDs or secrets.

## What you need

| Item | Purpose |
| --- | --- |
| Cloudflare account with Workers and Durable Objects (SQLite) | Hosts the Worker (`HubDO` and `StreamDO`) |
| Cloudflare **Realtime SFU** app | Primary SFU: generates `CF_REALTIME_APP_ID` and `CF_REALTIME_APP_SECRET` |
| Cloudflare **TURN** key | Generates `TURN_KEY_ID` and `TURN_KEY_API_TOKEN` for users behind restrictive NAT |
| **Discord** and/or **Google** OAuth app (optional) | Social login; without them, username + invitation access remains |
| Tauri signing key | Signs app updates ([Updates](updater.md)) |
| Self-hosted LiveKit (optional) | Backup SFU; see [SFU](sfu.md) |

## Step by step

1. **Clone and install.** `npm ci --prefix viewer` and `npm ci --prefix tools/cloudflare/worker` (the Worker embeds
   the web client from `viewer/`).
2. **Configure `wrangler.toml`.** Replace `CF_REALTIME_APP_ID` with your Realtime SFU App ID (not a secret).
   Durable Object migrations are already declared for a new account.
3. **Register secrets** (`wrangler secret put NOME`): `CF_REALTIME_APP_SECRET`, `SESSION_SECRET` (a long random
   value), `TURN_KEY_ID`, `TURN_KEY_API_TOKEN` and, if using social login, `DISCORD_CLIENT_ID`,
   `DISCORD_CLIENT_SECRET`, `GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET`. Optionally set `APP_DOWNLOAD_URL`
   (HTTPS) to the installer link used on invitation pages.
4. **Deploy.**

   ```powershell
   cd tools/cloudflare/worker
   npm run build              # gera src/web.generated.ts (embute o viewer web)
   npm run typecheck
   npx wrangler deploy        # o wrangler.toml roda `node build.mjs --minify` antes do deploy
   ```

   There is also `deploy-mcp.mjs`, which generates deployment code for the Cloudflare MCP while preserving
   existing secrets (see the [Worker README](../tools/cloudflare/worker/README.md)).
5. **Register OAuth redirects** in the Discord/Google dashboards with
   `https://<seu-worker>/auth/discord/callback` and `https://<seu-worker>/auth/google/callback` (step-by-step in
   [Authentication and channels](cloudflare-auth.md#3-criar-o-app-no-discord-passo-a-passo)).
6. **Test.** Open `https://<seu-worker>/health` (should respond with `{"ok":true,...}`) and `https://<seu-worker>/watch`.
   The first user does not need an invitation: create the account and **save the recovery key**.
7. **Point the app to your Worker** when building: `VITE_MADSCREEN_URL=https://<seu-worker>` (see
   [Build and test](building.md)).

## Precautions

- **Do not reapply migration resets in production.** Old Durable Object class migrations delete accounts and
  sessions; the Worker applies the directory's SQLite migration idempotently without deleting data.
- **`SESSION_SECRET` is generated once** and preserved between deployments; changing it invalidates all sessions.
- **Costs:** Cloudflare SFU egress has an allowance. The project's automatic usage policy is an *estimate*,
  not billing; always check the provider dashboard ([SFU](sfu.md)).
- **Privacy:** the Worker only handles signaling. It does not receive or record media, and does not persist tokens or SDP.
- **App distribution:** if you publish builds, the license is GPL-3.0-or-later and you must offer the corresponding
  source code, including the FFmpeg runtime ([Runtime](runtime-distribution.md)).
