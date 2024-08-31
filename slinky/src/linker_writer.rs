/* SPDX-FileCopyrightText: © 2024 decompals */
/* SPDX-License-Identifier: MIT */

use std::borrow::Cow;
use std::io::Write;

use crate::{
    utils, version, AssertEntry, Document, EscapedPath, FileInfo, FileKind, KeepSections,
    RequiredSymbol, RuntimeSettings, ScriptExporter, ScriptGenerator, ScriptImporter, Segment,
    SlinkyError, SymbolAssignment, VramClass,
};

use crate::script_buffer::ScriptBuffer;

pub struct LinkerWriter<'a> {
    buffer: ScriptBuffer,

    // Used for dependency generation
    files_paths: indexmap::IndexSet<EscapedPath>,

    vram_classes: indexmap::IndexMap<String, VramClass>,

    single_segment: bool,
    reference_partial_objects: bool,

    /* Options to control stuff */
    emit_sections_kind_symbols: bool,
    emit_section_symbols: bool,

    d: &'a Document,
    rs: &'a RuntimeSettings,
}

impl<'a> LinkerWriter<'a> {
    pub fn new(d: &'a Document, rs: &'a RuntimeSettings) -> Self {
        let mut vram_classes = indexmap::IndexMap::with_capacity(d.vram_classes.len());
        for vram_class in &d.vram_classes {
            vram_classes.insert(vram_class.name.clone(), vram_class.clone());
        }

        let mut buffer = ScriptBuffer::new();

        if rs.emit_version_comment() {
            buffer.writeln(&format!(
                "/* Generated by slinky {}.{}.{} */",
                version::VERSION_MAJOR,
                version::VERSION_MINOR,
                version::VERSION_PATCH
            ));
            buffer.write_empty_line();
        }

        Self {
            buffer,

            files_paths: indexmap::IndexSet::new(),

            vram_classes,

            single_segment: false,
            reference_partial_objects: false,

            emit_sections_kind_symbols: true,
            emit_section_symbols: true,

            d,
            rs,
        }
    }

    pub fn new_reference_partial_objects(d: &'a Document, rs: &'a RuntimeSettings) -> Self {
        let mut s = Self::new(d, rs);

        s.reference_partial_objects = true;

        s
    }
}

impl ScriptImporter for LinkerWriter<'_> {
    fn add_all_segments(&mut self, segments: &[Segment]) -> Result<(), SlinkyError> {
        if self.d.settings.single_segment_mode {
            // TODO: change assert to proper error
            assert!(segments.len() == 1);

            self.add_single_segment(&segments[0])?;
        } else {
            self.begin_sections()?;
            for segment in segments {
                self.add_segment(segment)?;
            }
            self.end_sections()?;
        }

        Ok(())
    }

    fn add_entry(&mut self, entry: &str) -> Result<(), SlinkyError> {
        if !self.buffer.is_empty() {
            self.buffer.write_empty_line();
        }

        self.buffer.writeln(&format!("ENTRY({});", entry));

        Ok(())
    }

    fn add_all_symbol_assignments(
        &mut self,
        symbol_assignments: &[SymbolAssignment],
    ) -> Result<(), SlinkyError> {
        if symbol_assignments.is_empty() {
            return Ok(());
        }

        self.begin_symbol_assignments()?;
        for symbol_assignment in symbol_assignments {
            self.add_symbol_assignment(symbol_assignment)?;
        }
        self.end_symbol_assignments()?;

        Ok(())
    }

    fn add_all_required_symbols(
        &mut self,
        required_symbols: &[RequiredSymbol],
    ) -> Result<(), SlinkyError> {
        if required_symbols.is_empty() {
            return Ok(());
        }

        self.begin_required_symbols()?;
        for required_symbol in required_symbols {
            self.add_required_symbol(required_symbol)?;
        }
        self.end_required_symbols()?;

        Ok(())
    }

    fn add_all_asserts(&mut self, asserts: &[AssertEntry]) -> Result<(), SlinkyError> {
        if asserts.is_empty() {
            return Ok(());
        }

        self.begin_asserts()?;
        for assert_entry in asserts {
            self.add_assert(assert_entry)?;
        }
        self.end_asserts()?;

        Ok(())
    }
}

