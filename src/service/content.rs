//! Immutable AnySeek content reads. All network IO occurs between authority
//! snapshots; callers recheck visibility before publishing or returning bytes.
use super::*;

impl Service {
    pub(crate) async fn read_chunk_content(
        &self,
        scope: Scope,
        references: &[ChunkRef],
    ) -> Result<Vec<Chunk>> {
        let ids: Vec<_> = references.iter().map(|reference| reference.id).collect();
        let payloads = self.engine.vector().read_payloads(scope, &ids).await?;
        references
            .iter()
            .map(|reference| {
                let payload = payloads
                    .get(&reference.id)
                    .ok_or_else(|| AppError::Unavailable("document content unavailable".into()))?;
                if hash(&payload.content) != reference.content_hash
                    || payload.locator != reference.locator
                {
                    return Err(AppError::Unavailable(
                        "document content does not match its publication".into(),
                    ));
                }
                Ok(Chunk {
                    content: payload.content.clone(),
                    locator: payload.locator.clone(),
                })
            })
            .collect()
    }

    pub(crate) async fn hydrate_hits(&self, scope: Scope, hits: &mut [SearchHit]) -> Result<()> {
        let references: Vec<_> = hits
            .iter()
            .map(|hit| ChunkRef {
                id: hit.chunk_id,
                content_hash: hit.content_hash.clone(),
                locator: hit.locator.clone(),
            })
            .collect();
        for (hit, chunk) in hits
            .iter_mut()
            .zip(self.read_chunk_content(scope, &references).await?)
        {
            hit.content = chunk.content;
            hit.locator = chunk.locator;
        }
        Ok(())
    }
}
