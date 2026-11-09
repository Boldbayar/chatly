'use strict';

const invoke = (cmd, args) => window.__TAURI_INTERNALS__.invoke(cmd, args || {});
const $ = (sel) => document.querySelector(sel);
const $$ = (sel) => Array.from(document.querySelectorAll(sel));

let current = null;
let lockoutTimer = null;
let triedTouchIdAtLaunch = false;
const unread = {};

window.chatly = {
  update: render,
  badge(service, count) {
    unread[service] = count;
    paintBadges();
  },
};

function render(s) {
  if (!s) return;
  const prev = current;
  current = s;
  const phase = s.phase;
  const entered = !prev || prev.phase !== phase;

  document.body.dataset.phase = phase;
  document.body.classList.toggle('overlay', s.overlay);
  $('#setup').hidden = phase !== 'setup';
  $('#lock').hidden = phase !== 'locked';
  $('#rail').hidden = phase !== 'unlocked';
  $('#settings').hidden = !s.overlay;

  if (prev && prev.phase === 'unlocked' && phase !== 'unlocked') closeSubforms();

  if (phase === 'setup' && entered) {
    resetSetup();
    focusSoon('#setup-pw');
  }

  if (phase === 'locked') {
    if (entered) {
      resetLock();
      focusSoon('#unlock-pw');
    }
    $('#touchid-btn').hidden = !(s.touchIdEnabled && s.touchIdAvailable);
    startLockout(s.lockoutSecs);
    if (!prev && !triedTouchIdAtLaunch && s.touchIdEnabled && s.touchIdAvailable) {
      triedTouchIdAtLaunch = true;
      touchIdUnlock();
    }
  }

  if (phase === 'unlocked') {
    for (const btn of $$('.svc')) {
      const on = btn.dataset.service === s.active;
      btn.classList.toggle('active', on);
      btn.setAttribute('aria-current', on ? 'page' : 'false');
    }
    if (s.overlay && !(prev && prev.overlay)) openSettings();
    if (s.overlay) fillSettings(s);
  }
}

function paintBadges() {
  for (const btn of $$('.svc')) {
    const n = unread[btn.dataset.service] || 0;
    const badge = btn.querySelector('.badge');
    badge.hidden = n === 0;
    badge.textContent = n > 99 ? '99+' : String(n);
    btn.setAttribute('aria-label', n ? `${btn.dataset.name}, ${n} unread` : btn.dataset.name);
  }
}

function focusSoon(sel) {
  requestAnimationFrame(() => {
    const el = $(sel);
    if (el && !el.disabled) el.focus();
  });
}

function setText(sel, text) {
  $(sel).textContent = text || '';
}

function busy(btn, label) {
  btn.dataset.label = btn.dataset.label || btn.textContent;
  btn.disabled = true;
  if (label) btn.textContent = label;
}

function idle(btn) {
  btn.disabled = false;
  if (btn.dataset.label) btn.textContent = btn.dataset.label;
  delete btn.dataset.label;
}

function shake(el) {
  el.classList.remove('shake');
  void el.offsetWidth;
  el.classList.add('shake');
}

function message(err) {
  return String(err && err.message ? err.message : err);
}

async function refresh() {
  const s = await invoke('status');
  render(s);
  return s;
}

for (const btn of $$('.reveal')) {
  btn.addEventListener('click', () => {
    const input = document.getElementById(btn.dataset.target);
    const show = input.type === 'password';
    input.type = show ? 'text' : 'password';
    btn.textContent = show ? 'Hide' : 'Show';
    input.focus();
  });
}

function strength(pw) {
  const len = [...pw].length;
  if (!len) return 0;
  if (/^(.)\1*$/.test(pw)) return 1;
  const pools = [/[a-z]/, /[A-Z]/, /[0-9]/, /[^A-Za-z0-9]/].filter((r) => r.test(pw)).length;
  const alphabet = [26, 52, 62, 94][pools - 1] || 26;
  let bits = len * Math.log2(alphabet);
  const words = pw.trim().split(/[\s\-_.]+/).filter(Boolean).length;
  if (words >= 4 && len >= 18) bits = Math.max(bits, 64);
  if (len < 8) return 1;
  return bits < 45 ? 2 : bits < 64 ? 3 : 4;
}

const STRENGTH_HINTS = [
  'At least 8 characters. Four random words work well.',
  'Too easy to guess.',
  'Okay. Longer is better.',
  'Good.',
  'Strong.',
];

function paintMeter() {
  const score = strength($('#setup-pw').value);
  $('#meter').dataset.score = String(score);
  setText('#meter-hint', STRENGTH_HINTS[score]);
}

function resetSetup() {
  $('#setup-pw').value = '';
  $('#setup-pw2').value = '';
  paintMeter();
}

$('#setup-pw').addEventListener('input', paintMeter);

