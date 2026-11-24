// DiaCraft's loader: the one script of the web app that is not generated. `cargo xtask site --web` copies it beside the page as `start.js`, in place of the inline script
// Trunk puts in the page, and writes the `manifest.json` it reads.
//
// What it does, in order:
//   1. reads manifest.json: the program's glue (JavaScript), the parts the WebAssembly module was cut into (at most 8 MiB each, each named by the hash of its bytes), and the assets the
//      program fetches later (the picture of the splash, the cover, the list of licences). It hands the manifest's text to the program as `window.diacraftManifest`.
//   2. fetches the parts in parallel, counts the bytes that arrive (the page shows them), and joins the parts, in order, into one stream. The stream is the body of a `Response` of type
//      application/wasm, which goes to the glue's start, so the browser compiles the module while the bytes are still arriving.
//   3. holds every part to its length and its SHA-256 (the browser's own `crypto.subtle`, where the page has it: a page served over https, or from localhost). The checks run while the
//      next part comes and while the browser finishes compiling, and **the program is not started until every one has passed**: bytes that have failed a check are compiled, which does them no
//      harm, and never run. A part that is missing, is cut short, is too long, has the wrong digest or stops coming is told on the page in words, with a button to try again: it is never a
//      hang or a blank.
//
//   4. keeps the browser's offer to open the app in a window of its own: Chrome and Edge fire `beforeinstallprompt` once, soon after the page has loaded, usually before the
//      program has started. The program asks for the offer later (`window.diacraftInstallOffer`, taken by its install button), so the event has to be kept from here. Nothing is sent anywhere.
//
// Everything is a file of this folder, named by the manifest: nothing here reaches another site, and no address is written in this script but the manifest's.

// The install offer (4): the event, kept for the program; `appinstalled` lets it go. The program also listens, to wake its window.
window.addEventListener('beforeinstallprompt', (event) => {
  event.preventDefault();
  window.diacraftInstallOffer = event;
});
window.addEventListener('appinstalled', () => {
  window.diacraftInstallOffer = null;
});

const MAX_MANIFEST_BYTES = 64 * 1024;
const MAX_PARTS = 16;
const MAX_PART_BYTES = 8 * 1024 * 1024;
const MAX_MODULE_BYTES = 64 * 1024 * 1024;
// How long a download may say nothing before it is taken to have stopped.
const STALL_MS = 30000;
const SPLASH_KEY = 'splash';

const status = document.getElementById('diacraft_status');
const bar = document.getElementById('diacraft_progress');
const again = document.getElementById('diacraft_again');
if (again) again.addEventListener('click', () => location.reload());

// Set when the page has said what went wrong: the words of a later step of the load (the bytes that came after) do not take them away.
let failed = false;

/** What the page says while it loads. */
function say(text) {
  if (status && !failed) status.textContent = text;
}

/** The page says what went wrong, once, in words, and offers another try. */
function fail(text) {
  if (failed) return;
  // The reason is the browser's or the server's text: shown as text, and cut short.
  text = String(text).slice(0, 300);
  console.error('DiaCraft: ' + text);
  say('DiaCraft could not start: ' + text);
  failed = true;
  if (bar) bar.hidden = true;
  if (again) again.hidden = false;
}

const mib = (bytes) => (bytes / (1024 * 1024)).toFixed(1);
const isName = (text, folder, extension) => typeof text === 'string' && new RegExp('^' + folder + '[A-Za-z0-9._-]{1,128}\\.' + extension + '$').test(text) && !text.includes('..');
const isSize = (n, most) => Number.isInteger(n) && n > 0 && n <= most;
const isDigest = (text) => typeof text === 'string' && /^[0-9a-f]{64}$/.test(text);

/** A fetch that is given up when the connection says nothing for STALL_MS. `onBytes` is told how many bytes each piece has. Resolves to the pieces, in order. */
async function download(url, onPiece) {
  const stop = new AbortController();
  let timer;
  const arm = () => {
    clearTimeout(timer);
    timer = setTimeout(() => stop.abort(new Error('the connection stopped (nothing came for ' + STALL_MS / 1000 + ' s)')), STALL_MS);
  };
  try {
    arm();
    const response = await fetch(url, { signal: stop.signal });
    if (!response.ok) throw new Error('the server answered ' + response.status);
    if (!response.body) throw new Error('the browser gave no body');
    const reader = response.body.getReader();
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      arm();
      onPiece(value);
    }
  } catch (error) {
    // Whatever went wrong, the connection is let go.
    stop.abort();
    throw error;
  } finally {
    clearTimeout(timer);
  }
}

