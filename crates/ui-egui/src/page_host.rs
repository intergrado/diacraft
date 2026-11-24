//! The window in a page of a browser: a window of its own, the keys the browser keeps, and full screen.
//!
//! A browser keeps some keys for itself and never passes them to the page: Ctrl+N, Ctrl+T, Ctrl+W (with Shift too) and Ctrl+Tab open, close and switch its tabs and windows. **In a tab, DiaCraft cannot
//! have them**: a press of Ctrl+N opens another browser window and the page is not told. There are two ways to get the keys, and the web build offers both:
//!
//! * **A window of its own.** A page that is installed as a web app (or opened with `--app`) is in a window with no tabs, and there the browser passes every key to the page first (Chromium's
//!   `IsReservedCommandOrKey` is `false` in "Apps mode"). The page takes the keys it uses by calling `preventDefault()` on them (`apps/diacraft-web/src/keys.rs`), and the window runs their commands.
//!   The File view's Home page offers it: a button where the browser gives an install prompt (Chrome, Edge), and the steps in words where it does not (Safari, Firefox).
//! * **Full screen with the keyboard locked.** In full screen a browser passes the keys it keeps as well, and the Keyboard Lock API asks for the rest. The View tab's Full Screen command enters it
//!   (it needs a click, which the command's press is) and says how to leave: hold Esc.
//!
//! * **Asking before the page is left**. A tab that is closed or reloaded loses a drawing that has unsaved changes with no word, unless the page asks the browser to ask: while the drawing has changes,
//!   the window tells the host ([`PageHost::guard_leaving`]), and the browser shows its own box, "Leave site?", when the person closes the tab, reloads the page or goes elsewhere. A saved drawing, a new one and an
//!   idle page ask nothing. The window's own Close (Ctrl+W) asks DiaCraft's question first and clears the guard before it closes the window, so the person answers once.
//!
//! The window sees all this as a [`PageHost`] (`DiacraftApp::with_page_host`), the page's side of it. **The rest of the window is the same code**: a command asks the host, and the host answers from
//! the browser. A window with no host (the desktop) has none of these commands (they say why they cannot run) and its keys are as they were. The window knows nothing of the browser: the host is a
//! trait, and the tests of this flow run on the desktop with a host of their own.

use std::sync::Arc;

use crate::app::DiacraftApp;

/// The browser a page is in, as far as what it can do for the window differs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Browser {
    /// Chrome, Edge and the other browsers made on Chromium: they give an install prompt (`beforeinstallprompt`) and open an installed app in a window of its own.
    Chromium,
    /// Safari: it has no install prompt, and File, Add to Dock makes the web app.
    Safari,
    /// Firefox: it has no windows for web apps on Linux or macOS.
    Firefox,
    /// Any other browser.
    Other,
}

/// The browser a user agent names. Chrome, Edge, Opera and the other browsers made on Chromium say `Chrome/` (and their own name besides); Safari says `Safari/` and no `Chrome/`; Firefox says
/// `Firefox/`. Every browser on an iPhone or iPad is Safari's engine, and has no install prompt and no keyboard lock, so they are [`Browser::Other`]: Add to Home Screen is their way.
pub fn browser_of(user_agent: &str) -> Browser {
    let has = |word: &str| user_agent.contains(word);
    if has("FxiOS") || has("CriOS") || has("EdgiOS") || has("OPiOS") || has("iPhone") || has("iPad") {
        Browser::Other
    } else if has("Firefox/") {
        Browser::Firefox
    } else if has("Chrome/") || has("Chromium/") {
        Browser::Chromium
    } else if has("Safari/") {
        Browser::Safari
    } else {
        Browser::Other
    }
}

/// What the window can offer to open it in a window of its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Offer {
    /// Nothing: the page is in a window of its own already.
    None,
    /// The browser gave an install prompt: a button asks it to open the app in a window of its own.
    Prompt,
    /// The browser gives none: the steps, in words.
    Steps(&'static str),
}

/// What the page's side of the window answers. Every call returns at once; what the browser answers later it sends by waking the window (`Context::request_repaint`), which the host keeps the context
/// for.
pub trait PageHost: Send + Sync {
    /// The browser the page is in.
    fn browser(&self) -> Browser;

