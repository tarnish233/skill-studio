// jsdom 缺少 Radix 依赖的若干浏览器 API，必须在测试前 polyfill。
import { vi } from "vitest";

// 惰性 stub，Radix 只需要它存在。过去让回调立即触发会让单个菜单卡 50s+，
// 根因其实是下面 nwsapi 的递归：floating-ui 每重新定位一次就要栈溢出一次。
class ResizeObserverStub {
  observe() {}
  unobserve() {}
  disconnect() {}
}
globalThis.ResizeObserver ??= ResizeObserverStub as never;

// jsdom 没有把 localStorage 暴露成裸全局（它是 window 上的不可枚举 accessor），
// 而 App / ThemeProvider 用的就是裸 localStorage。补一个内存实现。
class MemoryStorage implements Storage {
  private store = new Map<string, string>();
  get length() {
    return this.store.size;
  }
  clear() {
    this.store.clear();
  }
  getItem(key: string) {
    return this.store.has(key) ? this.store.get(key)! : null;
  }
  key(index: number) {
    return Array.from(this.store.keys())[index] ?? null;
  }
  removeItem(key: string) {
    this.store.delete(key);
  }
  setItem(key: string, value: string) {
    this.store.set(key, String(value));
  }
}

if (typeof globalThis.localStorage === "undefined") {
  const storage = new MemoryStorage();
  Object.defineProperty(globalThis, "localStorage", {
    value: storage,
    configurable: true,
  });
  if (typeof window !== "undefined") {
    Object.defineProperty(window, "localStorage", {
      value: storage,
      configurable: true,
    });
  }
}

// Radix 的 Select / DropdownMenu 在 jsdom 下会调用这些指针捕获 API
if (typeof Element !== "undefined") {
  Element.prototype.hasPointerCapture ??= () => false;
  Element.prototype.setPointerCapture ??= () => {};
  Element.prototype.releasePointerCapture ??= () => {};
  Element.prototype.scrollIntoView ??= () => {};

  // nwsapi（jsdom 的选择器引擎）匹配 :modal、:popover-open 等状态伪类时，会再调
  // node.matches 询问“原生”状态，而 jsdom 的 matches 就是 nwsapi 自己，于是一路递归到
  // 栈溢出才被吞掉、返回 false。floating-ui 每次定位浮层都会查 :popover-open / :modal，
  // 打开一个 Radix 提示框或下拉框要卡几秒，CI 上几十秒。jsdom 不模拟这些状态，
  // 重入的查询直接答 false，结果与原来一致；最外层调用照常交给 nwsapi。
  const STATES =
    /^:(open|closed|fullscreen|modal|picture-in-picture|popover-open)$/;
  const matches = Element.prototype.matches;
  let depth = 0;
  Element.prototype.matches = function (this: Element, selector: string) {
    if (depth > 0 && STATES.test(selector)) return false;
    depth += 1;
    try {
      return matches.call(this, selector);
    } finally {
      depth -= 1;
    }
  };
}

// Radix 的浮层定位会用到它，jsdom 没有实现
if (typeof document !== "undefined" && !document.elementFromPoint) {
  document.elementFromPoint = () => null;
}

if (typeof window !== "undefined" && !window.matchMedia) {
  window.matchMedia = vi.fn().mockImplementation((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    dispatchEvent: vi.fn(),
  }));
}