/** The manifest, as text and as an object; checked, so that nothing it says can make this script fetch more than it should or from anywhere else. */
async function readManifest() {
  let text = '';
  const decoder = new TextDecoder();
  try {
    let length = 0;
    await download('./manifest.json', (piece) => {
      // Counted before it is read: a piece that would take the text past the bound is not read.
      length += piece.length;
      if (length > MAX_MANIFEST_BYTES) throw new Error('it is longer than ' + MAX_MANIFEST_BYTES + ' bytes');
      text += decoder.decode(piece, { stream: true });
    });
  } catch (error) {
    throw new Error('manifest.json could not be fetched: ' + (error && error.message ? error.message : String(error)));
  }
  let manifest;
  try {
    manifest = JSON.parse(text);
  } catch (e) {
    throw new Error('manifest.json is not JSON (' + e.message + ')');
  }
  const module = manifest && manifest.module;
  if (!manifest || manifest.version !== 1 || !module || !Array.isArray(module.parts)) throw new Error('manifest.json is not one this page reads');
  if (!isName(manifest.glue, '', 'js')) throw new Error('manifest.json names a start script that is not a file of this folder');
  if (module.parts.length < 1 || module.parts.length > MAX_PARTS) throw new Error('manifest.json lists ' + module.parts.length + ' parts');
  let total = 0;
  for (const part of module.parts) {
    if (!part || !isName(part.file, 'parts/', 'wasm') || !isSize(part.bytes, MAX_PART_BYTES) || !isDigest(part.sha256)) throw new Error('manifest.json has a part that is not valid');
    total += part.bytes;
  }
  if (total > MAX_MODULE_BYTES || total !== module.bytes) throw new Error('the parts in manifest.json do not add up to the program\'s size');
  return { text, manifest, total };
}

/** The SHA-256 of `chunks` in hexadecimal, or null where the browser has no crypto.subtle (a page that is not in a secure context). */
async function digest(chunks) {
  if (!(globalThis.crypto && crypto.subtle)) return null;
  const buffer = await new Blob(chunks).arrayBuffer();
  const sum = new Uint8Array(await crypto.subtle.digest('SHA-256', buffer));
  return Array.from(sum, (b) => b.toString(16).padStart(2, '0')).join('');
}

/** One part, as it arrives: its pieces, whether it is whole, and the promise of its check. */
class Part {
  constructor(info, index) {
    this.info = info;
    this.index = index;
    this.pieces = [];
    this.got = 0;
    this.done = false;
    this.wake = null;
    this.checked = null;
  }

  /** Says the part has changed, to the stream that waits for it. */
  changed() {
    const wake = this.wake;
    this.wake = null;
    if (wake) wake();
  }

  name() {
    return 'part ' + (this.index + 1) + ' (' + this.info.file + ')';
  }
}