$('#setup-form').addEventListener('submit', async (e) => {
  e.preventDefault();
  const pw = $('#setup-pw').value;
  const again = $('#setup-pw2').value;
  setText('#setup-error', '');
  if ([...pw].length < 8) return setText('#setup-error', 'Use at least 8 characters.');
  if (pw !== again) {
    $('#setup-pw2').select();
    return setText('#setup-error', "The two passwords don't match.");
  }
  const btn = $('#setup-btn');
  busy(btn, 'Encrypting…');
  try {
    const s = await invoke('setup', { password: pw });
    resetSetup();
    render(s);
  } catch (err) {
    setText('#setup-error', message(err));
  } finally {
    idle(btn);
  }
});

function resetLock() {
  $('#unlock-pw').value = '';
  $('#unlock-pw').type = 'password';
  $('.reveal[data-target="unlock-pw"]').textContent = 'Show';
  setText('#unlock-error', '');
  $('#forgot-panel').hidden = true;
  $('#forgot-btn').setAttribute('aria-expanded', 'false');
  $('#forgot-confirm').value = '';
  $('#forgot-erase').disabled = true;
}

function startLockout(secs) {
  clearInterval(lockoutTimer);
  const btn = $('#unlock-btn');
  if (!secs) {
    if (!btn.dataset.label) {
      btn.disabled = false;
      btn.textContent = 'Unlock';
    }
    return;
  }
  const until = Date.now() + secs * 1000;
  const tick = () => {
    const left = Math.ceil((until - Date.now()) / 1000);
    if (left <= 0) {
      clearInterval(lockoutTimer);
      btn.disabled = false;
      btn.textContent = 'Unlock';
      return;
    }
    btn.disabled = true;
    const m = Math.floor(left / 60);
    const sec = String(left % 60).padStart(2, '0');
    btn.textContent = `Try again in ${m}:${sec}`;
  };
  tick();
  lockoutTimer = setInterval(tick, 1000);
}

$('#unlock-form').addEventListener('submit', async (e) => {
  e.preventDefault();
  const input = $('#unlock-pw');
  const btn = $('#unlock-btn');
  if (!input.value || btn.disabled) return;
  setText('#unlock-error', '');
  busy(btn, 'Unlocking…');
  try {
    const s = await invoke('unlock', { password: input.value });
    input.value = '';
    idle(btn);
    render(s);
  } catch (err) {
    idle(btn);
    input.select();
    shake($('#lock .gate-card'));
    const s = await refresh();
    setText(s.phase === 'setup' ? '#setup-error' : '#unlock-error', message(err));
  }
});

async function touchIdUnlock() {
  const btn = $('#touchid-btn');
  setText('#unlock-error', '');
  btn.disabled = true;
  try {
    render(await invoke('unlock_touch_id'));
  } catch (err) {
    const text = message(err);
    if (!/cancel/i.test(text)) setText('#unlock-error', text);
    await refresh();
    focusSoon('#unlock-pw');
  } finally {
    btn.disabled = false;
  }
}

$('#touchid-btn').addEventListener('click', touchIdUnlock);

$('#forgot-btn').addEventListener('click', () => {
  const panel = $('#forgot-panel');
  panel.hidden = !panel.hidden;
  $('#forgot-btn').setAttribute('aria-expanded', String(!panel.hidden));
  if (!panel.hidden) focusSoon('#forgot-confirm');
});

function wireEraseConfirm(inputSel, buttonSel) {
  const input = $(inputSel);
  const button = $(buttonSel);
  input.addEventListener('input', () => {
    button.disabled = input.value.trim() !== 'ERASE';
  });
  button.addEventListener('click', async () => {
    if (input.value.trim() !== 'ERASE') return;
    busy(button, 'Erasing…');
    try {
      render(await invoke('erase_everything'));
    } finally {
      input.value = '';
      idle(button);
      button.disabled = true;
    }
  });
}

wireEraseConfirm('#forgot-confirm', '#forgot-erase');

for (const btn of $$('.svc')) {
  btn.addEventListener('click', async () => {
    try {
      render(await invoke('select_service', { service: btn.dataset.service }));
    } catch (_) {
      await refresh();
    }
  });
}

$('#settings-btn').addEventListener('click', () => setOverlay(!(current && current.overlay)));
$('#settings-done').addEventListener('click', () => setOverlay(false));
$('#lock-btn').addEventListener('click', () => invoke('lock'));

async function setOverlay(open) {
  try {
    render(await invoke('set_overlay', { open }));
  } catch (_) {
    await refresh();
  }
}

document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape' && current && current.overlay) setOverlay(false);
});

function openSettings() {
  closeSubforms();
  $('#check-results').hidden = true;
  $('#check-results').replaceChildren();
  setText('#pw-status', 'Encrypts everything Chatly saves.');
  $('.settings-body').scrollTop = 0;
  focusSoon('#settings-done');
}

