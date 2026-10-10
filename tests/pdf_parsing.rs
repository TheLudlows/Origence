use origence::{error::AppError, parsing, types::Chunk};
use std::path::Path;

fn assemble(objects: &[String]) -> Vec<u8> {
    let mut pdf = "%PDF-1.4\n".to_owned();
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf += &format!("{} 0 obj\n{object}\nendobj\n", i + 1);
    }
    let xref = pdf.len();
    pdf += &format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1);
    for offset in offsets {
        pdf += &format!("{offset:010} 00000 n \n");
    }
    pdf += &format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        objects.len() + 1
    );
    pdf.into_bytes()
}

fn stream(content: &str) -> String {
    format!(
        "<< /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    )
}

// Small, deterministic PDFs with real xref tables; no external fixture tools.
pub(super) fn text_pdf(pages: &[&str]) -> Vec<u8> {
    let streams: Vec<_> = pages
        .iter()
        .map(|text| {
            let escaped = text
                .replace('\\', "\\\\")
                .replace('(', "\\(")
                .replace(')', "\\)");
            format!("BT /F1 12 Tf 72 720 Td ({escaped}) Tj ET")
        })
        .collect();
    content_pdf(&streams)
}

fn content_pdf(pages: &[String]) -> Vec<u8> {
    let kids = (0..pages.len())
        .map(|i| format!("{} 0 R", 4 + i * 2))
        .collect::<Vec<_>>()
        .join(" ");
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        format!("<< /Type /Pages /Kids [{kids}] /Count {} >>", pages.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
    ];
    for (i, content) in pages.iter().enumerate() {
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>",
            5 + i * 2
        ));
        objects.push(stream(content));
    }
    assemble(&objects)
}

async fn parse(dir: &Path, bytes: &[u8]) -> Result<Vec<Chunk>, AppError> {
    let path = dir.join("evidence.pdf");
    tokio::fs::write(&path, bytes).await.unwrap();
    parsing::parse_file(&path, "pdf").await
}

fn assert_ranges(chunks: &[Chunk], page: u64, expected: &str) {
    let parts: Vec<_> = chunks
        .iter()
        .filter(|c| c.locator["page"] == page)
        .collect();
    assert!(!parts.is_empty());
    let text: String = parts.iter().map(|c| c.content.as_str()).collect();
    assert_eq!(text.trim(), expected.trim());
    let mut end = 0;
    for chunk in parts {
        let a = chunk.locator["byte_start"].as_u64().unwrap() as usize;
        let b = chunk.locator["byte_end"].as_u64().unwrap() as usize;
        assert_eq!(a, end);
        assert_eq!(chunk.content, text[a..b]);
        assert!(chunk.content.len() <= 2400);
        assert_eq!(chunk.locator["parser"], "pdf-oxide-0.3.78-v1");
        end = b;
    }
    assert_eq!(end, text.len());
}

#[tokio::test]
async fn pdf_library_api_preserves_pages_and_chunk_ranges() {
    let dir = tempfile::tempdir().unwrap();
    let first = "Release approval evidence ".repeat(130);
    let second = "Second page evidence (with parentheses)";
    let chunks = parse(dir.path(), &text_pdf(&[&first, second]))
        .await
        .unwrap();
    assert!(chunks.len() >= 3);
    assert_ranges(&chunks, 1, &first);
    assert_ranges(&chunks, 2, second);
}

#[tokio::test]
async fn pdf_chinese_tounicode_preserves_utf8_ranges() {
    let dir = tempfile::tempdir().unwrap();
    let chinese = "生产发布需要审批";
    let mappings: String = chinese
        .chars()
        .enumerate()
        .map(|(i, c)| format!("<{:04X}> <{:04X}>\n", i + 1, c as u32))
        .collect();
    let cmap = format!(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n8 beginbfchar\n{mappings}endbfchar\nendcmap\nCMapName currentdict /CMap defineresource pop\nend\nend"
    );
    let encoded = "00010002000300040005000600070008".repeat(150);
    let pdf = assemble(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 7 0 R >>".into(),
        "<< /Type /Font /Subtype /Type0 /BaseFont /TestChinese /Encoding /Identity-H /DescendantFonts [5 0 R] /ToUnicode 6 0 R >>".into(),
        "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /TestChinese /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /DW 1000 /CIDToGIDMap /Identity >>".into(),
        stream(&cmap),
        stream(&format!("BT /F1 12 Tf 72 720 Td <{encoded}> Tj ET")),
    ]);
    let chunks = parse(dir.path(), &pdf).await.unwrap();
    assert!(chunks.len() > 1);
    assert_ranges(&chunks, 1, &chinese.repeat(150));
}

