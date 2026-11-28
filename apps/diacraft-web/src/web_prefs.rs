//! The browser's side of the person's settings: `localStorage` as the [`Storage`] under the window's settings. It exists on `wasm32` only, and is, with `web.rs`,
//! `web_files.rs` and `web_assets.rs`, a place that touches the browser. `prefs.rs` holds what does not need one (the keys, the budget, the host over any storage).
//!
//! **Nothing here may panic** (WebAssembly aborts on a panic): every browser call that can fail is matched, and a failure is the reason in words. `localStorage` is not there in a page
//! the browser keeps it from (a private window of some browsers, a page with storage turned off) or full: reads then find nothing and writes fail, and the window says so once.
//! The storage is the page's own origin's and is never sent anywhere.

use web_sys::Storage as BrowserStorage;

use crate::prefs::Storage;
use crate::web_files::describe;

/// The most keys a listing walks: a page of another app on the same origin can have filled the storage with them.
const MAX_KEYS: u32 = 10_000;

/// The page's `localStorage`.
pub(crate) struct LocalStorage;

impl LocalStorage {
    fn storage() -> Result<BrowserStorage, String> {
        let window = web_sys::window().ok_or_else(|| "there is no window to keep settings in".to_owned())?;
        match window.local_storage() {
            Ok(Some(storage)) => Ok(storage),
            Ok(None) => Err("this browser keeps no local storage for the page".to_owned()),
            Err(e) => Err(format!("this browser does not let the page use its local storage ({})", describe(&e))),
        }
    }
}

impl Storage for LocalStorage {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        Self::storage()?.get_item(key).map_err(|e| format!("the browser's storage could not be read ({})", describe(&e)))
    }

    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        Self::storage()?.set_item(key, value).map_err(|e| format!("the browser's storage would not take the settings ({})", describe(&e)))
    }

    fn remove(&self, key: &str) -> Result<(), String> {
        Self::storage()?.remove_item(key).map_err(|e| format!("the browser's storage could not be changed ({})", describe(&e)))
    }

    fn keys(&self) -> Result<Vec<String>, String> {
        let storage = Self::storage()?;
        let count = storage.length().map_err(|e| format!("the browser's storage could not be listed ({})", describe(&e)))?;
        let mut keys = Vec::new();
        for index in 0..count.min(MAX_KEYS) {
            if let Ok(Some(key)) = storage.key(index) {
                keys.push(key);
            }
        }
        Ok(keys)
    }
}
