import type { ShaderListing, ShaderFile } from '../types/shader.js';

export interface FileBrowserCallbacks {
  onFileSelect: (provider: string, filename: string) => void;
  onRefresh: () => void;
  hasOverride?: (provider: string, filename: string) => boolean;
  onRevert?: (provider: string, filename: string) => void;
}

const PROVIDER_ICONS: Record<string, string> = {
  claude: '#cba6f7',
  gemini: '#89b4fa',
  openai: '#a6e3a1',
};

export class FileBrowser {
  el: HTMLElement;
  private sections: Map<string, HTMLElement> = new Map();
  private activeItem: HTMLElement | null = null;

  constructor(private cb: FileBrowserCallbacks) {
    this.el = document.createElement('div');
    this.el.className = 'file-browser';
    this.el.innerHTML = `<div class="fb-header">
      <span>Shaders</span>
      <button class="fb-refresh" title="Refresh">&#8635;</button>
    </div>
    <div class="fb-tree"></div>`;

    this.el.querySelector('.fb-refresh')!.addEventListener('click', () => this.cb.onRefresh());
  }

  render(data: ShaderListing) {
    const tree = this.el.querySelector('.fb-tree')!;
    tree.innerHTML = '';
    this.sections.clear();

    const providers = Object.keys(data).sort();

    for (const provider of providers) {
      const files = data[provider] || [];
      const section = document.createElement('div');
      section.className = 'fb-section';

      const header = document.createElement('div');
      header.className = 'fb-section-header';
      // Use fallback color if provider is custom
      const color = (PROVIDER_ICONS as Record<string, string>)[provider] || '#8e8e93';
      header.innerHTML = `<span class="fb-dot" style="background:${color}"></span>
        <span class="fb-provider-name">${provider}</span>
        <span class="fb-count">${files.length}</span>`;
      header.addEventListener('click', () => {
        section.classList.toggle('collapsed');
      });

      const list = document.createElement('div');
      list.className = 'fb-file-list';
      this.sections.set(provider, list);

      for (const file of files) {
        list.appendChild(this.createFileItem(file));
      }

      section.appendChild(header);
      section.appendChild(list);
      tree.appendChild(section);
    }
  }

  /**
   * A failed manifest fetch must not look like an empty library — a broken
   * deploy has to be visibly broken.
   */
  renderError(message: string) {
    const tree = this.el.querySelector('.fb-tree')!;
    tree.innerHTML = '';
    this.sections.clear();
    this.activeItem = null;

    const error = document.createElement('div');
    error.className = 'fb-error';
    error.textContent = message;
    tree.appendChild(error);
  }

  private createFileItem(file: ShaderFile): HTMLElement {
    const item = document.createElement('div');
    item.className = 'fb-file';
    item.dataset.provider = file.provider;
    item.dataset.filename = file.name;
    const typeColor = file.type === 'glsl' ? '#a6e3a1' : file.type === 'wgsl' ? '#89b4fa' : '#fab387';
    item.innerHTML = `<span class="fb-file-type" style="color:${typeColor}">${file.type}</span>
      <span class="fb-file-name">${file.name}</span>`;
    item.addEventListener('click', () => {
      if (this.activeItem) this.activeItem.classList.remove('active');
      item.classList.add('active');
      this.activeItem = item;
      this.cb.onFileSelect(file.provider, file.name);
    });

    if (this.cb.hasOverride?.(file.provider, file.name)) {
      this.attachRevert(item, file.provider, file.name);
    }

    return item;
  }

  markOverride(provider: string, filename: string) {
    const item = this.findItem(provider, filename);
    if (item) this.attachRevert(item, provider, filename);
  }

  clearOverrideMark(provider: string, filename: string) {
    const item = this.findItem(provider, filename);
    if (!item) return;
    item.classList.remove('edited');
    item.querySelector('.fb-revert')?.remove();
  }

  private attachRevert(item: HTMLElement, provider: string, filename: string) {
    if (item.querySelector('.fb-revert')) return;
    item.classList.add('edited');

    const btn = document.createElement('button');
    btn.className = 'fb-revert';
    btn.title = 'Discard your edits and restore the original shader';
    btn.textContent = '↺';
    btn.addEventListener('click', (e) => {
      // Without this the click also selects the file and reloads the override.
      e.stopPropagation();
      this.cb.onRevert?.(provider, filename);
    });
    item.appendChild(btn);
  }

  private findItem(provider: string, filename: string): HTMLElement | null {
    const list = this.sections.get(provider);
    if (!list) return null;
    for (const child of Array.from(list.children)) {
      if ((child as HTMLElement).dataset.filename === filename) {
        return child as HTMLElement;
      }
    }
    return null;
  }

  addFile(provider: string, filename: string) {
    const type = filename.split('.').pop() === 'wgsl' ? 'wgsl' : filename.split('.').pop() === 'hlsl' ? 'hlsl' : 'glsl';
    const list = this.sections.get(provider);
    if (list) {
      list.appendChild(this.createFileItem({
        name: filename, provider, type, modified: Date.now(),
      }));
    }
    // Update count
    const section = list?.parentElement;
    if (section) {
      const count = section.querySelector('.fb-count');
      if (count) count.textContent = String(list!.children.length);
    }
  }

  removeFile(provider: string, filename: string) {
    const list = this.sections.get(provider);
    if (!list) return;
    for (const child of Array.from(list.children)) {
      if (child.querySelector('.fb-file-name')?.textContent === filename) {
        child.remove();
        break;
      }
    }
    const section = list.parentElement;
    if (section) {
      const count = section.querySelector('.fb-count');
      if (count) count.textContent = String(list.children.length);
    }
  }
}
