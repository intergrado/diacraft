//! Tests of the oracle harness: the hash, the category, the names, the refusals, the score and what is left out of it, the salted control and a
//! calculator that echoes the saved values, the aggregation, and that no formula, no text and no name a person made up crosses. Test inputs
//! are built in code from the specs (`diacraft-testkit`); no file from anywhere is read.

mod aggregate;
mod control;
mod file;
mod fuzz;
mod privacy;
mod score;
mod sha;

use diacraft_calc::Native;
use diacraft_testkit::package::{PageSpec, VsdxBuilder, shape};
use diacraft_vsdx::Limits;

use super::control::{self as control_mod, Control};
use super::file::{OracleReport, report};

/// The control of a run with the evaluator of this project.
pub(super) fn native_control() -> Control {
    control_mod::run(&mut Native)
}

/// Reports on `bytes` as a run does: the evaluator, and its control.
pub(super) fn run(bytes: &[u8]) -> OracleReport {
    report(bytes, &Limits::default(), None, &mut Native, &native_control())
}

pub(super) fn reference_app_props() -> &'static str {
    "<Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\"><Application>Microsoft Visio</Application><AppVersion>16.0000</AppVersion></Properties>"
}

/// A package with one page holding `shapes`, written by the reference app as far as the census can tell.
pub(super) fn one_page(shapes: &str) -> Vec<u8> {
    VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(shapes)).app(reference_app_props()).build()
}

/// A package with one shape of `ID` 1 holding `cells`.
pub(super) fn one_shape(cells: &str) -> Vec<u8> {
    one_page(&shape(1, cells))
}
