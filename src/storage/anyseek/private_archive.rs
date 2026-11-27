//! Private archive collections have no route through workspace asset retrieval.
use super::*;
use crate::session::{
    ArchiveDomain, ArchiveLocator, ArchivedEvidence, ArchivedTurnBody, SessionTurnInput,
};

const PRIVATE_FORMAT: &str = "origence-private-archive-v1";
const NATIVE_FORMAT: &str = "origence-private-archive-v2";
const PRIVATE_FIELDS: &[&str] = &["archive", "kind"];
const NATIVE_FIELDS: &[&str] = &["archive", "kind", "session", "turn", "source", "sequence"];
const RECORD_BYTES: usize = 1024 * 1024;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NativeArchiveTurn {
    pub(crate) session: Uuid,
    pub(crate) turn: Uuid,
    pub(crate) source: Uuid,
    pub(crate) created_by: Uuid,
    pub(crate) created_at: i64,
    pub(crate) sequence: i64,
    pub(crate) request_hash: String,
    pub(crate) evidence: Vec<ArchivedEvidence>,
    pub(crate) body: ArchivedTurnBody,
}

impl NativeArchiveTurn {
    fn validate(&self) -> StorageResult<()> {
        let input = SessionTurnInput {
            turn_id: self.turn,
            question: self.body.question.clone(),
            answer: self.body.answer.clone(),
            used_evidence: self
                .evidence
                .iter()
                .map(|item| item.reference.clone())
                .collect(),
            evidence_use: self.body.evidence_use,
        };
        if self.session.is_nil()
            || self.source.is_nil()
            || self.created_by.is_nil()
            || self.created_at <= 0
            || !(1..i64::MAX).contains(&self.sequence)
            || input.validate().is_err()
            || self.evidence.iter().any(|item| item.validate().is_err())
            || self.request_hash
                != hash(json!({"session_id":self.session,"input":input}).to_string())
        {
            return Err(invalid_archive());
        }
        Ok(())
    }
}

fn native_collection(domain: ArchiveDomain) -> String {
    let digest = hash(format!("{NATIVE_FORMAT}:{}", domain.collection()));
    format!("pr_{}", &digest[..60])
}

fn archive_group(session: Uuid, turn: Uuid) -> String {
    format!("t{}", hash(format!("{session}:{turn}")))
}

fn archive_digest(domain: ArchiveDomain, payload: &Value) -> String {
    hash(
        json!([
            domain.scope.tenant_id,
            domain.scope.workspace_id,
            domain.owner,
            payload
        ])
        .to_string(),
    )
}

#[derive(Clone, Copy)]
pub(crate) enum ArchiveQuery<'a> {
    Keyword(&'a str),
    Vector(&'a [f32]),
}

impl ArchiveQuery<'_> {
    fn request(self, filters: Vec<Value>, fields: &[&str], limit: usize) -> Value {
        let mut request = json!({"filter":filters,"fields":fields,"length":limit,"count":true});
        match self {
            Self::Keyword(terms) => {
                request["query"] = json!(terms);
                request["operator"] = json!("and");
                request["search_fields"] = json!(["terms"]);
            }
            Self::Vector(vector) => {
                request["query_vector"] = json!(vector);
                request["all"] = json!(true);
            }
        }
        request
    }
}

struct ArchiveHits {
    hits: Vec<Value>,
    count: usize,
    visible_seq: u64,
}

impl ArchiveHits {
    fn pair(&self, group: &str) -> StorageResult<(&Value, &Value)> {
        if self.count != 2 || self.hits.len() != 2 {
            return Err(invalid_archive());
        }
        let find = |id: &str, kind: &str| {
            self.hits
                .iter()
                .map(|hit| &hit["record"])
                .find(|record| archive_member(record, id, group, kind))
                .ok_or_else(invalid_archive)
        };
        Ok((find(group, "body")?, find(&format!("{group}s"), "search")?))
    }
}

