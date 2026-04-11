import type { OutputLevel, OutputStats } from '../ui/OutputPanel.js';

export interface DiagnosticsSink {
  log(message: string, level?: OutputLevel): void;
  setStats(stats: Partial<OutputStats>): void;
  clear(): void;
}
