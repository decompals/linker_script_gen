/* SPDX-FileCopyrightText: © 2024 decompals */
/* SPDX-License-Identifier: MIT */

use std::path::Path;

use slinky::{Document, LinkerWriter};

fn main() {
    let document = Document::read(Path::new("test_case.yaml"));

    let mut writer = LinkerWriter::new(&document.options);
    writer.begin_sections();
    for segment in &document.segments {
        writer.add_segment(segment);
    }
    writer.end_sections();

    writer.save_linker_script(Path::new("test_case.ld")).expect("Error writing the linker script");
}