    /// Whether the page is in a window of its own: an installed web app, or a window opened with `--app`. A tab is not, and nor is a tab in full screen.
    fn own_window(&self) -> bool;

    /// Whether the browser gave the page an install prompt that has not been used: the page can ask it to open the app in a window of its own.
    fn can_prompt(&self) -> bool;

    /// Asks the browser to open the app in a window of its own (its install prompt). It needs a click, which the press of the command is.
    fn prompt(&self);

    /// `None` while the page is not in full screen. In full screen, whether the keyboard is locked: `Some(true)` when the browser gave the page the keys it keeps for itself, `Some(false)` when it did not
    /// (a browser with no keyboard lock, or one that refused).
    fn full_screen(&self) -> Option<bool>;

    /// Goes to full screen and locks the keyboard. It needs a click, which the press of the command is. A refusal comes back through [`PageHost::full_screen`] staying `None`, and
    /// [`PageHost::refusal`].
    fn enter_full_screen(&self);

    /// Leaves full screen, and lets the keyboard go.
    fn leave_full_screen(&self);

    /// What the browser said when a request for full screen or for the install prompt was refused, in words, once. `None` when there is nothing new.
    fn refusal(&self) -> Option<String> {
        None
    }

    /// Asks the browser to ask the person before the page is closed, reloaded or left (`on`), or stops asking (`!on`). The window says `on` while the drawing has unsaved changes and `off` otherwise, and
    /// only when its answer changes: the host puts the browser's `beforeunload` handler on the page while it is `on` and takes it off when it is not, so that a page with nothing to lose asks nothing and
    /// stays eligible for the browser's back/forward cache. The browser's box has words of its own: a page cannot give it any. The default does nothing (a host that is not a page).
    ///
    /// [`PageHost::close`] comes after the window has said `off`, and a host that closes the window holds the handler off until it knows whether the browser kept the window.
    fn guard_leaving(&self, _on: bool) {}

    /// Closes the window or the tab, as far as the page may: `false` when the browser keeps it (a script may close only a window that a script opened, or a page with no history before it).
    fn close(&self) -> bool;
}

/// What the window keeps of its host.
#[derive(Default, Clone)]
pub(crate) struct Page {
    host: Option<Arc<dyn PageHost>>,
    /// `app.closeWindow` ran: the frame asks about unsaved changes and closes (a command has no context to ask with).
    pub(crate) close_asked: bool,
    /// What the last frame saw of full screen (`PageHost::full_screen`), to say it when it changes: `None` before the first frame.
    was_full: Option<Option<bool>>,
    /// What the host was last told by [`PageHost::guard_leaving`]: a page starts with nothing to lose, so `false`.
    guarding: bool,
}

impl Page {
    pub(crate) fn host(&self) -> Option<&Arc<dyn PageHost>> {
        self.host.as_ref()
    }
}

/// What a browser's install steps say, where it gives no prompt.
pub const SAFARI_STEPS: &str = "In Safari, choose File, then Add to Dock. DiaCraft then opens in a window of its own from the Dock.";
/// Firefox has none on Linux or macOS.
pub const FIREFOX_STEPS: &str = "Firefox has no windows of their own for web apps on Linux or macOS. Chrome and Edge do: open this page in one of them and use Open in its own window.";
/// A Chromium browser that gave no prompt: the install is where the browser keeps it.
pub const CHROMIUM_STEPS: &str = "Use the install button at the end of the address bar, or the browser's menu, to open DiaCraft in a window of its own. If it is installed already, open it from there.";
/// Any other browser.
pub const OTHER_STEPS: &str = "This browser may not open web apps in windows of their own. In Chrome or Edge, DiaCraft can open in one.";

/// What the window offers in `browser`, given whether the page is in a window of its own and whether the browser gave an install prompt.
pub fn offer_for(browser: Browser, own_window: bool, can_prompt: bool) -> Offer {
    if own_window {
        Offer::None
    } else if can_prompt {
        Offer::Prompt
    } else {
        Offer::Steps(match browser {
            Browser::Chromium => CHROMIUM_STEPS,
            Browser::Safari => SAFARI_STEPS,
            Browser::Firefox => FIREFOX_STEPS,
            Browser::Other => OTHER_STEPS,
        })
    }
}

