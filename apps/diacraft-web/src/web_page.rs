//! The browser's side of the window's place in its page: taking the keys a browser would act on, a window of its own, full screen with the keyboard locked, and closing. It exists on
//! `wasm32` only, and is one of the places besides `web.rs` and `web_files.rs` that touches the DOM. What it can decide without a browser is in `diacraft_ui_egui` (`page_host.rs`,
//! `shortcuts.rs`), where the tests run on the desktop.
//!
//! **The keys.** A browser keeps some keys for itself in a tab (Ctrl+N, Ctrl+T, Ctrl+W and Ctrl+Tab among them) and never passes them to the page. In a window of its own (an installed web app, or one
//! opened with `--app`) and in full screen with the keyboard locked it passes every key to the page first, and **acts on the key itself unless the page called `preventDefault()`**. The same holds in a
//! tab for the keys the browser does pass (Ctrl+R reloads the page, Ctrl+H opens its history, Ctrl+U its source), and the eframe web runner calls `preventDefault()` for Ctrl+O, Ctrl+P, Ctrl+S, Tab, Backspace,
//! Space and the arrow keys only. So a handler on the window, in the capture phase (it runs before eframe's, which is on the canvas), calls it for each chord of the command key and each function key
//! that a command of the window has, except the clipboard's own ([`diacraft_ui_egui::shortcuts::keys_to_take`]), and stops nothing: the key reaches eframe, and the window runs the command.
//!
//! **Asking before the page is left**. A tab that is closed or reloaded loses a drawing that has unsaved changes with no word, and the web runner's own handler for it never runs (it listens for
//! `onbeforeunload`, an event name no browser fires; the event is `beforeunload`, and that handler only offers the app a chance to save to storage, which this build has none of). While the window says
//! the drawing has unsaved changes ([`PageHost::guard_leaving`]), a handler of ours for `beforeunload` is on the page, and it cancels the event and sets `returnValue`, which makes the browser show its own
//! "Leave site?" box (a page cannot give the box any words). It is put on and taken off as the window says, so an idle page, a saved drawing and a new one ask nothing and keep the browser's back/forward
//! cache; and the window's own close takes it off before `window.close()` and keeps it off until the page knows whether the browser kept the window ([`crate::page::LeaveGuard`] decides, and this file does).
//!
//! **Nothing leaves the page.** The install prompt is the browser's own (`beforeinstallprompt`, kept by the loader because the browser sends it before the program has started), and nothing here makes a
//! request. WebAssembly aborts on a panic, so nothing here may panic: every browser call that can fail is matched and its failure is words for the person or a line in the console.

use std::cell::RefCell;

use diacraft_ui_egui::shortcuts::KeyTaker;
use diacraft_ui_egui::{Browser, PageHost, browser_of};
use js_sys::{Object, Reflect};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast as _, JsValue};
use web_sys::{BeforeUnloadEvent, Event, KeyboardEvent};

use crate::page::{GuardStep, LeaveGuard};
use crate::web_files::{call, call_async, describe};

/// The event a browser fires at a page that is closed, reloaded or left, and lets it cancel. The web runner's handler is registered as `onbeforeunload`, which no browser fires.
const LEAVE_EVENT: &str = "beforeunload";

/// How long after the page asks the browser to close it a page that is still here is taken to have been kept: the browser closes a window it allows at once.
const CLOSE_GRACE_MS: i32 = 700;

/// The window's own place in the page, as far as the page remembers it.
#[derive(Default)]
struct State {
    /// The window's context, to ask for a frame when the browser says something.
    ctx: Option<egui::Context>,
    /// The page was in a window of its own the last time it was not in full screen (in full screen the browser reports the display mode as fullscreen, whatever the window is).
    own_window: bool,
    /// The keyboard is locked: the browser granted the lock in the full screen the page is in.
    locked: bool,
    /// Full screen was asked for and the browser has not yet answered the lock: the page says it is not in full screen until it has, so that the window says what it got and not a guess.
    entering: bool,
    /// What the browser refused, in words, and the window has not yet said.
    refusal: Option<String>,
    /// Whether the handler for `beforeunload` should be on the page: the window's word on unsaved changes, and whether the window is being closed.
    leave: LeaveGuard,
    /// The handler for `beforeunload` while it is on the page.
    leave_handler: Option<Closure<dyn FnMut(Event)>>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Runs `f` on the page's state. `f` never calls anything that reaches the state again.
fn with<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    // A state that is in use (a call inside a call) is left alone and the work is dropped: the page has one thread, so this is a bug that must not abort.
    STATE.with(|state| state.try_borrow_mut().ok().map(|mut state| f(&mut state)))
}

