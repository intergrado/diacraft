//! The browser shell: finds the canvas, starts eframe's web runner on it and, when that cannot be done, says so on
//! the page. The only code in the web build that touches the DOM, and it exists on `wasm32` only.
//!
//! WebAssembly aborts on a panic, so nothing here may panic: every browser call that can fail is matched, and a
//! failure is logged to the console and, where there is a page to show it on, shown there.

use js_sys::Error;
use wasm_bindgen::{JsCast as _, JsValue};
use web_sys::{Document, Element, HtmlCanvasElement};

use crate::page::{self, CANVAS_ID, LOADING_ID};

/// Starts the app on the page's canvas. Returns at once: the runner starts in the background, and when it has
/// started the loading text goes. Nothing it can meet is fatal to the page: a failure is shown on it.
pub fn start() {
    // `Err` only when a logger is installed already, and that one then takes the messages below.
    let _ = eframe::WebLogger::init(log::LevelFilter::Info);
    let Some(canvas) = canvas() else {
        log::error!("diacraft-web: the page has no <canvas id=\"{CANVAS_ID}\">");
        show(&page::missing_canvas());
        return;
    };
    // The splash picture is the first thing the window shows: it is fetched while the graphics start, and not when the first frame asks for it.
    crate::web_assets::begin(diacraft_ui_egui::Asset::Splash);
    wasm_bindgen_futures::spawn_local(run(canvas));
}

/// Starts the app on `canvas` and returns once the runner has started (the browser's events keep the app going from
/// there), or once it has failed to start and the page says why.
async fn run(canvas: HtmlCanvasElement) {
    let started = eframe::WebRunner::new()
        .start(
            canvas,
            eframe::WebOptions::default(),
            Box::new(|cc| {
                // The canvas's pixels are held to the page's size times the device pixel ratio before every frame.
                crate::web_canvas::install(&cc.egui_ctx);
                // Files dropped on the page are taken from now on, and the window is asked for a frame when one has been read.
                crate::web_files::install(&cc.egui_ctx);
                // The keys a browser would act on are taken from now on, and the window is told when the browser offers a window of its own or goes to full screen.
                let page_host = crate::web_page::WebPage::install(&cc.egui_ctx);
                let app = page::new_app_in_browser(
                    std::sync::Arc::new(crate::web_files::WebFiles),
                    std::sync::Arc::new(crate::web_assets::WebAssets),
                    std::sync::Arc::new(page_host),
                );
                // The person's settings are the page's `localStorage`, within a budget of 1 MiB.
                let storage = crate::prefs::StorageHost::new(crate::web_prefs::LocalStorage);
                Ok(Box::new(page::with_prefs_in_browser(app, std::sync::Arc::new(storage))))
            }),
        )
        .await;
    match started {
        Ok(()) => {
            if let Some(loading) = element(LOADING_ID) {
                loading.remove();
            }
        }
        Err(e) => {
            let reason = describe(&e);
            log::error!("diacraft-web: could not start: {reason}");
            show(&page::start_failed(&reason));
        }
    }
}

/// The page's canvas, if it has one.
pub(crate) fn canvas() -> Option<HtmlCanvasElement> {
    // `Err` holds the element back when the id belongs to something that is not a `<canvas>`. Dropping it is right:
    // there is then no canvas to draw on, and the caller says so on the page and in the console.
    element(CANVAS_ID)?.dyn_into::<HtmlCanvasElement>().ok()
}

fn document() -> Option<Document> {
    web_sys::window()?.document()
}

fn element(id: &str) -> Option<Element> {
    document()?.get_element_by_id(id)
}

/// What the browser said went wrong: the message of a JavaScript error, else the text it threw.
fn describe(value: &JsValue) -> String {
    match value.dyn_ref::<Error>() {
        Some(error) => String::from(error.message()),
        None => value.as_string().unwrap_or_else(|| format!("{value:?}")),
    }
}

/// Puts `lines` on the page, one paragraph each, where the loading text was. As text, never as markup: a line can
/// hold what the browser said.
fn show(lines: &[String]) {
    let Some(document) = document() else {
        log::error!("diacraft-web: no document to show a message on");
        return;
    };
    let Some(place) = element(LOADING_ID).or_else(|| new_place(&document)) else { return };
    place.set_text_content(None);
    for line in lines {
        let paragraph = match document.create_element("p") {
            Ok(paragraph) => paragraph,
            Err(e) => {
                log::error!("diacraft-web: could not make a paragraph: {}", describe(&e));
                continue;
            }
        };
        paragraph.set_text_content(Some(line));
        if let Err(e) = place.append_child(&paragraph) {
            log::error!("diacraft-web: could not show a message: {}", describe(&e));
        }
    }
}

/// A place for messages, in a page that has none: a `<div>` with the loading text's id, at the end of the body.
fn new_place(document: &Document) -> Option<Element> {
    let Some(body) = document.body() else {
        log::error!("diacraft-web: the page has no body to show a message in");
        return None;
    };
    let place = match document.create_element("div") {
        Ok(place) => place,
        Err(e) => {
            log::error!("diacraft-web: could not make a place for a message: {}", describe(&e));
            return None;
        }
    };
    place.set_id(LOADING_ID);
    if let Err(e) = body.append_child(&place) {
        log::error!("diacraft-web: could not add a place for a message: {}", describe(&e));
        return None;
    }
    Some(place)
}
