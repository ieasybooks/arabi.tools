// Text and document submissions share one model selector. All messages are supplied by
// the translated page; neither source text nor result text is logged or persisted.
const initialized = new WeakSet();
const attemptKey = "diacritize:attempt";
const choiceKey = "diacritize:choice";

function stored(key) {
  try { return sessionStorage.getItem(key); } catch { return null; }
}

function store(key, value) {
  try {
    if (value === null) sessionStorage.removeItem(key);
    else sessionStorage.setItem(key, value);
  } catch { /* The form also works when storage is blocked. */ }
}

function initialize(form) {
  if (initialized.has(form)) return;
  initialized.add(form);
  const source = form.elements.text;
  const file = form.elements.file;
  const models = Array.from(form.querySelectorAll('input[name="model"]'));
  const modes = Array.from(form.querySelectorAll('input[name="input_mode"]'));
  const buttons = Array.from(form.querySelectorAll('button[type="submit"]'));
  const textPanel = form.querySelector("#diacritize-text-panel");
  const documentPanel = form.querySelector("#diacritize-document-panel");
  const count = form.querySelector("#text-count");
  const fileName = form.querySelector(".file-name");
  const error = form.querySelector(".error");
  const back = form.querySelector(".return-to-job");
  const progress = form.querySelector(".upload-progress");
  const bar = progress.querySelector("progress");
  const status = progress.querySelector(".submission-status");
  const cancel = progress.querySelector(".cancel-upload");
  const maxCharacters = Number(form.dataset.maxCharacters);
  const maxBytes = Number(form.dataset.maxBytes);
  const extensions = form.dataset.extensions.split(",");
  let inProgress = false;
  let cancelled = false;
  let cancelling = false;
  let request = null;
  let reservation = null;
  let attempt = stored(attemptKey);
  // As in the upload tool, only a page being left keeps its attempt. Newly opened tabs
  // must not be able to replace another tab's active upload.
  store(attemptKey, null);

  function updateCount() {
    const length = Array.from(source.value).length;
    count.textContent = form.dataset.count
      .replace("{count}", length).replace("{max}", maxCharacters);
    const tooLong = length > maxCharacters;
    source.setAttribute("aria-invalid", String(tooLong));
    count.dataset.invalid = String(tooLong);
    if (tooLong) showError(form.dataset.textLimit);
    else if (error.textContent === form.dataset.textLimit) error.hidden = true;
  }

  function showMode() {
    const documentMode = form.elements.input_mode.value === "document";
    textPanel.hidden = documentMode;
    documentPanel.hidden = !documentMode;
    error.hidden = true;
    back.hidden = true;
    if (!documentMode) updateCount();
  }

  function rememberChoice() {
    store(choiceKey, JSON.stringify({ model: form.elements.model.value, mode: form.elements.input_mode.value }));
  }

  // A result page carries the model that produced that result. Other visits retain
  // this tab's model and mode, including when changing the interface language.
  if (form.dataset.retry !== "true") {
    try {
      const choice = JSON.parse(stored(choiceKey));
      const model = models.find((input) => input.value === choice?.model && !input.disabled);
      const mode = modes.find((input) => input.value === choice?.mode);
      if (model) model.checked = true;
      if (mode) mode.checked = true;
    } catch { /* Ignore an old or malformed choice. */ }
  }

  const originallyDisabled = new Set(models.filter((input) => input.disabled));
  function setInProgress(value) {
    inProgress = value;
    source.readOnly = value;
    for (const input of [...models, ...modes, file, ...buttons]) {
      input.disabled = value || originallyDisabled.has(input);
    }
  }

  function showError(message, job) {
    error.textContent = message;
    error.hidden = false;
    back.hidden = !job;
    if (job) back.href = job;
  }

  function problemWithFile() {
    const selected = file.files[0];
    if (!selected) return form.dataset.missing;
    const dot = selected.name.lastIndexOf(".");
    const extension = dot > 0 ? selected.name.slice(dot + 1).toLowerCase() : "";
    if (!extensions.includes(extension)) return form.dataset.wrongType;
    if (selected.size > maxBytes) return form.dataset.tooLarge;
    return null;
  }

  function post(url, body, onProgress) {
    return new Promise((resolve, reject) => {
      const xhr = new XMLHttpRequest();
      request = xhr;
      xhr.open("POST", url);
      xhr.responseType = "json";
      if (onProgress) xhr.upload.onprogress = (event) => {
        if (event.lengthComputable) onProgress(event.loaded / event.total);
      };
      xhr.onload = () => {
        request = null;
        const reply = xhr.response;
        if (reply?.error) reject(reply);
        else if (xhr.status === 413) reject({ error: form.dataset.tooLarge });
        else if (xhr.status >= 200 && xhr.status < 300 && reply) resolve(reply);
        else reject({ error: form.dataset.failed });
      };
      xhr.onerror = () => { request = null; reject({ error: form.dataset.failed }); };
      xhr.onabort = () => { request = null; reject({ cancelled: true }); };
      xhr.send(body);
    });
  }

  async function cancelReservation() {
    if (cancelling) return;
    const pending = reservation;
    cancelling = true;
    cancel.disabled = true;
    error.hidden = true;
    back.hidden = true;
    status.textContent = form.dataset.cancelling;
    try {
      const response = await fetch(pending.cancel, {
        method: "POST", headers: { Accept: "application/json" }, redirect: "error",
      });
      if (!response.ok) throw new Error();
      const reply = await response.json();
      if (typeof reply.cancelled !== "boolean" || typeof reply.next !== "string") throw new Error();
      reservation = null;
      attempt = null;
      store(attemptKey, null);
      if (!reply.cancelled) {
        // Completion (or another terminal state) won the race. Show the server's state.
        window.location.assign(reply.next);
        return;
      }
      status.textContent = form.dataset.cancelled;
      bar.hidden = true;
      cancel.hidden = true;
      setInProgress(false);
    } catch {
      // Keep this reservation locked to its source and expose both recovery paths.
      // A retry calls this function directly; there is no submission left to abort.
      reservation = pending;
      status.textContent = "";
      showError(form.dataset.cancelFailed, pending.cancel.replace(/\/cancel$/, ""));
      bar.hidden = true;
      cancel.disabled = false;
    } finally {
      cancelling = false;
    }
  }

  cancel.addEventListener("click", () => {
    if (!inProgress || cancelling) return;
    if (cancelled) {
      if (reservation) void cancelReservation();
      return;
    }
    cancelled = true;
    cancel.disabled = true;
    status.textContent = form.dataset.cancelling;
    // Let reservation creation finish so its cookie and cancellation URL are known.
    if (reservation && request) request.abort();
  });

  source.addEventListener("input", updateCount);
  for (const input of modes) input.addEventListener("change", () => { showMode(); rememberChoice(); });
  for (const input of models) input.addEventListener("change", () => { error.hidden = true; rememberChoice(); });
  file.addEventListener("change", () => {
    if (inProgress) return;
    fileName.textContent = file.files[0]?.name || form.dataset.noFile;
    const problem = problemWithFile();
    if (problem) showError(problem);
    else { error.hidden = true; back.hidden = true; }
  });

  window.addEventListener("pagehide", () => {
    if (attempt) store(attemptKey, attempt);
  });
  window.addEventListener("pageshow", (event) => {
    if (!event.persisted) return;
    attempt = stored(attemptKey) || attempt;
    store(attemptKey, null);
    setInProgress(false);
    progress.hidden = true;
  });

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    if (inProgress) return;
    const model = models.find((input) => input.checked);
    if (!model || model.disabled) return showError(form.dataset.modelUnavailable);
    const documentMode = form.elements.input_mode.value === "document";
    const text = source.value;
    const problem = documentMode ? problemWithFile()
      : !text.trim() ? form.dataset.textMissing
      : Array.from(text).length > maxCharacters ? form.dataset.textLimit : null;
    if (problem) return showError(problem);
    const chosenFile = file.files[0];
    rememberChoice();
    cancelled = false;
    cancel.disabled = false;
    reservation = null;
    setInProgress(true);
    error.hidden = true;
    back.hidden = true;
    progress.hidden = false;
    bar.hidden = !documentMode;
    bar.value = 0;
    status.textContent = documentMode ? form.dataset.uploading : form.dataset.submitting;
    try {
      attempt ??= Array.from(crypto.getRandomValues(new Uint8Array(16)), (byte) => byte.toString(16).padStart(2, "0")).join("");
      reservation = await post(form.dataset.reserve, new URLSearchParams({ attempt }));
      if (cancelled) throw { cancelled: true };
      let body;
      if (documentMode) {
        body = new FormData();
        body.append("model", model.value);
        body.append("file", chosenFile);
      } else {
        body = new URLSearchParams({ model: model.value, text });
      }
      const reply = await post(documentMode ? reservation.upload : reservation.text, body, documentMode ? (done) => { bar.value = Math.round(done * 100); } : null);
      if (cancelled) throw { cancelled: true };
      attempt = null;
      store(attemptKey, null);
      window.location.assign(reply.next);
    } catch (failure) {
      if (cancelled && reservation) {
        await cancelReservation();
      } else {
        progress.hidden = true;
        showError(failure?.error || form.dataset.failed, failure?.job);
        setInProgress(false);
      }
    }
  });
  form.addEventListener("submit", () => { cancel.hidden = false; });
  showMode();
  updateCount();
}

function discover(root) {
  if (root.nodeType !== Node.ELEMENT_NODE && root.nodeType !== Node.DOCUMENT_NODE) return;
  if (root.matches?.("#diacritize-form")) initialize(root);
  root.querySelectorAll("#diacritize-form").forEach(initialize);
}

discover(document);
// Topcoat inserts the finished result and retry form through its live view.
new MutationObserver((mutations) => {
  for (const mutation of mutations) for (const node of mutation.addedNodes) discover(node);
}).observe(document.body, { childList: true, subtree: true });

document.addEventListener("click", async (event) => {
  const button = event.target.closest?.("button[data-copy]");
  if (!button) return;
  const output = document.getElementById(button.dataset.copy);
  const status = button.closest(".result").querySelector(".copy-status");
  try {
    await navigator.clipboard.writeText(output.value);
    status.textContent = button.dataset.copied;
  } catch {
    output.focus();
    output.select();
    status.textContent = button.dataset.copyFailed;
  }
});