function closeSubforms() {
  for (const sel of ['#pw-current', '#pw-next', '#pw-next2', '#erase-confirm']) $(sel).value = '';
  setText('#pw-error', '');
  $('#pw-form').hidden = true;
  $('#pw-toggle').setAttribute('aria-expanded', 'false');
  $('#erase-panel').hidden = true;
  $('#erase-toggle').setAttribute('aria-expanded', 'false');
  $('#erase-go').disabled = true;
}

function fillSettings(s) {
  for (const el of $$('[data-setting]')) {
    const value = s.settings[el.dataset.setting];
    if (el.type === 'checkbox') el.checked = Boolean(value);
    else el.value = String(value);
  }
  const touch = $('#s-touchid');
  touch.checked = s.touchIdEnabled;
  touch.disabled = !s.touchIdAvailable && !s.touchIdEnabled;
  setText(
    '#touchid-note',
    s.touchIdAvailable
      ? "Fingerprint only. Your Mac's login password can't unlock Chatly."
      : "Touch ID isn't available on this Mac right now."
  );
}

async function saveSettings() {
  const settings = { ...current.settings };
  for (const el of $$('[data-setting]')) {
    settings[el.dataset.setting] = el.type === 'checkbox' ? el.checked : Number(el.value);
  }
  try {
    render(await invoke('update_settings', { settings }));
  } catch (_) {
    fillSettings(current);
  }
}

for (const el of $$('[data-setting]')) el.addEventListener('change', saveSettings);

$('#s-touchid').addEventListener('change', async (e) => {
  const want = e.target.checked;
  e.target.disabled = true;
  try {
    render(await invoke('set_touch_id', { enabled: want }));
    setText('#touchid-note', want ? 'On. Touch the sensor on the lock screen to unlock.' : 'Off.');
  } catch (err) {
    e.target.checked = !want;
    setText('#touchid-note', message(err));
  } finally {
    e.target.disabled = !current.touchIdAvailable && !current.touchIdEnabled;
  }
});

$('#pw-toggle').addEventListener('click', () => {
  const form = $('#pw-form');
  const open = form.hidden;
  closeSubforms();
  form.hidden = !open;
  $('#pw-toggle').setAttribute('aria-expanded', String(open));
  if (open) focusSoon('#pw-current');
});

$('#pw-cancel').addEventListener('click', closeSubforms);

$('#pw-form').addEventListener('submit', async (e) => {
  e.preventDefault();
  const currentPw = $('#pw-current').value;
  const next = $('#pw-next').value;
  setText('#pw-error', '');
  if (!currentPw) return setText('#pw-error', 'Enter your current password.');
  if ([...next].length < 8) return setText('#pw-error', 'Use at least 8 characters for the new password.');
  if (next !== $('#pw-next2').value) return setText('#pw-error', "The new passwords don't match.");
  const btn = $('#pw-save');
  busy(btn, 'Changing…');
  try {
    await invoke('change_password', { current: currentPw, next });
    closeSubforms();
    setText('#pw-status', 'Password changed.');
  } catch (err) {
    setText('#pw-error', message(err));
  } finally {
    idle(btn);
  }
});

for (const btn of $$('[data-signout]')) {
  let armed = null;
  btn.addEventListener('click', async () => {
    if (!armed) {
      btn.textContent = 'Click to confirm';
      armed = setTimeout(() => {
        armed = null;
        btn.textContent = 'Sign out';
      }, 4000);
      return;
    }
    clearTimeout(armed);
    armed = null;
    busy(btn, 'Signing out…');
    try {
      render(await invoke('sign_out', { service: btn.dataset.signout }));
      btn.dataset.label = 'Signed out';
    } catch (_) {
      btn.dataset.label = 'Sign out';
    } finally {
      idle(btn);
      setTimeout(() => { btn.textContent = 'Sign out'; }, 2500);
    }
  });
}

$('#check-btn').addEventListener('click', async () => {
  const btn = $('#check-btn');
  const list = $('#check-results');
  busy(btn, 'Checking…');
  try {
    const findings = await invoke('security_check');
    list.replaceChildren(
      ...findings.map((f) => {
        const li = document.createElement('li');
        li.className = f.level;
        const dot = document.createElement('span');
        dot.className = 'dot';
        const body = document.createElement('div');
        const title = document.createElement('strong');
        title.textContent = f.label;
        const detail = document.createElement('p');
        detail.textContent = f.detail;
        body.append(title, detail);
        li.append(dot, body);
        return li;
      })
    );
    list.hidden = false;
  } catch (err) {
    list.replaceChildren(Object.assign(document.createElement('li'), { textContent: message(err) }));
    list.hidden = false;
  } finally {
    idle(btn);
    btn.textContent = 'Run again';
  }
});

$('#erase-toggle').addEventListener('click', () => {
  const panel = $('#erase-panel');
  const open = panel.hidden;
  closeSubforms();
  panel.hidden = !open;
  $('#erase-toggle').setAttribute('aria-expanded', String(open));
  if (open) focusSoon('#erase-confirm');
});

wireEraseConfirm('#erase-confirm', '#erase-go');

refresh();
