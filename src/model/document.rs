use super::page_ref::Comment;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// A form field extracted from a PDF document.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct FormField {
    pub obj_id: lopdf::ObjectId,
    pub name: String,
    pub field_type: FormFieldType,
    pub value: String,
    pub rect: [f64; 4], // [x1, y1, x2, y2] in PDF coordinates
    pub page_num: usize, // 0-indexed
    pub y_fraction: f32, // for tick marks, like Comment
    /// Available options for checkbox/radio fields (appearance state names excluding "Off").
    pub options: Vec<String>,
}

/// Type of PDF form field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormFieldType {
    Text,
    Checkbox,
    Choice,
}

/// Undo info for a form field value change.
#[derive(Debug, Clone)]
pub struct FormFieldUndo {
    pub obj_id: lopdf::ObjectId,
    pub old_value: String,
    pub field_index: usize,
}

/// A loaded PDF document, wrapping lopdf for manipulation.
/// Optionally retains the raw file bytes so hayro can parse without re-reading.
/// Object IDs added by a signature embed, for undo.
#[derive(Debug, Clone)]
pub struct SignatureUndo {
    #[allow(dead_code)]
    pub page_num: usize,
    pub img_obj_id: lopdf::ObjectId,
    pub smask_obj_id: Option<lopdf::ObjectId>,
    pub content_obj_id: lopdf::ObjectId,
    pub xobj_name: Vec<u8>,
    /// The ObjectId of the resources dict where the XObject was registered.
    pub resources_obj_id: Option<lopdf::ObjectId>,
    /// The page ObjectId (for inline resources).
    pub page_id: lopdf::ObjectId,
}

/// Object IDs added by a text stamp, for undo.
#[derive(Debug, Clone)]
pub struct TextStampUndo {
    pub content_obj_id: lopdf::ObjectId,
    pub page_id: lopdf::ObjectId,
    pub font_name: Vec<u8>,
    pub resources_obj_id: Option<lopdf::ObjectId>,
}

pub struct PdfDocument {
    pub path: PathBuf,
    pub label: String,
    doc: lopdf::Document,
    /// Raw PDF bytes, retained for hayro rendering.
    raw_bytes: Option<Vec<u8>>,
    /// Stack of signature undos.
    pub signature_undos: Vec<SignatureUndo>,
    /// Stack of text stamp undos.
    pub text_stamp_undos: Vec<TextStampUndo>,
}

/// Extract f64 from a PDF numeric object (Integer or Real).
fn obj_to_f64(obj: &lopdf::Object) -> Option<f64> {
    match obj {
        lopdf::Object::Real(f) => Some(*f as f64),
        lopdf::Object::Integer(i) => Some(*i as f64),
        _ => None,
    }
}

/// Format a PDF date string (D:YYYYMMDDHHmmSS) into a readable format.
fn format_pdf_date(s: &str) -> String {
    let s = s.strip_prefix("D:").unwrap_or(s);
    if s.len() >= 8 {
        let year = &s[0..4];
        let month = s.get(4..6).unwrap_or("01");
        let day = s.get(6..8).unwrap_or("01");
        let hour = s.get(8..10).unwrap_or("00");
        let min = s.get(10..12).unwrap_or("00");
        if hour == "00" && min == "00" {
            format!("{year}-{month}-{day}")
        } else {
            format!("{year}-{month}-{day} {hour}:{min}")
        }
    } else {
        s.to_string()
    }
}

impl PdfDocument {
    /// Open a PDF from a file path, reading bytes once for both lopdf and hayro.
    pub fn open(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let doc = lopdf::Document::load_mem(&bytes)
            .with_context(|| format!("failed to parse PDF: {}", path.display()))?;
        let label = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        Ok(Self {
            path: path.to_owned(),
            label,
            doc,
            raw_bytes: Some(bytes),
            signature_undos: Vec::new(),
            text_stamp_undos: Vec::new(),
        })
    }

    /// Number of pages in the document.
    pub fn page_count(&self) -> usize {
        self.doc.get_pages().len()
    }

    /// Get the page dimensions (width, height) in PDF points for each page.
    pub fn page_dimensions(&self) -> Vec<(f64, f64)> {
        let pages = self.doc.get_pages();
        let mut dims = Vec::with_capacity(pages.len());
        for page_num in 1..=pages.len() {
            let dim = (|| -> Option<(f64, f64)> {
                let &page_id = pages.get(&(page_num as u32))?;
                let media_box = self.find_media_box(page_id)?;
                if media_box.len() >= 4 {
                    let w = obj_to_f64(&media_box[2]).unwrap_or(612.0);
                    let h = obj_to_f64(&media_box[3]).unwrap_or(792.0);
                    Some((w, h))
                } else {
                    None
                }
            })();
            dims.push(dim.unwrap_or((612.0, 792.0)));
        }
        dims
    }

    /// Find MediaBox for a page, walking up the page tree if inherited.
    fn find_media_box(&self, page_id: lopdf::ObjectId) -> Option<Vec<lopdf::Object>> {
        let obj = self.doc.get_object(page_id).ok()?;
        let dict = obj.as_dict().ok()?;

        // Try direct MediaBox (may be array or reference)
        if let Ok(mb) = dict.get(b"MediaBox") {
            match mb {
                lopdf::Object::Array(arr) => return Some(arr.clone()),
                lopdf::Object::Reference(id) => {
                    if let Ok(obj) = self.doc.get_object(*id) {
                        if let Ok(arr) = obj.as_array() {
                            return Some(arr.clone());
                        }
                    }
                }
                _ => {}
            }
        }

        // Inherit from parent
        if let Ok(parent_ref) = dict.get(b"Parent") {
            if let Ok(parent_id) = parent_ref.as_reference() {
                return self.find_media_box(parent_id);
            }
        }

        None
    }

    /// Get the raw PDF bytes (for hayro rendering).
    pub fn raw_bytes(&self) -> Option<&[u8]> {
        self.raw_bytes.as_deref()
    }

