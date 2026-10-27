// Enhances both initial and live-added transcript roots. Cue text is captured
// once before highlighting; copy and search never read decorated text back.
(() => {
  const app = (window.arabiTools ??= {});
  if (app.transcripts) return;
  const enhanced = new WeakSet();
  const viewers = new Map();

  function enhance(root) {
    if (enhanced.has(root)) return;
    enhanced.add(root);
    const events = new AbortController();
    const cues = [...root.querySelectorAll("[data-cue]")];
    const elements = cues.map((cue) => cue.querySelector(".cue-text"));
    const texts = elements.map((element) => element.textContent);
    const copyOverride = root.hasAttribute("data-copy-text") ? JSON.parse(root.dataset.copyText) : null;
    const region = root.querySelector(".cues");
    const select = root.querySelector("[data-cue-select]");
    const status = root.querySelector("[data-copy-status]");
    const fallback = root.querySelector("[data-copy-fallback]");
    const on = (element, type, listener) => element?.addEventListener(type, listener, { signal: events.signal });
    const show = (segment) => {
      const cue = cues[segment];
      const area = region.getBoundingClientRect();
      const box = cue.getBoundingClientRect();
      if (box.top < area.top || box.bottom > area.bottom) {
        region.scrollTop += box.top - area.top - 12;
      }
      select.selectedIndex = segment;
      return elements[segment];
    };
    const search = app.search.attach(root.querySelector("[data-search]"), {
      texts,
      show,
      element: (segment) => elements[segment],
      label: (segment) => select.options[segment].textContent,
    });
    root.querySelector("[data-transcript-controls]").hidden = false;
    const copy = (text) => app.copyText(text, { status, fallback });
    on(root.querySelector("[data-copy-all]"), "click", () => {
      void copy(copyOverride ?? texts.map((text) => `${text}\n`).join(""));
    });
    on(select, "change", () => show(select.selectedIndex));
    on(root.querySelector("[data-show-timestamps]"), "change", (event) => {
      root.classList.toggle("timestamps-hidden", !event.target.checked);
    });
    for (const [segment, cue] of cues.entries()) {
      const button = cue.querySelector("[data-copy-cue]");
      button.hidden = false;
      on(button, "click", () => void copy(texts[segment]));
    }
    root.dataset.transcriptReady = "true";
    viewers.set(root, () => {
      events.abort();
      search.dispose();
      fallback.replaceChildren();
      delete root.dataset.transcriptReady;
      enhanced.delete(root);
    });
  }

  function scan(node) {
    if (!(node instanceof Element) || !node.isConnected) return;
    if (node.matches('[data-result="transcript"]')) enhance(node);
    for (const root of node.querySelectorAll('[data-result="transcript"]')) enhance(root);
  }
  scan(document.documentElement);
  const observer = new MutationObserver((changes) => {
    for (const [root, dispose] of viewers) {
      if (!root.isConnected) {
        dispose();
        viewers.delete(root);
      }
    }
    for (const change of changes) for (const node of change.addedNodes) scan(node);
  });
  observer.observe(document.documentElement, { childList: true, subtree: true });
  app.transcripts = true;
})();
