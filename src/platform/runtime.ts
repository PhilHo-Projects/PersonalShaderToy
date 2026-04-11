export function isTauriRuntime(): boolean {
  const candidate = window as Window & {
    __TAURI__?: unknown;
    __TAURI_INTERNALS__?: unknown;
  };

  return Boolean(candidate.__TAURI__ || candidate.__TAURI_INTERNALS__);
}