/// Does what the guard decided to the handler for `beforeunload`.
fn settle_leave_guard(state: &mut State, step: GuardStep) {
    match step {
        GuardStep::Keep => {}
        GuardStep::Attach => state.leave_handler = attach_leave_handler(),
        GuardStep::Detach => {
            if let Some(handler) = state.leave_handler.take() {
                detach_leave_handler(&handler);
            }
        }
    }
}

/// Puts a handler for `beforeunload` on the window that makes the browser ask. `None` where the browser would not take it (logged).
fn attach_leave_handler() -> Option<Closure<dyn FnMut(Event)>> {
    let window = window().map(JsValue::from)?;
    let handler = Closure::<dyn FnMut(Event)>::new(|event: Event| {
        // The browsers show their box when the event is cancelled, and the ones before that standard when `returnValue` is set: both are done. The words in the box are the browser's.
        event.prevent_default();
        if let Some(event) = event.dyn_ref::<BeforeUnloadEvent>() {
            event.set_return_value("");
        }
    });
    match call(&window, "addEventListener", &[JsValue::from_str(LEAVE_EVENT), handler.as_ref().clone()]) {
        Ok(_) => Some(handler),
        Err(e) => {
            log::error!("diacraft-web: could not listen for {LEAVE_EVENT}: {}", describe(&e));
            None
        }
    }
}

/// Takes `handler` off the window. A failure leaves a handler that asks once too often, never one that loses a drawing: it is logged.
fn detach_leave_handler(handler: &Closure<dyn FnMut(Event)>) {
    let Some(window) = window().map(JsValue::from) else { return };
    if let Err(e) = call(&window, "removeEventListener", &[JsValue::from_str(LEAVE_EVENT), handler.as_ref().clone()]) {
        log::error!("diacraft-web: could not stop listening for {LEAVE_EVENT}: {}", describe(&e));
    }
}

/// Moves the guard on with `change` and does what it says to the handler.
fn leave_guard(change: impl FnOnce(&mut LeaveGuard) -> GuardStep) {
    let _ = with(|s| {
        let step = change(&mut s.leave);
        settle_leave_guard(s, step);
    });
}

fn wake() {
    if let Some(Some(ctx)) = with(|s| s.ctx.clone()) {
        ctx.request_repaint();
    }
}

fn refuse(words: String) {
    log::warn!("diacraft-web: {words}");
    let _ = with(|s| s.refusal = Some(words));
    wake();
}

fn window() -> Option<web_sys::Window> {
    web_sys::window()
}

fn document() -> Option<JsValue> {
    window()?.document().map(JsValue::from)
}

/// The property `name` of `target`, when it is there (not `undefined` or `null`).
fn property(target: &JsValue, name: &str) -> Option<JsValue> {
    Reflect::get(target, &JsValue::from_str(name)).ok().filter(|v| !v.is_undefined() && !v.is_null())
}

fn flag(target: &JsValue, name: &str) -> bool {
    property(target, name).and_then(|v| v.as_bool()).unwrap_or(false)
}

/// Whether the display mode is one of a window of its own: an installed web app, a window opened with `--app`, and (on an iPhone or iPad) a page added to the Home Screen.
fn display_is_own_window() -> bool {
    let Some(window) = window() else { return false };
    let target = JsValue::from(window.clone());
    let matches = |mode: &str| {
        call(&target, "matchMedia", &[JsValue::from_str(&format!("(display-mode: {mode})"))]).ok().is_some_and(|list| flag(&list, "matches"))
    };
    ["standalone", "minimal-ui", "window-controls-overlay"].into_iter().any(matches)
        || property(&JsValue::from(window.navigator()), "standalone").and_then(|v| v.as_bool()) == Some(true)
}

