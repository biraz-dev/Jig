//! Portable OpenDocument files and printable PDFs. All exchange happens in
//! memory; adapters decide whether to read or download a local file.
use crate::{model::State, spreadsheet::Workbook};

#[path = "ods.rs"]
mod ods;
#[path = "pdf_export.rs"]
mod pdf_export;

pub use ods::{read_ods, save_ods};
pub use pdf_export::export_pdf;

/// An ordinary ODS supplies a workbook. A Jig ODS also carries the original
/// task/checklist state. Callers can show their normal replacement confirmation
/// before adopting that complete state.
#[derive(Clone, Debug)]
pub struct DocumentImport {
    pub workbook: Workbook,
    pub embedded_state: Option<State>,
}
