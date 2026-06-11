import type { PreviewBackend, PreviewBackendOption, RenderPreset, ShaderLanguage } from '../types/shader.js';

export interface ToolbarCallbacks {
  onShaderTypeChange: (type: ShaderLanguage) => void;
  onBackendChange: (backend: PreviewBackend) => void;
  onResolutionChange: (presetId: string) => void;
  onPlayPause: () => void;
  onReset: () => void;
  onScreenshot: () => void;
  onFullscreen: () => void;
}

export class Toolbar {
  el: HTMLElement;
  private timeDisplay: HTMLElement;
  private fpsDisplay: HTMLElement;
  private frameDisplay: HTMLElement;
  private playBtn: HTMLButtonElement;
  private backendSelect: HTMLSelectElement;
  private resolutionSelect: HTMLSelectElement;
  private currentType: ShaderLanguage = 'glsl';

  constructor(
    private cb: ToolbarCallbacks,
    presets: RenderPreset[],
    backendOptions: PreviewBackendOption[],
    initialPresetId: string,
    initialBackend: PreviewBackend,
  ) {
    this.el = document.createElement('div');
    this.el.className = 'toolbar';
    this.el.innerHTML = `
      <div class="toolbar-left">
        <span class="toolbar-title">PersonalShaderToy</span>
      </div>
      <div class="toolbar-center">
        <button class="tool-btn play-btn" title="Play/Pause (Space)">&#9654;</button>
        <button class="tool-btn" title="Reset (R)">&#8634;</button>
        <span class="toolbar-stat time-display">0.00s</span>
        <span class="toolbar-stat frame-display">F: 0</span>
      </div>
      <div class="toolbar-right">
        <label class="toolbar-select-wrap" title="Preview backend">
          <span class="toolbar-select-label">Backend</span>
          <select class="toolbar-select backend-select"></select>
        </label>
        <label class="toolbar-select-wrap" title="Render resolution">
          <span class="toolbar-select-label">Resolution</span>
          <select class="toolbar-select resolution-select">
            ${presets.map((preset) => `<option value="${preset.id}">${preset.label}</option>`).join('')}
          </select>
        </label>
        <span class="toolbar-stat fps-display">0 fps</span>
        <button class="tool-btn screenshot-btn" title="Download screenshot">PNG</button>
        <button class="tool-btn fullscreen-btn" title="Fullscreen (F11)">&#x26F6;</button>
      </div>
    `;

    this.timeDisplay = this.el.querySelector('.time-display')!;
    this.fpsDisplay = this.el.querySelector('.fps-display')!;
    this.frameDisplay = this.el.querySelector('.frame-display')!;
    this.playBtn = this.el.querySelector('.play-btn')!;
    this.backendSelect = this.el.querySelector('.backend-select')!;
    this.resolutionSelect = this.el.querySelector('.resolution-select')!;
    this.setBackendOptions(backendOptions);
    this.backendSelect.value = initialBackend;
    this.resolutionSelect.value = initialPresetId;

    // Play/pause
    this.playBtn.addEventListener('click', () => cb.onPlayPause());

    // Reset
    this.el.querySelector('.tool-btn[title="Reset (R)"]')!.addEventListener('click', () => cb.onReset());

    this.el.querySelector('.screenshot-btn')!.addEventListener('click', () => cb.onScreenshot());
    this.el.querySelector('.fullscreen-btn')!.addEventListener('click', () => cb.onFullscreen());

    this.backendSelect.addEventListener('change', () => cb.onBackendChange(this.backendSelect.value as PreviewBackend));
    this.resolutionSelect.addEventListener('change', () => cb.onResolutionChange(this.resolutionSelect.value));
  }

  setActiveType(type: ShaderLanguage) {
    this.currentType = type;
  }

  setBackendOptions(options: PreviewBackendOption[]) {
    this.backendSelect.innerHTML = options.map((option) => {
      const disabled = option.disabled ? ' disabled' : '';
      return `<option value="${option.value}"${disabled}>${option.label}</option>`;
    }).join('');
  }

  setBackend(backend: PreviewBackend) {
    this.backendSelect.value = backend;
  }

  setResolution(presetId: string) {
    this.resolutionSelect.value = presetId;
  }

  updateStats(time: number, fps: number, frame: number) {
    this.timeDisplay.textContent = time.toFixed(2) + 's';
    this.fpsDisplay.textContent = Math.round(fps) + ' fps';
    this.frameDisplay.textContent = 'F: ' + frame;
  }

  setPlaying(playing: boolean) {
    this.playBtn.innerHTML = playing ? '&#9646;&#9646;' : '&#9654;';
  }
}
