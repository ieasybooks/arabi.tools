// Browser controllers communicate with Topcoat only through documented DOM events.
(() => {
  const app = window.arabiTools ??= {};
  app.sources ??= new Map();
  app.stored = key => { try { return sessionStorage.getItem(key); } catch { return null; } };
  app.store = (key, value) => {
    try { if (value === null) sessionStorage.removeItem(key); else sessionStorage.setItem(key, value); } catch { /* Storage is optional. */ }
  };
  app.randomId = () => Array.from(crypto.getRandomValues(new Uint8Array(16)), byte => byte.toString(16).padStart(2, '0')).join('');
  const mounts = [];
  const controllers = new Map();
  const discover = () => {
    for (const [root, dispose] of controllers) {
      if (!root.isConnected) { dispose(); controllers.delete(root); }
    }
    for (const [selector, initialize] of mounts) {
      for (const root of document.querySelectorAll(selector)) {
        if (!controllers.has(root)) controllers.set(root, initialize(root));
      }
    }
  };
  app.mount = (selector, initialize) => { mounts.push([selector, initialize]); discover(); };
  new MutationObserver(discover).observe(document.documentElement, { childList: true, subtree: true });
  // This also covers a local file reselected by a viewer on a direct job page.
  document.addEventListener('submit', event => {
    const form = event.target;
    if (!form.matches?.('.language-switch')) return;
    const localInput = document.querySelector('#source-text')?.value
      || [...document.querySelectorAll('input[type=file]')].some(input => input.files.length)
      || app.sources.size
      || document.querySelector('[data-result="ocr"][data-source-attached]');
    if (localInput && !window.confirm(form.dataset.reloadMessage)) event.preventDefault();
  });
  const extensionOf = name => {
    const dot = name.lastIndexOf('.');
    return dot > 0 ? name.slice(dot + 1).toLowerCase() : '';
  };
  // Why the chosen file cannot be sent, in the form's own words, or null. The server checks again.
  app.fileProblem = form => {
    const input = form.elements.file;
    const file = input.files[0];
    if (!file) return form.dataset.missing;
    if (input.files.length !== 1 || !form.dataset.extensions.split(',').includes(extensionOf(file.name))) return form.dataset.wrongType;
    if (file.size > Number(form.dataset.maxBytes)) return form.dataset.tooLarge;
    return null;
  };
  // A file the tool cannot take is refused as it is chosen: it never becomes the file row, the
  // drop zone stays with the message, and the run button waits for a file it can send.
  app.fileSelection = (form, button, locked, changed, signal) => {
    const input = form.elements.file;
    const remove = form.querySelector('.remove-file');
    const update = () => {
      const file = input.files[0];
      form.querySelector('.file-name').textContent = file?.name || '';
      const size = form.querySelector('.file-size');
      if (size) {
        const divisor = !file || file.size < 1000 ? 1 : file.size < 1_000_000 ? 1000 : 1_000_000;
        const unit = divisor === 1 ? form.dataset.bytes : divisor === 1000 ? form.dataset.kilobytes : form.dataset.megabytes;
        size.textContent = file ? `${new Intl.NumberFormat(document.documentElement.lang, { maximumFractionDigits: 1 }).format(file.size / divisor)} ${unit} · ${file.name.split('.').pop().toUpperCase()}` : '';
      }
      const extension = file ? extensionOf(file.name) : '';
      for (const icon of form.querySelectorAll('.file-icon')) icon.hidden = !icon.dataset.extensions.split(',').includes(extension);
      if (remove) remove.disabled = locked() || !file;
      button.disabled = !file;
      form.querySelector('.dropzone').dataset.selected = String(Boolean(file));
    };
    input.addEventListener('change', () => {
      if (locked()) return;
      const problem = input.files.length ? app.fileProblem(form) : null;
      if (problem) input.value = '';
      update();
      changed(problem);
    }, { signal });
    remove?.addEventListener('click', () => {
      if (locked()) return;
      input.value = '';
      update();
      changed(null);
      // The file row is gone; the drop zone takes its place and the focus.
      input.focus({ preventScroll: true });
    }, { signal });
    update();
    return update;
  };
  const terminal = new Set(['completed', 'failed', 'cancelled', 'missing']);
  app.observeWorkspace = (root, changed) => {
    const bridge = root.querySelector('.workspace-bridge');
    const observer = root.querySelector('.workspace-observer');
    observer.hidden = true;
    const output = root.querySelector('.workspace-result');
    const delayed = root.querySelector('.observation-status');
    const retry = root.querySelector('.retry-observation');
    const back = root.querySelector('.workspace-recovery');
    const previous = root.querySelector('.previous-result');
    const view = root.querySelector('.view-result');
    const lifetime = new AbortController();
    let jobId = null;
    let resultId = output.querySelector('[data-result]')?.dataset.jobId;
    let desired = '';
    let ended = false;
    let lastUpdate = Date.now();
    let retryTimer;
    const sync = () => {
      if (bridge.dataset.ack !== desired) {
        bridge.value = desired;
        bridge.dispatchEvent(new Event('input', { bubbles: true }));
      }
    };
    const release = (id) => {
      if (id && app.sources.delete(id)) document.dispatchEvent(new CustomEvent('arabi:source', { detail: { jobId: id } }));
    };
    const read = () => {
      const update = observer.querySelector('[data-phase]');
      if (!update || ended || update.dataset.jobId !== jobId) return;
      lastUpdate = Date.now();
      if (delayed) delayed.hidden = true;
      if (retry) retry.hidden = true;
      const phase = update.dataset.phase;
      const result = update.querySelector('[data-result]');
      if (result) {
        if (resultId !== jobId) release(resultId);
        resultId = jobId;
        output.replaceChildren(result);
        if (previous) previous.hidden = true;
        if (view) view.hidden = false;
      }
      if (terminal.has(phase)) {
        if (phase === 'completed' && !result) return; // Keep the recovery path if rendering is incomplete.
        ended = true;
        desired = '';
        if (resultId !== jobId) release(jobId);
        sync(); // End the shard subscription without remounting the finished result.
      }
      changed(phase, update.querySelector('.status')?.textContent || '', result, update);
    };
    const mutations = new MutationObserver(read);
    mutations.observe(observer, { childList: true, subtree: true, attributes: true, characterData: true });
    const timer = setInterval(() => {
      sync();
      if (jobId && !ended && Date.now() - lastUpdate > 45_000) {
        if (delayed) delayed.hidden = false;
        if (retry) retry.hidden = false;
      }
    }, 250);
    retry?.addEventListener('click', () => {
      const id = jobId;
      desired = '';
      sync();
      clearTimeout(retryTimer);
      retryTimer = setTimeout(() => {
        if (jobId !== id || ended) return;
        desired = id;
        lastUpdate = Date.now();
        sync();
      }, 0);
    }, { signal: lifetime.signal });
    view?.addEventListener('click', event => {
      event.preventDefault(); // Keep the page's URL; the result takes the focus instead.
      output.scrollIntoView({ block: 'nearest', behavior: 'instant' });
      output.focus({ preventScroll: true });
    }, { signal: lifetime.signal });
    const recover = reply => {
      if (back) { back.href = reply.next; back.hidden = false; }
      const language = document.querySelector('.language-switch input[name=back]');
      if (language) language.value = reply.next;
    };
    return {
      reserve: recover,
      begin() {
        if (jobId !== resultId) release(jobId);
        if (back) back.hidden = true; // Until the next job has a link of its own.
        jobId = null;
        desired = '';
        ended = false;
        if (delayed) delayed.hidden = true;
        if (retry) retry.hidden = true;
        sync();
        const result = output.querySelector('[data-result]');
        if (previous && result) {
          previous.textContent = result.dataset.model
            ? root.dataset.previousModel.replace('{model}', result.dataset.model)
            : root.dataset.previous;
          previous.hidden = false;
        }
      },
      watch(reply, file) {
        jobId = reply.job_id;
        ended = false;
        desired = jobId;
        lastUpdate = Date.now();
        if (file) {
          app.sources.set(jobId, file);
          document.dispatchEvent(new CustomEvent('arabi:source', { detail: { jobId } }));
        }
        recover(reply);
        sync();
        read();
      },
      dispose() {
        clearInterval(timer);
        clearTimeout(retryTimer);
        lifetime.abort();
        mutations.disconnect();
        desired = '';
        sync();
        release(jobId);
        release(resultId);
      },
    };
  };
})();