impl ScriptExporter for LinkerWriter<'_> {
    fn export_linker_script_to_file(&self, path: &EscapedPath) -> Result<(), SlinkyError> {
        let mut f = utils::create_file_and_parents(path.as_ref())?;

        self.export_linker_script(&mut f)
    }

    fn export_linker_script_to_string(&self) -> Result<String, SlinkyError> {
        let mut s = Vec::new();

        self.export_linker_script(&mut s)?;

        match String::from_utf8(s) {
            Err(e) => Err(SlinkyError::FailedStringConversion {
                description: e.to_string(),
            }),
            Ok(ret) => Ok(ret),
        }
    }

    fn save_other_files(&self) -> Result<(), SlinkyError> {
        if let Some(d_path) = &self.d.settings.d_path_escaped(self.rs)? {
            if let Some(target_path) = &self.d.settings.target_path_escaped(self.rs)? {
                self.export_dependencies_file_to_file(d_path, target_path)?;
            }
        }

        if let Some(symbols_header_path) = &self.d.settings.symbols_header_path_escaped(self.rs)? {
            self.export_symbol_header_to_file(symbols_header_path)?;
        }

        Ok(())
    }
}

impl ScriptGenerator for LinkerWriter<'_> {}

impl LinkerWriter<'_> {
    pub fn export_linker_script(&self, dst: &mut impl Write) -> Result<(), SlinkyError> {
        for line in self.buffer.get_buffer() {
            if let Err(e) = writeln!(dst, "{}", line) {
                return Err(SlinkyError::FailedWrite {
                    description: e.to_string(),
                    contents: line.into(),
                });
            }
        }

        Ok(())
    }
}

impl LinkerWriter<'_> {
    pub fn export_dependencies_file(
        &self,
        dst: &mut impl Write,
        target_path: &EscapedPath,
    ) -> Result<(), SlinkyError> {
        if self.rs.emit_version_comment() {
            if let Err(e) = write!(
                dst,
                "# Generated by slinky {}.{}.{}\n\n",
                version::VERSION_MAJOR,
                version::VERSION_MINOR,
                version::VERSION_PATCH
            ) {
                return Err(SlinkyError::FailedWrite {
                    description: e.to_string(),
                    contents: "Version comment".to_string(),
                });
            }
        }

        if let Err(e) = write!(dst, "{}:", target_path) {
            return Err(SlinkyError::FailedWrite {
                description: e.to_string(),
                contents: target_path.to_string(),
            });
        }

        for p in &self.files_paths {
            if let Err(e) = write!(dst, " \\\n    {}", p) {
                return Err(SlinkyError::FailedWrite {
                    description: e.to_string(),
                    contents: p.to_string(),
                });
            }
        }

        if let Err(e) = write!(dst, "\n\n") {
            return Err(SlinkyError::FailedWrite {
                description: e.to_string(),
                contents: "".to_string(),
            });
        }

        for p in &self.files_paths {
            if let Err(e) = writeln!(dst, "{}:", p) {
                return Err(SlinkyError::FailedWrite {
                    description: e.to_string(),
                    contents: p.to_string(),
                });
            }
        }

        Ok(())
    }

    pub fn export_dependencies_file_to_file(
        &self,
        path: &EscapedPath,
        target_path: &EscapedPath,
    ) -> Result<(), SlinkyError> {
        let mut f = utils::create_file_and_parents(path.as_ref())?;

        self.export_dependencies_file(&mut f, target_path)
    }

    pub fn export_dependencies_file_to_string(
        &self,
        target_path: &EscapedPath,
    ) -> Result<String, SlinkyError> {
        let mut s = Vec::new();

        self.export_dependencies_file(&mut s, target_path)?;

        match String::from_utf8(s) {
            Err(e) => Err(SlinkyError::FailedStringConversion {
                description: e.to_string(),
            }),
            Ok(ret) => Ok(ret),
        }
    }
}

