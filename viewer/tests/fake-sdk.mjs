class EventEmitter {
  listeners = new Map();
  on(event, callback) { const callbacks=this.listeners.get(event)??[]; callbacks.push(callback); this.listeners.set(event,callbacks); return this; }
  emit(event,...args) { for(const callback of this.listeners.get(event)??[]) callback(...args); }
  removeAllListeners() { this.listeners.clear(); }
}

export const ConnectionState = { Connected: "connected", Disconnected: "disconnected" };
export const RoomEvent = Object.fromEntries(["TrackPublished", "TrackSubscribed", "TrackUnsubscribed", "Reconnecting", "Reconnected", "Disconnected"].map(x => [x,x]));
export const VideoQuality = { HIGH: 2 };
export const Track = { Kind: { Audio: "audio", Video: "video" }, Source: { ScreenShare: "screen_share", ScreenShareAudio: "screen_share_audio" } };

export class Room extends EventEmitter {
  static instances = [];
  static nextConnect = null;
  remoteParticipants = new Map();
  state = "disconnected";
  disconnects = 0;
  constructor(options) { super(); this.options = options; Room.instances.push(this); }
  async connect(url, token, options) {
    this.connection = { url, token, options };
    const pending = Room.nextConnect;
    Room.nextConnect = null;
    if (pending) await pending;
    this.state = "connected";
  }
  async disconnect() { this.disconnects++; this.state = "disconnected"; this.emit(RoomEvent.Disconnected); }
}

export function publication(kind, name = kind === "video" ? "screen" : "audio") {
  return { kind, trackName: name, source: kind === "video" ? "screen_share" : "screen_share_audio", subscriptions: [],
    setSubscribed(value) { this.subscriptions.push(value); }, setVideoQuality(value) { this.quality = value; } };
}

export function track(kind, id = kind) {
  return { kind, receiver: {}, mediaStreamTrack: { kind, id }, setPlayoutDelay(value) { this.delay = value; }, async getRTCStatsReport() { return new Map([["video", { type: "inbound-rtp", kind: "video", framesDecoded: 60, bytesReceived: 1000 }]]); } };
}
