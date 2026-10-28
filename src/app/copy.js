// arabiTools.copyText(text, { status, fallback }) resolves to true on clipboard
// success, false on fallback. status is a polite live element with data-copied
// and data-copy-failed localized messages. fallback is either a callback(text)
// or an empty container with data-copy-label and data-copy-close. The container
// gets an on-demand selectable panel, removed on close/Escape or the next copy.
(() => {
  const app = (window.arabiTools ??= {});
  if (app.copyText) return;
  const requests = new WeakMap();
  app.copyText = async (text, { status, fallback }) => {
    const request = {};
    requests.set(status, request);
    const trigger = document.activeElement;
    status.textContent = "";
    if (fallback instanceof Element) fallback.replaceChildren();
    try {
      await navigator.clipboard.writeText(text);
      if (status.isConnected && requests.get(status) === request) {
        status.textContent = status.dataset.copied;
      }
      return true;
    } catch {
      if (!status.isConnected || requests.get(status) !== request) return false;
      status.textContent = status.dataset.copyFailed;
      if (typeof fallback === "function") {
        fallback(text);
      } else if (fallback?.isConnected) {
        const panel = document.createElement("div");
        panel.className = "copy-panel";
        const label = document.createElement("label");
        label.textContent = fallback.dataset.copyLabel;
        const field = document.createElement("textarea");
        field.readOnly = true;
        field.dir = "auto";
        field.value = text;
        label.append(field);
        const close = document.createElement("button");
        close.type = "button";
        close.className = "button ghost compact";
        close.textContent = fallback.dataset.copyClose;
        const dismiss = () => {
          const restoreFocus = panel.contains(document.activeElement);
          panel.remove();
          if (restoreFocus && trigger?.isConnected) trigger.focus({ preventScroll: true });
        };
        close.addEventListener("click", dismiss);
        panel.addEventListener("keydown", (event) => {
          if (event.key === "Escape") {
            event.preventDefault();
            dismiss();
          }
        });
        panel.append(label, close);
        fallback.replaceChildren(panel);
        field.focus({ preventScroll: true });
        field.select();
        // A cue can be far below the shared feedback area. Reveal the manual
        // copy panel with the smallest scroll needed to use the selected text.
        panel.scrollIntoView({ block: "nearest", inline: "nearest" });
      }
      return false;
    }
  };

  // A result's Download menu is a <details> disclosure. Choosing a download, clicking
  // elsewhere or pressing Escape closes it; Escape returns focus to its button.
  const menus = () => document.querySelectorAll("details[data-download-menu][open]");
  document.addEventListener("click", (event) => {
    for (const menu of menus()) {
      if (!menu.contains(event.target) || event.target.closest("a[download]")) menu.open = false;
    }
  });
  document.addEventListener("keydown", (event) => {
    if (event.key !== "Escape") return;
    for (const menu of menus()) {
      const focused = menu.contains(document.activeElement);
      menu.open = false;
      if (focused) menu.querySelector("summary").focus();
    }
  });
})();
