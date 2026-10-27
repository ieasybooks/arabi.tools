// One state owns submission, observation and cancellation. Source text stays in the DOM.
(() => {
  const app = window.arabiTools;
  const terminal = new Set(['completed', 'failed', 'cancelled', 'missing']);
  const active = new Set(['reserving', 'uploading', 'validating', 'submitting', 'observing', 'reserved', 'queued', 'processing', 'cancelling', 'cancel-unconfirmed']);
  app.mount('[data-workspace="diacritization"]', root => {
    const form = root.querySelector('#diacritize-form');
    const source = form.elements.text;
    const file = form.elements.file;
    const models = [...form.querySelectorAll('[name=model]')];
    const modes = [...form.querySelectorAll('[name=input_mode]')];
    const buttons = [...form.querySelectorAll('[type=submit]')];
    const unavailable = new Set(models.filter(input => input.disabled));
    const count = form.querySelector('#text-count');
    const error = form.querySelector('.error');
    const back = form.querySelector('.return-to-job');
    const progress = form.querySelector('.upload-progress');
    const bar = progress.querySelector('progress');
    const status = progress.querySelector('.submission-status');
    const cancel = progress.querySelector('.cancel-upload');
    const lifetime = new AbortController();
    const attemptKey = 'diacritize:attempt';
    const choiceKey = 'diacritize:choice';
    let attempt = app.stored(attemptKey);
    app.store(attemptKey, null);
    let state = { phase: 'idle', job: null };
    let generation = 0;
    let request = null;
    let disposed = false;
    let restoreSource = root.dataset.restoreSource === 'true';
    const locked = () => active.has(state.phase);
    const current = run => !disposed && run === generation;
    const updateFile = app.fileSelection(form, locked, () => {
      clearError();
      if (file.files.length) {
        const problem = fileProblem();
        if (problem) { file.setAttribute('aria-invalid', 'true'); showError(problem); }
      }
    }, lifetime.signal);

    function clearError() {
      error.hidden = true;
      back.hidden = true;
      file.setAttribute('aria-invalid', 'false');
      source.setAttribute('aria-invalid', String(Array.from(source.value).length > Number(form.dataset.maxCharacters)));
    }
    function showError(message, job) {
      if (error.textContent !== message) error.textContent = message;
      error.hidden = false;
      back.hidden = !job;
      if (job) back.href = job;
    }
    function transition(phase, message = '', percent) {
      state = { ...state, phase };
      root.dataset.phase = phase;
      source.readOnly = locked();
      for (const input of [...models, ...modes, file]) input.disabled = locked() || unavailable.has(input);
      for (const button of buttons) button.setAttribute('aria-disabled', String(locked()));
      updateFile();
      progress.hidden = phase === 'idle' || phase === 'invalid' || phase === 'failed';
      if (status.textContent !== message) status.textContent = message;
      bar.hidden = !['uploading', 'validating', 'queued', 'processing', 'reserving', 'observing'].includes(phase);
      bar.setAttribute('aria-label', message || form.dataset.observing);
      if (percent === undefined) bar.removeAttribute('value'); else bar.value = percent;
      const cancelledFocus = document.activeElement === cancel;
      cancel.hidden = !locked() || (!state.job?.cancel && phase !== 'reserving' && phase !== 'cancelling' && phase !== 'cancel-unconfirmed');
      cancel.setAttribute('aria-disabled', String(phase === 'cancelling'));
      if (cancelledFocus && cancel.hidden) buttons.find(button => !button.closest('.diacritize-panel').hidden)?.focus({ preventScroll: true });
    }
    function updateCount() {
      const length = Array.from(source.value).length;
      const fraction = document.createElement('bdi');
      fraction.dir = 'ltr';
      fraction.textContent = `${length} / ${form.dataset.maxCharacters}`;
      count.replaceChildren(fraction, document.createTextNode(form.dataset.count.split('{max}')[1] || ''));
      count.setAttribute('aria-label', form.dataset.count.replace('{count}', length).replace('{max}', form.dataset.maxCharacters));
      const invalid = length > Number(form.dataset.maxCharacters);
      source.setAttribute('aria-invalid', String(invalid));
      count.dataset.invalid = String(invalid);
      if (invalid && form.elements.input_mode.value === 'text') showError(form.dataset.textLimit);
      else if (error.textContent === form.dataset.textLimit || (error.textContent === form.dataset.textMissing && source.value.trim())) error.hidden = true;
    }
    function showMode() {
      const documentMode = form.elements.input_mode.value === 'document';
      form.querySelector('#diacritize-text-panel').hidden = documentMode;
      form.querySelector('#diacritize-document-panel').hidden = !documentMode;
      clearError();
      if (!documentMode) updateCount();
    }
    function rememberChoice() {
      app.store(choiceKey, JSON.stringify({ model: form.elements.model.value, mode: form.elements.input_mode.value }));
    }
    if (form.dataset.retry === 'true') {
      modes.find(input => input.value === form.dataset.initialMode).checked = true;
    } else {
      try {
        const choice = JSON.parse(app.stored(choiceKey));
        const model = models.find(input => input.value === choice?.model && !input.disabled);
        const mode = modes.find(input => input.value === choice?.mode);
        if (model) model.checked = true;
        if (mode) mode.checked = true;
      } catch { /* Old or unavailable preferences use the page defaults. */ }
    }
    function fileProblem() {
      const selected = file.files[0];
      if (!selected) return form.dataset.missing;
      const dot = selected.name.lastIndexOf('.');
      const extension = dot > 0 ? selected.name.slice(dot + 1).toLowerCase() : '';
      if (file.files.length !== 1 || !form.dataset.extensions.split(',').includes(extension)) return form.dataset.wrongType;
      if (selected.size > Number(form.dataset.maxBytes)) return form.dataset.tooLarge;
      return null;
    }
    function post(url, body, upload = false) {
      return new Promise((resolve, reject) => {
        const run = generation;
        const xhr = new XMLHttpRequest();
        request = xhr;
        xhr.open('POST', url);
        xhr.responseType = 'json';
        if (upload) {
          xhr.upload.onprogress = event => {
            if (current(run) && state.phase === 'uploading' && event.lengthComputable) transition('uploading', form.dataset.uploading, Math.round(event.loaded / event.total * 100));
          };
          xhr.upload.onload = () => { if (current(run) && state.phase === 'uploading') transition('validating', form.dataset.validating); };
        }
        const finish = () => { if (request === xhr) request = null; };
        xhr.onload = () => {
          finish();
          if (xhr.response?.error) reject(xhr.response);
          else if (xhr.status === 413) reject({ error: form.dataset.tooLarge });
          else if (xhr.status >= 200 && xhr.status < 300 && xhr.response) resolve(xhr.response);
          else reject({ error: form.dataset.failed, uncertain: true });
        };
        xhr.onerror = () => { finish(); reject({ error: form.dataset.failed, uncertain: true }); };
        xhr.onabort = () => { finish(); reject({ error: form.dataset.failed, uncertain: true }); };
        xhr.send(body);
      });
    }
    const observer = app.observeWorkspace(root, (phase, message, result, update) => {
      if (state.phase === 'cancelling' || state.phase === 'cancel-unconfirmed') {
        if (!terminal.has(phase)) return;
      }
      if (result && restoreSource) {
        source.value = result.dataset.source || '';
        models.find(input => input.value === result.dataset.modelValue).checked = true;
        modes.find(input => input.value === result.dataset.mode).checked = true;
        restoreSource = false;
        showMode();
        updateCount();
        const explanation = root.querySelector('.source-needed');
        if (explanation) explanation.hidden = result.dataset.mode === 'text';
      }
      if (terminal.has(phase)) {
        attempt = null;
        app.store(attemptKey, null);
        clearError();
      }
      transition(phase, message, update.querySelector('progress[value]')?.value);
      if (phase === 'failed') showError(message);
    });
    async function cancelJob() {
      if (!locked() || state.phase === 'cancelling') return;
      const run = generation;
      let pending = state.job;
      const waitingForReservation = state.phase === 'reserving';
      clearError();
      transition('cancelling', form.dataset.cancelling);
      if (!pending) {
        if (waitingForReservation) return; // The original request continues with this intent.
        try {
          pending = await post(form.dataset.reserve, new URLSearchParams({ attempt }));
          if (!current(run)) return;
          state.job = pending;
          observer.reserve(pending);
        } catch {
          if (current(run)) { transition('cancel-unconfirmed'); showError(form.dataset.cancelFailed); }
          return;
        }
      }
      try {
        const response = await fetch(pending.cancel, { method: 'POST', headers: { Accept: 'application/json' }, redirect: 'error', signal: lifetime.signal });
        const reply = await response.json();
        if (!response.ok || typeof reply.cancelled !== 'boolean' || typeof reply.next !== 'string') throw new Error();
        if (!current(run)) return;
        // Aborting a partial upload can remove its reservation. Let the HTTP
        // cancellation acknowledge this job before ending the transfer.
        request?.abort();
        if (terminal.has(state.phase)) return;
        attempt = null;
        app.store(attemptKey, null);
        if (reply.cancelled) {
          observer.begin();
          transition('cancelled', form.dataset.cancelled);
        } else {
          transition('observing', form.dataset.observing);
          observer.watch({ ...pending, ...reply });
        }
      } catch {
        if (!current(run) || terminal.has(state.phase)) return;
        transition('cancel-unconfirmed');
        showError(form.dataset.cancelFailed, pending.next);
      }
    }
    cancel.addEventListener('click', () => { void cancelJob(); }, { signal: lifetime.signal });
    source.addEventListener('input', updateCount, { signal: lifetime.signal });
    for (const input of modes) input.addEventListener('change', () => { showMode(); rememberChoice(); }, { signal: lifetime.signal });
    for (const input of models) input.addEventListener('change', () => { clearError(); rememberChoice(); }, { signal: lifetime.signal });
    window.addEventListener('pagehide', () => { if (attempt) app.store(attemptKey, attempt); }, { signal: lifetime.signal });
    window.addEventListener('pageshow', event => { if (event.persisted) app.store(attemptKey, null); }, { signal: lifetime.signal });
    form.addEventListener('submit', async event => {
      event.preventDefault();
      if (locked()) return;
      const model = models.find(input => input.checked && !unavailable.has(input));
      const documentMode = form.elements.input_mode.value === 'document';
      const text = source.value;
      const problem = !model ? form.dataset.modelUnavailable : documentMode ? fileProblem()
        : !text.trim() ? form.dataset.textMissing : Array.from(text).length > Number(form.dataset.maxCharacters) ? form.dataset.textLimit : null;
      if (problem) {
        transition('invalid');
        showError(problem);
        (documentMode ? file : source).setAttribute('aria-invalid', 'true');
        return;
      }
      const chosenFile = file.files[0];
      const run = ++generation;
      restoreSource = false;
      observer.begin();
      state = { phase: 'idle', job: null };
      clearError();
      rememberChoice();
      transition('reserving', form.dataset.reserving);
      try {
        attempt ??= app.randomId();
        const reservation = await post(form.dataset.reserve, new URLSearchParams({ attempt }));
        if (!current(run)) return;
        state.job = reservation;
        observer.reserve(reservation);
        if (state.phase === 'cancelling') {
          transition('cancel-unconfirmed');
          await cancelJob();
          return;
        }
        let body;
        if (documentMode) {
          body = new FormData(); body.append('model', model.value); body.append('file', chosenFile);
        } else body = new URLSearchParams({ model: model.value, text });
        transition(documentMode ? 'uploading' : 'submitting', documentMode ? form.dataset.uploading : form.dataset.submitting, documentMode ? 0 : undefined);
        const reply = await post(documentMode ? reservation.upload : reservation.text, body, documentMode);
        if (!current(run) || ['cancelling', 'cancel-unconfirmed', 'cancelled'].includes(state.phase)) return;
        attempt = null;
        app.store(attemptKey, null);
        state.job = { ...reservation, ...reply };
        transition('observing', form.dataset.observing);
        observer.watch(state.job, documentMode ? chosenFile : null);
      } catch (failure) {
        if (current(run) && state.phase === 'cancelling' && !state.job) {
          transition('cancel-unconfirmed');
          showError(form.dataset.cancelFailed);
          return;
        }
        if (!current(run) || ['cancelling', 'cancel-unconfirmed', 'cancelled'].includes(state.phase)) return;
        if (failure?.uncertain && state.job) {
          transition('observing', form.dataset.observing);
          observer.watch(state.job, documentMode ? chosenFile : null);
        } else {
          transition('failed');
        }
        showError(failure?.error || form.dataset.failed, failure?.job || state.job?.next);
      }
    }, { signal: lifetime.signal });
    showMode();
    updateCount();
    transition('idle');
    for (const button of buttons) button.disabled = false;
    if (root.dataset.initialJob) {
      state.job = { job_id: root.dataset.initialJob, next: root.dataset.initialUrl, cancel: root.dataset.cancelUrl };
      transition('observing', form.dataset.observing);
      observer.watch(state.job);
    }
    return () => { disposed = true; generation++; lifetime.abort(); request?.abort(); observer.dispose(); };
  });
  document.addEventListener('click', async event => {
    const button = event.target.closest?.('.diacritize-result button[data-copy]');
    if (!button) return;
    const result = button.closest('.result');
    const output = result.querySelector('[name=result]');
    const status = result.querySelector('.copy-status');
    try {
      await navigator.clipboard.writeText(output.value);
      status.textContent = button.dataset.copied;
    } catch {
      output.focus({ preventScroll: true });
      output.select();
      status.textContent = button.dataset.copyFailed;
    }
  });
})();