/// How to leave full screen, as the status bar says it on entering.
pub const LEAVE_WORDS: &str = "Full screen. Hold Esc to leave it.";
/// Said when the browser let the page into full screen and gave it no keyboard lock.
pub const UNLOCKED_WORDS: &str = "Full screen, but this browser kept its keys (Ctrl+N, Ctrl+W and the like). Hold Esc to leave it.";

impl DiacraftApp {
    /// Gives the window the page's side of its place in the browser ([`PageHost`]). With one, the window runs the commands that open it in a window of its own, put it in full screen and close it, and
    /// takes Ctrl+W (Cmd+W on a Mac) to close it as the browser would; without one (the desktop) these say why they cannot run.
    pub fn with_page_host(mut self, host: Arc<dyn PageHost>) -> DiacraftApp {
        self.page.host = Some(host);
        self
    }

    /// Whether the window is in a page of a browser (it has a [`PageHost`]).
    pub fn in_browser(&self) -> bool {
        self.page.host().is_some()
    }

    /// What the window offers for a window of its own: [`Offer::None`] in a window of its own and on the desktop.
    pub fn own_window_offer(&self) -> Offer {
        match self.page.host() {
            Some(host) => offer_for(host.browser(), host.own_window(), host.can_prompt()),
            None => Offer::None,
        }
    }

    /// Whether the keys a browser keeps for itself reach the window now: it is in a window of its own, or in full screen with the keyboard locked. In a tab they do not, and a window with no host is not in
    /// a browser (the keys are the desktop's).
    pub fn browser_keys_reach_window(&self) -> bool {
        match self.page.host() {
            Some(host) => host.own_window() || host.full_screen() == Some(true),
            None => true,
        }
    }

    /// Whether the page is in full screen.
    pub fn is_full_screen(&self) -> bool {
        self.page.host().is_some_and(|host| host.full_screen().is_some())
    }

    /// Why `app.ownWindow` cannot run, or `Ok`: only where the browser gave an install prompt.
    pub(crate) fn own_window_enabled(&self) -> Result<(), String> {
        if self.page.host().is_none() {
            return Err("only the browser version opens in a window of its own".to_owned());
        }
        match self.own_window_offer() {
            Offer::Prompt => Ok(()),
            Offer::None => Err("DiaCraft is in a window of its own already".to_owned()),
            Offer::Steps(steps) => Err(steps.to_owned()),
        }
    }

    /// Why `window.fullScreen` cannot run, or `Ok`: only in the browser.
    pub(crate) fn full_screen_enabled(&self) -> Result<(), String> {
        if self.page.host().is_none() {
            Err("only the browser version has this: the window of the desktop has its own full screen".to_owned())
        } else {
            Ok(())
        }
    }

    /// Why `app.closeWindow` cannot run, or `Ok`: only in the browser.
    pub(crate) fn close_window_enabled(&self) -> Result<(), String> {
        if self.page.host().is_none() {
            Err("only the browser version has this: the window of the desktop has its own close button".to_owned())
        } else {
            Ok(())
        }
    }

    /// `app.ownWindow`: asks the browser to open the app in a window of its own.
    pub(crate) fn run_own_window(&mut self) {
        // The browser's own prompt is the answer; a refusal comes back as words through `PageHost::refusal`.
        if let Some(host) = self.page.host() {
            host.prompt();
        }
    }

    /// `window.fullScreen {on?}`: goes to full screen with the keyboard locked, or leaves it; without `on` it turns the other way from how the page is.
    pub(crate) fn run_full_screen(&mut self, on: Option<bool>) {
        let Some(host) = self.page.host().cloned() else { return };
        let on = on.unwrap_or(host.full_screen().is_none());
        if on {
            if host.full_screen().is_none() {
                host.enter_full_screen();
            }
        } else {
            host.leave_full_screen();
        }
    }

    /// Tells the host whether the drawing has unsaved changes, when that has changed since it was last told ([`PageHost::guard_leaving`]): the browser asks before the page is left while it has. Runs at the
    /// start of every frame and again at the end of the window's drawing, so that an edit, a save or a new drawing is known to the browser in the frame that made it, and not one later.
    pub(crate) fn sync_leave_guard(&mut self) {
        let Some(host) = self.page.host().cloned() else { return };
        let dirty = self.is_dirty();
        if dirty != self.page.guarding {
            self.page.guarding = dirty;
            host.guard_leaving(dirty);
        }
    }

