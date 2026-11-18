//! Authorized page previews for an exact archived Run citation.
use crate::{
    error::{AppError, Result},
    parsing::{self, PdfPageSpan},
    service::Service,
    session::ArchivedEvidence,
    storage::{DomainTx, Permission, hash},
    types::AuthContext,
};
use serde::Serialize;
use serde_json::Value;
use tokio::{io::AsyncReadExt, sync::Semaphore};
use uuid::Uuid;

// A dropped HTTP request cannot stop a blocking parser. The permit remains with
// the task until it exits, so retrying does not create an unbounded parser queue.
static PREVIEWS: Semaphore = Semaphore::const_new(2);

#[derive(Serialize)]
pub struct RunPdfPreview {
    pub asset_version: i32,
    pub page: Option<usize>,
    pub span: Option<PdfPageSpan>,
}

#[derive(PartialEq)]
struct Snapshot {
    reference: ArchivedEvidence,
    file_hash: String,
    file_id: Uuid,
    chunk_hash: Option<String>,
}
impl Snapshot {
    fn page(&self) -> Option<usize> {
        let page = usize::try_from(self.reference.locator["page"].as_u64()?).ok()?;
        (1..=parsing::MAX_PDF_PAGES).contains(&page).then_some(page)
    }
}

impl Service {
    async fn run_pdf_snapshot(
        &self,
        a: &AuthContext,
        run: Uuid,
        sequence: i64,
        evidence: Uuid,
    ) -> Result<Snapshot> {
        if !(1..=10000).contains(&sequence) || evidence.is_nil() {
            return Err(AppError::Invalid("invalid Run citation".into()));
        }
        // Match both Run ownership/trace guards and the existing original-file
        // permission. The caller supplies neither an asset version nor a locator.
        let mut tx = self.read(a, Permission::Write).await?;
        let trace = tx.private_run_trace(run, sequence - 1, 1).await?;
        let event = &trace["events"][0];
        if event["sequence"].as_i64() != Some(sequence) {
            return Err(AppError::NotFound);
        }
        let references: Vec<ArchivedEvidence> = serde_json::from_value(event["references"].clone())
            .map_err(|_| AppError::Unavailable("invalid archived Run citation".into()))?;
        let reference = references
            .into_iter()
            .find(|r| r.evidence_ref_id == evidence)
            .ok_or(AppError::NotFound)?;
        let locator = &reference.locator;
        if locator["page"].is_null()
            && !locator["parser"]
                .as_str()
                .is_some_and(|p| p.starts_with("pdf-"))
        {
            return Err(AppError::NotFound);
        }
        let source = tx.event_view(reference.source_event_id).await?;
        let file_id = serde_json::from_value::<Option<Uuid>>(source["file_id"].clone())
            .map_err(|_| AppError::Unavailable("invalid archived file identity".into()))?
            .ok_or(AppError::NotFound)?;
        let file_hash = tx.file_hash(file_id).await?.ok_or(AppError::NotFound)?;
        let archived = &reference.reference;
        let chunk_hash = tx
            .chunk_refs_for_version(archived.asset_id, archived.asset_version)
            .await?
            .into_iter()
            .find(|chunk| chunk.id == archived.chunk_id && chunk.locator == *locator)
            .map(|chunk| chunk.content_hash);
        tx.commit().await?;
        Ok(Snapshot {
            reference,
            file_hash,
            file_id,
            chunk_hash,
        })
    }

