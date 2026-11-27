//! Private archive collections have no route through workspace asset retrieval.
use super::*;
use crate::session::ArchivedTurnBody;

const PRIVATE_FORMAT: &str = "origence-private-archive-v1";
const PRIVATE_FIELDS: &[&str] = &["archive", "kind"];
const RECORD_BYTES: usize = 1024 * 1024;

use crate::session::{ArchiveDomain, ArchiveLocator};

#[derive(Clone, Copy)]
pub(crate) enum ArchiveQuery<'a> {
    Keyword(&'a str),
    Vector(&'a [f32]),
}

fn invalid_archive() -> StorageError {
    StorageError::Unavailable("private archive contents failed integrity verification".into())
}

impl AnySeekFullTextStore {
    pub(crate) async fn archive_backend_version(&self) -> StorageResult<String> {
        let (status, value) = self.request_inner(None, Request::Live, &mut None).await?;
        if !status.is_success() {
            return Err(unavailable());
        }
        value
            .as_str()
            .or_else(|| value["version"].as_str())
            .map(str::to_owned)
            .ok_or_else(unavailable)
    }

    async fn private_request(
        &self,
        domain: ArchiveDomain,
        request: Request,
    ) -> StorageResult<(StatusCode, Value)> {
        self.request_collection(
            Some(domain.scope),
            Some(&domain.collection()),
            request,
            &mut None,
        )
        .await
    }

    fn private_config(&self) -> Value {
        let mut config = json!({
            "index_name":PRIVATE_FORMAT,"tokenizer":"TantivySimpleV1",
            "text_normalization":"None","stop_words":"None",
            "schema":[
                {"field":"archive","field_type":"String","store":true,"index_lexical":false,"index_vector":false,"filterable":true,"boost":1.0},
                {"field":"kind","field_type":"String","store":true,"index_lexical":false,"index_vector":false,"filterable":true,"boost":1.0},
                {"field":"body","field_type":"Json","store":true,"index_lexical":false,"index_vector":false,"filterable":false,"boost":1.0},
                {"field":"terms","field_type":"Text","store":false,"index_lexical":true,"index_vector":false,"filterable":false,"boost":1.0}
            ]
        });
        if let Some((_, dimension)) = &self.embedding {
            config["vector"] = json!({"dimensions":dimension});
            config["schema"].as_array_mut().unwrap().push(json!({
                "field":"vector","field_type":"Json","store":false,
                "index_lexical":false,"index_vector":true,"filterable":false,"boost":1.0
            }));
        }
        config
    }

    pub(crate) async fn private_collection(
        &self,
        domain: ArchiveDomain,
        create: bool,
    ) -> StorageResult<bool> {
        let id = domain.collection();
        let (mut status, mut info) = self.private_request(domain, Request::Info).await?;
        let absent = if status == StatusCode::FORBIDDEN {
            let (listed, collections) = self.private_request(domain, Request::List).await?;
            if !listed.is_success() {
                return Err(unavailable());
            }
            let collections = collections.as_array().ok_or_else(unavailable)?;
            if collections.iter().any(|item| !item["id"].is_string()) {
                return Err(unavailable());
            }
            if collections.iter().any(|item| item["id"] == id) {
                // Another request may have provisioned it after the first Info.
                (status, info) = self.private_request(domain, Request::Info).await?;
                false
            } else {
                true
            }
        } else {
            status == StatusCode::NOT_FOUND
        };
        if absent && !create {
            return Ok(false);
        }
        let config = self.private_config();
        let profile = self.embedding.as_ref().map(|(p, _)| p);
        if absent {
            let epoch = operation_epoch()?;
            let nonce = format!(
                "private-init:{}",
                hash(json!([&id, &config, profile]).to_string())
            );
            let create_result = self
                .private_request(
                    domain,
                    Request::Create(json!({
                        "operation_id":{"epoch":epoch,"nonce":nonce},"collection_id":id,
                        "config":config,"embedding_space_id":profile
                    })),
                )
                .await;
            (status, info) = self.private_request(domain, Request::Info).await?;
            if !status.is_success() {
                create_result?;
            }
        }
        if !status.is_success() {
            return Err(unavailable());
        }
        if info["schema"] != config["schema"]
            || info["name"] != PRIVATE_FORMAT
            || info["text_normalization"] != "None"
            || info["analysis"]["tokenizer"] != "TantivySimpleV1"
            || info["analysis"]["stop_words"] != "None"
            || info["embedding_space_id"] != json!(profile)
            || info["vector_dimensions"] != config["vector"]["dimensions"]
        {
            return Err(invalid_archive());
        }
        Ok(true)
    }

    async fn private_version(&self, locator: &ArchiveLocator) -> StorageResult<Option<u64>> {
        let group = locator.group();
        let (status, value) = self
            .private_request(locator.domain, Request::GroupVersion(group.clone()))
            .await?;
        if status == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !status.is_success() || value["group_id"] != group {
            return Err(unavailable());
        }
        value["group_version"]
            .as_u64()
            .filter(|v| *v > 0)
            .map(Some)
            .ok_or_else(invalid_archive)
    }

    /// Group version 1 is the only state eligible for delivery. A withdrawal
    /// from an empty group also creates version 1, but has no canonical record.
    pub(crate) async fn archive_read(
        &self,
        locator: &ArchiveLocator,
        pending: bool,
    ) -> StorageResult<Option<(ArchivedTurnBody, u64)>> {
        if !self.private_collection(locator.domain, false).await? {
            return if pending {
                Ok(None)
            } else {
                Err(invalid_archive())
            };
        }
        let version = self.private_version(locator).await?;
        if version != Some(1) {
            return if pending {
                Ok(None)
            } else {
                Err(invalid_archive())
            };
        }
        let group = locator.group();
        let (status, value) = self
            .private_request(
                locator.domain,
                Request::Query(json!({
                    "query":"","filter":[filter("archive",[group.clone()])],
                    "fields":["archive","kind","body"],"length":3,"count":true
                })),
            )
            .await?;
        if !status.is_success() {
            return Err(unavailable());
        }
        let hits = value["hits"].as_array().ok_or_else(invalid_archive)?;
        if hits.is_empty() && value["count_total"] == 0 && pending {
            return Ok(None);
        }
        if hits.len() != 2 || value["count_total"] != 2 {
            return Err(invalid_archive());
        }
        let search_id = format!("{group}s");
        if !hits.iter().any(|hit| {
            let record = &hit["record"];
            record["record_id"] == search_id
                && record["document"]["archive"] == group
                && record["document"]["kind"] == "search"
        }) {
            return Err(invalid_archive());
        }
        let record = hits
            .iter()
            .map(|hit| &hit["record"])
            .find(|record| record["record_id"] == group)
            .ok_or_else(invalid_archive)?;
        if record["record_id"] != group
            || record["document"]["archive"] != group
            || record["document"]["kind"] != "body"
        {
            return Err(invalid_archive());
        }
        let body: ArchivedTurnBody = serde_json::from_value(record["document"]["body"].clone())
            .map_err(|_| invalid_archive())?;
        if body.digest() != locator.body_hash {
            return Err(invalid_archive());
        }
        let seq = value["visible_seq"]
            .as_u64()
            .filter(|s| *s > 0)
            .ok_or_else(invalid_archive)?;
        Ok(Some((body, seq)))
    }

    /// Ordinary writes always require version zero. Cleanup uses the same CAS
    /// even for a never-written pending row, fencing writes accepted late.
    pub(crate) async fn archive_write(
        &self,
        locator: &ArchiveLocator,
        body: &ArchivedTurnBody,
        terms: String,
        embedding: Option<Vec<f32>>,
    ) -> StorageResult<u64> {
        self.private_collection(locator.domain, true).await?;
        let group = locator.group();
        let canonical = json!({"archive":group,"kind":"body","body":body});
        let mut projection = json!({"archive":group,"kind":"search","terms":terms});
        if let Some(vector) = embedding {
            projection["vector"] = json!(vector);
        }
        if serde_json::to_vec(&canonical)
            .map_err(|_| invalid_archive())?
            .len()
            > RECORD_BYTES
            || serde_json::to_vec(&projection)
                .map_err(|_| invalid_archive())?
                .len()
                > RECORD_BYTES
        {
            return Err(invalid("private archive record exceeds 1 MiB"));
        }
        let records = json!([
            {"record_id":group,"document":canonical},
            {"record_id":format!("{group}s"),"document":projection}
        ]);
        let nonce = format!("private-archive:{group}");
        let request = |epoch| {
            Request::ReplaceGroup(json!({
                "operation_id":{"epoch":epoch,"nonce":nonce},"group_id":group,
                "expected_group_version":0,"records":records
            }))
        };
        let mut epoch = locator.epoch;
        let mut response = self.private_request(locator.domain, request(epoch)).await;
        if matches!(response, Ok((StatusCode::GONE, _)))
            && self.private_version(locator).await?.is_none()
        {
            // AnySeek retains operation receipts for the current/previous UTC
            // day. An absent group at CAS=0 allows a fresh epoch for the same
            // logical operation and immutable Record IDs.
            epoch = operation_epoch()?;
            response = self.private_request(locator.domain, request(epoch)).await;
        }
        if let Ok((status, result)) = &response
            && status.is_success()
            && result["atomic"] == true
            && result["operation_id"]["epoch"] == epoch
            && result["operation_id"]["nonce"] == nonce
            && result["receipt"]["group_id"] == group
            && result["receipt"]["group_version"] == 1
            && result["receipt"]["record_count"] == 2
            && let Some(seq) = result["receipt"]["seq"].as_u64().filter(|s| *s > 0)
        {
            return Ok(seq);
        }
        if let Some((_, seq)) = self.archive_read(locator, true).await? {
            return Ok(seq);
        }
        response?;
        Err(transient())
    }

    pub(crate) async fn archive_delete(&self, locator: &ArchiveLocator) -> StorageResult<()> {
        self.private_collection(locator.domain, true).await?;
        let group = locator.group();
        for _ in 0..3 {
            let version = self.private_version(locator).await?.unwrap_or(0);
            if version > 0 {
                let (status, result) = self
                    .private_request(
                        locator.domain,
                        Request::Query(json!({
                            "query":"","filter":[filter("archive",[group.clone()])],
                            "fields":PRIVATE_FIELDS,"length":1,"count":true
                        })),
                    )
                    .await?;
                if !status.is_success() || result["count_total"].as_u64().is_none() {
                    return Err(unavailable());
                }
                if result["count_total"] == 0 {
                    return Ok(());
                }
            }
            let epoch = operation_epoch()?;
            let nonce = format!("private-delete:{group}:{version}");
            let (status, result) = self
                .private_request(
                    locator.domain,
                    Request::ReplaceGroup(json!({
                        "operation_id":{"epoch":epoch,"nonce":nonce},"group_id":group,
                        "expected_group_version":version,"records":[]
                    })),
                )
                .await?;
            if status == StatusCode::CONFLICT {
                continue;
            }
            if status.is_success()
                && result["atomic"] == true
                && result["operation_id"]["epoch"] == epoch
                && result["operation_id"]["nonce"] == nonce
                && result["receipt"]["group_id"] == group
                && result["receipt"]["group_version"] == version + 1
                && result["receipt"]["record_count"] == 0
            {
                return Ok(());
            }
            return Err(transient());
        }
        Err(transient())
    }

    pub(crate) async fn archive_search(
        &self,
        domain: ArchiveDomain,
        query: ArchiveQuery<'_>,
        groups: &[String],
        limit: usize,
    ) -> StorageResult<(usize, Vec<(String, f64)>)> {
        let mut body = json!({
            "filter":[filter("kind",["search".to_owned()]),filter("archive",groups.iter().cloned())],
            "fields":PRIVATE_FIELDS,"length":limit,"count":true
        });
        match query {
            ArchiveQuery::Keyword(terms) => {
                body["query"] = json!(terms);
                body["operator"] = json!("and");
                body["search_fields"] = json!(["terms"]);
            }
            ArchiveQuery::Vector(vector) => {
                body["query_vector"] = json!(vector);
                body["all"] = json!(true);
            }
        }
        let (status, value) = self.private_request(domain, Request::Query(body)).await?;
        if !status.is_success() {
            return Err(unavailable());
        }
        let count = value["count_total"].as_u64().ok_or_else(unavailable)? as usize;
        let hits = value["hits"].as_array().ok_or_else(unavailable)?;
        if hits.len() > limit {
            return Err(invalid_archive());
        }
        let mut out = Vec::with_capacity(hits.len());
        for hit in hits {
            let record = &hit["record"];
            let group = record["document"]["archive"]
                .as_str()
                .ok_or_else(invalid_archive)?;
            if group.len() != 65
                || !group.starts_with('t')
                || !group[1..].bytes().all(|b| b.is_ascii_hexdigit())
                || !groups.iter().any(|id| id == group)
                || record["record_id"] != format!("{group}s")
                || record["document"]["kind"] != "search"
            {
                return Err(invalid_archive());
            }
            let score = hit["score"]
                .as_f64()
                .filter(|s| s.is_finite())
                .ok_or_else(invalid_archive)?;
            out.push((group.to_owned(), score));
        }
        Ok((count, out))
    }
}