impl LinkerWriter<'_> {
    pub fn export_symbol_header(&self, dst: &mut impl Write) -> Result<(), SlinkyError> {
        if self.rs.emit_version_comment() {
            if let Err(e) = write!(
                dst,
                "/* Generated by slinky {}.{}.{} */\n\n",
                version::VERSION_MAJOR,
                version::VERSION_MINOR,
                version::VERSION_PATCH
            ) {
                return Err(SlinkyError::FailedWrite {
                    description: e.to_string(),
                    contents: "Version comment".to_string(),
                });
            }
        }

        if let Err(e) = write!(
            dst,
            "#ifndef HEADER_SYMBOLS_H\n#define HEADER_SYMBOLS_H\n\n"
        ) {
            return Err(SlinkyError::FailedWrite {
                description: e.to_string(),
                contents: "".into(),
            });
        }

        let arr_suffix = if self.d.settings.symbols_header_as_array {
            "[]"
        } else {
            ""
        };

        for sym in self.get_linker_symbols() {
            if let Err(e) = writeln!(
                dst,
                "extern {} {}{};",
                self.d.settings.symbols_header_type, sym, arr_suffix
            ) {
                return Err(SlinkyError::FailedWrite {
                    description: e.to_string(),
                    contents: sym.into(),
                });
            }
        }

        if let Err(e) = write!(dst, "\n#endif\n") {
            return Err(SlinkyError::FailedWrite {
                description: e.to_string(),
                contents: "".into(),
            });
        }

        Ok(())
    }

    pub fn export_symbol_header_to_file(&self, path: &EscapedPath) -> Result<(), SlinkyError> {
        let mut f = utils::create_file_and_parents(path.as_ref())?;

        self.export_symbol_header(&mut f)
    }

    pub fn export_symbol_header_to_string(&self) -> Result<String, SlinkyError> {
        let mut s = Vec::new();

        self.export_symbol_header(&mut s)?;

        match String::from_utf8(s) {
            Err(e) => Err(SlinkyError::FailedStringConversion {
                description: e.to_string(),
            }),
            Ok(ret) => Ok(ret),
        }
    }
}

// Getters / Setters
impl LinkerWriter<'_> {
    #[must_use]
    pub fn get_linker_symbols(&self) -> &indexmap::IndexSet<String> {
        self.buffer.get_linker_symbols()
    }

    pub fn set_emit_sections_kind_symbols(&mut self, value: bool) {
        self.emit_sections_kind_symbols = value;
    }

    #[must_use]
    pub fn get_emit_sections_kind_symbols(&mut self) -> bool {
        self.emit_sections_kind_symbols
    }

    pub fn set_emit_section_symbols(&mut self, value: bool) {
        self.emit_section_symbols = value;
    }

    #[must_use]
    pub fn get_emit_section_symbols(&mut self) -> bool {
        self.emit_section_symbols
    }
}