    /// Clone the underlying lopdf document for manipulation.
    pub fn clone_lopdf(&self) -> lopdf::Document {
        self.doc.clone()
    }

    /// Print PDF metadata (Title, Author, Subject, Creator, Producer, dates).
    pub fn print_metadata(&self) {
        // Try the Info dictionary (trailer -> /Info reference)
        let info_id = self.doc.trailer.get(b"Info")
            .ok()
            .and_then(|o| o.as_reference().ok());

        if let Some(id) = info_id {
            if let Ok(obj) = self.doc.get_object(id) {
                if let Ok(dict) = obj.as_dict() {
                    let fields = [
                        ("Title", "Title"),
                        ("Author", "Author"),
                        ("Subject", "Subject"),
                        ("Creator", "Creator"),
                        ("Producer", "Producer"),
                        ("CreationDate", "Created"),
                        ("ModDate", "Modified"),
                    ];
                    let mut any = false;
                    for (key, label) in fields {
                        if let Ok(val) = dict.get(key.as_bytes()) {
                            let text = match val.as_str() {
                                Ok(b) => {
                                    // Handle UTF-16BE BOM
                                    if b.len() >= 2 && b[0] == 0xFE && b[1] == 0xFF {
                                        let chars: Vec<u16> = b[2..].chunks(2)
                                            .map(|c| u16::from_be_bytes([c[0], *c.get(1).unwrap_or(&0)]))
                                            .collect();
                                        String::from_utf16_lossy(&chars)
                                    } else {
                                        String::from_utf8_lossy(b).into_owned()
                                    }
                                }
                                Err(_) => continue,
                            };
                            let text = text.trim().to_string();
                            if !text.is_empty() {
                                if !any {
                                    println!();
                                    any = true;
                                }
                                // Clean up date format: D:20240101120000 -> 2024-01-01 12:00:00
                                let display = if key.starts_with("Creat") || key == "ModDate" {
                                    format_pdf_date(&text)
                                } else {
                                    text
                                };
                                println!("{label}: {display}");
                            }
                        }
                    }
                }
            }
        }
    }

    /// Undo the last signature embed. Returns true if something was undone.
    pub fn undo_signature(&mut self) -> bool {
        let undo = match self.signature_undos.pop() {
            Some(u) => u,
            None => return false,
        };

        // Remove the added objects
        self.doc.delete_object(undo.content_obj_id);
        self.doc.delete_object(undo.img_obj_id);
        if let Some(smask_id) = undo.smask_obj_id {
            self.doc.delete_object(smask_id);
        }

        // Remove XObject entry from resources
        let res_result = if let Some(res_id) = undo.resources_obj_id {
            self.doc.get_object_mut(res_id)
        } else {
            self.doc.get_object_mut(undo.page_id)
                .and_then(|o| o.as_dict_mut().map_err(|e| lopdf::Error::from(e)))
                .and_then(|d| d.get_mut(b"Resources").map_err(|e| lopdf::Error::from(e)))
        };
        if let Ok(res_obj) = res_result {
            if let Ok(res_dict) = res_obj.as_dict_mut() {
                if let Ok(xobjs) = res_dict.get_mut(b"XObject") {
                    if let Ok(xobj_dict) = xobjs.as_dict_mut() {
                        xobj_dict.remove(&undo.xobj_name);
                    }
                }
            }
        }

        // Remove the content stream reference from the page's /Contents
        if let Ok(page_obj) = self.doc.get_object_mut(undo.page_id) {
            if let Ok(page_dict) = page_obj.as_dict_mut() {
                if let Ok(contents) = page_dict.get_mut(b"Contents") {
                    if let lopdf::Object::Array(arr) = contents {
                        arr.retain(|o| {
                            !matches!(o, lopdf::Object::Reference(id) if *id == undo.content_obj_id)
                        });
                    }
                }
            }
        }

        true
    }

    /// Undo the last text stamp. Returns true if something was undone.
    pub fn undo_text_stamp(&mut self) -> bool {
        let undo = match self.text_stamp_undos.pop() {
            Some(u) => u,
            None => return false,
        };

        // Remove the content stream object
        self.doc.delete_object(undo.content_obj_id);

        // Remove font entry from resources (only if we added it — but safe to remove
        // since we use a unique name PdftFont that nothing else references)
        let res_result = if let Some(res_id) = undo.resources_obj_id {
            self.doc.get_object_mut(res_id)
        } else {
            self.doc.get_object_mut(undo.page_id)
                .and_then(|o| o.as_dict_mut().map_err(|e| lopdf::Error::from(e)))
                .and_then(|d| d.get_mut(b"Resources").map_err(|e| lopdf::Error::from(e)))
        };
        if let Ok(res_obj) = res_result {
            if let Ok(res_dict) = res_obj.as_dict_mut() {
                if let Ok(fonts) = res_dict.get_mut(b"Font") {
                    if let Ok(font_dict) = fonts.as_dict_mut() {
                        font_dict.remove(&undo.font_name);
                    }
                }
            }
        }

        // Remove content stream from page's /Contents
        if let Ok(page_obj) = self.doc.get_object_mut(undo.page_id) {
            if let Ok(page_dict) = page_obj.as_dict_mut() {
                if let Ok(contents) = page_dict.get_mut(b"Contents") {
                    if let lopdf::Object::Array(arr) = contents {
                        arr.retain(|o| {
                            !matches!(o, lopdf::Object::Reference(id) if *id == undo.content_obj_id)
                        });
                    }
                }
            }
        }

        true
    }