    /// Words the page's host has for the person, said in the status bar, and a change of full screen said once. Runs every frame before the window is drawn.
    pub(crate) fn advance_page(&mut self, ctx: &egui::Context) {
        let Some(host) = self.page.host().cloned() else { return };
        self.sync_leave_guard();
        if let Some(words) = host.refusal() {
            self.set_notice(&words);
        }
        // The state of full screen and of the lock: said when it changes, so that a lock that comes a moment after the full screen (the browser answers it later) replaces the first words.
        let now = host.full_screen();
        let before = self.page.was_full.unwrap_or(None);
        if before != now {
            if let Some(locked) = now {
                self.set_notice(if locked { LEAVE_WORDS } else { UNLOCKED_WORDS });
            }
            ctx.request_repaint();
        }
        self.page.was_full = Some(now);
        if std::mem::take(&mut self.page.close_asked) {
            self.ask_then(ctx, crate::save::Then::Close);
        }
    }

    /// The window is closing for good (the person said so, or had nothing to save): in a browser, ask the page to close it, and say so when the browser keeps the window.
    pub(crate) fn close_in_browser(&mut self) -> bool {
        let Some(host) = self.page.host().cloned() else { return false };
        host.leave_full_screen();
        // The person has answered (Save, Don't Save, or had nothing to save): the browser must not ask a second question when the window closes.
        if self.page.guarding {
            self.page.guarding = false;
            host.guard_leaving(false);
        }
        if !host.close() {
            self.saving.may_close = false;
            self.set_notice("The browser keeps this window open: close it with the browser's own button or key.");
        }
        true
    }

    /// A tooltip for a control of the command `id`: `label`, and the command's key as this window has it (`Ctrl+N`, or `⌘N` on a Mac) when the key reaches the window; where the browser keeps the key
    /// (in a tab), what keeps it, and not the key: a tooltip never shows a key that opens a browser tab. The desktop window's tooltips are unchanged.
    pub fn tip_with_key(&self, ctx: &egui::Context, label: &str, enabled: bool, id: &str) -> String {
        let base = crate::widgets::tip(label, enabled);
        if !self.in_browser() {
            return base;
        }
        let Some(shortcut) = crate::shortcuts::shortcut_of(ctx.os(), id, true) else { return base };
        let key = ctx.format_shortcut(&shortcut);
        if !crate::shortcuts::kept_by_browser(ctx.os(), &shortcut) || self.browser_keys_reach_window() {
            return format!("{base} ({key})");
        }
        format!("{base} ({key} belongs to the browser in a tab; open DiaCraft in its own window, or in full screen, to use it)")
    }

    /// The question about unsaved changes before the window closes, then the close. Asked by `app.closeWindow`, on the frame after it ran.
    pub(crate) fn ask_to_close(&mut self) {
        self.page.close_asked = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_browser_has_its_own_steps_and_a_window_of_its_own_needs_none() {
        assert_eq!(offer_for(Browser::Chromium, false, true), Offer::Prompt);
        assert_eq!(offer_for(Browser::Chromium, true, true), Offer::None);
        assert_eq!(offer_for(Browser::Safari, false, false), Offer::Steps(SAFARI_STEPS));
        assert_eq!(offer_for(Browser::Firefox, false, false), Offer::Steps(FIREFOX_STEPS));
        assert_eq!(offer_for(Browser::Chromium, false, false), Offer::Steps(CHROMIUM_STEPS));
        assert_eq!(offer_for(Browser::Other, false, false), Offer::Steps(OTHER_STEPS));
        for browser in [Browser::Chromium, Browser::Safari, Browser::Firefox, Browser::Other] {
            assert_eq!(offer_for(browser, true, false), Offer::None, "{browser:?}: in a window of its own there is nothing to offer");
        }
    }

    #[test]
    fn the_words_name_what_they_are_about() {
        assert!(SAFARI_STEPS.contains("File") && SAFARI_STEPS.contains("Add to Dock"));
        assert!(FIREFOX_STEPS.contains("Linux") && FIREFOX_STEPS.contains("macOS"));
        assert!(LEAVE_WORDS.contains("Hold Esc"));
        assert!(UNLOCKED_WORDS.contains("Hold Esc"));
    }
}
