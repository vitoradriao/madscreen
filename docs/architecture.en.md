[Português](architecture.md)

# Architecture

madscreen has three communicating parts: the **desktop app** (Tauri 2 + WebView2) with a capture and encoding
**engine** per stream, an authenticated **Worker** on Cloudflare and an **SFU** (WebRTC media server).

```text
App Tauri 2 / WebView2
  ├─ UI compartilhada com o cliente web: login, convites, canais e dois players
  └─ supervisor ──stdin JSON──► sidecar madscreen-encode-test --ipc   (um processo por transmissão)
                    ◄─stdout JSON─  WGC / Desktop Duplication → D3D11 (escala + BGRA→NV12) → encoder automático
                                    WASAPI process loopback → fonte de áudio → Opus
                                                  │
                          Worker autenticado: HubDO + StreamDO (SQLite, WebSockets)
                                   ├─ Cloudflare Realtime SFU   (principal)
                                   └─ LiveKit auto-hospedado    (reserva)
                                                  │
                                    WebRTC → player no app ou no navegador
```

## Components

| Part | Location | Responsibility |
| --- | --- | --- |
| Desktop app | `apps/madscreen-app` | Window (Tauri + WebView2), system tray, engine supervisor, signed updates |
| Engine (sidecar) | `apps/madscreen-encode-test` | Capture → conversion → encoder → transport; communicates with the app through JSON IPC |
| Capture | `crates/madscreen-capture` | WGC and Desktop Duplication, *latest-frame-wins* mailbox |
| Encoders | `crates/madscreen-encode` | NVENC (D3D11 zero-copy), AMF, QuickSync and x264 through FFmpeg |
| Audio | `crates/madscreen-audio` | WASAPI *process loopback* and PCM mixing (Opus is encoded by WebRTC) |
| Transport | `crates/madscreen-transport`, `crates/madscreen-transport-cf` | Publishing to LiveKit and Cloudflare |
| IPC | `crates/madscreen-ipc` | JSON protocol, one line per message |
| Viewer/UI | `viewer/shared`, `viewer/web` | Worker client, login, directory and player, used by the app and the web |
| Worker | `tools/cloudflare/worker` | Accounts, servers, channels, invitations, presence and SFU signaling |
| Vendored SDK | `third_party/livekit`, `third_party/webrtc-sys` | Copies with minimal patches (see the notices in each folder) |

## Performance principles

The central goal is **not to reduce the streamer's game FPS**.

- **One process per stream.** Stopping the stream ends the process and returns GPU resources to the game.
- **Latest-frame-wins pacer.** The capture mailbox has a depth of one: frames are never queued; the old frame is
  discarded. The encoder has few frames in flight and no per-frame allocations in the hot path.
- **NVENC without readback.** Scaling, BGRA→NV12 conversion and encoding stay on the GPU. AMF, QuickSync and x264
  use explicit NV12 readback for FFmpeg; these are compatibility paths and do not promise the same cost.
- **Never REALTIME priority.** The game comes first.
- **Official APIs only.** WGC and Desktop Duplication; no DLL injection or game hooks.
- **No server-side transcoding.** The engine's H.264 reaches the viewer as encoded.

## Stream flow

1. The app requests a session from the Worker (`/api/*`, authenticated). The Worker chooses the SFU and returns what the engine needs.
2. The app starts the sidecar with the chosen window/monitor; the engine captures, converts and encodes.
3. The engine publishes H.264 (Annex B) and Opus to the SFU over WebRTC. The Worker only handles signaling and does not see media.
4. Viewers open the stream in the app or browser and receive media directly from the SFU.
5. The app shows FPS and bitrate measured every second; the preview of your own stream is always muted.

## Capture

- **Window capture and Windows 11:** Windows Graphics Capture (WGC).
- **Monitor on Windows 10:** Desktop Duplication, without WGC's yellow indicator. The legacy native picker still uses WGC.
- The cursor, rotation and cursor shape changes are composited on the GPU. Window capture is not replaced by
  a cropped monitor capture.

## Audio

WASAPI *process loopback* at 48 kHz, stereo, with Opus (up to 128 kbps, DTX disabled). Audio is per application;
PC audio excludes the app itself and its descendants to avoid recapturing what the viewer hears. Without the
API on the installed Windows version, video continues. A/V synchronization is still being measured.

## Server and SFU

The Worker stores accounts, identities, servers, channels, invitations and presence in Durable Objects with SQLite
(one `HubDO` and one `StreamDO` per user). It chooses the SFU provider through configuration, and each publisher,
viewer and cleanup **records which SFU allocated its resources**: changing the configuration never changes open
sessions. LiveKit is a backup; the automatic health and usage policy is opt-in. See [SFU](sfu.md).

## Security and privacy

- SFU credentials and the session secret stay only in the Worker. The app and viewer communicate with the Worker
  (`/api/*` and, during login, `/auth/*`) and, once authorized, send and receive media **directly from the SFU**,
  with short-lived tokens.
- The Worker does not write secrets, SFU tokens or SDP to logs or persistent state. The client stores only the
  user's session token locally (in the browser/WebView2).
- Diagnostics are local, copied through a user action and contain no names, tokens, SDP or addresses.
- The app does not request microphone access. Updates validate the signature and version before installation
  ([Updates](updater.md)).
- To report a vulnerability, see [SECURITY.md](../SECURITY.md).

## Outside the current scope

macOS and Linux (pure logic still builds on Linux), end-to-end HDR, HEVC/AV1, *simulcast*, a native player and
text or voice chat. Some of these items are under evaluation; see the [roadmap](roadmap.md).