/** Fetches every part at once and joins them, in order, into one stream. Returns the stream; `failed` is told the first thing that goes wrong, and the stream is stopped with it. */
function joined(parts, onBytes, failed) {
  let control = null;
  let broken = false;
  const fail = (error) => {
    broken = true;
    failed(error);
    if (control) control.error(error);
    // A stream that waits for a part is let go of, to find that it is over.
    for (const part of parts) part.changed();
  };
  for (const part of parts) {
    const word = part.name();
    part.checked = (async () => {
      try {
        await download('./' + part.info.file, (piece) => {
          part.got += piece.length;
          if (part.got > part.info.bytes) throw new Error(word + ' is longer than the manifest says');
          part.pieces.push(piece);
          onBytes(piece.length);
          part.changed();
        });
        if (part.got !== part.info.bytes) throw new Error(word + ' is shorter than the manifest says (' + part.got + ' bytes, not ' + part.info.bytes + ')');
        part.done = true;
        part.changed();
        const sum = await digest(part.pieces);
        if (sum === null) console.warn('DiaCraft: this page has no crypto.subtle, so ' + word + ' was checked by its length only');
        else if (sum !== part.info.sha256) throw new Error(word + ' does not match its digest in the manifest (the download is damaged)');
      } catch (error) {
        const reason = error && error.message ? error.message : String(error);
        const wrapped = new Error(reason.startsWith('part ') ? reason : word + ' could not be fetched: ' + reason);
        fail(wrapped);
        throw wrapped;
      }
    })();
    // The failure is told through `fail`; this keeps the browser from reporting it a second time as unhandled.
    part.checked.catch(() => {});
  }
  let at = 0;
  let sent = 0;
  return new ReadableStream({
    start(controller) {
      control = controller;
    },
    async pull(controller) {
      for (;;) {
        if (broken) return;
        if (at >= parts.length) {
          // Every part has been handed over, and the compile may finish: the checks are awaited by whoever starts the program (`start`), and a check that fails has stopped the stream
          // already, if it has not ended.
          controller.close();
          return;
        }
        const part = parts[at];
        if (sent < part.pieces.length) {
          controller.enqueue(part.pieces[sent]);
          sent += 1;
          return;
        }
        if (part.done) {
          at += 1;
          sent = 0;
          continue;
        }
        await new Promise((resolve) => {
          part.wake = resolve;
        });
      }
    },
  });
}

/** The assets the program fetches after its first frame are asked for now, so that the splash picture is in the browser's cache when the program asks: it is the first thing the window shows. */
function prefetch(manifest) {
  const entry = manifest.assets && manifest.assets[SPLASH_KEY];
  if (!entry || !isName(entry.file, 'assets/', '(png|txt)')) return;
  fetch('./' + entry.file)
    .then((response) => response.arrayBuffer())
    .catch(() => {
      // The program asks for it again and says in words if it cannot be had.
    });
}

async function start() {
  say('Loading DiaCraft…');
  const { text, manifest, total } = await readManifest();
  // The program reads the files it fetches later from this text.
  window.diacraftManifest = text;
  prefetch(manifest);
  if (bar) {
    bar.max = total;
    bar.value = 0;
  }
  let received = 0;
  let drawing = false;
  const onBytes = (n) => {
    received += n;
    if (drawing) return;
    drawing = true;
    requestAnimationFrame(() => {
      drawing = false;
      if (bar) bar.value = received;
      say('Loading DiaCraft… ' + mib(received) + ' of ' + mib(total) + ' MB');
    });
  };
  let first = null;
  const parts = manifest.module.parts.map((info, index) => new Part(info, index));
  const stream = joined(parts, onBytes, (error) => {
    if (!first) first = error;
  });
  const response = new Response(stream, { status: 200, headers: { 'Content-Type': 'application/wasm' } });
  // The browser compiles while the bytes come, **and while the program's glue is fetched**: the two start together, and the glue gets the module compiled. (A module the browser
  // compiles from a stream we made has no address, so it cannot keep the compiled code for the next visit as it can for one file it fetched itself; starting the compile at once is
  // what keeps a visit that has every part in its cache from waiting for it after the glue.)
  const compiled = WebAssembly.compileStreaming(response);
  // Told through `first` and the `catch` below; this keeps a failure that the glue's own import outruns from being reported a second time as unhandled.
  compiled.catch(() => {});
  const glueUrl = new URL(manifest.glue, location.href).href;
  try {
    // The glue, the compiled module and every part's check: the program starts when all three are done, and not before the last check has passed.
    const [glue, module] = await Promise.all([import(glueUrl), compiled, Promise.all(parts.map((part) => part.checked))]);
    const wasm = await glue.default({ module_or_path: module });
    window.wasmBindings = glue;
    dispatchEvent(new CustomEvent('TrunkApplicationStarted', { detail: { wasm } }));
    // The page's own text goes when the program has drawn its first frame; until then it says the program is starting.
    if (bar) bar.removeAttribute('value');
    say('Starting DiaCraft…');
  } catch (error) {
    // A part that went wrong is the reason, whatever the browser says of the stream it broke.
    throw first || error;
  }
}

start().catch((error) => fail(error && error.message ? error.message : String(error)));