/// Whether the page is in full screen now.
fn in_full_screen() -> bool {
    document().is_some_and(|document| property(&document, "fullscreenElement").is_some())
}

/// Calls `handler` for each `event` that reaches `target`, in the capture phase when `capture`.
fn listen(target: &JsValue, event: &str, capture: bool, handler: impl FnMut(JsValue) + 'static) {
    let closure = Closure::<dyn FnMut(JsValue)>::new(handler);
    if let Err(e) = call(target, "addEventListener", &[JsValue::from_str(event), closure.as_ref().clone(), JsValue::from_bool(capture)]) {
        log::error!("diacraft-web: could not listen for {event}: {}", describe(&e));
    }
    // The handler stays as long as the page does.
    closure.forget();
}

/// The page's side of the window's place in the page. The browser answers go through the thread's state, so this holds only what the browser decides once.
pub struct WebPage {
    browser: Browser,
}

impl WebPage {
    /// Makes the page's side, and starts what it needs: the handler that takes keys, and the listeners that wake the window when the browser changes something.
    pub fn install(ctx: &egui::Context) -> WebPage {
        let user_agent = window().map(|w| w.navigator()).and_then(|n| n.user_agent().ok()).unwrap_or_default();
        let _ = with(|s| {
            s.ctx = Some(ctx.clone());
            s.own_window = display_is_own_window();
        });
        if let Some(window) = window().map(JsValue::from) {
            let taker = KeyTaker::for_user_agent(&user_agent);
            // The first listener of the window: it runs before eframe's, which is on the canvas. It never stops the event, so the key reaches the window.
            listen(&window, "keydown", true, move |event| {
                let Some(event) = event.dyn_ref::<KeyboardEvent>() else { return };
                if taker.takes(&event.key(), &event.code(), event.ctrl_key(), event.meta_key(), event.shift_key(), event.alt_key()) {
                    event.prevent_default();
                }
            });
            for name in ["beforeinstallprompt", "appinstalled"] {
                listen(&window, name, false, |_| wake());
            }
            for mode in ["standalone", "minimal-ui", "window-controls-overlay", "browser"] {
                if let Ok(list) = call(&window, "matchMedia", &[JsValue::from_str(&format!("(display-mode: {mode})"))]) {
                    listen(&list, "change", false, |_| wake());
                }
            }
        }
        if let Some(document) = document() {
            listen(&document, "fullscreenchange", false, |_| {
                // Leaving full screen (by the command, or by Esc held) lets the keyboard go with it.
                if !in_full_screen() {
                    let _ = with(|s| s.locked = false);
                    unlock_keyboard();
                }
                wake();
            });
        }
        WebPage { browser: browser_of(&user_agent) }
    }
}

fn navigator_keyboard() -> Option<JsValue> {
    let navigator = JsValue::from(window()?.navigator());
    property(&navigator, "keyboard")
}

fn unlock_keyboard() {
    if let Some(keyboard) = navigator_keyboard() {
        // `Err` only where the browser has no `unlock`; the lock ends with the full screen anyway.
        let _ = call(&keyboard, "unlock", &[]);
    }
}

/// Asks for the keys the browser keeps, in the full screen the page is in. The lock needs full screen that the page asked for; a refusal (no API, no permission) leaves the keyboard as it was and the
/// window says so.
async fn lock_keyboard() {
    let locked = match navigator_keyboard() {
        Some(keyboard) => match call_async(&keyboard, "lock", &[]).await {
            Ok(_) => true,
            Err(e) => {
                log::warn!("diacraft-web: the keyboard was not locked: {}", describe(&e));
                false
            }
        },
        None => false,
    };
    // A lock granted after the person has left full screen again is no lock.
    let locked = locked && in_full_screen();
    if !locked {
        unlock_keyboard();
    }
    let _ = with(|s| s.locked = locked);
    wake();
}

/// The install prompt the loader kept, taken: it can be used once.
fn take_install_offer() -> Option<JsValue> {
    let window = JsValue::from(window()?);
    let offer = property(&window, "diacraftInstallOffer")?;
    // `Err` only for a window that cannot be written to.
    let _ = Reflect::set(&window, &JsValue::from_str("diacraftInstallOffer"), &JsValue::NULL);
    Some(offer)
}

