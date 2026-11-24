//! The system's printing, through `craft-print`: the printers the spooler knows and the jobs sent to it.
//!
//! The window crate depends on no spooler (so it builds for the web, where there is none): it hands a [`PrintService`] the PDF of a print and the
//! job's options, and this is the desktop's service. **On macOS and Linux** it is CUPS (`lpstat` and `lp`); **on Windows** it is the spooler in this
//! process, with no program started. The PDF is a page for each sheet at the paper's size, so a job is sent as it is, with the sheet size kept (the
//! printer's own margins may clip what lies in them, as they do for any exact-size job); copies, collation and two-sided printing travel as the
//! job's options for the spooler to do, and are not made in the PDF. Black and white is made in the PDF (every colour as its grey, as the preview
//! shows it) and is also asked of the spooler as monochrome, so that a printer that reads the option uses its black alone. The job says
//! its pages are already grey (`grey_pages`), so a Windows driver with no grey option still prints it and does not refuse it.
//!
//! Nothing here runs unless the person presses Print: listing the printers reads the spooler and sends nothing. A job is cancelled through the
//! spooler's own token ([`craft_print::Cancel`]) until it starts reaching the spooler.

use diacraft_ui_egui::{Duplex, PrintCancel, PrintJob, PrintService, PrinterInfo};

/// The desktop's [`PrintService`].
pub struct SystemPrint;

impl PrintService for SystemPrint {
    fn printers(&self) -> Vec<PrinterInfo> {
        // The queues only, without asking any driver anything: a Print view lists names, and on Windows a networked printer in power save can
        // hold the drivers' own answers for a long time.
        craft_print::printer_queues().into_iter().map(|p| PrinterInfo { name: p.name, label: p.description, default: p.default }).collect()
    }

    fn submit(&self, pdf: &[u8], job: &PrintJob, cancel: &PrintCancel) -> Result<String, String> {
        let token = craft_print::Cancel::new();
        let held = token.clone();
        cancel.on_cancel(move || {
            // `false` means the job is already being sent, which completes as usual: there is nothing to tell.
            let _ = held.cancel();
        });
        craft_print::submit_cancellable(pdf, &spooler_job(job), &mut |_, _| {}, &token).map_err(|e| e.to_string())
    }
}

/// The job as the spooler is given it: the sheets keep their size, and what the person chose goes as options.
pub fn spooler_job(job: &PrintJob) -> craft_print::Job {
    craft_print::Job {
        printer: job.printer.clone(),
        copies: job.copies,
        collate: job.collate,
        duplex: match job.duplex {
            Duplex::PrinterDefault => None,
            Duplex::OneSided => Some(craft_print::Duplex::Off),
            Duplex::LongEdge => Some(craft_print::Duplex::LongEdge),
            Duplex::ShortEdge => Some(craft_print::Duplex::ShortEdge),
        },
        scaling: craft_print::Scaling::Exact,
        grayscale: job.grayscale,
        // The PDF of a black and white print is already grey, so a Windows driver that has no grey option, or does not keep it, still
        // prints the job and is not refused for it.
        grey_pages: job.grayscale,
        title: job.title.clone(),
        options: Vec::new(),
        // The driver's own settings dialog (Windows) is not offered, so none are chosen.
        driver_settings: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job() -> PrintJob {
        PrintJob {
            printer: Some("Office".to_owned()),
            title: "plan.vsdx".to_owned(),
            copies: 3,
            collate: true,
            duplex: Duplex::LongEdge,
            grayscale: true,
        }
    }

    #[test]
    fn the_choices_become_the_spoolers_job_and_the_sheets_keep_their_size() {
        let spooler = spooler_job(&job());
        assert_eq!(spooler.printer.as_deref(), Some("Office"));
        assert_eq!((spooler.copies, spooler.collate, spooler.grayscale), (3, true, true));
        assert_eq!(spooler.duplex, Some(craft_print::Duplex::LongEdge));
        assert_eq!(spooler.scaling, craft_print::Scaling::Exact);
        assert_eq!(spooler.title, "plan.vsdx");
        assert!(spooler.grey_pages, "a black and white print is a PDF made grey, which a driver with no grey option still prints");
        assert_eq!(spooler.driver_settings, None, "no driver dialog is opened, so no driver's settings travel");
        let colour = spooler_job(&PrintJob { grayscale: false, ..job() });
        assert!(!colour.grey_pages && !colour.grayscale, "a colour print is neither");
        let default = spooler_job(&PrintJob { duplex: Duplex::PrinterDefault, ..job() });
        assert_eq!(default.duplex, None, "the printer's own setting stands");
        assert_eq!(spooler_job(&PrintJob { duplex: Duplex::OneSided, ..job() }).duplex, Some(craft_print::Duplex::Off));
        assert_eq!(spooler_job(&PrintJob { duplex: Duplex::ShortEdge, ..job() }).duplex, Some(craft_print::Duplex::ShortEdge));
    }

    /// Where a real printer would be reached, said without reaching it: `command_preview` describes the job and starts nothing.
    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn a_job_is_the_lp_command_for_the_printer_with_the_options_chosen() {
        let words = craft_print::command_preview(&spooler_job(&job())).expect("a spooler on this system");
        assert_eq!(
            words,
            [
                "lp",
                "-d",
                "Office",
                "-n",
                "3",
                "-t",
                "plan.vsdx",
                "-o",
                "collate=true",
                "-o",
                "sides=two-sided-long-edge",
                "-o",
                "print-color-mode=monochrome",
                "-o",
                "fit-to-page=false"
            ]
        );
    }

    /// A job cancelled before it starts is refused by the spooler's token and **nothing is started**: no `lp`, no file.
    #[cfg(unix)]
    #[test]
    fn a_job_cancelled_before_it_is_sent_starts_no_process() {
        let cancel = PrintCancel::new();
        cancel.cancel();
        let error = SystemPrint.submit(b"%PDF-1.7", &job(), &cancel).unwrap_err();
        assert!(error.contains("cancelled before anything was sent"), "{error}");
    }
}