    /// Embed text on a page using Courier (a PDF standard font).
    /// `page_num` is 0-indexed, coordinates are in PDF points from bottom-left.
    pub fn embed_text(
        &mut self,
        page_num: usize,
        text: &str,
        x_pt: f64,
        y_pt: f64,
        font_size: f64,
    ) -> Result<()> {
        use lopdf::{Dictionary, Object, Stream};

        let pages = self.doc.get_pages();
        let &page_id = pages.get(&(page_num as u32 + 1))
            .context("page not found")?;

        let font_name = b"PdftFont".to_vec();

        // Register Courier font in page resources
        let resources_obj_id;
        {
            let page_obj = self.doc.get_object(page_id)?;
            let page_dict = page_obj.as_dict()
                .map_err(|e| anyhow::anyhow!("{e}"))?;

            let res_obj_id = match page_dict.get(b"Resources") {
                Ok(Object::Reference(id)) => Some(*id),
                Ok(Object::Dictionary(_)) => None,
                _ => {
                    page_dict.get(b"Parent")
                        .ok()
                        .and_then(|p| p.as_reference().ok())
                        .and_then(|parent_id| {
                            self.doc.get_dictionary(parent_id).ok()
                                .and_then(|parent| parent.get(b"Resources").ok())
                                .and_then(|r| r.as_reference().ok())
                        })
                }
            };
            resources_obj_id = res_obj_id;

            let font_obj = Dictionary::from_iter(vec![
                ("Type", Object::Name(b"Font".to_vec())),
                ("Subtype", Object::Name(b"Type1".to_vec())),
                ("BaseFont", Object::Name(b"Courier".to_vec())),
            ]);

            if let Some(res_id) = res_obj_id {
                let res_obj = self.doc.get_object_mut(res_id)?;
                let res_dict = res_obj.as_dict_mut()
                    .map_err(|e| anyhow::anyhow!("{e}"))?;

                if res_dict.get(b"Font").is_err() {
                    res_dict.set("Font", Object::Dictionary(Dictionary::new()));
                }
                let fonts = res_dict.get_mut(b"Font")
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                let font_dict = fonts.as_dict_mut()
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                font_dict.set(font_name.clone(), Object::Dictionary(font_obj));
            } else {
                let page_obj = self.doc.get_object_mut(page_id)?;
                let page_dict = page_obj.as_dict_mut()
                    .map_err(|e| anyhow::anyhow!("{e}"))?;

                if page_dict.get(b"Resources").is_err() {
                    page_dict.set("Resources", Object::Dictionary(Dictionary::new()));
                }
                let resources = page_dict.get_mut(b"Resources")
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                let res_dict = resources.as_dict_mut()
                    .map_err(|e| anyhow::anyhow!("{e}"))?;

                if res_dict.get(b"Font").is_err() {
                    res_dict.set("Font", Object::Dictionary(Dictionary::new()));
                }
                let fonts = res_dict.get_mut(b"Font")
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                let font_dict = fonts.as_dict_mut()
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                font_dict.set(font_name.clone(), Object::Dictionary(font_obj));
            }
        }

        // Escape special PDF string characters
        let escaped: String = text.chars().map(|c| match c {
            '(' => "\\(".to_string(),
            ')' => "\\)".to_string(),
            '\\' => "\\\\".to_string(),
            _ => c.to_string(),
        }).collect();

        // Create content stream with text drawing commands
        let content_bytes = format!(
            "\nq BT /PdftFont {:.1} Tf {:.2} {:.2} Td ({}) Tj ET Q\n",
            font_size, x_pt, y_pt, escaped
        ).into_bytes();

        let content_stream = Stream::new(Dictionary::new(), content_bytes);
        let content_id = self.doc.add_object(content_stream);

        // Append to page /Contents
        {
            let page_obj = self.doc.get_object_mut(page_id)?;
            let page_dict = page_obj.as_dict_mut()
                .map_err(|e| anyhow::anyhow!("{e}"))?;

            match page_dict.get_mut(b"Contents") {
                Ok(contents) => {
                    match contents {
                        Object::Array(arr) => {
                            arr.push(Object::Reference(content_id));
                        }
                        Object::Reference(existing_ref) => {
                            let existing = *existing_ref;
                            *contents = Object::Array(vec![
                                Object::Reference(existing),
                                Object::Reference(content_id),
                            ]);
                        }
                        _ => {
                            page_dict.set("Contents", Object::Reference(content_id));
                        }
                    }
                }
                Err(_) => {
                    page_dict.set("Contents", Object::Reference(content_id));
                }
            }
        }

        self.text_stamp_undos.push(TextStampUndo {
            content_obj_id: content_id,
            page_id,
            font_name,
            resources_obj_id,
        });

        Ok(())
    }

    /// Re-serialize the lopdf document to update raw_bytes (for hayro re-rendering).
    pub fn refresh_bytes(&mut self) -> Result<()> {
        // Clone before saving — save_to mutates internal state (max_id, trailer)
        // which would corrupt the document for subsequent edits.
        let mut doc_clone = self.doc.clone();
        let mut buf = Vec::new();
        doc_clone.save_to(&mut buf)
            .with_context(|| "failed to serialize PDF after modification")?;
        self.raw_bytes = Some(buf);
        Ok(())
    }