impl PageHost for WebPage {
    fn browser(&self) -> Browser {
        self.browser
    }

    fn own_window(&self) -> bool {
        if in_full_screen() {
            return with(|s| s.own_window).unwrap_or(false);
        }
        let own = display_is_own_window();
        let _ = with(|s| s.own_window = own);
        own
    }

    fn can_prompt(&self) -> bool {
        window().is_some_and(|w| property(&JsValue::from(w), "diacraftInstallOffer").is_some())
    }

    fn prompt(&self) {
        let Some(offer) = take_install_offer() else { return };
        wasm_bindgen_futures::spawn_local(async move {
            if let Err(e) = call_async(&offer, "prompt", &[]).await {
                refuse(format!("The browser did not open its install prompt: {}.", describe(&e)));
                return;
            }
            // What the person answered is the browser's business: nothing more to say on either answer.
            wake();
        });
    }

    fn full_screen(&self) -> Option<bool> {
        if !in_full_screen() || with(|s| s.entering).unwrap_or(false) {
            return None;
        }
        Some(with(|s| s.locked).unwrap_or(false))
    }

    fn enter_full_screen(&self) {
        let Some(element) = document().and_then(|d| property(&d, "documentElement")) else {
            refuse("The page has nothing to put in full screen.".to_owned());
            return;
        };
        // `keyboardLock` asks, where the browser takes the option, for the keys it keeps; `navigationUI` hides the controls where it has them. A browser that does not know an option ignores it.
        let options = Object::new();
        // `Err` only for an object that cannot be written to; this one was made here.
        let _ = Reflect::set(&options, &JsValue::from_str("navigationUI"), &JsValue::from_str("hide"));
        let _ = Reflect::set(&options, &JsValue::from_str("keyboardLock"), &JsValue::from_str("browser"));
        // Called now, inside the press that asked: a browser lets the page into full screen only from a click or a key.
        let asked = call(&element, "requestFullscreen", &[options.into()]);
        let _ = with(|s| s.entering = true);
        wasm_bindgen_futures::spawn_local(async move {
            match asked {
                Ok(answer) => {
                    if let Ok(promise) = answer.dyn_into::<js_sys::Promise>()
                        && let Err(e) = wasm_bindgen_futures::JsFuture::from(promise).await
                    {
                        refuse(format!("The browser did not go to full screen: {}.", describe(&e)));
                        let _ = with(|s| s.entering = false);
                        return;
                    }
                    lock_keyboard().await;
                }
                Err(e) => refuse(format!("The browser did not go to full screen: {}.", describe(&e))),
            }
            let _ = with(|s| s.entering = false);
            wake();
        });
    }

    fn leave_full_screen(&self) {
        if !in_full_screen() {
            return;
        }
        unlock_keyboard();
        if let Some(document) = document()
            && let Err(e) = call(&document, "exitFullscreen", &[])
        {
            log::warn!("diacraft-web: could not leave full screen: {}", describe(&e));
        }
        let _ = with(|s| s.locked = false);
    }

    fn refusal(&self) -> Option<String> {
        with(|s| s.refusal.take()).flatten()
    }

    fn guard_leaving(&self, on: bool) {
        leave_guard(|guard| guard.want(on));
    }

    fn close(&self) -> bool {
        let Some(window) = window() else { return false };
        // The window has answered its own question: the browser's handler for `beforeunload` is off before the close, and stays off while the close is under way (a window that a script closes is asked by
        // the browser, with this handler, before it goes).
        leave_guard(LeaveGuard::closing);
        if let Err(e) = window.close() {
            log::warn!("diacraft-web: could not ask the browser to close the window: {}", describe(&e));
            leave_guard(LeaveGuard::kept);
            return false;
        }
        // A window the browser closes is gone at once; a page still here after a moment was kept (a script may close only a window a script opened, or one with nothing before it in its history): the
        // handler comes back if the drawing has changes.
        let late = Closure::once_into_js(|| {
            leave_guard(LeaveGuard::kept);
            refuse("The browser keeps this window open: close it with the browser's own button or key.".to_owned());
        });
        window.set_timeout_with_callback_and_timeout_and_arguments_0(late.unchecked_ref(), CLOSE_GRACE_MS).is_ok()
    }
}
