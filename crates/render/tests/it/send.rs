//! What the window needs to know about threads: a `Renderer` moves to a worker, a `Document` is shared with it.

use std::sync::Arc;

use diacraft_doc::Document;
use diacraft_render::{Renderer, View};

use crate::support::*;

fn assert_send<T: Send>() {}
fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn a_renderer_is_send_and_a_document_is_shared() {
    assert_send::<Renderer>();
    assert_send_sync::<Document>();
    assert_send_sync::<View>();
}

#[test]
fn a_worker_thread_renders_a_shared_document() {
    let doc = Arc::new(one_page(vec![rect(1, 1.0, 1.0, 2.0, 2.0).cell("FillForegnd", "#FF0000")]));
    let view = View::fit_page(&doc, 1, PPI).expect("the page has a size");
    let worker_doc = Arc::clone(&doc);
    let mut renderer = Renderer::new();
    let handle = std::thread::spawn(move || renderer.render(&worker_doc, 1, &view));
    let rendered = handle.join().expect("the worker finished");
    assert_eq!((rendered.width, rendered.height), (400, 400));
    assert_eq!(rendered.rgba_premul.len(), 400 * 400 * 4);
    assert!(rendered.skipped.is_empty(), "{:?}", rendered.skipped);
}
