/* SPDX-FileCopyrightText: © 2024 decompals */
/* SPDX-License-Identifier: MIT */

use serde::Deserialize;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use crate::{absent_nullable::AbsentNullable, file_kind::FileKind, Settings, SlinkyError};

#[derive(PartialEq, Debug)]
pub struct FileInfo {
    pub path: PathBuf,

    pub kind: FileKind,

    // Used for archives
    pub subfile: String,

    pub pad_amount: u32,
    pub section: String,

    pub linker_offset_name: String,

    pub section_order: HashMap<String, String>,
}

#[derive(Deserialize, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub(crate) struct FileInfoSerial {
    #[serde(default)]
    pub path: AbsentNullable<PathBuf>,

    #[serde(default)]
    pub kind: AbsentNullable<FileKind>,

    #[serde(default)]
    pub subfile: AbsentNullable<String>,

    #[serde(default)]
    pub pad_amount: AbsentNullable<u32>,
    #[serde(default)]
    pub section: AbsentNullable<String>,

    #[serde(default)]
    pub linker_offset_name: AbsentNullable<String>,

    #[serde(default)]
    pub section_order: AbsentNullable<HashMap<String, String>>,
}

impl FileInfoSerial {
    pub(crate) fn unserialize(self, _settings: &Settings) -> Result<FileInfo, SlinkyError> {
        // Since a `kind` can be deduced from a `path` (which requires a `path`) then we need to do both simultaneously
        let (path, kind) = match self.kind.get_non_null_no_default("kind")? {
            Some(k) => match k {
                FileKind::Object | FileKind::Archive => {
                    let p = self.path.get("path")?;

                    if p == Path::new("") {
                        return Err(SlinkyError::EmptyValue {
                            name: "path".to_string(),
                        });
                    }

                    (p, k)
                }
                FileKind::Pad | FileKind::LinkerOffset => {
                    // pad doesn't allow for paths
                    if self.path.has_value() {
                        return Err(SlinkyError::InvalidFieldCombo {
                            field1: "kind: pad or kind: linker_offset".into(),
                            field2: "path".into(),
                        });
                    }

                    (PathBuf::new(), k)
                }
            },
            None => {
                let p = self.path.get("path")?;

                if p == Path::new("") {
                    return Err(SlinkyError::EmptyValue {
                        name: "path".to_string(),
                    });
                }

                let k = FileKind::from_path(&p);
                (p, k)
            }
        };

        let subfile = match kind {
            FileKind::Object | FileKind::LinkerOffset | FileKind::Pad => {
                if self.subfile.has_value() {
                    return Err(SlinkyError::InvalidFieldCombo {
                        field1: "subfile".into(),
                        field2: "non `kind: archive`".into(),
                    });
                }
                "*".to_string()
            }
            FileKind::Archive => self.subfile.get_non_null("subfile", || "*".to_string())?,
        };

        let pad_amount = match kind {
            FileKind::Object | FileKind::LinkerOffset | FileKind::Archive => {
                if self.pad_amount.has_value() {
                    return Err(SlinkyError::InvalidFieldCombo {
                        field1: "pad_amount".into(),
                        field2: "non `kind: pad`".into(),
                    });
                }
                0
            }
            FileKind::Pad => self.pad_amount.get("pad_amount")?,
        };

        let section = match kind {
            FileKind::Object | FileKind::Archive => {
                if self.section.has_value() {
                    return Err(SlinkyError::InvalidFieldCombo {
                        field1: "section".into(),
                        field2: "non `kind: pad or kind: linker_offset`".into(),
                    });
                }
                "".into()
            }
            FileKind::Pad | FileKind::LinkerOffset => self.section.get("section")?,
        };

        let linker_offset_name = match kind {
            FileKind::Object | FileKind::Pad | FileKind::Archive => {
                if self.linker_offset_name.has_value() {
                    return Err(SlinkyError::InvalidFieldCombo {
                        field1: "linker_offset_name".into(),
                        field2: "non `kind: linker_offset`".into(),
                    });
                }
                "".into()
            }
            FileKind::LinkerOffset => self.linker_offset_name.get("linker_offset_name")?,
        };

        let section_order = match kind {
            FileKind::Pad | FileKind::LinkerOffset => {
                if self.section_order.has_value() {
                    return Err(SlinkyError::InvalidFieldCombo {
                        field1: "section_order".into(),
                        field2: "non `kind: object` or `kind: archive`".into(),
                    });
                }
                HashMap::default()
            }
            FileKind::Object | FileKind::Archive => self
                .section_order
                .get_non_null("section_order", HashMap::default)?,
        };

        Ok(FileInfo {
            path,
            kind,
            subfile,
            pad_amount,
            section,
            linker_offset_name,
            section_order,
        })
    }
}
