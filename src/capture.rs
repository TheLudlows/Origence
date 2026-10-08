//! Evidence validation for capture into one caller-selected identity.
use crate::{
    error::{AppError, Result},
    types::Chunk,
};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Extraction {
    memories: Vec<Span>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Span {
    quote: String,
    byte_start: usize,
    byte_end: usize,
}

/// Exact quotes establish source provenance, not semantic truth. Distinct
/// assertions for one identity require clarification rather than overwriting.
pub fn parse_identified_extraction(source: &str, value: Value) -> Result<Option<Chunk>> {
    let extraction: Extraction = serde_json::from_value(value)
        .map_err(|_| AppError::Invalid("invalid identified extraction schema".into()))?;
    if extraction.memories.len() > 20 {
        return Err(AppError::Invalid("too many identified statements".into()));
    }
    let mut result: Option<Chunk> = None;
    for span in extraction.memories {
        crate::parsing::validate_text(&span.quote)?;
        if source.get(span.byte_start..span.byte_end) != Some(span.quote.as_str()) {
            return Err(AppError::Invalid(
                "extracted quote does not match source span".into(),
            ));
        }
        if let Some(previous) = &result {
            if previous.content != span.quote {
                return Err(AppError::Invalid(
                    "multiple distinct statements for one memory identity".into(),
                ));
            }
        } else {
            result = Some(Chunk {
                content: span.quote,
                locator: json!({"byte_start":span.byte_start,"byte_end":span.byte_end,"source_span":true}),
            });
        }
    }
    Ok(result)
}

pub fn evidence_chunks(source: &str, statement: Chunk) -> Result<Vec<Chunk>> {
    let start = statement.locator["byte_start"].as_u64()
        .ok_or_else(|| AppError::Invalid("missing source offset".into()))? as usize;
    let end = statement.locator["byte_end"].as_u64()
        .ok_or_else(|| AppError::Invalid("missing source offset".into()))? as usize;
    if source.get(start..end) != Some(statement.content.as_str()) {
        return Err(AppError::Invalid("invalid statement source span".into()));
    }
    let mut chunks = crate::parsing::chunks(&statement.content, "text")?;
    for chunk in &mut chunks {
        let first = start + chunk.locator["byte_start"].as_u64().unwrap() as usize;
        let last = start + chunk.locator["byte_end"].as_u64().unwrap() as usize;
        chunk.locator = json!({
            "byte_start":first,"byte_end":last,"source_span":true,
            "line_start":source[..first].matches('\n').count()+1,
            "line_end":source[..last].matches('\n').count()+1,
        });
    }
    Ok(chunks)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quotes_preserve_utf8_source_offsets() {
        let source = "说明：生产发布需要审批。";
        let quote = "生产发布需要审批";
        let start = source.find(quote).unwrap();
        let chunk = parse_identified_extraction(
            source,
            json!({"memories":[{"quote":quote,"byte_start":start,"byte_end":start+quote.len()}]}),
        )
        .unwrap()
        .unwrap();
        assert_eq!(chunk.content, quote);
        assert_eq!(chunk.locator["byte_start"], start);
    }
    #[test]
    fn empty_extraction_has_no_memory() {
        assert!(
            parse_identified_extraction("source", json!({"memories":[]}))
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn repeated_quote_is_one_statement() {
        let span = json!({"quote":"fact","byte_start":0,"byte_end":4});
        assert!(
            parse_identified_extraction("fact", json!({"memories":[span,span]}))
                .unwrap()
                .is_some()
        );
    }
    #[test]
    fn invalid_boundaries_and_fabricated_quotes_are_rejected() {
        for (start, end, quote) in [(1, 3, "中"), (0, 99, "中"), (3, 0, "中"), (0, 3, "假")] {
            assert!(
                parse_identified_extraction(
                    "中文",
                    json!({"memories":[{"quote":quote,"byte_start":start,"byte_end":end}]})
                )
                .is_err()
            );
        }
    }
    #[test]
    fn distinct_assertions_are_not_last_write_wins() {
        assert!(parse_identified_extraction("yes no", json!({"memories":[{"quote":"yes","byte_start":0,"byte_end":3},{"quote":"no","byte_start":4,"byte_end":6}]})).is_err());
    }
    #[test]
    fn model_cannot_supply_identity_or_scope() {
        assert!(parse_identified_extraction("fact", json!({"memories":[],"identity":{}})).is_err());
        assert!(parse_identified_extraction("fact", json!({"memories":[{"quote":"fact","byte_start":0,"byte_end":4,"workspace_id":"other"}]})).is_err());
    }
    #[test]
    fn oversized_batch_is_rejected() {
        let spans = vec![json!({"quote":"fact","byte_start":0,"byte_end":4}); 21];
        assert!(parse_identified_extraction("fact", json!({"memories":spans})).is_err());
    }
    #[test]
    fn long_quote_chunks_keep_original_source_positions() {
        let quote = "审批".repeat(2000);
        let source = format!("前言\n{quote}");
        let start = source.find(&quote).unwrap();
        let statement = parse_identified_extraction(&source, json!({"memories":[{"quote":quote,"byte_start":start,"byte_end":source.len()}]})).unwrap().unwrap();
        let chunks = evidence_chunks(&source, statement).unwrap();
        assert!(chunks.len() > 1);
        for chunk in chunks {
            let start = chunk.locator["byte_start"].as_u64().unwrap() as usize;
            let end = chunk.locator["byte_end"].as_u64().unwrap() as usize;
            assert_eq!(&source[start..end], chunk.content);
            assert_eq!(chunk.locator["line_start"], 2);
        }
    }

}
