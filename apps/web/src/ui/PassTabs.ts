export interface PassTabsCallbacks {
  onTabSelect: (index: number) => void;
}

export class PassTabs {
  el: HTMLElement;
  private tabs: HTMLButtonElement[] = [];
  private activeIndex = 0;

  constructor(private cb: PassTabsCallbacks) {
    this.el = document.createElement('div');
    this.el.className = 'pass-tabs hidden';
  }

  setPasses(passNames: string[]) {
    this.el.innerHTML = '';
    this.tabs = [];

    if (passNames.length <= 1) {
      this.el.classList.add('hidden');
      return;
    }

    this.el.classList.remove('hidden');

    for (let i = 0; i < passNames.length; i++) {
      const btn = document.createElement('button');
      btn.className = 'pass-tab' + (i === 0 ? ' active' : '');
      btn.textContent = passNames[i];
      btn.addEventListener('click', () => {
        this.setActive(i);
        this.cb.onTabSelect(i);
      });
      this.el.appendChild(btn);
      this.tabs.push(btn);
    }

    this.activeIndex = 0;
  }

  setActive(index: number) {
    for (const tab of this.tabs) tab.classList.remove('active');
    if (this.tabs[index]) {
      this.tabs[index].classList.add('active');
      this.activeIndex = index;
    }
  }

  hide() {
    this.el.classList.add('hidden');
    this.tabs = [];
    this.el.innerHTML = '';
  }

  get currentIndex() {
    return this.activeIndex;
  }
}