    /// Embed a PNG image as a signature on the given page.
    /// `page_num` is 0-indexed, coordinates are in PDF points from bottom-left.
    pub fn embed_signature(
        &mut self,
        page_num: usize,
        png_path: &std::path::Path,
        x_pt: f64,
        y_pt: f64,
        display_width_pt: f64,
    ) -> Result<()> {
        use lopdf::{Dictionary, Object, Stream};

        // Read and decode the PNG
        let img = image::open(png_path)
            .with_context(|| format!("failed to open {}", png_path.display()))?
            .into_rgba8();
        let (img_w, img_h) = img.dimensions();

        // Separate RGB and alpha channels
        let mut rgb_data = Vec::with_capacity((img_w * img_h * 3) as usize);
        let mut alpha_data = Vec::with_capacity((img_w * img_h) as usize);
        let mut has_alpha = false;
        for pixel in img.pixels() {
            let a = pixel[3] as f32 / 255.0;
            if pixel[3] != 255 {
                has_alpha = true;
            }
            // Composite onto white background for RGB channel
            // (PDF SMask will handle the actual transparency)
            rgb_data.push((pixel[0] as f32 * a + 255.0 * (1.0 - a)) as u8);
            rgb_data.push((pixel[1] as f32 * a + 255.0 * (1.0 - a)) as u8);
            rgb_data.push((pixel[2] as f32 * a + 255.0 * (1.0 - a)) as u8);
            alpha_data.push(pixel[3]);
        }

        // Create SMask (alpha) stream if image has transparency
        let smask_id = if has_alpha {
            let smask_dict = Dictionary::from_iter(vec![
                ("Type", Object::Name(b"XObject".to_vec())),
                ("Subtype", Object::Name(b"Image".to_vec())),
                ("Width", Object::Integer(img_w as i64)),
                ("Height", Object::Integer(img_h as i64)),
                ("ColorSpace", Object::Name(b"DeviceGray".to_vec())),
                ("BitsPerComponent", Object::Integer(8)),
            ]);
            let mut smask_stream = Stream::new(smask_dict, alpha_data);
            let _ = smask_stream.compress();
            Some(self.doc.add_object(smask_stream))
        } else {
            None
        };

        // Create image XObject stream
        let mut img_dict = Dictionary::from_iter(vec![
            ("Type", Object::Name(b"XObject".to_vec())),
            ("Subtype", Object::Name(b"Image".to_vec())),
            ("Width", Object::Integer(img_w as i64)),
            ("Height", Object::Integer(img_h as i64)),
            ("ColorSpace", Object::Name(b"DeviceRGB".to_vec())),
            ("BitsPerComponent", Object::Integer(8)),
        ]);
        if let Some(smask_ref) = smask_id {
            img_dict.set("SMask", Object::Reference(smask_ref));
        }
        let mut img_stream = Stream::new(img_dict, rgb_data);
        let _ = img_stream.compress();
        let img_obj_id = self.doc.add_object(img_stream);

        // Find the page object
        let pages = self.doc.get_pages();
        let &page_id = pages.get(&(page_num as u32 + 1))
            .context("page not found")?;

        // Generate a unique XObject name
        let xobj_name = b"PdftSig".to_vec();

        // Add XObject to page resources.
        // Resources may be: inline dict on the page, a reference to a shared dict,
        // or inherited from a parent Pages node. We need to find the actual dict to modify.
        let resources_obj_id;
        {
            let page_obj = self.doc.get_object(page_id)?;
            let page_dict = page_obj.as_dict()
                .map_err(|e| anyhow::anyhow!("{e}"))?;

            // Find where Resources lives: direct dict, reference, or inherited
            let res_obj_id = match page_dict.get(b"Resources") {
                Ok(Object::Reference(id)) => Some(*id),
                Ok(Object::Dictionary(_)) => None, // inline, handle below
                _ => {
                    // No Resources on page — check parent (inherited)
                    page_dict.get(b"Parent")
                        .ok()
                        .and_then(|p| p.as_reference().ok())
                        .and_then(|parent_id| {
                            self.doc.get_dictionary(parent_id).ok()
                                .and_then(|parent| parent.get(b"Resources").ok())
                                .and_then(|r| r.as_reference().ok())
                        })
                }
            };
            resources_obj_id = res_obj_id;

            if let Some(res_id) = res_obj_id {
                // Resources is a reference — modify the referenced object
                let res_obj = self.doc.get_object_mut(res_id)?;
                let res_dict = res_obj.as_dict_mut()
                    .map_err(|e| anyhow::anyhow!("{e}"))?;

                // Get or create /XObject sub-dict
                match res_dict.get(b"XObject") {
                    Ok(Object::Reference(xobj_ref)) => {
                        let xobj_ref = *xobj_ref;
                        let xobj_obj = self.doc.get_object_mut(xobj_ref)?;
                        let xobj_dict = xobj_obj.as_dict_mut()
                            .map_err(|e| anyhow::anyhow!("{e}"))?;
                        xobj_dict.set(xobj_name.clone(), Object::Reference(img_obj_id));
                    }
                    Ok(Object::Dictionary(_)) => {
                        let xobjects = res_dict.get_mut(b"XObject")
                            .map_err(|e| anyhow::anyhow!("{e}"))?;
                        let xobj_dict = xobjects.as_dict_mut()
                            .map_err(|e| anyhow::anyhow!("{e}"))?;
                        xobj_dict.set(xobj_name.clone(), Object::Reference(img_obj_id));
                    }
                    _ => {
                        let mut xobj_dict = Dictionary::new();
                        xobj_dict.set(xobj_name.clone(), Object::Reference(img_obj_id));
                        res_dict.set("XObject", Object::Dictionary(xobj_dict));
                    }
                }
            } else {
                // Resources is an inline dict on the page (or missing — create it)
                let page_obj = self.doc.get_object_mut(page_id)?;
                let page_dict = page_obj.as_dict_mut()
                    .map_err(|e| anyhow::anyhow!("{e}"))?;

                if page_dict.get(b"Resources").is_err() {
                    page_dict.set("Resources", Object::Dictionary(Dictionary::new()));
                }
                let resources = page_dict.get_mut(b"Resources")
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                let res_dict = resources.as_dict_mut()
                    .map_err(|e| anyhow::anyhow!("{e}"))?;

                if res_dict.get(b"XObject").is_err() {
                    res_dict.set("XObject", Object::Dictionary(Dictionary::new()));
                }
                let xobjects = res_dict.get_mut(b"XObject")
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                let xobj_dict = xobjects.as_dict_mut()
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                xobj_dict.set(xobj_name.clone(), Object::Reference(img_obj_id));
            }
        }

        // Create content stream with drawing commands
        let display_height = display_width_pt * (img_h as f64 / img_w as f64);
        let content_bytes = format!(
            "\nq {:.2} 0 0 {:.2} {:.2} {:.2} cm /PdftSig Do Q\n",
            display_width_pt, display_height, x_pt, y_pt
        ).into_bytes();

        let content_stream = Stream::new(Dictionary::new(), content_bytes);
        let content_id = self.doc.add_object(content_stream);

        // Append to the page's /Contents
        {
            let page_obj = self.doc.get_object_mut(page_id)?;
            let page_dict = page_obj.as_dict_mut()
                .map_err(|e| anyhow::anyhow!("{e}"))?;

            match page_dict.get_mut(b"Contents") {
                Ok(contents) => {
                    match contents {
                        Object::Array(arr) => {
                            arr.push(Object::Reference(content_id));
                        }
                        Object::Reference(existing_ref) => {
                            let existing = *existing_ref;
                            *contents = Object::Array(vec![
                                Object::Reference(existing),
                                Object::Reference(content_id),
                            ]);
                        }
                        _ => {
                            page_dict.set("Contents", Object::Reference(content_id));
                        }
                    }
                }
                Err(_) => {
                    page_dict.set("Contents", Object::Reference(content_id));
                }
            }
        }

        // Record undo info
        self.signature_undos.push(SignatureUndo {
            page_num,
            img_obj_id,
            smask_obj_id: smask_id,
            content_obj_id: content_id,
            xobj_name,
            resources_obj_id,
            page_id,
        });

        Ok(())
    }