// semi internal functions
impl LinkerWriter<'_> {
    pub(crate) fn begin_sections(&mut self) -> Result<(), SlinkyError> {
        self.buffer.writeln("SECTIONS");
        self.buffer.begin_block();

        self.buffer.writeln("__romPos = 0x0;");

        if let Some(hardcoded_gp_value) = self.d.settings.hardcoded_gp_value {
            self.buffer
                .writeln(&format!("_gp = 0x{:08X};", hardcoded_gp_value));
        }

        self.buffer.write_empty_line();

        Ok(())
    }

    pub(crate) fn end_sections(&mut self) -> Result<(), SlinkyError> {
        let style = &self.d.settings.linker_symbols_style;
        let mut need_ln = false;

        for (vram_class_name, vram_class) in &self.vram_classes {
            if !vram_class.emitted {
                continue;
            }

            self.buffer.write_linker_symbol(
                &style.vram_class_size(vram_class_name),
                &format!(
                    "{} - {}",
                    style.vram_class_end(vram_class_name),
                    style.vram_class_start(vram_class_name),
                ),
            );

            need_ln = true;
        }

        if !self.d.settings.sections_allowlist.is_empty() {
            if need_ln {
                self.buffer.write_empty_line();
            }

            for sect in &self.d.settings.sections_allowlist {
                self.buffer.write_single_entry_section(sect, "0");
            }

            need_ln = true;
        }

        if !self.d.settings.sections_allowlist_extra.is_empty() {
            if need_ln {
                self.buffer.write_empty_line();
            }

            for sect in &self.d.settings.sections_allowlist_extra {
                self.buffer.write_single_entry_section(sect, "0");
            }

            need_ln = true;
        }

        if self.d.settings.discard_wildcard_section || !self.d.settings.sections_denylist.is_empty()
        {
            if need_ln {
                self.buffer.write_empty_line();
            }

            self.buffer.writeln("/DISCARD/ :");
            self.buffer.begin_block();

            for sect in &self.d.settings.sections_denylist {
                self.buffer.writeln(&format!("*({});", sect));
            }

            if self.d.settings.discard_wildcard_section {
                self.buffer.writeln("*(*);")
            }

            self.buffer.end_block();
        }

        self.buffer.end_block();
        self.buffer.finish();

        Ok(())
    }

    pub(crate) fn add_segment(&mut self, segment: &Segment) -> Result<(), SlinkyError> {
        if !self.rs.should_emit_entry(
            &segment.exclude_if_any,
            &segment.exclude_if_all,
            &segment.include_if_any,
            &segment.include_if_all,
        ) {
            return Ok(());
        }

        assert!(!self.single_segment);

        let style = &self.d.settings.linker_symbols_style;

        // rom segment symbols
        let main_seg_rom_sym_start: String = style.segment_rom_start(&segment.name);
        let main_seg_rom_sym_end: String = style.segment_rom_end(&segment.name);
        let main_seg_rom_sym_size: String = style.segment_rom_size(&segment.name);

        // vram segment symbols
        let main_seg_sym_start: String = style.segment_vram_start(&segment.name);
        let main_seg_sym_end: String = style.segment_vram_end(&segment.name);
        let main_seg_sym_size: String = style.segment_vram_size(&segment.name);

        if let Some(vram_class_name) = &segment.vram_class {
            let vram_class = match self.vram_classes.get_mut(vram_class_name) {
                Some(vc) => vc,
                None => {
                    return Err(SlinkyError::MissingVramClassForSegment {
                        segment: Cow::from(segment.name.clone()),
                        vram_class: Cow::from(vram_class_name.clone()),
                    })
                }
            };

            if !vram_class.emitted {
                let vram_class_sym = style.vram_class_start(vram_class_name);

                if let Some(fixed_vram) = vram_class.fixed_vram {
                    self.buffer
                        .write_linker_symbol(&vram_class_sym, &format!("0x{:08X}", fixed_vram));
                } else if let Some(fixed_symbol) = &vram_class.fixed_symbol {
                    self.buffer
                        .write_linker_symbol(&vram_class_sym, fixed_symbol);
                } else {
                    self.buffer
                        .write_linker_symbol(&vram_class_sym, "0x00000000");
                    for other_class_name in &vram_class.follows_classes {
                        self.buffer.write_symbol_max_self(
                            &vram_class_sym,
                            &style.vram_class_end(other_class_name),
                        );
                    }
                }
                self.buffer
                    .write_linker_symbol(&style.vram_class_end(vram_class_name), "0x00000000");

                self.buffer.write_empty_line();

                vram_class.emitted = true;
            }
        }

        if let Some(segment_start_align) = segment.segment_start_align {
            self.buffer.align_symbol("__romPos", segment_start_align);
            self.buffer.align_symbol(".", segment_start_align);
        }

        self.buffer
            .write_linker_symbol(&main_seg_rom_sym_start, "__romPos");
        self.buffer
            .write_linker_symbol(&main_seg_sym_start, &format!("ADDR(.{})", segment.name));

        // Emit alloc segment
        self.write_segment(segment, &segment.alloc_sections, false)?;

        self.buffer.write_empty_line();

        // Emit noload segment
        self.write_segment(segment, &segment.noload_sections, true)?;

        self.buffer.write_empty_line();

        self.buffer
            .writeln(&format!("__romPos += SIZEOF(.{});", segment.name));

        if let Some(segment_end_align) = segment.segment_end_align {
            self.buffer.align_symbol("__romPos", segment_end_align);
            self.buffer.align_symbol(".", segment_end_align);
        }

        self.write_sym_end_size(
            &main_seg_sym_start,
            &main_seg_sym_end,
            &main_seg_sym_size,
            ".",
        );

        self.write_sym_end_size(
            &main_seg_rom_sym_start,
            &main_seg_rom_sym_end,
            &main_seg_rom_sym_size,
            "__romPos",
        );

        if let Some(vram_class_name) = &segment.vram_class {
            self.buffer.write_empty_line();

            let vram_class_sym_end = style.vram_class_end(vram_class_name);
            self.buffer
                .write_symbol_max_self(&vram_class_sym_end, &main_seg_sym_end);
        }

        self.buffer.write_empty_line();

        Ok(())
    }

    pub(crate) fn add_single_segment(&mut self, segment: &Segment) -> Result<(), SlinkyError> {
        // Make sure this function is called only once
        assert!(!self.single_segment);
        self.single_segment = true;

        self.buffer.writeln("SECTIONS");
        self.buffer.begin_block();

        if let Some(fixed_vram) = segment.fixed_vram {
            self.buffer.writeln(&format!(". = 0x{:08X};", fixed_vram));
            self.buffer.write_empty_line();
        }

        // Emit alloc segment
        self.write_single_segment(segment, &segment.alloc_sections, false)?;

        self.buffer.write_empty_line();

        // Emit noload segment
        self.write_single_segment(segment, &segment.noload_sections, true)?;

        self.buffer.write_empty_line();

        self.end_sections()?;

        Ok(())
    }

    pub(crate) fn begin_symbol_assignments(&mut self) -> Result<(), SlinkyError> {
        if !self.buffer.is_empty() {
            self.buffer.write_empty_line();
        }

        Ok(())
    }

    pub(crate) fn end_symbol_assignments(&mut self) -> Result<(), SlinkyError> {
        Ok(())
    }

    pub(crate) fn add_symbol_assignment(
        &mut self,
        symbol_assignment: &SymbolAssignment,
    ) -> Result<(), SlinkyError> {
        if !self.rs.should_emit_entry(
            &symbol_assignment.exclude_if_any,
            &symbol_assignment.exclude_if_all,
            &symbol_assignment.include_if_any,
            &symbol_assignment.include_if_all,
        ) {
            return Ok(());
        }

        self.buffer.write_symbol_assignment(
            &symbol_assignment.name,
            &symbol_assignment.value,
            symbol_assignment.provide,
            symbol_assignment.hidden,
        );

        Ok(())
    }

    pub(crate) fn begin_required_symbols(&mut self) -> Result<(), SlinkyError> {
        if !self.buffer.is_empty() {
            self.buffer.write_empty_line();
        }

        Ok(())
    }

    pub(crate) fn end_required_symbols(&mut self) -> Result<(), SlinkyError> {
        Ok(())
    }

    pub(crate) fn add_required_symbol(
        &mut self,
        required_symbol: &RequiredSymbol,
    ) -> Result<(), SlinkyError> {
        if !self.rs.should_emit_entry(
            &required_symbol.exclude_if_any,
            &required_symbol.exclude_if_all,
            &required_symbol.include_if_any,
            &required_symbol.include_if_all,
        ) {
            return Ok(());
        }

        self.buffer.write_required_symbol(&required_symbol.name);

        Ok(())
    }

    pub(crate) fn begin_asserts(&mut self) -> Result<(), SlinkyError> {
        if !self.buffer.is_empty() {
            self.buffer.write_empty_line();
        }

        Ok(())
    }

    pub(crate) fn end_asserts(&mut self) -> Result<(), SlinkyError> {
        Ok(())
    }

    pub(crate) fn add_assert(&mut self, assert_entry: &AssertEntry) -> Result<(), SlinkyError> {
        if !self.rs.should_emit_entry(
            &assert_entry.exclude_if_any,
            &assert_entry.exclude_if_all,
            &assert_entry.include_if_any,
            &assert_entry.include_if_all,
        ) {
            return Ok(());
        }

        self.buffer
            .write_assert(&assert_entry.check, &assert_entry.error_message);

        Ok(())
    }
}

