import type { FriendView, Snapshot } from "@shared/client";
import { createFriends, type FriendsPanel } from "@shared/ui";
import { el, type View } from "../dom";
import { client } from "../session";

export interface FriendsView extends View {
  panel: FriendsPanel;
  update(snapshot: Snapshot): void;
}

export function createFriendsView(onWatch: (friend: FriendView) => void): FriendsView {
  const panel = createFriends(client, { onWatch });
  const root = el(
    "section",
    { class: "view" },
    panel.root,
  );
  let latest: Snapshot | null = null;
  return {
    root, panel,
    update(snapshot) { latest = snapshot; panel.update(snapshot); },
    show() { if (latest) panel.update(latest); },
    dispose: () => panel.dispose(),
  };
}
