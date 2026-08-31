import './style.css';
import { ShaderSession } from './core/ShaderSession.js';
import { ShaderEditor } from './editor/ShaderEditor.js';
import { computeUniforms } from './renderer/uniforms.js';
import { BrowserPreviewHost } from './services/BrowserPreviewHost.js';
import { HttpShaderLibraryService } from './services/HttpShaderLibraryService.js';
import { StaticShaderLibraryService } from './services/StaticShaderLibraryService.js';
import { LocalStorageSettingsStore } from './services/LocalStorageSettingsStore.js';
import type { PreviewHost, PreviewHostStatus } from './services/PreviewHost.js';
import type { ShaderLibraryService } from './services/ShaderLibraryService.js';
import type {
  AppState,
  PreviewBackend,
  PreviewBackendOption,
  PreviewCompileStatus,
  PreviewStats,
  RenderPreset,
} from './types/shader.js';
import { FileBrowser } from './ui/FileBrowser.js';
import { OutputPanel } from './ui/OutputPanel.js';
import { PassTabs } from './ui/PassTabs.js';
import { Toolbar } from './ui/Toolbar.js';

const DEFAULT_SHADER = `// ShaderToy-style GLSL shader
// Available uniforms: iTime, iResolution, iMouse, iFrame, iDate

void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec2 uv = fragCoord / iResolution.xy;
    vec3 col = 0.5 + 0.5 * cos(iTime + uv.xyx + vec3(0, 2, 4));
    fragColor = vec4(col, 1.0);
}`;

const RENDER_PRESETS: RenderPreset[] = [
  { id: 'square-1080', label: '1080 x 1080', width: 1080, height: 1080 },
  { id: 'full-hd', label: '1920 x 1080', width: 1920, height: 1080 },
  { id: 'hd', label: '1280 x 720', width: 1280, height: 720 },
];

const BROWSER_BACKENDS: PreviewBackend[] = ['auto', 'webgl2', 'webgpu'];

const BACKEND_LABELS: Record<PreviewBackend, string> = {
  auto: 'Auto',
  webgl2: 'WebGL2',
  webgpu: 'WebGPU',
};

function getRenderPreset(presetId: string): RenderPreset {
  return RENDER_PRESETS.find((preset) => preset.id === presetId) || RENDER_PRESETS[1];
}

function sanitizeBackend(value: string | null | undefined): PreviewBackend {
  return BROWSER_BACKENDS.includes(value as PreviewBackend) ? value as PreviewBackend : 'auto';
}

const settings = new LocalStorageSettingsStore();
const shaderLibrary: ShaderLibraryService = import.meta.env.PROD
  ? new StaticShaderLibraryService()
  : new HttpShaderLibraryService();

// Only the static library carries per-visitor overrides; in dev, saves go to disk.
const overrideStore = shaderLibrary instanceof StaticShaderLibraryService ? shaderLibrary : null;

const initialRenderPreset = getRenderPreset(settings.get('render-preset', 'full-hd'));
const initialBackend = sanitizeBackend(settings.get('preview-backend', 'auto'));

const state: AppState = {
  currentSource: DEFAULT_SHADER,
  currentLanguage: 'glsl',
  currentFile: null,
  preview: {
    runtime: 'browser',
    backend: initialBackend,
    renderPresetId: initialRenderPreset.id,
  },
  isPlaying: true,
  startTime: performance.now() / 1000,
  pausedTime: 0,
  frameCount: 0,
  mouse: { x: 0, y: 0, clickX: 0, clickY: 0, down: false },
};

const session = new ShaderSession();
session.load({ source: DEFAULT_SHADER, language: 'glsl' });

let currentRenderPreset = initialRenderPreset;
let lastCompileStatus: PreviewCompileStatus = 'ready';
let lastCompileMessage = '';
let lastFpsTime = performance.now();
let fpsFrames = 0;
let currentFps = 0;
const pressedKeys = new Set<number>();