// internal functions
impl LinkerWriter<'_> {
    fn write_sym_end_size(&mut self, start: &str, end: &str, size: &str, value: &str) {
        self.buffer.write_linker_symbol(end, value);

        self.buffer
            .write_linker_symbol(size, &format!("ABSOLUTE({} - {})", end, start));
    }

    fn write_sections_kind_start(&mut self, segment: &Segment, noload: bool) {
        if self.emit_sections_kind_symbols {
            let style = &self.d.settings.linker_symbols_style;

            let seg_sym_suffix = if noload { "noload" } else { "alloc" };
            let seg_sym = format!("{}_{}", segment.name, seg_sym_suffix);

            let seg_sym_start = style.segment_vram_start(&seg_sym);

            self.buffer.write_linker_symbol(&seg_sym_start, ".");

            self.buffer.write_empty_line();
        }
    }

    fn write_sections_kind_end(&mut self, segment: &Segment, noload: bool) {
        if self.emit_sections_kind_symbols {
            self.buffer.write_empty_line();

            let style = &self.d.settings.linker_symbols_style;

            let seg_sym_suffix = if noload { "noload" } else { "alloc" };
            let seg_sym = format!("{}_{}", segment.name, seg_sym_suffix);

            let seg_sym_start = style.segment_vram_start(&seg_sym);
            let seg_sym_end = style.segment_vram_end(&seg_sym);
            let seg_sym_size = style.segment_vram_size(&seg_sym);

            self.write_sym_end_size(&seg_sym_start, &seg_sym_end, &seg_sym_size, ".");
        }
    }

    fn write_section_symbol_start(&mut self, segment: &Segment, section: &str) {
        if self.emit_section_symbols {
            if let Some(section_start_align) = segment.section_start_align {
                self.buffer.align_symbol(".", section_start_align);
            }
            if let Some(align_value) = segment.sections_start_alignment.get(section) {
                self.buffer.align_symbol(".", *align_value);
            }

            if let Some(gp_info) = &segment.gp_info {
                if self.rs.should_emit_entry(
                    &gp_info.exclude_if_any,
                    &gp_info.exclude_if_all,
                    &gp_info.include_if_any,
                    &gp_info.include_if_all,
                ) && gp_info.section == *section
                {
                    self.buffer.write_symbol_assignment(
                        "_gp",
                        &format!(". + 0x{:X}", gp_info.offset),
                        gp_info.provide,
                        gp_info.hidden,
                    );
                }
            }

            let style = &self.d.settings.linker_symbols_style;

            let section_start_sym = style.segment_section_start(&segment.name, section);

            self.buffer.write_linker_symbol(&section_start_sym, ".");
        }
    }

    fn write_section_symbol_end(&mut self, segment: &Segment, section: &str) {
        if self.emit_section_symbols {
            if let Some(section_end_align) = segment.section_end_align {
                self.buffer.align_symbol(".", section_end_align);
            }
            if let Some(align_value) = segment.sections_end_alignment.get(section) {
                self.buffer.align_symbol(".", *align_value);
            }

            let style = &self.d.settings.linker_symbols_style;

            let section_start_sym = style.segment_section_start(&segment.name, section);
            let section_end_sym = style.segment_section_end(&segment.name, section);
            let section_size_sym = style.segment_section_size(&segment.name, section);

            self.write_sym_end_size(&section_start_sym, &section_end_sym, &section_size_sym, ".");
        }
    }

    fn write_segment_start(&mut self, segment: &Segment, noload: bool) {
        let style = &self.d.settings.linker_symbols_style;

        self.write_sections_kind_start(segment, noload);

        let name_suffix = if noload { ".noload" } else { "" };
        let mut line = format!(".{}{}", segment.name, name_suffix);

        if noload {
            line += " (NOLOAD) :";
        } else {
            if let Some(fixed_vram) = segment.fixed_vram {
                line += &format!(" 0x{:08X}", fixed_vram);
            } else if let Some(fixed_symbol) = &segment.fixed_symbol {
                line += &format!(" {}", fixed_symbol);
            } else if let Some(follows_segment) = &segment.follows_segment {
                line += &format!(" {}", style.segment_vram_end(follows_segment));
            } else if let Some(vram_class) = &segment.vram_class {
                line += &format!(" {}", style.vram_class_start(vram_class));
            }

            line += &format!(" : AT({})", style.segment_rom_start(&segment.name));
        }

        if let Some(subalign) = segment.subalign {
            line += &format!(" SUBALIGN({})", subalign);
        }

        self.buffer.writeln(&line);
        self.buffer.begin_block();
    }

    fn write_segment_end(&mut self, segment: &Segment, noload: bool) {
        self.buffer.end_block();

        self.write_sections_kind_end(segment, noload);
    }

    fn emit_file(
        &mut self,
        file: &FileInfo,
        segment: &Segment,
        section: &str,
        sections: &[String],
        base_path: &EscapedPath,
    ) -> Result<(), SlinkyError> {
        if !self.rs.should_emit_entry(
            &file.exclude_if_any,
            &file.exclude_if_all,
            &file.include_if_any,
            &file.include_if_all,
        ) {
            return Ok(());
        }

        let style = &self.d.settings.linker_symbols_style;

        let wildcard = if segment.wildcard_sections { "*" } else { "" };

        let (left_side, right_side) = match &file.keep_sections {
            KeepSections::Absent => ("", ""),
            KeepSections::All(all) => {
                if *all {
                    ("KEEP(", ")")
                } else {
                    ("", "")
                }
            }
            KeepSections::WhichOnes(which_ones) => {
                if which_ones.contains(section) {
                    ("KEEP(", ")")
                } else {
                    ("", "")
                }
            }
        };

        // TODO: figure out glob support
        match file.kind {
            FileKind::Object => {
                let mut path = base_path.clone();
                path.push(file.path_escaped(self.rs)?);

                self.buffer.writeln(&format!(
                    "{}{}({}{}){};",
                    left_side, path, section, wildcard, right_side
                ));
                if !self.files_paths.contains(&path) {
                    self.files_paths.insert(path);
                }
            }
            FileKind::Archive => {
                let mut path = base_path.clone();
                path.push(file.path_escaped(self.rs)?);

                self.buffer.writeln(&format!(
                    "{}{}:{}({}{}){};",
                    left_side, path, file.subfile, section, wildcard, right_side
                ));
                if !self.files_paths.contains(&path) {
                    self.files_paths.insert(path);
                }
            }
            FileKind::Pad => {
                if file.section == section {
                    self.buffer
                        .writeln(&format!(". += 0x{:X};", file.pad_amount));
                }
            }
            FileKind::LinkerOffset => {
                if file.section == section {
                    self.buffer
                        .write_linker_symbol(&style.linker_offset(&file.linker_offset_name), ".");
                }
            }
            FileKind::Group => {
                let mut new_base_path = base_path.clone();

                new_base_path.push(file.dir_escaped(self.rs)?);

                for file_of_group in &file.files {
                    self.emit_section_for_file(
                        file_of_group,
                        segment,
                        section,
                        sections,
                        &new_base_path,
                    )?;
                }
            }
        }

        Ok(())
    }

    fn emit_section_for_file(
        &mut self,
        file: &FileInfo,
        segment: &Segment,
        section: &str,
        sections: &[String],
        base_path: &EscapedPath,
    ) -> Result<(), SlinkyError> {
        if !file.section_order.is_empty() {
            // Keys specify the section and value specify where it will be put.
            // For example: `section_order: { .data: .rodata }`, meaning the `.data` of the file should be put within its `.rodata`.
            // It was done this way instead of the other way around (ie keys specifying the destination section) because the other way would not allow specifying multiple sections should be put in the same destination section.

            let mut sections_to_emit_here = if file.section_order.contains_key(section) {
                // This section should be placed somewhere else
                vec![]
            } else {
                vec![section]
            };

            // Check if any other section should be placed be placed here
            for (k, v) in &file.section_order {
                if v == section {
                    sections_to_emit_here.push(k);
                }
            }

            // We need to preserve the order given by alloc_sections or noload_sections
            sections_to_emit_here.sort_unstable_by_key(|&k| sections.iter().position(|s| s == k));

            for k in sections_to_emit_here {
                self.emit_file(file, segment, k, sections, base_path)?;

                if !self.reference_partial_objects {
                    if let Some(other_sections) = segment.sections_subgroups.get(k) {
                        for other in other_sections {
                            self.emit_section_for_file(file, segment, other, sections, base_path)?;
                        }
                    }
                }
            }
        } else {
            // No need to mess with section ordering, just emit the file
            self.emit_file(file, segment, section, sections, base_path)?;

            if !self.reference_partial_objects {
                if let Some(other_sections) = segment.sections_subgroups.get(section) {
                    for other in other_sections {
                        self.emit_section_for_file(file, segment, other, sections, base_path)?;
                    }
                }
            }
        }

        Ok(())
    }

    fn emit_section(
        &mut self,
        segment: &Segment,
        section: &str,
        sections: &[String],
    ) -> Result<(), SlinkyError> {
        let mut base_path = self.d.settings.base_path_escaped(self.rs)?;

        if !self.reference_partial_objects {
            base_path.push(segment.dir_escaped(self.rs)?);
        }

        for file in &segment.files {
            self.emit_section_for_file(file, segment, section, sections, &base_path)?;
        }

        Ok(())
    }

    fn write_segment(
        &mut self,
        segment: &Segment,
        sections: &[String],
        noload: bool,
    ) -> Result<(), SlinkyError> {
        self.write_segment_start(segment, noload);

        if let Some(fill_value) = segment.fill_value {
            self.buffer.writeln(&format!("FILL(0x{:08X});", fill_value));
        }

        for (i, section) in sections.iter().enumerate() {
            self.write_section_symbol_start(segment, section);

            self.emit_section(segment, section, sections)?;

            self.write_section_symbol_end(segment, section);

            if i + 1 < sections.len() {
                self.buffer.write_empty_line();
            }
        }

        self.write_segment_end(segment, noload);

        Ok(())
    }

    fn write_single_segment(
        &mut self,
        segment: &Segment,
        sections: &[String],
        noload: bool,
    ) -> Result<(), SlinkyError> {
        self.write_sections_kind_start(segment, noload);

        for (i, section) in sections.iter().enumerate() {
            let mut line = String::new();

            self.write_section_symbol_start(segment, section);

            line += &format!("{}{} :", section, if noload { " (NOLOAD)" } else { "" });

            if let Some(subalign) = segment.subalign {
                line += &format!(" SUBALIGN({})", subalign);
            }

            self.buffer.writeln(&line);
            self.buffer.begin_block();

            if let Some(fill_value) = segment.fill_value {
                self.buffer.writeln(&format!("FILL(0x{:08X});", fill_value));
            }

            self.emit_section(segment, section, sections)?;

            self.buffer.end_block();
            self.write_section_symbol_end(segment, section);

            if i + 1 < sections.len() {
                self.buffer.write_empty_line();
            }
        }

        self.write_sections_kind_end(segment, noload);

        Ok(())
    }
}
