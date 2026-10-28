// ASR and OCR keep their own routes and validation contracts. This controller owns
// only file selection, one submission attempt, and the accepted job's observation.
(() => {
  const app = window.arabiTools;
  const active = new Set(['reserving', 'uploading', 'validating', 'observing', 'reserved', 'queued', 'processing']);
  app.mount('[data-workspace="transcript"], [data-workspace="ocr"]', root => {
    const form = root.querySelector('#upload-form');
    const input = form.elements.file;
    const button = form.querySelector('[type=submit]');
    const progress = form.querySelector('.upload-progress');
    const bar = progress.querySelector('progress');
    const status = progress.querySelector('.submission-status');
    const percentLabel = progress.querySelector('.submission-percent');
    const detail = progress.querySelector('.server-detail');
    const error = form.querySelector('.error');
    const back = form.querySelector('.return-to-job');
    const lifetime = new AbortController();
    const attemptKey = `${form.dataset.tool}:attempt`;
    let attempt = app.stored(attemptKey);
    // Only a page being left stores its attempt. A tab opened from this page
    // cannot inherit the identity of its still-active upload.
    app.store(attemptKey, null);
    let state = { phase: 'idle', job: null };
    let generation = 0;
    let request;
    let disposed = false;
    const locked = () => active.has(state.phase);
    const current = run => !disposed && run === generation;
    const clearError = () => {
      error.hidden = true;
      back.hidden = true;
      input.setAttribute('aria-invalid', 'false');
    };
    const showError = (message, job, invalid = false) => {
      if (error.textContent !== message) error.textContent = message;
      error.hidden = false;
      back.hidden = !job;
      if (job) back.href = job;
      input.setAttribute('aria-invalid', String(invalid));
    };
    // A chosen file is one bar with the run button at its end. Result first: while the workspace
    // holds the result of the file still chosen, the bar folds into the result's card as its top
    // row and its button steps down to secondary. Choosing another file, or removing it, unfolds
    // it. The button stays the same element, so it keeps its focus. Folding above the visitor's
    // view would move the page, so then it waits until the input is back in view.
    let resultIsCurrent = false;
    const fold = () => {
      form.toggleAttribute('data-collapsed', resultIsCurrent);
      button.classList.toggle('primary', !resultIsCurrent);
      button.classList.toggle('secondary', resultIsCurrent);
    };
    const foldInView = () => {
      if (form.hasAttribute('data-collapsed') !== resultIsCurrent && form.getBoundingClientRect().top >= 0) fold();
    };
    window.addEventListener('scroll', foldInView, { passive: true, signal: lifetime.signal });
    const updateFile = app.fileSelection(form, button, locked, problem => {
      resultIsCurrent = false;
      fold();
      clearError();
      if (problem) showError(problem, null, true);
      // Without a file the run button is not shown; the drop zone takes its focus.
      if (!input.files.length && document.activeElement === button) input.focus({ preventScroll: true });
    }, lifetime.signal);
    const transition = (phase, message = '', percent) => {
      state = { ...state, phase };
      root.dataset.phase = phase;
      input.disabled = locked();
      // Keep the user's Submit button focusable throughout the operation.
      button.setAttribute('aria-disabled', String(locked()));
      updateFile();
      progress.hidden = ['idle', 'invalid', 'failed'].includes(phase);
      if (status.textContent !== message) status.textContent = message;
      bar.hidden = !active.has(phase);
      bar.setAttribute('aria-label', message || form.dataset.observing);
      if (percent === undefined) bar.removeAttribute('value'); else bar.value = percent;
      percentLabel.textContent = percent === undefined ? '' : new Intl.NumberFormat(document.documentElement.lang, { style: 'percent' }).format(percent / 100);
      if (phase !== 'processing') detail.textContent = '';
    };
    const observer = app.observeWorkspace(root, (phase, message, result, update) => {
      if (phase === 'completed' && result) {
        resultIsCurrent = true; // The input stays locked while a job runs, so it is the file sent.
        foldInView();
      }
      if (['completed', 'failed', 'missing'].includes(phase)) {
        attempt = null;
        app.store(attemptKey, null);
        clearError();
      }
      const serverBar = update.querySelector('progress[value]');
      transition(phase, message, serverBar ? serverBar.value : undefined);
      detail.textContent = update.querySelector('.page-progress')?.textContent || '';
      if (phase === 'failed') showError(message);
    });
    const post = (url, body, upload, run) => new Promise((resolve, reject) => {
      const xhr = new XMLHttpRequest();
      request = xhr;
      xhr.open('POST', url);
      xhr.responseType = 'json';
      if (upload) {
        xhr.upload.onprogress = event => {
          if (current(run) && state.phase === 'uploading' && event.lengthComputable) {
            transition('uploading', form.dataset.uploading, Math.round(event.loaded / event.total * 100));
          }
        };
        xhr.upload.onload = () => {
          if (current(run) && state.phase === 'uploading') transition('validating', form.dataset.validating);
        };
      }
      const finish = () => { if (request === xhr) request = null; };
      xhr.onload = () => {
        finish();
        if (xhr.response?.error) reject(xhr.response);
        else if (xhr.status === 413) reject({ error: form.dataset.tooLarge });
        else if (xhr.status >= 200 && xhr.status < 300 && xhr.response) resolve(xhr.response);
        else reject({ error: form.dataset.failed, uncertain: true });
      };
      xhr.onerror = xhr.onabort = () => { finish(); reject({ error: form.dataset.failed, uncertain: true }); };
      xhr.send(body);
    });
    form.addEventListener('submit', async event => {
      event.preventDefault();
      if (locked()) return;
      const problem = app.fileProblem(form);
      if (problem) { transition('invalid'); showError(problem, null, true); return; }
      const file = input.files[0];
      const run = ++generation;
      observer.begin();
      state = { phase: 'idle', job: null };
      clearError();
      transition('reserving', form.dataset.reserving);
      try {
        attempt ??= app.randomId();
        const reservation = await post(form.dataset.reserve, new URLSearchParams({ attempt }), false, run);
        if (!current(run)) return;
        state.job = reservation;
        observer.reserve(reservation);
        const body = new FormData();
        body.append('file', file);
        transition('uploading', form.dataset.uploading, 0);
        const reply = await post(reservation.upload, body, true, run);
        if (!current(run)) return;
        attempt = null;
        app.store(attemptKey, null);
        state.job = { ...reservation, ...reply };
        transition('observing', form.dataset.observing);
        // Set and announce the exact File before any result can be rendered.
        observer.watch(state.job, file);
      } catch (failure) {
        if (!current(run)) return;
        if (failure?.uncertain && state.job) {
          // An upload can have arrived even if its response was lost. Observe
          // its existing identity and keep duplicate submission locked.
          transition('observing', form.dataset.observing);
          observer.watch(state.job, file);
        } else {
          transition('failed');
        }
        showError(failure?.error || form.dataset.failed, failure?.job || state.job?.next);
      }
    }, { signal: lifetime.signal });
    window.addEventListener('pagehide', () => { if (attempt) app.store(attemptKey, attempt); }, { signal: lifetime.signal });
    window.addEventListener('pageshow', event => { if (event.persisted) app.store(attemptKey, null); }, { signal: lifetime.signal });
    transition('idle');
    return () => {
      disposed = true;
      generation++;
      lifetime.abort();
      request?.abort();
      observer.dispose();
    };
  });
})();