fn invalid_archive() -> StorageError {
    StorageError::Unavailable("private archive contents failed integrity verification".into())
}

fn archive_records(group: &str, body: Value, search: Value) -> StorageResult<Value> {
    for document in [&body, &search] {
        if serde_json::to_vec(document)
            .map_err(|_| invalid_archive())?
            .len()
            > RECORD_BYTES
        {
            return Err(invalid("private archive record exceeds 1 MiB"));
        }
    }
    Ok(json!([
        {"record_id":group,"document":body},
        {"record_id":format!("{group}s"),"document":search}
    ]))
}

fn archive_member(record: &Value, id: &str, group: &str, kind: &str) -> bool {
    record["record_id"] == id
        && record["version"] == 1
        && record["document"]["kind"] == kind
        && record["document"]["archive"] == group
}

fn receipt_sequence(
    result: &Value,
    group: &str,
    epoch: u64,
    nonce: &str,
    version: u64,
    records: usize,
) -> Option<u64> {
    let receipt = &result["receipt"];
    if result["atomic"] != true
        || result["operation_id"]["epoch"] != epoch
        || result["operation_id"]["nonce"] != nonce
        || receipt["group_id"] != group
        || receipt["group_version"] != version
        || receipt["record_count"] != records
    {
        return None;
    }
    receipt["seq"].as_u64().filter(|seq| *seq > 0)
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

    async fn archive_request(
        &self,
        domain: ArchiveDomain,
        collection: &str,
        request: Request,
    ) -> StorageResult<(StatusCode, Value)> {
        self.request_collection(Some(domain.scope), Some(collection), request, &mut None)
            .await
    }

    async fn archive_query(
        &self,
        domain: ArchiveDomain,
        collection: &str,
        query: Value,
    ) -> StorageResult<ArchiveHits> {
        let (status, mut value) = self
            .archive_request(domain, collection, Request::Query(query))
            .await?;
        if status == StatusCode::TOO_MANY_REQUESTS {
            return Err(transient());
        }
        if !status.is_success() {
            return Err(unavailable());
        }
        let visible_seq = value["visible_seq"].as_u64().ok_or_else(invalid_archive)?;
        let count = value["count_total"]
            .as_u64()
            .and_then(|count| usize::try_from(count).ok())
            .ok_or_else(invalid_archive)?;
        let Value::Array(hits) = value["hits"].take() else {
            return Err(invalid_archive());
        };
        if count < hits.len() || (count > 0 && visible_seq == 0) {
            return Err(invalid_archive());
        }
        Ok(ArchiveHits {
            hits,
            count,
            visible_seq,
        })
    }

    fn private_config(&self, native: bool) -> Value {
        let name = if native {
            NATIVE_FORMAT
        } else {
            PRIVATE_FORMAT
        };
        let mut config = json!({
            "index_name":name,"tokenizer":"TantivySimpleV1",
            "text_normalization":"None","stop_words":"None",
            "schema":[
                {"field":"archive","field_type":"String","store":true,"index_lexical":false,"index_vector":false,"filterable":true,"boost":1.0},
                {"field":"kind","field_type":"String","store":true,"index_lexical":false,"index_vector":false,"filterable":true,"boost":1.0},
                {"field":"body","field_type":"Json","store":true,"index_lexical":false,"index_vector":false,"filterable":false,"boost":1.0},
                {"field":"terms","field_type":"Text","store":false,"index_lexical":true,"index_vector":false,"filterable":false,"boost":1.0}
            ]
        });
        if native {
            let schema = config["schema"].as_array_mut().unwrap();
            for (field, field_type) in [
                ("session", "String"),
                ("turn", "String"),
                ("source", "String"),
                ("sequence", "Int"),
            ] {
                schema.push(json!({
                    "field":field,"field_type":field_type,"store":true,
                    "index_lexical":false,"index_vector":false,"filterable":true,"boost":1.0
                }));
            }
            for (field, field_type) in [("digest", "String"), ("metadata", "Json")] {
                schema.push(json!({
                    "field":field,"field_type":field_type,"store":true,
                    "index_lexical":false,"index_vector":false,"filterable":false,"boost":1.0
                }));
            }
        }
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
        self.archive_collection(domain, &domain.collection(), false, create)
            .await
    }

    pub(crate) async fn native_archive_collection(
        &self,
        domain: ArchiveDomain,
        create: bool,
    ) -> StorageResult<bool> {
        // The pinned engine admits only one collection recovery operation per
        // collection. Concurrent first writers recheck after the initializer.
        let _provisioning = if create {
            Some(self.native_archive_provisioning.lock().await)
        } else {
            None
        };
        self.archive_collection(domain, &native_collection(domain), true, create)
            .await
    }

    async fn archive_collection(
        &self,
        domain: ArchiveDomain,
        id: &str,
        native: bool,
        create: bool,
    ) -> StorageResult<bool> {
        let (mut status, mut info) = self.archive_request(domain, id, Request::Info).await?;
        let absent = if status == StatusCode::FORBIDDEN {
            let (listed, collections) = self.archive_request(domain, id, Request::List).await?;
            if !listed.is_success() {
                return Err(unavailable());
            }
            let collections = collections.as_array().ok_or_else(unavailable)?;
            if collections.iter().any(|item| !item["id"].is_string()) {
                return Err(unavailable());
            }
            if collections.iter().any(|item| item["id"] == id) {
                // Another request may have provisioned it after the first Info.
                (status, info) = self.archive_request(domain, id, Request::Info).await?;
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
        let config = self.private_config(native);
        let profile = self.embedding.as_ref().map(|(p, _)| p);
        if absent {
            let epoch = operation_epoch()?;
            let nonce = format!(
                "private-init:{}",
                hash(json!([id, &config, profile]).to_string())
            );
            let create_result = self
                .archive_request(
                    domain,
                    id,
                    Request::Create(json!({
                        "operation_id":{"epoch":epoch,"nonce":nonce},"collection_id":id,
                        "config":config,"embedding_space_id":profile
                    })),
                )
                .await;
            (status, info) = self.archive_request(domain, id, Request::Info).await?;
            if !status.is_success() {
                create_result?;
            }
        }
        if !status.is_success() {
            return Err(unavailable());
        }
        if info["schema"] != config["schema"]
            || info["name"] != config["index_name"]
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

    async fn archive_version(
        &self,
        domain: ArchiveDomain,
        collection: &str,
        group: &str,
    ) -> StorageResult<Option<u64>> {
        let (status, value) = self
            .archive_request(domain, collection, Request::GroupVersion(group.to_owned()))
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
        let collection = locator.domain.collection();
        let group = locator.group();
        let version = self
            .archive_version(locator.domain, &collection, &group)
            .await?;
        if version != Some(1) {
            return if pending {
                Ok(None)
            } else {
                Err(invalid_archive())
            };
        }
        let result = self
            .archive_query(
                locator.domain,
                &collection,
                json!({
                    "query":"","filter":[filter("archive",[group.clone()])],
                    "fields":["archive","kind","body"],"length":3,"count":true
                }),
            )
            .await?;
        if result.count == 0 && pending {
            return Ok(None);
        }
        let (record, _) = result.pair(&group)?;
        let body: ArchivedTurnBody = serde_json::from_value(record["document"]["body"].clone())
            .map_err(|_| invalid_archive())?;
        if body.digest() != locator.body_hash {
            return Err(invalid_archive());
        }
        Ok(Some((body, result.visible_seq)))
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
        let collection = locator.domain.collection();
        let group = locator.group();
        let canonical = json!({"archive":group,"kind":"body","body":body});
        let mut projection = json!({"archive":group,"kind":"search","terms":terms});
        if let Some(vector) = embedding {
            projection["vector"] = json!(vector);
        }
        let records = archive_records(&group, canonical, projection)?;
        let nonce = format!("private-archive:{group}");
        let request = |epoch| {
            Request::ReplaceGroup(json!({
                "operation_id":{"epoch":epoch,"nonce":nonce},"group_id":group,
                "expected_group_version":0,"records":records
            }))
        };
        let mut epoch = locator.epoch;
        let mut response = self
            .archive_request(locator.domain, &collection, request(epoch))
            .await;
        if matches!(response, Ok((StatusCode::GONE, _)))
            && self
                .archive_version(locator.domain, &collection, &group)
                .await?
                .is_none()
        {
            // AnySeek retains operation receipts for the current/previous UTC
            // day. An absent group at CAS=0 allows a fresh epoch for the same
            // logical operation and immutable Record IDs.
            epoch = operation_epoch()?;
            response = self
                .archive_request(locator.domain, &collection, request(epoch))
                .await;
        }
        if let Ok((status, result)) = &response
            && status.is_success()
            && let Some(seq) = receipt_sequence(result, &group, epoch, &nonce, 1, 2)
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
        let collection = locator.domain.collection();
        let group = locator.group();
        for _ in 0..3 {
            let version = self
                .archive_version(locator.domain, &collection, &group)
                .await?
                .unwrap_or(0);
            if version > 0 {
                let result = self
                    .archive_query(
                        locator.domain,
                        &collection,
                        json!({
                            "query":"","filter":[filter("archive",[group.clone()])],
                            "fields":PRIVATE_FIELDS,"length":1,"count":true
                        }),
                    )
                    .await?;
                if result.count == 0 {
                    return Ok(());
                }
            }
            if self
                .replace_withdrawn_archive(locator.domain, &collection, &group, version, Vec::new())
                .await?
            {
                return Ok(());
            }
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
        let request = query.request(
            vec![
                filter("kind", ["search".to_owned()]),
                filter("archive", groups.iter().cloned()),
            ],
            PRIVATE_FIELDS,
            limit,
        );
        let ArchiveHits { hits, count, .. } = self
            .archive_query(domain, &domain.collection(), request)
            .await?;
        if hits.len() > limit {
            return Err(invalid_archive());
        }
        let mut seen = HashSet::new();
        let mut out = Vec::with_capacity(hits.len());
        for hit in &hits {
            let record = &hit["record"];
            let group = record["document"]["archive"]
                .as_str()
                .ok_or_else(invalid_archive)?;
            if !groups.iter().any(|id| id == group)
                || !archive_member(record, &format!("{group}s"), group, "search")
                || !seen.insert(group)
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

    async fn native_version(
        &self,
        domain: ArchiveDomain,
        collection: &str,
        group: &str,
    ) -> StorageResult<Option<u64>> {
        let version = self.archive_version(domain, collection, group).await?;
        // Embedded group lookup returns 404 for both an absent group and a
        // missing collection. Verify the collection before reporting absence.
        if version.is_none() {
            self.require_native_collection(domain).await?;
        }
        Ok(version)
    }

    async fn require_native_collection(&self, domain: ArchiveDomain) -> StorageResult<()> {
        if !self.native_archive_collection(domain, false).await? {
            return Err(StorageError::Unavailable(
                "private archive collection unavailable".into(),
            ));
        }
        Ok(())
    }

    pub(crate) async fn native_archive_read(
        &self,
        domain: ArchiveDomain,
        session: Uuid,
        turn: Uuid,
    ) -> StorageResult<Option<NativeArchiveTurn>> {
        self.read_native_archive(domain, session, turn, false).await
    }

    /// An index hit proves the group once existed. Withdrawals keep a group
    /// witness, including when the first write was fenced before it committed.
    pub(crate) async fn native_archive_read_candidate(
        &self,
        domain: ArchiveDomain,
        session: Uuid,
        turn: Uuid,
    ) -> StorageResult<Option<NativeArchiveTurn>> {
        self.read_native_archive(domain, session, turn, true).await
    }

    async fn read_native_archive(
        &self,
        domain: ArchiveDomain,
        session: Uuid,
        turn: Uuid,
        seen_in_index: bool,
    ) -> StorageResult<Option<NativeArchiveTurn>> {
        let collection = native_collection(domain);
        let group = archive_group(session, turn);
        let Some(version) = self.native_version(domain, &collection, &group).await? else {
            return if seen_in_index {
                Err(invalid_archive())
            } else {
                Ok(None)
            };
        };
        if !(1..=2).contains(&version) {
            return Err(invalid_archive());
        }
        let result = self.archive_query(
            domain,
            &collection,
            json!({
                "query":"","filter":[filter("archive",[group.clone()])],"fields":[
                    "archive","kind","session","turn","source","sequence","body","digest","metadata"
                ],"length":3,"count":true
            }),
        ).await?;
        let current = self.native_version(domain, &collection, &group).await?;
        if version == 1 && current == Some(2) {
            return Ok(None);
        }
        if current != Some(version) {
            return Err(invalid_archive());
        }
        if version == 2 && result.count == 0 {
            return Ok(None);
        }
        if version != 1 {
            return Err(invalid_archive());
        }
        let identity = (session, turn);
        if result.count == 1 && result.hits.len() == 1 {
            let record = &result.hits[0]["record"];
            if archive_member(record, &group, &group, "withdrawn")
                && native_identity(record)? == identity
            {
                return Ok(None);
            }
        }
        let (canonical, search) = result.pair(&group)?;
        if native_identity(canonical)? != identity || native_identity(search)? != identity {
            return Err(invalid_archive());
        }
        let document = &canonical["document"];
        let mut payload = search["document"]["metadata"].clone();
        let metadata = payload.as_object_mut().ok_or_else(invalid_archive)?;
        if metadata
            .insert("body".into(), document["body"].clone())
            .is_some()
        {
            return Err(invalid_archive());
        }
        let digest = archive_digest(domain, &payload);
        let archive: NativeArchiveTurn =
            serde_json::from_value(payload).map_err(|_| invalid_archive())?;
        archive.validate()?;
        if archive.session != session
            || archive.turn != turn
            || document["source"] != archive.source.to_string()
            || document["sequence"] != archive.sequence
            || document["digest"] != digest
        {
            return Err(invalid_archive());
        }
        Ok(Some(archive))
    }

    pub(crate) async fn native_archive_source(
        &self,
        domain: ArchiveDomain,
        session: Uuid,
        source: Uuid,
    ) -> StorageResult<Option<NativeArchiveTurn>> {
        let ArchiveHits { hits, count, .. } = self
            .archive_query(
                domain,
                &native_collection(domain),
                json!({
                    "query":"","fields":NATIVE_FIELDS,"length":2,"count":true,
                    "filter":[filter("kind",["body".into()]),filter("source",[source.to_string()])]
                }),
            )
            .await?;
        if count == 0 {
            return Ok(None);
        }
        if count != 1 || hits.len() != 1 {
            return Err(invalid_archive());
        }
        let (found_session, turn, found_source, sequence) = native_directory_hit(&hits[0])?;
        if found_session != session || found_source != source {
            return Err(invalid_archive());
        }
        let found = self
            .native_archive_read_candidate(domain, session, turn)
            .await?;
        if found
            .as_ref()
            .is_some_and(|archive| archive.source != source || archive.sequence != sequence)
        {
            return Err(invalid_archive());
        }
        Ok(found)
    }

    pub(crate) async fn native_archive_write(
        &self,
        domain: ArchiveDomain,
        archive: &NativeArchiveTurn,
        terms: String,
        embedding: Option<Vec<f32>>,
    ) -> StorageResult<NativeArchiveTurn> {
        archive.validate()?;
        let group = archive_group(archive.session, archive.turn);
        let mut metadata = json!(archive);
        let digest = archive_digest(domain, &metadata);
        let body = metadata
            .as_object_mut()
            .and_then(|fields| fields.remove("body"))
            .ok_or_else(invalid_archive)?;
        let canonical = json!({
            "archive": group, "kind": "body", "session": archive.session, "turn": archive.turn,
            "source": archive.source, "sequence": archive.sequence, "body": body,
            "digest": digest
        });
        let mut projection = json!({
            "archive": group, "kind": "search", "session": archive.session, "turn": archive.turn,
            "metadata": metadata, "terms": terms
        });
        if let Some(vector) = embedding {
            projection["vector"] = json!(vector);
        }
        let records = archive_records(&group, canonical, projection)?;
        let epoch = operation_epoch()?;
        let nonce = format!("private-archive:{group}");
        let response = self
            .archive_request(
                domain,
                &native_collection(domain),
                Request::ReplaceGroup(json!({
                    "operation_id":{"epoch":epoch,"nonce":nonce},"group_id":group,
                    "expected_group_version":0,"records":records
                })),
            )
            .await;
        if let Some(live) = self
            .native_archive_read(domain, archive.session, archive.turn)
            .await?
        {
            if live.request_hash != archive.request_hash {
                return Err(StorageError::Conflict(
                    "session turn identity reused with different input".into(),
                ));
            }
            return Ok(live);
        }
        let (status, value) = response?;
        if status == StatusCode::CONFLICT
            || status.is_success()
                && receipt_sequence(&value, &group, epoch, &nonce, 1, 2).is_some()
        {
            return Err(StorageError::NotFound);
        }
        Err(transient())
    }

    pub(crate) async fn native_archive_page(
        &self,
        domain: ArchiveDomain,
        session: Uuid,
        after: i64,
        limit: usize,
    ) -> StorageResult<(Option<i64>, Vec<NativeArchiveTurn>)> {
        if after >= i64::MAX - 1 {
            self.require_native_collection(domain).await?;
            return Ok((None, Vec::new()));
        }
        let ArchiveHits { hits, count, .. } = self.archive_query(domain, &native_collection(domain), json!({
            "query":"","fields":NATIVE_FIELDS,"length":limit,"count":true,
            "filter":[filter("kind",["body".into()]),filter("session",[session.to_string()]),
                {"Int":{"field":"sequence","filter":{"start":after + 1,"end":i64::MAX}}}],
            "sort":[{"field":"sequence","order":"Ascending"}]
        })).await?;
        if hits.len() != count.min(limit) {
            return Err(invalid_archive());
        }
        let mut next = None;
        let mut turns = Vec::with_capacity(hits.len());
        for hit in &hits {
            let (found_session, turn, source, sequence) = native_directory_hit(hit)?;
            if found_session != session || sequence <= next.unwrap_or(after) {
                return Err(invalid_archive());
            }
            next = Some(sequence);
            if let Some(archive) = self
                .native_archive_read_candidate(domain, session, turn)
                .await?
            {
                if archive.source != source || archive.sequence != sequence {
                    return Err(invalid_archive());
                }
                turns.push(archive);
            }
        }
        Ok((next, turns))
    }

    pub(crate) async fn native_archive_search(
        &self,
        domain: ArchiveDomain,
        query: ArchiveQuery<'_>,
        limit: usize,
    ) -> StorageResult<(usize, Vec<(Uuid, f64)>)> {
        let request = query.request(
            vec![filter("kind", ["search".to_owned()])],
            NATIVE_FIELDS,
            limit,
        );
        let ArchiveHits { hits, count, .. } = self
            .archive_query(domain, &native_collection(domain), request)
            .await?;
        if hits.len() > limit {
            return Err(invalid_archive());
        }
        let mut seen = HashSet::new();
        let mut out = Vec::with_capacity(hits.len());
        for hit in hits {
            let record = &hit["record"];
            let (session, turn) = native_identity(record)?;
            let group = archive_group(session, turn);
            if !archive_member(record, &format!("{group}s"), &group, "search") || !seen.insert(turn)
            {
                return Err(invalid_archive());
            }
            let score = hit["score"]
                .as_f64()
                .filter(|score| score.is_finite())
                .ok_or_else(invalid_archive)?;
            out.push((turn, score));
        }
        Ok((count, out))
    }

    /// A replaced group retains its CAS version even with no members. An
    /// explicit marker distinguishes a fenced late first write from corruption.
    pub(crate) async fn native_archive_delete(
        &self,
        domain: ArchiveDomain,
        session: Uuid,
        turn: Uuid,
    ) -> StorageResult<()> {
        let collection = native_collection(domain);
        let group = archive_group(session, turn);
        for _ in 0..3 {
            let version = self
                .native_version(domain, &collection, &group)
                .await?
                .unwrap_or(0);
            if version == 2 {
                return Ok(());
            }
            if version > 2 {
                return Err(invalid_archive());
            }
            let records = if version == 0 {
                vec![
                    json!({"record_id":group,"document":{"archive":group,"kind":"withdrawn","session":session,"turn":turn}}),
                ]
            } else {
                Vec::new()
            };
            if self
                .replace_withdrawn_archive(domain, &collection, &group, version, records)
                .await?
            {
                return Ok(());
            }
        }
        Err(transient())
    }

    // False means the group changed between inspection and the CAS.
    async fn replace_withdrawn_archive(
        &self,
        domain: ArchiveDomain,
        collection: &str,
        group: &str,
        version: u64,
        records: Vec<Value>,
    ) -> StorageResult<bool> {
        let next = version.checked_add(1).ok_or_else(invalid_archive)?;
        let count = records.len();
        let epoch = operation_epoch()?;
        let nonce = format!("private-delete:{group}:{version}");
        let (status, value) = self
            .archive_request(
                domain,
                collection,
                Request::ReplaceGroup(json!({
                    "operation_id":{"epoch":epoch,"nonce":nonce},"group_id":group,
                    "expected_group_version":version,"records":records
                })),
            )
            .await?;
        if status == StatusCode::CONFLICT {
            return Ok(false);
        }
        if status.is_success()
            && receipt_sequence(&value, group, epoch, &nonce, next, count).is_some()
        {
            return Ok(true);
        }
        Err(transient())
    }
}

fn native_identity(record: &Value) -> StorageResult<(Uuid, Uuid)> {
    let document = &record["document"];
    Ok((
        native_uuid(&document["session"])?,
        native_uuid(&document["turn"])?,
    ))
}

fn native_uuid(value: &Value) -> StorageResult<Uuid> {
    value
        .as_str()
        .and_then(|id| Uuid::parse_str(id).ok())
        .filter(|id| !id.is_nil())
        .ok_or_else(invalid_archive)
}

fn native_directory_hit(hit: &Value) -> StorageResult<(Uuid, Uuid, Uuid, i64)> {
    let record = &hit["record"];
    let (session, turn) = native_identity(record)?;
    let group = archive_group(session, turn);
    if !archive_member(record, &group, &group, "body") {
        return Err(invalid_archive());
    }
    let document = &record["document"];
    let source = native_uuid(&document["source"])?;
    let sequence = document["sequence"]
        .as_i64()
        .filter(|sequence| (1..i64::MAX).contains(sequence))
        .ok_or_else(invalid_archive)?;
    Ok((session, turn, source, sequence))
}

#[cfg(all(test, feature = "embedded-anyseek"))]
mod tests {
    use super::*;

    fn turn(session: Uuid, turn: Uuid, sequence: i64, text: &str) -> NativeArchiveTurn {
        let input = SessionTurnInput {
            turn_id: turn,
            question: text.into(),
            answer: "fixture reply".into(),
            used_evidence: Vec::new(),
            evidence_use: Default::default(),
        };
        NativeArchiveTurn {
            session,
            turn,
            source: Uuid::new_v4(),
            created_by: Uuid::new_v4(),
            created_at: 1,
            sequence,
            request_hash: hash(json!({"session_id":session,"input":input}).to_string()),
            body: ArchivedTurnBody::from_input(&input),
            evidence: Vec::new(),
        }
    }

    #[tokio::test]
    async fn native_archive_orders_recovers_and_fences_groups() {
        let directory = tempfile::tempdir().unwrap();
        let store = AnySeekFullTextStore::open_configured(
            AnySeekConfig::embedded(),
            &directory.path().join("native"),
            true,
        )
        .await
        .unwrap();
        let domain = ArchiveDomain {
            scope: Scope {
                tenant_id: Uuid::new_v4(),
                workspace_id: Uuid::new_v4(),
            },
            owner: Uuid::new_v4(),
        };
        let session = Uuid::new_v4();
        let first = turn(session, Uuid::new_v4(), 2, "alpha");
        let second = turn(session, Uuid::new_v4(), 9, "beta");
        assert!(
            !store
                .native_archive_collection(domain, false)
                .await
                .unwrap()
        );
        assert!(matches!(
            store.native_archive_read(domain, session, first.turn).await,
            Err(StorageError::Unavailable(_))
        ));
        let create = || store.native_archive_collection(domain, true);
        let (a, b, c, d, e) = tokio::join!(create(), create(), create(), create(), create());
        for result in [a, b, c, d, e] {
            assert!(result.unwrap());
        }
        assert!(
            store
                .native_archive_read(domain, session, first.turn)
                .await
                .unwrap()
                .is_none()
        );
        assert!(matches!(
            store
                .native_archive_read_candidate(domain, session, first.turn)
                .await,
            Err(StorageError::Unavailable(_))
        ));
        store
            .native_archive_write(domain, &second, "beta".into(), None)
            .await
            .unwrap();
        store
            .native_archive_write(domain, &first, "alpha".into(), None)
            .await
            .unwrap();

        let (next, rows) = store
            .native_archive_page(domain, session, 0, 1)
            .await
            .unwrap();
        assert_eq!(next, Some(first.sequence));
        assert_eq!(rows[0].turn, first.turn);
        let (next, rows) = store
            .native_archive_page(domain, session, next.unwrap(), 1)
            .await
            .unwrap();
        assert_eq!(next, Some(second.sequence));
        assert_eq!(rows[0].turn, second.turn);

        let retry = turn(session, first.turn, 11, "alpha");
        let recovered = store
            .native_archive_write(domain, &retry, "alpha".into(), None)
            .await
            .unwrap();
        assert_eq!(recovered.source, first.source);
        assert_eq!(recovered.sequence, first.sequence);
        let changed = turn(session, first.turn, 12, "changed");
        assert!(matches!(
            store
                .native_archive_write(domain, &changed, "changed".into(), None)
                .await,
            Err(StorageError::Conflict(_))
        ));
        assert_eq!(
            store
                .native_archive_source(domain, session, second.source)
                .await
                .unwrap()
                .unwrap()
                .turn,
            second.turn
        );
        let (_, hits) = store
            .native_archive_search(domain, ArchiveQuery::Keyword("beta"), 5)
            .await
            .unwrap();
        assert_eq!(hits[0].0, second.turn);

        store
            .native_archive_delete(domain, session, first.turn)
            .await
            .unwrap();
        assert!(
            store
                .native_archive_read_candidate(domain, session, first.turn)
                .await
                .unwrap()
                .is_none()
        );
        let late = turn(session, Uuid::new_v4(), 13, "late");
        store
            .native_archive_delete(domain, session, late.turn)
            .await
            .unwrap();
        for archive in [&first, &late] {
            assert!(matches!(
                store
                    .native_archive_write(domain, archive, archive.body.question.clone(), None)
                    .await,
                Err(StorageError::NotFound)
            ));
        }
        store.shutdown().await.unwrap();
    }
}
