export type TauriEventUnlisten = () => void;
export type TauriApi = {
  invoke: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
  listen: <T>(event: string, handler: (event: { payload: T }) => void | Promise<void>) => Promise<TauriEventUnlisten>;
};

export async function loadTauriApi(): Promise<TauriApi | null> {
  if (!(window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__) {
    return null;
  }

  const [{ invoke }, { listen }] = await Promise.all([
    import("@tauri-apps/api/core"),
    import("@tauri-apps/api/event"),
  ]);

  return { invoke, listen };
}