const app = document.getElementById('app')!;
const outputPanel = new OutputPanel();
const previewHost: PreviewHost = new BrowserPreviewHost(initialRenderPreset);

const toolbar = new Toolbar({
  onShaderTypeChange: (language) => switchShaderLanguage(language),
  onBackendChange: (backend) => applyPreviewBackend(backend),
  onResolutionChange: (presetId) => applyRenderPreset(presetId),
  onPlayPause: () => togglePlayPause(),
  onReset: () => resetTime(),
  onScreenshot: () => takeScreenshot(),
  onFullscreen: () => toggleFullscreen(),
}, RENDER_PRESETS, getBackendOptions(previewHost.getStatus()), state.preview.renderPresetId, state.preview.backend);

const fileBrowser = new FileBrowser({
  onRefresh: () => refreshShaderLibrary(),
  hasOverride: (provider, filename) => overrideStore?.hasOverride(provider, filename) ?? false,
  onRevert: async (provider, filename) => {
    if (!overrideStore) return;
    overrideStore.clearOverride(provider, filename);
    fileBrowser.clearOverrideMark(provider, filename);
    try {
      const data = await shaderLibrary.load(provider, filename);
      session.setCurrentFile({ provider, filename });
      state.currentFile = { provider, filename };
      toolbar.setActiveType(data.type);
      await applyDocument(data.content, data.type);
      outputPanel.log(`Reverted ${provider}/${filename} to the original`, 'info');
    } catch {
      outputPanel.log(`Failed to reload ${provider}/${filename}`, 'error');
    }
  },
  onFileSelect: async (provider, filename) => {
    try {
      const data = await shaderLibrary.load(provider, filename);
      session.setCurrentFile({ provider, filename });
      state.currentFile = { provider, filename };
      toolbar.setActiveType(data.type);
      await applyDocument(data.content, data.type);
      outputPanel.log(`Loaded ${provider}/${filename}`, 'info');
    } catch {
      outputPanel.log(`Failed to load ${provider}/${filename}`, 'error');
    }
  },
});

const passTabs = new PassTabs({
  onTabSelect: (index) => {
    const source = session.selectPass(index);
    editor.setValue(source);
  },
});

const mainArea = document.createElement('div');
mainArea.className = 'main-area';

const splitContainer = document.createElement('div');
splitContainer.className = 'split-container';

const editorPanel = document.createElement('div');
editorPanel.className = 'editor-panel';
editorPanel.appendChild(passTabs.el);

const editorContainer = document.createElement('div');
editorContainer.className = 'editor-container';
editorPanel.appendChild(editorContainer);

const splitHandle = document.createElement('div');
splitHandle.className = 'split-handle';

const canvasPanel = document.createElement('div');
canvasPanel.className = 'canvas-panel';

const renderArea = document.createElement('div');
renderArea.className = 'render-area';
renderArea.appendChild(previewHost.element);
canvasPanel.appendChild(renderArea);
canvasPanel.appendChild(outputPanel.el);

splitContainer.append(editorPanel, splitHandle, canvasPanel);
mainArea.append(fileBrowser.el, splitContainer);

app.append(toolbar.el, mainArea);

const editor = new ShaderEditor(
  editorContainer,
  (source) => onEditorChange(source),
  () => saveCurrentShader(),
);

let splitDragging = false;
const savedSplit = settings.get('split-width', '40%');
editorPanel.style.width = savedSplit;

splitHandle.addEventListener('mousedown', (event) => {
  event.preventDefault();
  splitDragging = true;
  splitHandle.classList.add('dragging');
});

window.addEventListener('mousemove', (event) => {
  if (!splitDragging) {
    return;
  }

  const rect = splitContainer.getBoundingClientRect();
  const x = event.clientX - rect.left;
  const pct = Math.max(15, Math.min(70, (x / rect.width) * 100));
  const width = `${pct}%`;
  editorPanel.style.width = width;
  settings.set('split-width', width);
});