#[tokio::test]
async fn pdf_invalid_input_is_not_a_retryable_internal_error() {
    let dir = tempfile::tempdir().unwrap();
    for bytes in [b"not a PDF".as_slice(), b"%PDF-1.4\ntruncated".as_slice()] {
        assert!(matches!(
            parse(dir.path(), bytes).await,
            Err(AppError::Invalid(_))
        ));
    }
}

#[tokio::test]
async fn pdf_unusable_later_page_rejects_the_whole_document() {
    let dir = tempfile::tempdir().unwrap();
    let error = parse(dir.path(), &text_pdf(&["Usable evidence", ""]))
        .await
        .unwrap_err();
    assert!(matches!(error, AppError::Invalid(_)));
    assert!(error.to_string().contains("page 2 has no usable text"));
}

#[tokio::test]
async fn pdf_operator_truncation_does_not_publish_partial_text() {
    let dir = tempfile::tempdir().unwrap();
    // pdf_oxide caps streams at 1,000,000 operators. Valid text precedes that
    // cap, so checking only nonempty text would silently accept a partial page.
    let content = "BT /F1 12 Tf 72 720 Td (Usable evidence) Tj ET\n".to_owned()
        + &"0 0 m\n".repeat(1_000_001);
    let error = parse(dir.path(), &content_pdf(&[content]))
        .await
        .unwrap_err();
    assert!(matches!(error, AppError::Invalid(_)));
    assert_eq!(error.to_string(), "PDF extraction was incomplete on page 1");
}

#[tokio::test]
async fn pdf_page_count_limits_include_the_boundary() {
    let dir = tempfile::tempdir().unwrap();
    for pages in [0, 201] {
        let bytes = text_pdf(&vec!["Evidence"; pages]);
        let error = parse(dir.path(), &bytes).await.unwrap_err();
        assert!(matches!(error, AppError::Invalid(_)));
        assert!(error.to_string().contains("page limit"));
    }
    let chunks = parse(dir.path(), &text_pdf(&vec!["Evidence"; 200]))
        .await
        .unwrap();
    assert_eq!(chunks.last().unwrap().locator["page"], 200);
}

#[tokio::test]
async fn pdf_file_size_is_checked_at_the_library_boundary() {
    let dir = tempfile::tempdir().unwrap();
    let error = parse(dir.path(), &vec![b' '; parsing::MAX_FILE + 1])
        .await
        .unwrap_err();
    assert!(matches!(error, AppError::Invalid(_)));
    assert_eq!(error.to_string(), "file too large");
}

#[tokio::test]
async fn pdf_extracted_text_limit_is_document_wide() {
    let dir = tempfile::tempdir().unwrap();
    let page = "Release approval evidence ".repeat(400);
    let error = parse(dir.path(), &text_pdf(&vec![page.as_str(); 100]))
        .await
        .unwrap_err();
    assert!(matches!(error, AppError::Invalid(_)));
    assert_eq!(error.to_string(), "PDF extracted text limit exceeded");
}

#[tokio::test]
async fn parsing_distinguishes_io_errors_and_unsupported_formats() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing.pdf");
    assert!(matches!(
        parsing::parse_file(&missing, "pdf").await,
        Err(AppError::Internal(_))
    ));
    assert!(matches!(
        parsing::parse_file(&missing, "unknown").await,
        Err(AppError::Invalid(_))
    ));
    let path = dir.path().join("text.txt");
    tokio::fs::write(&path, "生产发布需要审批").await.unwrap();
    let chunks = parsing::parse_file(&path, "text").await.unwrap();
    assert_eq!(chunks[0].content, "生产发布需要审批");
    tokio::fs::write(&path, [0xff]).await.unwrap();
    assert!(matches!(
        parsing::parse_file(&path, "text").await,
        Err(AppError::Invalid(_))
    ));
}
