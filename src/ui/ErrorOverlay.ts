export class ErrorOverlay {
  el: HTMLElement;

  constructor() {
    this.el = document.createElement('div');
    this.el.className = 'error-overlay hidden';
  }

  show(errors: string) {
    this.el.classList.remove('hidden');
    // Format errors: highlight line numbers
    const html = errors
      .split('\n')
      .filter(l => l.trim())
      .map(line => {
        const highlighted = line.replace(
          /ERROR:\s*\d+:(\d+)/g,
          '<span class="error-line">ERROR: line $1</span>'
        );
        return `<div class="error-line-entry">${highlighted}</div>`;
      })
      .join('');
    this.el.innerHTML = `<div class="error-title">Compilation Error</div>${html}`;
  }

  hide() {
    this.el.classList.add('hidden');
    this.el.innerHTML = '';
  }
}