    /// Extract form fields from the PDF's AcroForm.
    /// Returns an error if the form uses XFA (not supported).
    pub fn extract_form_fields(&self) -> Result<Vec<FormField>> {
        use lopdf::Object;

        let catalog = self.doc.catalog()
            .map_err(|e| anyhow::anyhow!("no catalog: {e}"))?;

        let acroform_obj = match catalog.get(b"AcroForm") {
            Ok(Object::Reference(id)) => self.doc.get_object(*id)
                .map_err(|e| anyhow::anyhow!("{e}"))?,
            Ok(obj @ Object::Dictionary(_)) => obj,
            _ => anyhow::bail!("no AcroForm found in this PDF"),
        };
        let acroform = acroform_obj.as_dict()
            .map_err(|e| anyhow::anyhow!("AcroForm is not a dict: {e}"))?;

        // Check for XFA
        if acroform.has(b"XFA") {
            anyhow::bail!("XFA forms not supported");
        }

        let fields_obj = match acroform.get(b"Fields") {
            Ok(obj) => obj,
            Err(_) => anyhow::bail!("no Fields in AcroForm"),
        };
        let fields_arr = match fields_obj {
            Object::Array(arr) => arr,
            Object::Reference(id) => {
                let obj = self.doc.get_object(*id)
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                obj.as_array().map_err(|e| anyhow::anyhow!("{e}"))?
            }
            _ => anyhow::bail!("Fields is not an array"),
        };

        // Build page_id -> 0-indexed page number map
        let pages = self.doc.get_pages();
        let page_dims = self.page_dimensions();
        let mut page_id_map: std::collections::HashMap<lopdf::ObjectId, usize> =
            std::collections::HashMap::new();
        for (&page_num_1, &page_id) in &pages {
            page_id_map.insert(page_id, (page_num_1 as usize).saturating_sub(1));
        }

        let mut result = Vec::new();
        for field_ref in fields_arr {
            let field_id = match field_ref {
                Object::Reference(id) => *id,
                _ => continue,
            };
            self.collect_form_fields(field_id, &[], None, None, &page_id_map, &page_dims, &mut result);
        }

        // Sort by page, then by visual row, then x (left to right).
        // Use the vertical center of each field's rect for row grouping.
        // Fields whose centers are within 10pt are considered the same row.
        result.sort_by(|a, b| {
            a.page_num.cmp(&b.page_num)
                .then_with(|| {
                    let mid_a = (a.rect[1] + a.rect[3]) / 2.0;
                    let mid_b = (b.rect[1] + b.rect[3]) / 2.0;
                    // Snap to 10pt rows (using center y in PDF coords)
                    let row_a = (mid_a / 10.0).round() as i64;
                    let row_b = (mid_b / 10.0).round() as i64;
                    // Higher PDF y = higher on page = should come first (reverse)
                    row_b.cmp(&row_a)
                })
                .then(a.rect[0].partial_cmp(&b.rect[0]).unwrap_or(std::cmp::Ordering::Equal))
        });

        Ok(result)
    }

