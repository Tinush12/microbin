(function () {
  'use strict';

  const CLIPBOARD_IMAGE_NAME = 'image.png';
  const MAX_INPUT_HEIGHT_PX = 160;
  const UNREACHABLE_MESSAGE = 'Could not reach the server';
  const BYTES_PER_MB = 1024 * 1024;
  const PERCENT = 100;
  const CREATED = 201;

  const input = document.getElementById('input');
  const sendButton = document.getElementById('send');
  const plusButton = document.getElementById('plus');
  const picker = document.getElementById('picker');
  const strip = document.getElementById('staged');
  const dropzone = document.getElementById('dropzone');
  const errorLine = document.getElementById('composer-error');
  const statusLine = document.getElementById('composer-status');
  const maxFileBytes = Number(document.body.dataset.maxFileBytes);

  const isTouch = () => window.matchMedia('(pointer: coarse)').matches;

  let staged = [];
  let sending = false;
  let dragDepth = 0;

  function refreshSendButton() {
    sendButton.disabled = sending || (staged.length === 0 && input.value.trim() === '');
  }

  function showError(message) {
    errorLine.textContent = message;
    errorLine.hidden = message === '';
  }

  function fitInputToContent() {
    input.style.height = 'auto';
    const border = input.offsetHeight - input.clientHeight;
    input.style.height = `${Math.min(input.scrollHeight + border, MAX_INPUT_HEIGHT_PX)}px`;
  }

  function unstage(item) {
    if (item.previewUrl) URL.revokeObjectURL(item.previewUrl);
    staged = staged.filter((other) => other !== item);
    renderStrip();
  }

  function buildChip(item) {
    const chip = document.createElement('div');
    chip.className = 'chip';
    chip.title = item.file.name;
    if (item.previewUrl) {
      const preview = document.createElement('img');
      preview.src = item.previewUrl;
      preview.alt = item.file.name;
      chip.append(preview);
    } else {
      chip.append(item.file.name);
    }
    const removeButton = document.createElement('button');
    removeButton.className = 'x';
    removeButton.type = 'button';
    removeButton.textContent = '×';
    removeButton.setAttribute('aria-label', `Remove ${item.file.name}`);
    removeButton.addEventListener('click', () => unstage(item));
    chip.append(removeButton);
    return chip;
  }

  function renderStrip() {
    strip.replaceChildren(...staged.map(buildChip));
    refreshSendButton();
  }

  function isSameFile(a, b) {
    return a.name === b.name && a.size === b.size && a.lastModified === b.lastModified;
  }

  function stage(files) {
    const fits = files.filter((file) => file.size <= maxFileBytes);
    const refused = files.filter((file) => file.size > maxFileBytes);
    const limitMb = Math.ceil(maxFileBytes / BYTES_PER_MB);
    const incoming = fits.reduce((kept, file) => (
      [...staged.map((item) => item.file), ...kept].some((other) => isSameFile(other, file)) ? kept : [...kept, file]
    ), []);
    const items = incoming.map((file) => ({
      file,
      previewUrl: file.type.startsWith('image/') ? URL.createObjectURL(file) : null,
    }));
    staged = [...staged, ...items];
    if (refused.length > 0) {
      showError(refused.map((file) => `${file.name} is larger than ${limitMb} MB`).join('. '));
    } else if (items.length > 0) {
      showError('');
    }
    if (items.length > 0) renderStrip();
  }

  function clipboardFiles(clipboard) {
    const files = Array.from(clipboard.files || []);
    if (files.length > 0) return files;
    return Array.from(clipboard.items || [])
      .filter((item) => item.kind === 'file')
      .map((item) => item.getAsFile())
      .filter(Boolean);
  }

  // Clipboard screenshots always arrive as image.png, so several in a row need their own names.
  function withUniqueScreenshotName(file, index) {
    if (file.name !== CLIPBOARD_IMAGE_NAME) return file;
    const stamp = Date.now() + index;
    return new File([file], `screenshot-${stamp}.png`, { type: file.type, lastModified: stamp });
  }

  function insertIntoInput(text) {
    const start = input.selectionStart;
    input.setRangeText(text, start, input.selectionEnd, 'end');
    input.dispatchEvent(new Event('input'));
    input.focus();
  }

  function isTypingTarget(element) {
    return element instanceof HTMLInputElement || element instanceof HTMLTextAreaElement;
  }

  function onPaste(event) {
    const clipboard = event.clipboardData;
    if (!clipboard) return;
    const text = clipboard.getData('text/plain');
    const files = clipboardFiles(clipboard).map(withUniqueScreenshotName);
    if (files.length > 0) {
      stage(files);
      if (text === '') event.preventDefault();
    }
    if (text !== '' && !isTypingTarget(document.activeElement)) {
      event.preventDefault();
      insertIntoInput(text);
    }
  }

  function dragHasFiles(event) {
    return Boolean(event.dataTransfer) && Array.from(event.dataTransfer.types).includes('Files');
  }

  function onDragEnter(event) {
    if (!dragHasFiles(event)) return;
    event.preventDefault();
    dragDepth += 1;
    dropzone.hidden = false;
  }

  function onDragLeave(event) {
    if (!dragHasFiles(event)) return;
    dragDepth = Math.max(0, dragDepth - 1);
    if (dragDepth === 0) dropzone.hidden = true;
  }

  function onDragOver(event) {
    if (dragHasFiles(event)) event.preventDefault();
  }

  function onDrop(event) {
    if (!dragHasFiles(event)) return;
    event.preventDefault();
    dragDepth = 0;
    dropzone.hidden = true;
    stage(Array.from(event.dataTransfer.files));
  }

  // Things added while the send was running were not part of it, so they stay.
  function clearSent(sentItems, sentText) {
    sentItems.forEach((item) => {
      if (item.previewUrl) URL.revokeObjectURL(item.previewUrl);
    });
    staged = staged.filter((item) => !sentItems.includes(item));
    if (input.value.startsWith(sentText)) input.value = input.value.slice(sentText.length).trimStart();
    fitInputToContent();
    renderStrip();
  }

  function serverMessage(request) {
    try {
      const { error } = JSON.parse(request.responseText);
      if (error) return error;
    } catch (parseError) {
      console.error(parseError);
    }
    return `Could not send (error ${request.status})`;
  }

  function showSentEntries(entries) {
    if (!window.Feed.holdsNewest()) {
      // Viewing the middle of the history: the new entries are at the end, so go there.
      window.location.assign('/');
      return;
    }
    window.Feed.upsert(entries);
    window.Feed.scrollToBottom();
    window.Feed.refresh();
  }

  function showProgress(event) {
    const percent = event.lengthComputable ? ` ${Math.floor((event.loaded / event.total) * PERCENT)}%` : '';
    statusLine.textContent = `Sending…${percent}`;
  }

  // XMLHttpRequest rather than fetch, because only it reports upload progress.
  function postForm(form) {
    return new Promise((resolve, reject) => {
      const request = new XMLHttpRequest();
      request.open('POST', '/api/entries');
      request.upload.addEventListener('progress', showProgress);
      request.addEventListener('load', () => {
        if (request.status === CREATED) resolve(JSON.parse(request.responseText).entries);
        else reject(new Error(serverMessage(request)));
      });
      request.addEventListener('error', () => reject(new Error(UNREACHABLE_MESSAGE)));
      request.send(form);
    });
  }

  async function send() {
    if (sendButton.disabled) return;
    const sentItems = staged;
    const sentText = input.value;
    const form = new FormData();
    if (sentText.trim() !== '') form.append('text', sentText);
    sentItems.forEach((item) => form.append('file', item.file, item.file.name));
    sending = true;
    refreshSendButton();
    showError('');
    statusLine.textContent = 'Sending…';
    statusLine.hidden = false;
    try {
      const entries = await postForm(form);
      clearSent(sentItems, sentText);
      showSentEntries(entries);
    } catch (error) {
      showError(error.message);
    } finally {
      sending = false;
      statusLine.hidden = true;
      refreshSendButton();
    }
  }

  function onInputKeydown(event) {
    if (event.key !== 'Enter' || event.shiftKey || event.isComposing || isTouch()) return;
    event.preventDefault();
    send();
  }

  input.addEventListener('input', () => {
    fitInputToContent();
    refreshSendButton();
  });
  input.addEventListener('keydown', onInputKeydown);
  sendButton.addEventListener('click', send);
  plusButton.addEventListener('click', () => picker.click());
  picker.addEventListener('change', () => {
    stage(Array.from(picker.files));
    picker.value = '';
  });
  document.addEventListener('paste', onPaste);
  document.addEventListener('dragenter', onDragEnter);
  document.addEventListener('dragleave', onDragLeave);
  document.addEventListener('dragover', onDragOver);
  document.addEventListener('drop', onDrop);

  fitInputToContent();
  if (!isTouch()) input.focus();
})();
