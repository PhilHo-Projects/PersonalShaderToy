export type OutputLevel = 'info' | 'success' | 'warning' | 'error';

export interface OutputStats {
  runtime: string;
  backend: string;
  renderer: string;
  resolution: string;
  file: string;
  compile: string;
  fps: string;
  frame: string;
  time: string;
}

const STAT_LABELS: Array<{ key: keyof OutputStats; label: string }> = [
  { key: 'runtime', label: 'Runtime' },
  { key: 'backend', label: 'Backend' },
  { key: 'renderer', label: 'Renderer' },
  { key: 'resolution', label: 'Resolution' },
  { key: 'file', label: 'File' },
  { key: 'compile', label: 'Compile' },
  { key: 'fps', label: 'FPS' },
  { key: 'frame', label: 'Frame' },
  { key: 'time', label: 'Time' },
];

const DEFAULT_STATS: OutputStats = {
  runtime: 'browser',
  backend: 'auto',
  renderer: 'WebGL2',
  resolution: '1920x1080',
  file: 'Untitled',
  compile: 'Ready',
  fps: '0',
  frame: '0',
  time: '0.00s',
};

export class OutputPanel {
  el: HTMLElement;
  private logList: HTMLElement;
  private statEls = {} as Record<keyof OutputStats, HTMLElement>;
  private lastKey = '';
  private lastEntry: HTMLElement | null = null;
  private lastRepeatEl: HTMLElement | null = null;
  private lastCount = 1;

  constructor() {
    this.el = document.createElement('div');
    this.el.className = 'output-panel';

    const header = document.createElement('div');
    header.className = 'output-header';

    const title = document.createElement('div');
    title.className = 'output-title';
    title.textContent = 'Output';

    const stats = document.createElement('div');
    stats.className = 'output-stats';
    for (const { key, label } of STAT_LABELS) {
      const pill = document.createElement('div');
      pill.className = 'output-pill';

      const labelEl = document.createElement('span');
      labelEl.className = 'output-pill-label';
      labelEl.textContent = label + ':';

      const valueEl = document.createElement('span');
      valueEl.className = 'output-pill-value';
      valueEl.textContent = DEFAULT_STATS[key];
      this.statEls[key] = valueEl;

      pill.append(labelEl, valueEl);
      stats.appendChild(pill);
    }

    const clearBtn = document.createElement('button');
    clearBtn.className = 'output-clear';
    clearBtn.textContent = 'Clear';
    clearBtn.addEventListener('click', () => this.clear());

    this.logList = document.createElement('div');
    this.logList.className = 'output-log';

    header.append(title, stats, clearBtn);
    this.el.append(header, this.logList);
  }

  setStats(stats: Partial<OutputStats>) {
    for (const [key, value] of Object.entries(stats) as Array<[keyof OutputStats, string]>) {
      this.statEls[key].textContent = value;
    }
  }

  log(message: string, level: OutputLevel = 'info') {
    const normalized = message.trim();
    if (!normalized) return;

    const key = `${level}:${normalized}`;
    if (key === this.lastKey && this.lastEntry && this.lastRepeatEl) {
      this.lastCount++;
      this.lastRepeatEl.textContent = `x${this.lastCount}`;
      const timeEl = this.lastEntry.querySelector('.output-entry-time');
      if (timeEl) timeEl.textContent = this.timestamp();
      return;
    }

    const nearBottom = this.logList.scrollHeight - this.logList.scrollTop - this.logList.clientHeight < 32;

    const entry = document.createElement('div');
    entry.className = 'output-entry';
    entry.dataset.level = level;

    const timeEl = document.createElement('div');
    timeEl.className = 'output-entry-time';
    timeEl.textContent = this.timestamp();

    const levelEl = document.createElement('div');
    levelEl.className = 'output-entry-level';
    levelEl.textContent = level;

    const messageEl = document.createElement('div');
    messageEl.className = 'output-entry-message';
    messageEl.textContent = normalized;

    const repeatEl = document.createElement('div');
    repeatEl.className = 'output-entry-repeat';

    entry.append(timeEl, levelEl, messageEl, repeatEl);
    this.logList.appendChild(entry);

    while (this.logList.children.length > 200) {
      this.logList.removeChild(this.logList.firstChild!);
    }

    if (nearBottom) {
      this.logList.scrollTop = this.logList.scrollHeight;
    }

    this.lastKey = key;
    this.lastEntry = entry;
    this.lastRepeatEl = repeatEl;
    this.lastCount = 1;
  }

  clear() {
    this.logList.innerHTML = '';
    this.lastKey = '';
    this.lastEntry = null;
    this.lastRepeatEl = null;
    this.lastCount = 1;
  }

  private timestamp() {
    return new Date().toLocaleTimeString([], {
      hour12: false,
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
    });
  }
}