window.addEventListener('mouseup', () => {
  if (!splitDragging) {
    return;
  }

  splitDragging = false;
  splitHandle.classList.remove('dragging');
});

const resizeObserver = new ResizeObserver(() => {
  const rect = renderArea.getBoundingClientRect();
  previewHost.updateStageSize(rect.width, rect.height);
});
resizeObserver.observe(renderArea);

previewHost.element.addEventListener('mousemove', (event) => {
  const pos = previewHost.getPointerPosition(event);
  if (!pos) {
    return;
  }

  state.mouse.x = pos.x;
  state.mouse.y = pos.y;
});

previewHost.element.addEventListener('mousedown', (event) => {
  const pos = previewHost.getPointerPosition(event);
  if (!pos) {
    return;
  }

  state.mouse.down = true;
  state.mouse.clickX = pos.x;
  state.mouse.clickY = pos.y;
});

window.addEventListener('mouseup', () => {
  state.mouse.down = false;
});

window.addEventListener('keydown', (event) => {
  if (event.keyCode < 256) {
    pressedKeys.add(event.keyCode);
  }

  if (document.activeElement?.closest('.editor-container')) {
    return;
  }

  if (event.code === 'Space' && !previewHost.usesKeyboard()) {
    event.preventDefault();
    togglePlayPause();
  } else if (event.code === 'KeyR' && !event.ctrlKey && !event.metaKey && !previewHost.usesKeyboard()) {
    resetTime();
  } else if (event.key === 'F11') {
    event.preventDefault();
    toggleFullscreen();
  }
});

window.addEventListener('keyup', (event) => {
  if (event.keyCode < 256) {
    pressedKeys.delete(event.keyCode);
  }
});

function getBackendOptions(status: PreviewHostStatus): PreviewBackendOption[] {
  return BROWSER_BACKENDS.map((value) => ({
    value,
    label: BACKEND_LABELS[value],
    disabled: !status.capabilities.availableBackends.includes(value),
    reason: status.capabilities.unavailableBackends[value],
  }));
}

async function refreshShaderLibrary() {
  try {
    const data = await shaderLibrary.list();
    fileBrowser.render(data);
  } catch {
    fileBrowser.renderError('Could not load the shader library.');
    outputPanel.log('Failed to refresh shader library.', 'error');
  }
}

async function applyDocument(source: string, language: AppState['currentLanguage']) {
  session.load({ source, language });
  state.currentSource = source;
  state.currentLanguage = language;

  const snapshot = session.getSnapshot();
  passTabs.setPasses(snapshot.passNames);
  editor.setValue(snapshot.activeSource);

  const status = await previewHost.setDocument(snapshot.document);
  applyPreviewStatus(status);
}

async function onEditorChange(source: string) {
  session.updateActiveSource(source);
  state.currentSource = session.getSerializedSource();
  const status = await previewHost.setDocument({
    source: state.currentSource,
    language: state.currentLanguage,
  });
  applyPreviewStatus(status);
}

async function switchShaderLanguage(language: AppState['currentLanguage']) {
  state.currentLanguage = language;
  session.setLanguage(language);
  editor.setLanguage(language);
  const status = await previewHost.setDocument({
    source: state.currentSource,
    language,
  });
  applyPreviewStatus(status);
}

async function applyPreviewBackend(backend: PreviewBackend) {
  state.preview.backend = backend;
  settings.set('preview-backend', backend);
  toolbar.setBackend(backend);
  const status = await previewHost.setBackend(backend);
  applyPreviewStatus(status);
}

