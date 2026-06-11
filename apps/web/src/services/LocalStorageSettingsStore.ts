import type { SettingsStore } from './SettingsStore.js';

export class LocalStorageSettingsStore implements SettingsStore {
  constructor(private prefix = 'personal-shader-toy') {}

  get<T>(key: string, fallback: T): T {
    try {
      const raw = localStorage.getItem(this.qualify(key));
      return raw === null ? fallback : JSON.parse(raw) as T;
    } catch {
      return fallback;
    }
  }

  set<T>(key: string, value: T) {
    localStorage.setItem(this.qualify(key), JSON.stringify(value));
  }

  remove(key: string) {
    localStorage.removeItem(this.qualify(key));
  }

  private qualify(key: string) {
    return `${this.prefix}:${key}`;
  }
}
