(function () {
  'use strict';

  const LOAD_MARGIN_PX = 200;
  const PINNED_MARGIN_PX = 40;
  const HIGHLIGHT_MS = 2000;
  const POLL_INTERVAL_MS = 2000;
  const MARKER_REFRESH_MS = 60000;
  const FAILURES_BEFORE_OFFLINE = 3;
  const SECONDS_PER_MINUTE = 60;
  const SECONDS_PER_HOUR = 3600;
  const SECONDS_PER_DAY = 86400;
  const WEEKDAYS = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];
  const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];

  const feed = document.getElementById('feed');
  const countLabel = document.getElementById('count');
  const offlineLabel = document.getElementById('offline');

  const view = { hasBefore: false, hasAfter: false, loading: false, pinned: true };
  const poll = { running: false, failures: 0 };

  function rows() {
    return Array.from(feed.querySelectorAll('.entry'));
  }

  function pad(number) {
    return String(number).padStart(2, '0');
  }

  function timeLabel(date) {
    return `${pad(date.getHours())}:${pad(date.getMinutes())}`;
  }

  function dayKey(date) {
    return `${date.getFullYear()}-${date.getMonth()}-${date.getDate()}`;
  }

  function dayLabel(date, now) {
    const yesterday = new Date(now.getFullYear(), now.getMonth(), now.getDate() - 1);
    if (dayKey(date) === dayKey(now)) return 'Today';
    if (dayKey(date) === dayKey(yesterday)) return 'Yesterday';
    const label = `${WEEKDAYS[date.getDay()]} ${date.getDate()} ${MONTHS[date.getMonth()]}`;
    return date.getFullYear() === now.getFullYear() ? label : `${label} ${date.getFullYear()}`;
  }

  function createdDate(row) {
    return new Date(Number(row.dataset.created) * 1000);
  }

  function refreshDayDividers() {
    feed.querySelectorAll('.day').forEach((divider) => divider.remove());
    const now = new Date();
    rows().reduce((previousKey, row) => {
      const date = createdDate(row);
      const key = dayKey(date);
      if (key !== previousKey) {
        const divider = document.createElement('div');
        divider.className = 'day';
        divider.textContent = dayLabel(date, now);
        feed.insertBefore(divider, row);
      }
      return key;
    }, null);
  }

  function expiryLabel(expires, nowSeconds) {
    const left = expires - nowSeconds;
    if (left < SECONDS_PER_MINUTE) return 'expires in <1m';
    if (left < SECONDS_PER_HOUR) return `expires in ${Math.floor(left / SECONDS_PER_MINUTE)}m`;
    if (left < SECONDS_PER_DAY) return `expires in ${Math.floor(left / SECONDS_PER_HOUR)}h`;
    return `expires in ${Math.floor(left / SECONDS_PER_DAY)}d`;
  }

  function buildMeta(entry) {
    const meta = document.createElement('div');
    meta.className = 'meta';
    const time = document.createElement('time');
    time.textContent = timeLabel(new Date(entry.created * 1000));
    const kind = document.createElement('span');
    kind.className = 'kind';
    kind.textContent = entry.kind;
    meta.append(time, kind);
    if (entry.expires !== null) {
      const marker = document.createElement('span');
      marker.className = 'expiry-marker';
      marker.dataset.expires = entry.expires;
      marker.textContent = expiryLabel(entry.expires, Date.now() / 1000);
      meta.append(marker);
    }
    return meta;
  }

  function buildRow(entry) {
    const row = document.createElement('article');
    row.className = 'entry';
    row.id = `e-${entry.id}`;
    row.tabIndex = 0;
    row.dataset.id = entry.id;
    row.dataset.kind = entry.kind;
    row.dataset.created = entry.created;
    row.dataset.expires = entry.expires === null ? '' : entry.expires;
    const body = document.createElement('div');
    body.className = 'body';
    // body_html is produced and sanitized by the server (src/feed/render.rs).
    body.innerHTML = entry.body_html;
    const actions = document.createElement('div');
    actions.className = 'actions';
    window.FeedActions.fill(actions, entry);
    row.append(buildMeta(entry), body, actions);
    if (window.hljs) body.querySelectorAll('pre code').forEach((code) => window.hljs.highlightElement(code));
    return row;
  }

  // An entry outside the loaded window stays out; paging brings it in at the right place.
  function isOutsideWindow(id, loadedIds) {
    if (loadedIds.length === 0) return false;
    const first = loadedIds[0];
    const last = loadedIds[loadedIds.length - 1];
    return (id > last && view.hasAfter) || (id < first && view.hasBefore);
  }

  // Only the expiry of an entry can change. Leaving unchanged rows alone keeps images from
  // reloading and open menus from closing when the change log reports an entry again.
  function placeRow(entry) {
    const existing = document.getElementById(`e-${entry.id}`);
    if (existing) {
      if (existing.dataset.expires === String(entry.expires ?? '')) return;
      const replacement = buildRow(entry);
      replacement.classList.toggle('open', existing.classList.contains('open'));
      existing.replaceWith(replacement);
      return;
    }
    const next = rows().find((row) => Number(row.dataset.id) > entry.id);
    feed.insertBefore(buildRow(entry), next || null);
  }

  function placeLiveRow(entry) {
    const loadedIds = rows().map((row) => Number(row.dataset.id));
    if (loadedIds.includes(entry.id) || !isOutsideWindow(entry.id, loadedIds)) placeRow(entry);
  }

  function scrollToBottom() {
    feed.scrollTop = feed.scrollHeight;
  }

  function upsert(entries) {
    const wasPinned = view.pinned;
    entries.forEach(placeLiveRow);
    refreshDayDividers();
    if (wasPinned && !view.hasAfter) scrollToBottom();
  }

  function remove(ids) {
    ids.forEach((id) => {
      const row = document.getElementById(`e-${id}`);
      if (row) row.remove();
    });
    refreshDayDividers();
  }

  function setCount(count) {
    countLabel.textContent = count;
  }

  async function fetchPage(query) {
    const response = await fetch(`/api/entries${query}`);
    if (!response.ok) throw new Error(`could not load entries: ${response.status}`);
    return response.json();
  }

  // Holds the given row at the same place on screen while rows are added above it.
  function keepingPlaceOf(row, change) {
    const before = row ? row.getBoundingClientRect().top : 0;
    change();
    if (row) feed.scrollTop += row.getBoundingClientRect().top - before;
  }

  async function loadOlder() {
    const first = rows()[0];
    const page = await fetchPage(`?before=${first.dataset.id}`);
    view.hasBefore = page.has_before;
    keepingPlaceOf(first, () => {
      page.entries.forEach(placeRow);
      refreshDayDividers();
    });
  }

  async function loadNewer() {
    const all = rows();
    const page = await fetchPage(`?after=${all[all.length - 1].dataset.id}`);
    view.hasAfter = page.has_after;
    page.entries.forEach(placeRow);
    refreshDayDividers();
  }

  function nearTop() {
    return feed.scrollTop < LOAD_MARGIN_PX;
  }

  function nearBottom(margin) {
    return feed.scrollHeight - feed.scrollTop - feed.clientHeight < margin;
  }

  async function loadWhatIsNeeded() {
    if (view.loading || rows().length === 0) return;
    const wantOlder = view.hasBefore && nearTop();
    const wantNewer = view.hasAfter && nearBottom(LOAD_MARGIN_PX);
    if (!wantOlder && !wantNewer) return;
    view.loading = true;
    try {
      await (wantOlder ? loadOlder() : loadNewer());
    } catch (error) {
      // The next scroll tries again.
      console.error(error);
      return;
    } finally {
      view.loading = false;
    }
    loadWhatIsNeeded();
  }

  function showLoadError() {
    const message = document.createElement('div');
    message.className = 'load-error';
    message.textContent = 'Could not load the feed. ';
    const retry = document.createElement('a');
    retry.href = window.location.href;
    retry.textContent = 'Try again';
    message.append(retry);
    feed.replaceChildren(message);
  }

  function highlight(row) {
    row.classList.add('highlight');
    setTimeout(() => row.classList.remove('highlight'), HIGHLIGHT_MS);
  }

  function showInitialPosition(around) {
    const target = around && document.getElementById(`e-${around}`);
    if (target) {
      view.pinned = false;
      target.scrollIntoView({ block: 'center' });
      highlight(target);
    } else {
      scrollToBottom();
    }
  }

  async function loadInitial() {
    const around = document.body.dataset.around;
    const page = await fetchPage(around ? `?around=${encodeURIComponent(around)}` : '');
    view.hasBefore = page.has_before;
    view.hasAfter = page.has_after;
    Feed.lastSeq = page.seq;
    setCount(page.count);
    page.entries.forEach(placeRow);
    refreshDayDividers();
    showInitialPosition(around);
    loadWhatIsNeeded();
  }

  function noteFailedPoll(error) {
    console.error(error);
    poll.failures += 1;
    if (poll.failures >= FAILURES_BEFORE_OFFLINE) offlineLabel.hidden = false;
  }

  function noteGoodPoll() {
    poll.failures = 0;
    offlineLabel.hidden = true;
  }

  async function pollNow() {
    if (poll.running || document.visibilityState !== 'visible') return;
    poll.running = true;
    try {
      const response = await fetch(`/api/changes?since=${Feed.lastSeq}`);
      if (!response.ok) throw new Error(`could not load changes: ${response.status}`);
      const changes = await response.json();
      Feed.lastSeq = changes.seq;
      upsert(changes.upserted);
      remove(changes.removed);
      setCount(changes.count);
      noteGoodPoll();
    } catch (error) {
      noteFailedPoll(error);
    } finally {
      poll.running = false;
    }
  }

  function refreshExpiryMarkers() {
    const nowSeconds = Date.now() / 1000;
    feed.querySelectorAll('.expiry-marker').forEach((marker) => {
      marker.textContent = expiryLabel(Number(marker.dataset.expires), nowSeconds);
    });
  }

  function toggleRowOnTap(event) {
    if (!window.matchMedia('(pointer: coarse)').matches) return;
    if (event.target.closest('a, button, video')) return;
    const row = event.target.closest('.entry');
    if (!row) return;
    const opening = !row.classList.contains('open');
    feed.querySelectorAll('.entry.open').forEach((other) => other.classList.remove('open'));
    row.classList.toggle('open', opening);
  }

  function startLiveUpdates() {
    setInterval(pollNow, POLL_INTERVAL_MS);
    setInterval(() => {
      refreshExpiryMarkers();
      refreshDayDividers();
    }, MARKER_REFRESH_MS);
    document.addEventListener('visibilitychange', pollNow);
    window.addEventListener('online', pollNow);
  }

  feed.addEventListener('scroll', () => {
    view.pinned = !view.hasAfter && nearBottom(PINNED_MARGIN_PX);
    loadWhatIsNeeded();
  }, { passive: true });

  feed.addEventListener('click', toggleRowOnTap);

  // Images load after the first scroll; stay at the newest entry while they grow the page.
  feed.addEventListener('load', () => {
    if (view.pinned) scrollToBottom();
  }, true);

  const Feed = {
    lastSeq: Number(document.body.dataset.seq),
    upsert,
    remove,
    setCount,
    scrollToBottom,
    holdsNewest: () => !view.hasAfter,
    refresh: pollNow,
  };
  Feed.ready = loadInitial().then(startLiveUpdates).catch((error) => {
    console.error(error);
    showLoadError();
  });
  window.Feed = Feed;
})();
