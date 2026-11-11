//! Bundled font bytes shared by the sheet renderer and PDF exporter.
//! See web/font-license.txt for the DejaVu/Bitstream license.
use crate::spreadsheet::CellFont;

pub const MONO: &[u8] = include_bytes!("../web/fonts/DejaVuSansMono.ttf");
pub const SANS: &[u8] = include_bytes!("../web/fonts/DejaVuSans.ttf");
pub const SERIF: &[u8] = include_bytes!("../web/fonts/DejaVuSerif.ttf");

pub fn bytes(family: CellFont) -> &'static [u8] {
    match family {
        CellFont::Mono => MONO,
        CellFont::Sans => SANS,
        CellFont::Serif => SERIF,
    }
}