    /// Recursively collect leaf form fields.
    /// `field_obj_id` is the ancestor node that holds /T and should receive /V writes.
    fn collect_form_fields(
        &self,
        obj_id: lopdf::ObjectId,
        parent_name_parts: &[String],
        parent_ft: Option<&[u8]>,
        parent_field_id: Option<lopdf::ObjectId>,
        page_id_map: &std::collections::HashMap<lopdf::ObjectId, usize>,
        page_dims: &[(f64, f64)],
        result: &mut Vec<FormField>,
    ) {
        use lopdf::Object;

        let obj = match self.doc.get_object(obj_id) {
            Ok(o) => o,
            Err(_) => return,
        };
        let dict = match obj.as_dict() {
            Ok(d) => d,
            Err(_) => return,
        };

        // Get partial name
        let partial_name = dict.get(b"T")
            .ok()
            .and_then(|o| o.as_str().ok())
            .map(|b| String::from_utf8_lossy(b).into_owned())
            .unwrap_or_default();

        // Build full name
        let mut name_parts: Vec<String> = parent_name_parts.to_vec();
        if !partial_name.is_empty() {
            name_parts.push(partial_name);
        }

        // If this node has /T, it's the field node (should hold /V)
        let field_id = if dict.has(b"T") { Some(obj_id) } else { parent_field_id };

        // Get field type (may be inherited from parent)
        let ft = dict.get(b"FT")
            .ok()
            .and_then(|o| o.as_name().ok())
            .or(parent_ft.map(|b| b as &[u8]));

        // Check for Kids (non-leaf)
        if let Ok(kids) = dict.get(b"Kids") {
            let kids_arr = match kids {
                Object::Array(arr) => Some(arr.as_slice()),
                Object::Reference(id) => self.doc.get_object(*id)
                    .ok()
                    .and_then(|o| o.as_array().ok())
                    .map(|a| a.as_slice()),
                _ => None,
            };
            if let Some(kids) = kids_arr {
                let owned_kids: Vec<Object> = kids.to_vec();
                for kid in &owned_kids {
                    if let Object::Reference(kid_id) = kid {
                        self.collect_form_fields(
                            *kid_id,
                            &name_parts,
                            ft,
                            field_id,
                            page_id_map,
                            page_dims,
                            result,
                        );
                    }
                }
                return;
            }
        }

        // Leaf field: must have FT and Rect
        let ft_bytes = match ft {
            Some(b) => b,
            None => return,
        };

        let field_type = match ft_bytes {
            b"Tx" => FormFieldType::Text,
            b"Btn" => FormFieldType::Checkbox,
            b"Ch" => FormFieldType::Choice,
            _ => return, // Skip Sig and unknown types
        };

        // Get value — check field node (parent with /T) first, then this widget
        let value = field_id
            .and_then(|fid| self.doc.get_object(fid).ok())
            .and_then(|o| o.as_dict().ok())
            .and_then(|d| d.get(b"V").ok())
            .or_else(|| dict.get(b"V").ok())
            .and_then(|o| match o {
                Object::String(b, _) => Some(String::from_utf8_lossy(b).into_owned()),
                Object::Name(b) => Some(String::from_utf8_lossy(b).into_owned()),
                _ => None,
            })
            .unwrap_or_default();

        // Get rect
        let rect = match dict.get(b"Rect") {
            Ok(obj) => match obj.as_array() {
                Ok(arr) if arr.len() >= 4 => {
                    [
                        arr[0].as_float().unwrap_or(0.0) as f64,
                        arr[1].as_float().unwrap_or(0.0) as f64,
                        arr[2].as_float().unwrap_or(0.0) as f64,
                        arr[3].as_float().unwrap_or(0.0) as f64,
                    ]
                }
                _ => return,
            },
            Err(_) => return,
        };

        // Determine page number
        let page_num = dict.get(b"P")
            .ok()
            .and_then(|o| o.as_reference().ok())
            .and_then(|pid| page_id_map.get(&pid).copied())
            .unwrap_or_else(|| {
                // Fallback: check page annotations
                self.find_page_for_annotation(obj_id, page_id_map)
                    .unwrap_or(0)
            });

        // Compute y_fraction
        let page_height = page_dims.get(page_num).map(|&(_, h)| h).unwrap_or(792.0);
        let mid_y = (rect[1] + rect[3]) / 2.0;
        let y_fraction = (1.0 - mid_y / page_height).clamp(0.0, 1.0) as f32;

        let full_name = if name_parts.is_empty() {
            format!("field_{}", obj_id.0)
        } else {
            name_parts.join(".")
        };

        // Extract options for checkbox/radio fields from /AP/N keys
        let options = if field_type == FormFieldType::Checkbox {
            dict.get(b"AP")
                .ok()
                .and_then(|ap| ap.as_dict().ok())
                .and_then(|ap| ap.get(b"N").ok())
                .and_then(|n| n.as_dict().ok())
                .map(|n_dict| {
                    n_dict.iter()
                        .map(|(k, _)| String::from_utf8_lossy(k).into_owned())
                        .filter(|k| k != "Off")
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        } else if field_type == FormFieldType::Choice {
            // /Opt array for dropdown/listbox
            dict.get(b"Opt")
                .ok()
                .and_then(|o| o.as_array().ok())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|item| match item {
                            lopdf::Object::String(b, _) => Some(String::from_utf8_lossy(b).into_owned()),
                            lopdf::Object::Array(pair) if pair.len() >= 2 => {
                                // [export_value, display_value]
                                match &pair[1] {
                                    lopdf::Object::String(b, _) => Some(String::from_utf8_lossy(b).into_owned()),
                                    _ => None,
                                }
                            }
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        // Use the field node (parent with /T) for value writes, not the leaf widget
        let value_obj_id = field_id.unwrap_or(obj_id);

        // Skip duplicate widgets for the same field (only keep the first one)
        if result.iter().any(|f| f.obj_id == value_obj_id) {
            return;
        }

        result.push(FormField {
            obj_id: value_obj_id,
            name: full_name,
            field_type,
            value,
            rect,
            page_num,
            y_fraction,
            options,
        });
    }

    /// Find which page an annotation belongs to by scanning page annotations.
    fn find_page_for_annotation(
        &self,
        obj_id: lopdf::ObjectId,
        page_id_map: &std::collections::HashMap<lopdf::ObjectId, usize>,
    ) -> Option<usize> {
        let pages = self.doc.get_pages();
        for (&_page_num, &page_id) in &pages {
            if let Ok(annots) = self.doc.get_page_annotations(page_id) {
                for annot in &annots {
                    // Check if this annotation dict's object ID matches
                    // We can't directly compare dicts, but we check the page's /Annots array
                    let _ = annot; // We need a different approach
                }
            }
            // Check /Annots array for references to our obj_id
            if let Ok(page_obj) = self.doc.get_object(page_id) {
                if let Ok(page_dict) = page_obj.as_dict() {
                    if let Ok(annots) = page_dict.get(b"Annots") {
                        let arr = match annots {
                            lopdf::Object::Array(a) => Some(a.as_slice()),
                            lopdf::Object::Reference(id) => self.doc.get_object(*id)
                                .ok()
                                .and_then(|o| o.as_array().ok())
                                .map(|a| a.as_slice()),
                            _ => None,
                        };
                        if let Some(arr) = arr {
                            for item in arr {
                                if let lopdf::Object::Reference(ref_id) = item {
                                    if *ref_id == obj_id {
                                        return page_id_map.get(&page_id).copied();
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        None
    }

    /// Set a form field's value. Returns the old value for undo.
    /// Also sets /NeedAppearances on the AcroForm dict.
    pub fn set_form_field_value(&mut self, obj_id: lopdf::ObjectId, value: &str) -> Result<String> {
        use lopdf::Object;

        // Read old value
        let old_value = {
            let obj = self.doc.get_object(obj_id)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let dict = obj.as_dict()
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            dict.get(b"V")
                .ok()
                .and_then(|o| match o {
                    Object::String(b, _) => Some(String::from_utf8_lossy(b).into_owned()),
                    Object::Name(b) => Some(String::from_utf8_lossy(b).into_owned()),
                    _ => None,
                })
                .unwrap_or_default()
        };

        // Determine field type to use the right value encoding
        let is_button = {
            let obj = self.doc.get_object(obj_id)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let dict = obj.as_dict()
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            dict.get(b"FT").ok()
                .and_then(|o| o.as_name().ok())
                .map(|n| n == b"Btn")
                .unwrap_or(false)
        };

        // Set new value and remove stale appearance
        {
            let obj = self.doc.get_object_mut(obj_id)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let dict = obj.as_dict_mut()
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            if is_button {
                // Checkboxes/radios use Name values and AS for appearance state
                dict.set("V", Object::Name(value.as_bytes().to_vec()));
                dict.set("AS", Object::Name(value.as_bytes().to_vec()));
            } else {
                dict.set("V", Object::String(value.as_bytes().to_vec(), lopdf::StringFormat::Literal));
            }
        }

        // Generate appearance streams for text fields
        if !is_button && !value.is_empty() {
            self.generate_field_appearances(obj_id, value)?;
        }

        // Set NeedAppearances on AcroForm
        self.set_need_appearances()?;

        Ok(old_value)
    }

    /// Generate simple appearance streams for a text field's widget annotations.
    fn generate_field_appearances(&mut self, field_obj_id: lopdf::ObjectId, value: &str) -> Result<()> {
        use lopdf::{Dictionary, Object, Stream};

        // Escape PDF string special chars
        let escaped: String = value.chars().map(|c| match c {
            '(' => "\\(".to_string(),
            ')' => "\\)".to_string(),
            '\\' => "\\\\".to_string(),
            _ => c.to_string(),
        }).collect();

        // Collect widget annotation IDs (Kids of the field, or the field itself if no Kids)
        let widget_ids: Vec<lopdf::ObjectId> = {
            let obj = self.doc.get_object(field_obj_id)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let dict = obj.as_dict()
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            match dict.get(b"Kids") {
                Ok(Object::Array(arr)) => {
                    arr.iter().filter_map(|o| {
                        if let Object::Reference(id) = o { Some(*id) } else { None }
                    }).collect()
                }
                _ => {
                    // Field is its own widget (merged field/widget)
                    if dict.has(b"Rect") {
                        vec![field_obj_id]
                    } else {
                        return Ok(());
                    }
                }
            }
        };

        for widget_id in widget_ids {
            let obj = match self.doc.get_object(widget_id) {
                Ok(o) => o,
                Err(_) => continue,
            };
            let dict = match obj.as_dict() {
                Ok(d) => d,
                Err(_) => continue,
            };

            // Get widget rect
            let rect = match dict.get(b"Rect") {
                Ok(Object::Array(arr)) if arr.len() >= 4 => {
                    let x1 = arr[0].as_float().unwrap_or(0.0) as f64;
                    let y1 = arr[1].as_float().unwrap_or(0.0) as f64;
                    let x2 = arr[2].as_float().unwrap_or(0.0) as f64;
                    let y2 = arr[3].as_float().unwrap_or(0.0) as f64;
                    [x1, y1, x2, y2]
                }
                _ => continue,
            };

            let w = (rect[2] - rect[0]).abs();
            let h = (rect[3] - rect[1]).abs();
            if w < 1.0 || h < 1.0 { continue; }

            // Get font info from /DA or use defaults
            let da = dict.get(b"DA")
                .or_else(|_| {
                    // Try parent field's /DA
                    self.doc.get_object(field_obj_id)
                        .and_then(|o| o.as_dict().map_err(|e| lopdf::Error::from(e)))
                        .and_then(|d| d.get(b"DA").map_err(|e| lopdf::Error::from(e)))
                })
                .ok()
                .and_then(|o| o.as_str().ok())
                .map(|b| String::from_utf8_lossy(b).into_owned())
                .unwrap_or_else(|| "/Helv 11 Tf 0 g".to_string());

            // Parse font name and size from DA (e.g., "/Helv 12 Tf 0 g")
            let da_parts: Vec<&str> = da.split_whitespace().collect();
            let mut font_name_da = "/Helv";
            let mut font_size: f64 = 11.0;
            for i in 0..da_parts.len() {
                if da_parts[i] == "Tf" && i >= 2 {
                    font_name_da = da_parts[i - 2];
                    font_size = da_parts[i - 1].parse().unwrap_or(11.0);
                }
            }
            // If font size is 0 (auto-size), pick a reasonable size based on field height
            if font_size < 1.0 {
                font_size = (h * 0.6).min(14.0).max(8.0);
            }

            // Rebuild DA with corrected font size
            let da_fixed = format!("{font_name_da} {font_size:.1} Tf 0 g");

            // Build appearance stream content
            let baseline_y = (h - font_size) / 2.0 + 2.0;
            let content = format!(
                "/Tx BMC q 0 0 {w:.2} {h:.2} re W n BT {da_fixed} {font_size:.1} {baseline_y:.2} Td ({escaped}) Tj ET Q EMC"
            );

            // Create Form XObject
            let mut ap_dict = Dictionary::from_iter(vec![
                ("Type", Object::Name(b"XObject".to_vec())),
                ("Subtype", Object::Name(b"Form".to_vec())),
                ("FormType", Object::Integer(1)),
                ("BBox", Object::Array(vec![
                    Object::Real(0.0),
                    Object::Real(0.0),
                    Object::Real(w as f32),
                    Object::Real(h as f32),
                ])),
            ]);

            // Copy resources from the widget or field if available
            let resources_ref = dict.get(b"DR")
                .or_else(|_| dict.get(b"Resources"))
                .or_else(|_| {
                    self.doc.get_object(field_obj_id)
                        .and_then(|o| o.as_dict().map_err(|e| lopdf::Error::from(e)))
                        .and_then(|d| d.get(b"DR").map_err(|e| lopdf::Error::from(e)))
                })
                .ok()
                .cloned();
            if let Some(resources) = resources_ref {
                ap_dict.set("Resources", resources);
            }

            let ap_stream = Stream::new(ap_dict, content.into_bytes());
            let ap_stream_id = self.doc.add_object(ap_stream);

            // Create /AP dict: << /N stream_ref >>
            let ap_obj = Dictionary::from_iter(vec![
                ("N", Object::Reference(ap_stream_id)),
            ]);

            // Set /AP on the widget
            let widget = self.doc.get_object_mut(widget_id)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let widget_dict = widget.as_dict_mut()
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            widget_dict.set("AP", Object::Dictionary(ap_obj));
        }

        Ok(())
    }

    /// Set /NeedAppearances true on the /AcroForm dict.
    fn set_need_appearances(&mut self) -> Result<()> {
        use lopdf::Object;

        let catalog = self.doc.catalog()
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let acroform_ref = match catalog.get(b"AcroForm") {
            Ok(Object::Reference(id)) => Some(*id),
            _ => None,
        };

        if let Some(af_id) = acroform_ref {
            let af_obj = self.doc.get_object_mut(af_id)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let af_dict = af_obj.as_dict_mut()
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            af_dict.set("NeedAppearances", Object::Boolean(true));
        }
        // If AcroForm is inline in the catalog we can't easily get a mutable ref
        // through catalog(), but this is rare. The field value is still set.

        Ok(())
    }

    #[cfg(test)]
    /// Get document byte count (for testing).
    pub fn byte_count(&self) -> usize {
        self.raw_bytes.as_ref().map(|b| b.len()).unwrap_or(0)
    }

    /// Extract comments/annotations from a page (0-indexed).
    pub fn extract_comments(&self, page_num: usize) -> Vec<Comment> {
        let pages = self.doc.get_pages();
        let page_id = match pages.get(&(page_num as u32 + 1)) {
            Some(&id) => id,
            None => return Vec::new(),
        };

        let page_height = self.page_dimensions()
            .get(page_num)
            .map(|&(_, h)| h)
            .unwrap_or(792.0);

        let annots = match self.doc.get_page_annotations(page_id) {
            Ok(a) => a,
            Err(_) => return Vec::new(),
        };

        let mut comments = Vec::new();
        for dict in &annots {
            // Filter to annotation types that carry user comments
            let subtype = dict.get(b"Subtype")
                .ok()
                .and_then(|o| o.as_name().ok())
                .unwrap_or(b"");
            if !matches!(subtype, b"Text" | b"FreeText" | b"Highlight" | b"StrikeOut" | b"Underline" | b"Squiggly") {
                continue;
            }

            let text: String = dict.get(b"Contents")
                .ok()
                .and_then(|o| {
                    o.as_str()
                        .ok()
                        .and_then(|b| String::from_utf8(b.to_vec()).ok())
                })
                .unwrap_or_default();

            if text.is_empty() {
                continue;
            }

            // Get vertical position from /Rect [x1, y1, x2, y2]
            let y_fraction = dict.get(b"Rect")
                .ok()
                .and_then(|o| o.as_array().ok())
                .and_then(|arr| {
                    if arr.len() >= 4 {
                        let y1 = arr[1].as_float().unwrap_or(0.0) as f64;
                        let y2 = arr[3].as_float().unwrap_or(0.0) as f64;
                        let mid_y = (y1 + y2) / 2.0;
                        Some((1.0 - mid_y / page_height).clamp(0.0, 1.0) as f32)
                    } else {
                        None
                    }
                })
                .unwrap_or(0.0);

            comments.push(Comment { text, y_fraction });
        }

        comments
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn embed_signature_and_reload() {
        // Create a small test PNG
        let sig_path = std::env::temp_dir().join("pdft_test_sig.png");
        let img = image::RgbaImage::from_fn(20, 10, |_, _| image::Rgba([0, 0, 255, 180]));
        img.save(&sig_path).unwrap();

        let mut doc = PdfDocument::open(Path::new("tests/fixtures/simple.pdf")).unwrap();
        doc.embed_signature(0, &sig_path, 100.0, 100.0, 50.0).unwrap();
        doc.refresh_bytes().unwrap();

        // Verify the saved file is valid by re-loading it
        let out = std::env::temp_dir().join("pdft_test_signed.pdf");
        doc.clone_lopdf().save(&out).unwrap();
        let reloaded = PdfDocument::open(&out).expect("saved signed PDF should be loadable");
        assert_eq!(reloaded.page_count(), 1);

        // Check that PdftSig XObject exists in the reloaded document
        let doc_copy = reloaded.clone_lopdf();
        let pages = doc_copy.get_pages();
        let &page_id = pages.get(&1).unwrap();
        let (res, _) = doc_copy.get_page_resources(page_id).unwrap();
        let has_sig = res.is_some_and(|r| {
            r.get(b"XObject").ok()
                .and_then(|x| x.as_dict().ok())
                .is_some_and(|d| d.has(b"PdftSig"))
        });
        assert!(has_sig, "page resources should contain PdftSig XObject");

        let _ = std::fs::remove_file(&sig_path);
        let _ = std::fs::remove_file(&out);
    }
}