function applyRenderPreset(presetId: string, shouldLog = true) {
  currentRenderPreset = getRenderPreset(presetId);
  state.preview.renderPresetId = currentRenderPreset.id;
  settings.set('render-preset', currentRenderPreset.id);
  toolbar.setResolution(currentRenderPreset.id);
  previewHost.setRenderPreset(currentRenderPreset);

  const rect = renderArea.getBoundingClientRect();
  previewHost.updateStageSize(rect.width, rect.height);
  syncOutputStats();

  if (shouldLog) {
    outputPanel.log(
      `Render preset set to ${currentRenderPreset.label} (${currentRenderPreset.width}x${currentRenderPreset.height})`,
      'info',
    );
  }
}

function applyPreviewStatus(status: PreviewHostStatus) {
  state.preview.runtime = status.runtime;
  toolbar.setBackendOptions(getBackendOptions(status));
  setCompileState(status.compileStatus, status.compileMessage);
  syncOutputStats();
}

function togglePlayPause() {
  if (state.isPlaying) {
    state.pausedTime = performance.now() / 1000 - state.startTime;
    state.isPlaying = false;
  } else {
    state.startTime = performance.now() / 1000 - state.pausedTime;
    state.isPlaying = true;
  }

  toolbar.setPlaying(state.isPlaying);
}

function resetTime() {
  state.startTime = performance.now() / 1000;
  state.pausedTime = 0;
  state.frameCount = 0;
}

function toggleFullscreen() {
  app.classList.toggle('app-fullscreen');
}

async function takeScreenshot() {
  outputPanel.log('Taking screenshot...', 'info');
  const filename = await previewHost.takeScreenshot();
  if (filename) {
    outputPanel.log(`Screenshot downloaded as ${filename}`, 'success');
    return;
  }

  outputPanel.log('Screenshot failed.', 'error');
}

async function saveCurrentShader() {
  if (!state.currentFile) {
    return;
  }

  const { provider, filename } = state.currentFile;

  try {
    await shaderLibrary.save(provider, filename, session.getSerializedSource());
    if (overrideStore) {
      fileBrowser.markOverride(provider, filename);
      outputPanel.log(`Saved ${provider}/${filename} to this browser`, 'info');
    }
  } catch {
    // Quota exceeded or storage refused; the edit is still live in the editor.
    outputPanel.log(`Failed to save ${provider}/${filename}`, 'error');
  }
}

function setCompileState(status: PreviewCompileStatus, message = '') {
  const previousStatus = lastCompileStatus;
  const previousMessage = lastCompileMessage;
  lastCompileStatus = status;
  lastCompileMessage = status === 'ready' ? '' : message;

  const compileLabel = status === 'ready'
    ? 'Ready'
    : status === 'transpiling'
      ? 'Transpiling'
      : status === 'offline'
        ? 'Offline'
        : 'Error';

  outputPanel.setStats({ compile: compileLabel });

  if (status === 'ready') {
    if (previousStatus !== 'ready') {
      outputPanel.log('Compile OK', 'success');
    }
    return;
  }

  if (message && (previousStatus !== status || previousMessage !== message)) {
    outputPanel.log(message, status === 'offline' ? 'warning' : 'error');
  }
}

function getPreviewStats(u?: ReturnType<typeof computeUniforms>): PreviewStats {
  const status = previewHost.getStatus();

  return {
    runtime: status.runtime,
    backend: status.requestedBackend,
    rendererLabel: status.rendererLabel,
    resolution: `${currentRenderPreset.width}x${currentRenderPreset.height}`,
    fps: currentFps,
    frame: state.frameCount,
    time: u ? u.iTime : state.pausedTime,
    compileStatus: lastCompileStatus,
  };
}

