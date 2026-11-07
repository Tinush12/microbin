(function () {
  'use strict';

  const DEBOUNCE_MS = 250;

  const toggle = document.getElementById('search-toggle');
  const field = document.getElementById('search-input');
  const results = document.getElementById('results');

  let debounceTimer = null;
  let latestRequest = 0;

  function showNote(text) {
    const note = document.createElement('div');
    note.className = 'results-note';
    note.textContent = text;
    results.replaceChildren(note);
    results.hidden = false;
  }

  function showResults(entries) {
    if (entries.length === 0) {
      showNote('No matches');
      return;
    }
    results.replaceChildren(...entries.map(window.Feed.buildResultRow));
    results.scrollTop = 0;
    results.hidden = false;
  }

  async function runSearch(query, requestNumber) {
    try {
      const response = await fetch(`/api/search?q=${encodeURIComponent(query)}`);
      if (!response.ok) throw new Error(`search failed: ${response.status}`);
      const { entries } = await response.json();
      // An older request answering late must not replace what the newest one showed.
      if (requestNumber === latestRequest) showResults(entries);
    } catch (error) {
      console.error(error);
      if (requestNumber === latestRequest) showNote('Search failed. Try again.');
    }
  }

  function cancelPending() {
    clearTimeout(debounceTimer);
    latestRequest += 1;
  }

  function onQueryChanged() {
    cancelPending();
    const query = field.value.trim();
    if (query === '') {
      results.hidden = true;
      results.replaceChildren();
      return;
    }
    const requestNumber = latestRequest;
    debounceTimer = setTimeout(() => runSearch(query, requestNumber), DEBOUNCE_MS);
  }

  function open() {
    field.hidden = false;
    field.focus();
  }

  function close() {
    cancelPending();
    field.value = '';
    field.hidden = true;
    results.hidden = true;
    results.replaceChildren();
  }

  function jumpToResult(event) {
    if (event.target.closest('a, button, video')) return;
    const row = event.target.closest('.entry');
    if (row) window.location.assign(`/?around=${row.dataset.id}`);
  }

  toggle.addEventListener('click', () => (field.hidden ? open() : close()));
  field.addEventListener('input', onQueryChanged);
  field.addEventListener('keydown', (event) => {
    if (event.key === 'Escape') close();
  });
  results.addEventListener('click', jumpToResult);
})();
