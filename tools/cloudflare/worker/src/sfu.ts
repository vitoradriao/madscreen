import { ApiError, type Env } from "./common";
import { CloudflareProvider, iceServers } from "./providers/cloudflare";
import { LiveKitProvider } from "./providers/livekit";
import type { ProviderId, SfuProvider } from "./providers/types";

export { iceServers, CloudflareProvider };
export { SfuError, allocationProvider } from "./providers/types";
export type { ProviderId, SfuProvider, TrackResult, SfuTrack, IceServer } from "./providers/types";

export function activeProvider(env: Env): ProviderId {
  const value = env.SFU_PROVIDER ?? "cloudflare";
  if (value === "cloudflare" || value === "livekit") return value;
  throw new ApiError(503, "sfu_configuracao", "O provedor de mídia não está configurado.");
}

export async function selectedProvider(env: Env): Promise<ProviderId> {
  if (env.SFU_FAILOVER_ENABLED !== "1") return activeProvider(env);
  return env.HUB.get(env.HUB.idFromName("hub")).selectSfu();
}

/** Seleção explícita permite terminar sessões antigas no provedor original. */
export function createProvider(env: Env, id: ProviderId = activeProvider(env)): SfuProvider {
  if (id === "cloudflare") return new CloudflareProvider(env);
  if (id === "livekit") return new LiveKitProvider(env);
  throw new ApiError(503, "sfu_configuracao", "O provedor de mídia não está configurado.");
}