    async fn run_pdf_bytes(&self, a: &AuthContext, snapshot: &Snapshot) -> Result<Vec<u8>> {
        let key = Self::blob_key(a, &snapshot.file_hash);
        let path = self.engine.blobs().path_for(&key)?;
        let file = tokio::fs::File::open(path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                AppError::NotFound
            } else {
                AppError::Unavailable("original PDF is unavailable".into())
            }
        })?;
        let mut bytes = Vec::new();
        file.take((parsing::MAX_FILE + 1) as u64)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| AppError::Unavailable("original PDF is unavailable".into()))?;
        if bytes.len() > parsing::MAX_FILE || hash(&bytes) != snapshot.file_hash {
            return Err(AppError::Unavailable(
                "stored file integrity check failed".into(),
            ));
        }
        if !bytes.starts_with(b"%PDF-") {
            return Err(AppError::Invalid("original file has no PDF header".into()));
        }
        Ok(bytes)
    }

    async fn check_run_pdf_snapshot(
        &self,
        a: &AuthContext,
        run: Uuid,
        sequence: i64,
        evidence: Uuid,
        snapshot: &Snapshot,
    ) -> Result<()> {
        if self.run_pdf_snapshot(a, run, sequence, evidence).await? != *snapshot {
            return Err(AppError::Conflict("Run PDF source changed".into()));
        }
        Ok(())
    }

    pub async fn run_pdf_preview(
        &self,
        a: &AuthContext,
        run: Uuid,
        sequence: i64,
        evidence: Uuid,
    ) -> Result<RunPdfPreview> {
        let snapshot = self.run_pdf_snapshot(a, run, sequence, evidence).await?;
        let permit = PREVIEWS
            .try_acquire()
            .map_err(|_| AppError::RetryableUnavailable("PDF preview is busy".into()))?;
        let bytes = self.run_pdf_bytes(a, &snapshot).await?;
        let locator: Value = snapshot.reference.locator.clone();
        let chunk_hash = snapshot.chunk_hash.clone();
        let span = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            parsing::pdf_page_span(bytes, &locator, chunk_hash.as_deref()?)
        })
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
        self.check_run_pdf_snapshot(a, run, sequence, evidence, &snapshot)
            .await?;
        Ok(RunPdfPreview {
            asset_version: snapshot.reference.reference.asset_version,
            page: snapshot.page(),
            span,
        })
    }

    pub async fn run_pdf_original(
        &self,
        a: &AuthContext,
        run: Uuid,
        sequence: i64,
        evidence: Uuid,
    ) -> Result<Vec<u8>> {
        let snapshot = self.run_pdf_snapshot(a, run, sequence, evidence).await?;
        let bytes = self.run_pdf_bytes(a, &snapshot).await?;
        self.check_run_pdf_snapshot(a, run, sequence, evidence, &snapshot)
            .await?;
        Ok(bytes)
    }
}

#[cfg(test)]
#[path = "../tests/pdf_fixture.rs"]
mod pdf_fixture;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pdf_page_span_requires_exact_unique_utf8_evidence() {
        let bytes = pdf_fixture::assemble(&[
            "<< /Type /Catalog /Pages 2 0 R >>".into(),
            "<< /Type /Pages /Kids [4 0 R] /Count 1 >>".into(),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".into(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 3 0 R >> >> /Contents 5 0 R >>".into(),
            pdf_fixture::stream(r"BT /F1 12 Tf 72 720 Td (avant Caf\351 puis caf\351) Tj ET"),
        ]);
        let doc = pdf_oxide::PdfDocument::from_bytes(bytes.clone()).unwrap();
        let page = doc.extract_text(0).unwrap();
        let start = page.find("Café").unwrap();
        let mut locator = json!({"parser":"pdf-oxide-0.3.78-v1", "page":1, "byte_start":start, "byte_end":start + 5});
        let span = parsing::pdf_page_span(bytes.clone(), &locator, &hash("Café")).unwrap();
        assert_eq!(span.highlight, "Café");
        assert_eq!(span.before + &span.highlight + &span.after, page);
        assert!(parsing::pdf_page_span(bytes.clone(), &locator, &hash("café")).is_none());
        locator["byte_end"] = json!(start + 4);
        assert!(parsing::pdf_page_span(bytes.clone(), &locator, &hash("Caf")).is_none());
        locator["byte_end"] = json!(start + 5);
        locator["parser"] = json!("unknown-parser");
        assert!(parsing::pdf_page_span(bytes, &locator, &hash("Café")).is_none());

        let repeated = pdf_fixture::text_pdf(&["token token"]);
        let loc = json!({"parser":"pdf-oxide-0.3.78-v1", "page":1, "byte_start":0, "byte_end":5});
        assert!(parsing::pdf_page_span(repeated, &loc, &hash("token")).is_none());
        let raster = pdf_fixture::content_pdf(&[
            "q 10 0 0 10 0 0 cm BI /W 1 /H 1 /BPC 8 /CS /RGB ID abc EI Q".into(),
        ]);
        assert!(parsing::pdf_page_span(raster, &loc, &hash("token")).is_none());
    }
}