function syncOutputStats(u?: ReturnType<typeof computeUniforms>) {
  const stats = getPreviewStats(u);
  const backendLabel = stats.backend === 'auto'
    ? `auto -> ${previewHost.getStatus().resolvedBackend}`
    : stats.backend;

  outputPanel.setStats({
    runtime: stats.runtime,
    backend: backendLabel,
    renderer: stats.rendererLabel,
    resolution: stats.resolution,
    file: state.currentFile ? `${state.currentFile.provider}/${state.currentFile.filename}` : 'Untitled',
    compile: stats.compileStatus === 'ready'
      ? 'Ready'
      : stats.compileStatus === 'transpiling'
        ? 'Transpiling'
        : stats.compileStatus === 'offline'
          ? 'Offline'
          : 'Error',
    fps: `${Math.round(stats.fps)}`,
    frame: `${stats.frame}`,
    time: `${stats.time.toFixed(2)}s`,
  });
}

function formatDiagnostic(value: unknown): string {
  if (value instanceof Error) {
    return value.stack || value.message;
  }
  if (typeof value === 'string') {
    return value;
  }
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

function attachDiagnostics() {
  const originalWarn = console.warn.bind(console);
  const originalError = console.error.bind(console);

  console.warn = (...args: unknown[]) => {
    outputPanel.log(args.map(formatDiagnostic).join(' '), 'warning');
    originalWarn(...args);
  };

  console.error = (...args: unknown[]) => {
    outputPanel.log(args.map(formatDiagnostic).join(' '), 'error');
    originalError(...args);
  };

  window.addEventListener('error', (event) => {
    outputPanel.log(event.message, 'error');
  });

  window.addEventListener('unhandledrejection', (event) => {
    outputPanel.log(`Unhandled rejection: ${formatDiagnostic(event.reason)}`, 'error');
  });
}

function renderLoop() {
  if (state.isPlaying) {
    state.frameCount++;
  }

  const uniforms = computeUniforms(state, previewHost.getActiveCanvas());
  previewHost.render(uniforms, pressedKeys);

  fpsFrames++;
  const now = performance.now();
  if (now - lastFpsTime >= 500) {
    currentFps = (fpsFrames / (now - lastFpsTime)) * 1000;
    fpsFrames = 0;
    lastFpsTime = now;
  }

  toolbar.updateStats(uniforms.iTime, currentFps, state.frameCount);
  syncOutputStats(uniforms);
  requestAnimationFrame(renderLoop);
}

async function bootstrap() {
  attachDiagnostics();
  applyRenderPreset(state.preview.renderPresetId, false);
  outputPanel.log('Browser runtime detected.', 'info');

  const hostStatus = await previewHost.initialize();
  applyPreviewStatus(hostStatus);

  if (hostStatus.capabilities.availableBackends.includes('webgpu')) {
    outputPanel.log('WebGPU browser preview ready.', 'success');
  } else {
    outputPanel.log('WebGPU is unavailable in this browser. WebGL2 preview remains online.', 'warning');
  }

  editor.setValue(DEFAULT_SHADER);
  editor.setLanguage(state.currentLanguage);
  await applyDocument(DEFAULT_SHADER, state.currentLanguage);
  await applyPreviewBackend(state.preview.backend);
  await refreshShaderLibrary();

  shaderLibrary.watch(async (event) => {
    if (event.type === 'file-added') {
      fileBrowser.addFile(event.provider, event.filename);
      return;
    }

    if (event.type === 'file-removed') {
      fileBrowser.removeFile(event.provider, event.filename);
      return;
    }

    if (state.currentFile?.provider === event.provider && state.currentFile?.filename === event.filename) {
      try {
        const data = await shaderLibrary.load(event.provider, event.filename);
        session.setCurrentFile({ provider: event.provider, filename: event.filename });
        state.currentFile = { provider: event.provider, filename: event.filename };
        toolbar.setActiveType(data.type);
        await applyDocument(data.content, data.type);
        outputPanel.log(`Reloaded ${event.provider}/${event.filename}`, 'info');
      } catch {
        outputPanel.log(`Failed to reload ${event.provider}/${event.filename}`, 'error');
      }
    }
  });

  requestAnimationFrame(renderLoop);
}

bootstrap();
