// Uploads the chosen file with a progress bar, after reserving a place in the queue so a
// full queue is reported before any time is spent uploading. Messages come from the form's
// data attributes, already in the page's language.

const form = document.getElementById("upload-form");

if (form) {
  const input = form.elements.file;
  const fileName = form.querySelector(".file-name");
  const button = form.querySelector("button[type=submit]");
  const progress = form.querySelector(".upload-progress");
  const bar = progress.querySelector("progress");
  const error = form.querySelector(".error");
  const back = form.querySelector(".return-to-job");
  const extensions = form.dataset.extensions.split(",");
  const maxBytes = Number(form.dataset.maxBytes);
  let inProgress = false;

  const setInProgress = (value) => {
    inProgress = value;
    input.disabled = value;
    button.disabled = value;
  };

  const randomId = () =>
    Array.from(crypto.getRandomValues(new Uint8Array(16)), (byte) =>
      byte.toString(16).padStart(2, "0"),
    ).join("");
  // This tab's upload attempt, which lets it replace a reservation it lost. It is made when a
  // file is sent and dropped once the upload arrives. A duplicated tab, or one this page opens,
  // starts with a copy of the session storage, so the attempt is kept there only while the
  // page is away, for a reload or the Back button to pick up.
  let attempt = null;
  const takeStoredAttempt = () => {
    try {
      attempt = sessionStorage.getItem("attempt") || attempt;
      sessionStorage.removeItem("attempt");
    } catch {
      // Storage is blocked, so the attempt ends with the page.
    }
  };
  takeStoredAttempt();
  window.addEventListener("pagehide", () => {
    try {
      if (attempt) sessionStorage.setItem("attempt", attempt);
    } catch {
      // Storage is blocked.
    }
  });

  const problemWith = (file) => {
    if (!file) return form.dataset.missing;
    const dot = file.name.lastIndexOf(".");
    const extension = dot > 0 ? file.name.slice(dot + 1).toLowerCase() : "";
    if (!extensions.includes(extension)) return form.dataset.wrongType;
    if (file.size > maxBytes) return form.dataset.tooLarge;
    return null;
  };

  const showError = (message, job) => {
    error.textContent = message;
    error.hidden = false;
    progress.hidden = true;
    back.hidden = !job;
    if (job) back.href = job;
  };

  // Sends a POST and resolves with its JSON reply, or rejects with an error to show.
  const post = (url, body, onProgress) =>
    new Promise((resolve, reject) => {
      const request = new XMLHttpRequest();
      request.open("POST", url);
      request.responseType = "json";
      if (onProgress) {
        request.upload.onprogress = (event) => {
          if (event.lengthComputable) onProgress(event.loaded / event.total);
        };
      }
      request.onload = () => {
        const reply = request.response;
        if (reply && reply.error) reject(reply);
        else if (request.status === 413) reject({ error: form.dataset.tooLarge });
        else if (request.status < 300 && reply) resolve(reply);
        else reject({ error: form.dataset.failed });
      };
      request.onerror = () => reject({ error: form.dataset.failed });
      request.send(body);
    });

  // The Back button can restore this page from the browser's cache, even mid-submission.
  window.addEventListener("pageshow", (event) => {
    if (!event.persisted) return;
    takeStoredAttempt();
    setInProgress(false);
    progress.hidden = true;
  });

  input.addEventListener("change", () => {
    if (inProgress) return;
    if (input.files[0]) fileName.textContent = input.files[0].name;
    const problem = problemWith(input.files[0]);
    if (problem) showError(problem);
    else {
      error.hidden = true;
      back.hidden = true;
    }
  });

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    if (inProgress) return;
    const file = input.files[0];
    const problem = problemWith(file);
    if (problem) return showError(problem);

    error.hidden = true;
    back.hidden = true;
    setInProgress(true);
    bar.value = 0;
    progress.hidden = false;
    try {
      attempt ??= randomId();
      const reservation = await post(form.dataset.reserve, new URLSearchParams({ attempt }));
      const data = new FormData();
      data.append("file", file);
      const result = await post(reservation.upload, data, (done) => {
        bar.value = Math.round(done * 100);
      });
      attempt = null; // Its reservation is used, so there is nothing left to replace.
      window.location.assign(result.next);
    } catch (failure) {
      setInProgress(false);
      showError(failure?.error || form.dataset.failed, failure?.job);
    }
  });
}
