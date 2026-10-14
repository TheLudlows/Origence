use crate::{
    error::{AppError, Result},
    types::Chunk,
};
use pdf_oxide::{
    PdfDocument,
    extractors::warnings::{WarningCategory, drain_global_warnings},
};
use serde_json::json;
use std::{io::Read, path::Path, sync::LazyLock};
static JIEBA: LazyLock<jieba_rs::Jieba> = LazyLock::new(jieba_rs::Jieba::new);
pub const MAX_TEXT: usize = 1_000_000;
pub const MAX_FILE: usize = 10 * 1024 * 1024;
const MAX_PDF_PAGES: usize = 200;
const PDF_PARSER: &str = "pdf-oxide-0.3.78-v1";

pub fn validate_text(text: &str) -> Result<()> {
    if text.trim().is_empty() || text.len() > MAX_TEXT || text.contains('\0') {
        return Err(AppError::Invalid(
            "text must be nonempty UTF-8, without NUL, and at most 1 MB".into(),
        ));
    }
    Ok(())
}

pub fn lexical(text: &str) -> String {
    JIEBA
        .cut_for_search(text, false)
        .into_iter()
        .filter(|s| s.chars().any(char::is_alphanumeric))
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn chunks(text: &str, format: &str) -> Result<Vec<Chunk>> {
    validate_text(text)?;
    if !["text", "markdown"].contains(&format) {
        return Err(AppError::Invalid("unsupported text format".into()));
    }
    let mut out = Vec::new();
    if format == "markdown" {
        // Preserve original Markdown ranges, including code and headings, rather than losing evidence offsets.
        let mut start = 0;
        let mut end = 0;
        for (_, range) in pulldown_cmark::Parser::new(text).into_offset_iter() {
            if range.end.saturating_sub(start) > 2400 && end > start {
                split_range(text, start, end, &mut out);
                start = end;
            }
            end = end.max(range.end);
        }
        if start < text.len() {
            split_range(text, start, text.len(), &mut out);
        }
    } else {
        split_range(text, 0, text.len(), &mut out);
    }
    if out.is_empty() {
        return Err(AppError::Invalid("no indexable text".into()));
    }
    Ok(out)
}

fn split_range(text: &str, start: usize, end: usize, out: &mut Vec<Chunk>) {
    let mut pos = start;
    while pos < end {
        let mut next = (pos + 2400).min(end);
        while !text.is_char_boundary(next) {
            next -= 1;
        }
        let value = &text[pos..next];
        out.push(Chunk { content: value.to_owned(), locator: json!({"byte_start":pos,"byte_end":next,"line_start":text[..pos].bytes().filter(|b| *b==b'\n').count()+1,"line_end":text[..next].bytes().filter(|b| *b==b'\n').count()+1,"parser":"source-ranges-v1"}) });
        pos = next;
    }
}

fn parse_pdf(path: &Path) -> Result<Vec<Chunk>> {
    // Bound the actual read rather than trusting metadata (the file could grow).
    let file = std::fs::File::open(path).map_err(anyhow::Error::from)?;
    let mut bytes = Vec::new();
    file.take((MAX_FILE + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(anyhow::Error::from)?;
    if bytes.len() > MAX_FILE {
        return Err(AppError::Invalid("file too large".into()));
    }
    // Free-function diagnostics are thread-local; a previous failed open on a
    // reused blocking thread must not contaminate this document's diagnostics.
    drain_global_warnings();
    let doc = PdfDocument::from_bytes(bytes)
        .map_err(|_| AppError::Invalid("PDF unsupported or invalid".into()))?;
    if doc.is_encrypted() {
        return Err(AppError::Invalid("encrypted PDF is unsupported".into()));
    }
    let count = doc
        .page_count()
        .map_err(|_| AppError::Invalid("PDF page tree is invalid".into()))?;
    if !(1..=MAX_PDF_PAGES).contains(&count) {
        return Err(AppError::Invalid(
            "PDF page limit exceeded or empty PDF".into(),
        ));
    }
    let mut result = Vec::new();
    let mut total = 0;
    for page in 0..count {
        let text = doc.extract_text(page).map_err(|_| {
            AppError::Invalid(format!("PDF text extraction failed on page {}", page + 1))
        })?;
        if doc.take_structured_warnings().iter().any(|warning| {
            matches!(
                warning.category,
                WarningCategory::OperatorCapExceeded | WarningCategory::EofPremature
            )
        }) {
            return Err(AppError::Invalid(format!(
                "PDF extraction was incomplete on page {}",
                page + 1
            )));
        }
        if text.chars().filter(|c| c.is_alphanumeric()).take(3).count() < 3 {
            return Err(AppError::Invalid(format!(
                "PDF page {} has no usable text; OCR is unsupported",
                page + 1
            )));
        }
        total += text.len();
        if total > MAX_TEXT {
            return Err(AppError::Invalid(
                "PDF extracted text limit exceeded".into(),
            ));
        }
        for mut chunk in chunks(&text, "text")? {
            chunk.locator["page"] = json!(page + 1);
            chunk.locator["parser"] = json!(PDF_PARSER);
            result.push(chunk);
        }
    }
    Ok(result)
}

pub async fn parse_file(path: &Path, format: &str) -> Result<Vec<Chunk>> {
    if !["text", "markdown", "pdf"].contains(&format) {
        return Err(AppError::Invalid("unsupported file format".into()));
    }
    if format != "pdf" {
        let bytes = tokio::fs::read(path).await.map_err(anyhow::Error::from)?;
        let text = String::from_utf8(bytes)
            .map_err(|_| AppError::Invalid("file must use UTF-8".into()))?;
        return chunks(&text, format);
    }
    let path = path.to_owned();
    // Await the blocking task to completion: dropping/timing out its future
    // cannot stop synchronous parsing. The durable worker already serializes jobs.
    tokio::task::spawn_blocking(move || parse_pdf(&path))
        .await
        .map_err(|e| AppError::Internal(e.into()))?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn utf8_ranges_and_chinese_terms() {
        let text = "# 决策\n".to_owned() + &"生产发布需要审批。\n".repeat(500);
        let parts = chunks(&text, "markdown").unwrap();
        assert!(parts.len() > 2);
        assert_eq!(
            parts.iter().map(|c| c.content.as_str()).collect::<String>(),
            text
        );
        for c in parts {
            let a = c.locator["byte_start"].as_u64().unwrap() as usize;
            let b = c.locator["byte_end"].as_u64().unwrap() as usize;
            assert_eq!(c.content, text[a..b]);
        }
        assert!(lexical("生产发布需要审批").contains("审批"));
    }
    #[test]
    fn rejects_empty_and_nul() {
        assert!(chunks("  ", "text").is_err());
        assert!(chunks("a\0b", "text").is_err());
    }
}
