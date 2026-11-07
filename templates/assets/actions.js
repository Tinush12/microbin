(function () {
  'use strict';

  const COPIED_MS = 1500;
  const EXPIRY_CHOICES = [
    ['never', 'Never'],
    ['1h', '1 hour'],
    ['1d', '1 day'],
    ['1w', '1 week'],
  ];

  function button(label, onClick, className) {
    const element = document.createElement('button');
    element.type = 'button';
    element.textContent = label;
    if (className) element.className = className;
    element.addEventListener('click', onClick);
    return element;
  }

  // navigator.clipboard only exists on https and localhost; over plain http this is the path taken.
  function copyWithSelection(text) {
    const previousFocus = document.activeElement;
    const area = document.createElement('textarea');
    area.value = text;
    area.setAttribute('readonly', '');
    area.style.position = 'fixed';
    area.style.opacity = '0';
    document.body.append(area);
    area.focus();
    area.select();
    const copied = document.execCommand('copy');
    area.remove();
    if (previousFocus) previousFocus.focus();
    return copied;
  }

  async function copyText(text) {
    if (navigator.clipboard) {
      try {
        await navigator.clipboard.writeText(text);
        return true;
      } catch (error) {
        console.error(error);
      }
    }
    return copyWithSelection(text);
  }

  function copyButton(label, getText) {
    const element = button(label, async () => {
      const copied = await copyText(getText());
      element.textContent = copied ? 'Copied' : 'Copy failed';
      setTimeout(() => { element.textContent = label; }, COPIED_MS);
    });
    return element;
  }

  function openInNewTab(url) {
    window.open(url, '_blank', 'noopener');
  }

  async function errorMessage(response) {
    try {
      return (await response.json()).error;
    } catch (error) {
      console.error(error);
      return `Error ${response.status}`;
    }
  }

  function kindButtons(entry) {
    const fileUrl = `/f/${entry.id}`;
    const qr = button('QR', () => openInNewTab(`/qr/${entry.id}`));
    if (entry.kind === 'text') return [copyButton('Copy', () => entry.text), qr];
    if (entry.kind === 'link') {
      return [
        copyButton('Copy', () => entry.text),
        button('Open', () => openInNewTab(entry.text)),
        qr,
        copyButton('Short link', () => `${window.location.origin}/u/${entry.id}`),
      ];
    }
    return [
      button('Download', () => window.location.assign(`${fileUrl}?download=1`)),
      button('Open', () => openInNewTab(fileUrl)),
      qr,
    ];
  }

  function showFailure(container, entry, message) {
    const note = document.createElement('span');
    note.className = 'action-error';
    note.textContent = message;
    showMain(container, entry);
    container.append(note);
  }

  async function runRequest(container, entry, request, onSuccess) {
    try {
      const response = await request();
      if (response.ok) {
        await onSuccess(response);
      } else {
        showFailure(container, entry, await errorMessage(response));
      }
    } catch (error) {
      console.error(error);
      showFailure(container, entry, 'Could not reach the server');
    }
  }

  function setExpiry(container, entry, value) {
    return runRequest(
      container,
      entry,
      () => fetch(`/api/entries/${entry.id}/expiry`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ expiry: value }),
      }),
      async (response) => {
        window.Feed.upsert([await response.json()]);
        window.Feed.refresh();
      },
    );
  }

  function deleteEntry(container, entry) {
    return runRequest(
      container,
      entry,
      () => fetch(`/api/entries/${entry.id}`, { method: 'DELETE' }),
      () => {
        window.Feed.remove([entry.id]);
        window.Feed.refresh();
      },
    );
  }

  function showExpireMenu(container, entry) {
    container.replaceChildren(
      ...EXPIRY_CHOICES.map(([value, label]) => button(label, () => setExpiry(container, entry, value))),
      button('Cancel', () => showMain(container, entry)),
    );
  }

  function showDeleteConfirm(container, entry) {
    const question = document.createElement('span');
    question.className = 'action-question';
    question.textContent = 'Delete?';
    container.replaceChildren(
      question,
      button('Yes', () => deleteEntry(container, entry), 'del'),
      button('No', () => showMain(container, entry)),
    );
  }

  function showMain(container, entry) {
    container.replaceChildren(
      ...kindButtons(entry),
      button('Expire', () => showExpireMenu(container, entry)),
      button('Delete', () => showDeleteConfirm(container, entry), 'del'),
    );
  }

  window.FeedActions = { fill: showMain };
})();
