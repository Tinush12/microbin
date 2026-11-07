(function () {
  'use strict';

  const CLIPBOARD_IMAGE_NAME = 'image.png';
  const MAX_INPUT_HEIGHT_PX = 160;
  const UNREACHABLE_MESSAGE = 'Could not reach the server';

  const input = document.getElementById('input');
  const sendButton = document.getElementById('send');
  const plusButton = document.getElementById('plus');
  const picker = document.getElementById('picker');
  const strip = document.getElementById('staged');
  const dropzone = document.getElementById('dropzone');
  const errorLine = document.getElementById('composer-error');

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
    const incoming = files.reduce((kept, file) => (
      [...staged.map((item) => item.file), ...kept].some((other) => isSameFile(other, file)) ? kept : [...kept, file]
    ), []);
    if (incoming.length === 0) return;
    const items = incoming.map((file) => ({
      file,
      previewUrl: file.type.startsWith('image/') ? URL.createObjectURL(file) : null,
    }));
    staged = [...staged, ...items];
    showError('');
    renderStrip();
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

  function clearComposer() {
    staged.forEach((item) => {
      if (item.previewUrl) URL.revokeObjectURL(item.previewUrl);
    });
    staged = [];
    input.value = '';
    fitInputToContent();
    renderStrip();
  }

  async function serverMessage(response) {
    try {
      const body = await response.json();
      if (body.error) return body.error;
    } catch (error) {
      console.error(error);
    }
    return `Could not send (error ${response.status})`;
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

  async function postForm(form) {
    const response = await fetch('/api/entries', { method: 'POST', body: form });
    if (response.status !== 201) throw new Error(await serverMessage(response));
    return (await response.json()).entries;
  }

  async function send() {
    if (sendButton.disabled) return;
    const form = new FormData();
    if (input.value.trim() !== '') form.append('text', input.value);
    staged.forEach((item) => form.append('file', item.file, item.file.name));
    sending = true;
    refreshSendButton();
    showError('');
    try {
      const entries = await postForm(form);
      clearComposer();
      showSentEntries(entries);
    } catch (error) {
      // fetch rejects with a TypeError when the request never got an answer.
      showError(error instanceof TypeError ? UNREACHABLE_MESSAGE : error.message);
    } finally {
      sending = false;
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
