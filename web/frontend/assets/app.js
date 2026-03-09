function fileUrlFromPath(path) {
  const parts = path.split('/').filter(Boolean).map(encodeURIComponent);
  return '/files/' + parts.join('/');
}

function setPreviewFromPath(inputEl, imgEl) {
  const val = inputEl.value.trim();
  if (!val) {
    imgEl.removeAttribute('src');
    return;
  }
  imgEl.src = fileUrlFromPath(val);
}

async function uploadFileAndSetPath(fileInputId, pathInput, statusEl, previewEl) {
  const fileInput = document.getElementById(fileInputId);
  if (!fileInput || !fileInput.files || fileInput.files.length === 0) return;

  const file = fileInput.files[0];
  previewEl.src = URL.createObjectURL(file);
  statusEl.textContent = 'Uploading...';

  const form = new FormData();
  form.append('image', file, file.name);
  const res = await fetch('/api/upload-image', { method: 'POST', body: form });
  if (!res.ok) {
    statusEl.textContent = 'Upload failed';
    return;
  }

  const data = await res.json();
  pathInput.value = data.path;
  statusEl.textContent = 'Uploaded to ' + data.path;
  setPreviewFromPath(pathInput, previewEl);
}

function encodeForm(form) {
  const params = new URLSearchParams();
  const data = new FormData(form);
  for (const [k, v] of data.entries()) {
    params.append(k, v);
  }
  return params;
}

function renderCmd(step) {
  const statusClass = step.ok ? 'ok' : 'bad';
  const status = step.ok ? 'success' : 'failed';
  return `<div><h4>${step.name}</h4><p class="${statusClass}">status: ${status}</p><pre>${escapeHtml(step.stdout || '')}${step.stderr ? '\n' + escapeHtml(step.stderr) : ''}</pre></div>`;
}

function escapeHtml(s) {
  return s
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#39;');
}

function initIndexPage() {
  const inputPath = document.getElementById('input_image');
  const preview = document.getElementById('mock_preview');
  const mockStatus = document.getElementById('mock_status');
  document.getElementById('mock_file').addEventListener('change', () => {
    uploadFileAndSetPath('mock_file', inputPath, mockStatus, preview);
  });
  inputPath.addEventListener('input', () => setPreviewFromPath(inputPath, preview));
  setPreviewFromPath(inputPath, preview);

  const c2paPath = document.getElementById('c2pa_input');
  const c2paPreview = document.getElementById('c2pa_preview');
  const c2paStatus = document.getElementById('c2pa_status');
  document.getElementById('c2pa_file').addEventListener('change', () => {
    uploadFileAndSetPath('c2pa_file', c2paPath, c2paStatus, c2paPreview);
  });
  c2paPath.addEventListener('input', () => setPreviewFromPath(c2paPath, c2paPreview));
  setPreviewFromPath(c2paPath, c2paPreview);

  const pipelineForm = document.getElementById('pipelineForm');
  const pipelineResult = document.getElementById('pipeline_result');
  pipelineForm.addEventListener('submit', async (e) => {
    e.preventDefault();
    pipelineResult.innerHTML = '<p>Running pipeline...</p>';

    const res = await fetch('/api/run-pipeline', {
      method: 'POST',
      headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
      body: encodeForm(pipelineForm),
    });
    const data = await res.json();

    const links = data.artifacts
      ? `<ul>
          <li><a href="/files/${data.artifacts.metadata}">metadata</a></li>
          <li><a href="/files/${data.artifacts.edited_image}">edited image</a></li>
          <li><a href="/files/${data.artifacts.riscv_proof}">riscv proof</a></li>
          <li><a href="/files/${data.artifacts.public_values}">public values</a></li>
        </ul>`
      : '';
    const verifyHref = data.artifacts
      ? `/verify-page?edited_image=${encodeURIComponent(data.artifacts.edited_image)}&metadata=${encodeURIComponent(data.artifacts.metadata)}&riscv_proof=${encodeURIComponent(data.artifacts.riscv_proof)}&public_values=${encodeURIComponent(data.artifacts.public_values)}`
      : '/verify-page';

    const stepsHtml = (data.steps || []).map(renderCmd).join('');
    pipelineResult.innerHTML = `
      <h3>Pipeline Result</h3>
      <p class="${data.ok ? 'ok' : 'bad'}">overall: ${data.ok ? 'success' : 'failed'}</p>
      <p>run_dir: <code>${escapeHtml(data.run_dir || '')}</code></p>
      ${links}
      <p><a href="${verifyHref}">Go to verify page with this run</a></p>
      ${stepsHtml}
    `;
  });

  const c2paForm = document.getElementById('c2paForm');
  const c2paResult = document.getElementById('c2pa_result');
  c2paForm.addEventListener('submit', async (e) => {
    e.preventDefault();
    c2paResult.innerHTML = '<p>Running verify-c2pa...</p>';
    const res = await fetch('/api/verify-c2pa', {
      method: 'POST',
      headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
      body: encodeForm(c2paForm),
    });
    const data = await res.json();
    c2paResult.innerHTML = `
      <h3>C2PA Result</h3>
      <p class="${data.ok ? 'ok' : 'bad'}">overall: ${data.ok ? 'success' : 'failed'}</p>
      ${data.verify ? renderCmd(data.verify) : ''}
    `;
  });
}

function initVerifyPage() {
  const params = new URLSearchParams(window.location.search);
  ['edited_image', 'metadata', 'riscv_proof', 'public_values'].forEach((key) => {
    const el = document.getElementById(key);
    if (el && params.get(key)) {
      el.value = params.get(key);
    }
  });

  const editedPath = document.getElementById('edited_image');
  const preview = document.getElementById('verify_preview');
  editedPath.addEventListener('input', () => setPreviewFromPath(editedPath, preview));
  setPreviewFromPath(editedPath, preview);

  const form = document.getElementById('verifyForm');
  const result = document.getElementById('verify_result');
  form.addEventListener('submit', async (e) => {
    e.preventDefault();
    result.innerHTML = '<p>Verifying...</p>';

    const res = await fetch('/api/verify-artifacts', {
      method: 'POST',
      headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
      body: encodeForm(form),
    });
    const data = await res.json();

    let pvSection = '';
    if (data.public_values_text) {
      pvSection = `<h4>public values</h4><pre>${escapeHtml(data.public_values_text)}</pre>`;
    }

    result.innerHTML = `
      <h3>Verification Result</h3>
      <p class="${data.ok ? 'ok' : 'bad'}">overall: ${data.ok ? 'success' : 'failed'}</p>
      ${data.verify ? renderCmd(data.verify) : ''}
      ${pvSection}
    `;
  });
}

const page = document.body.dataset.page;
if (page === 'index') initIndexPage();
if (page === 'verify') initVerifyPage();
