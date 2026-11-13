// Minimal Tauri v2 IPC helpers — replaces the @tauri-apps/api dependency.
//
// `__TAURI_INTERNALS__` exposes `invoke` and `transformCallback` but NOT
// `listen`: the event API is a core plugin driven over IPC. Mirrors the
// implementation in @tauri-apps/api/event so live events keep working.

type Unlisten = () => void;

export interface TauriEvent<T = unknown> {
  event: string;
  id: number;
  payload: T;
}

const I = (window as any).__TAURI_INTERNALS__;

export function invoke<T = unknown>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  return I.invoke(cmd, args);
}

export function listen<T = unknown>(
  event: string,
  handler: (e: TauriEvent<T>) => void,
): Promise<Unlisten> {
  const handlerId: number = I.transformCallback(handler, false);
  return invoke<number>("plugin:event|listen", {
    event,
    target: { kind: "Any" },
    handler: handlerId,
  }).then((eventId) => () => {
    void invoke("plugin:event|unlisten", { event, eventId });
  });
}
