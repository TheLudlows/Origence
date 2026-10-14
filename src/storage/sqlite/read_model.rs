//! Transaction-local retrieval reads. Every SQL statement reads one table;
//! scoped keys are collected and associated in Rust, never cached across calls.
use super::*;
use crate::types::SearchHit;
use std::collections::HashSet;

/// Preserve native key types while associating rows. Decode domain fields only
/// after the same predicates that filtered them in the original statement.
#[derive(Clone)]
pub(super) struct AssociationKey(turso::Value);

impl AssociationKey {
    pub(super) fn from_uuid(value: Uuid) -> Self {
        Self(turso::Value::Blob(value.as_bytes().to_vec()))
    }

    pub(super) fn from_optional_i32(value: Option<i32>) -> Self {
        Self(value.map_or(turso::Value::Null, |value| {
            turso::Value::Integer(i64::from(value))
        }))
    }

    pub(super) fn is_null(&self) -> bool {
        matches!(&self.0, turso::Value::Null)
    }
}

impl native_turso::Decode<'_> for AssociationKey {
    fn decode(value: &turso::Value) -> native_turso::Result<Self> {
        Ok(Self(value.clone()))
    }
}

impl native_turso::Encode for AssociationKey {
    fn encode(self) -> native_turso::Result<turso::Value> {
        Ok(self.0)
    }
}

pub(super) const READ_KEYS: usize = 128;
const VERSION_KEYS: usize = 32;
pub(super) type VersionKey = (Uuid, i32);
pub(super) type NativeHitKey = (Uuid, SourceVersion, i64);

pub(super) fn unique_ids(ids: impl IntoIterator<Item = Uuid>) -> Vec<Uuid> {
    let mut ids: Vec<_> = ids.into_iter().collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// Table/column/select fragments are adapter-owned constants, never caller SQL.
pub(super) async fn rows_by_ids(
    conn: &mut Connection,
    scope: Scope,
    table: &str,
    columns: &str,
    key: &str,
    ids: &[Uuid],
) -> StorageResult<Vec<native_turso::Row>> {
    let ids = unique_ids(ids.iter().copied());
    let mut rows = Vec::new();
    for batch in ids.chunks(READ_KEYS) {
        let sql = format!(
            "SELECT {columns} FROM {table} WHERE tenant_id=? AND workspace_id=? AND {key} IN ({})",
            vec!["?"; batch.len()].join(",")
        );
        let mut query = native_turso::query(&sql)
            .bind(scope.tenant_id)
            .bind(scope.workspace_id);
        for id in batch {
            query = query.bind(*id);
        }
        rows.extend(query.fetch_all(&mut *conn).await.map_err(turso_err)?);
    }
    Ok(rows)
}

async fn version_rows(
    conn: &mut Connection,
    scope: Scope,
    table: &str,
    columns: &str,
    versions: &[VersionKey],
) -> StorageResult<Vec<native_turso::Row>> {
    let mut versions = versions.to_vec();
    versions.sort_unstable();
    versions.dedup();
    let mut rows = Vec::new();
    for batch in versions.chunks(VERSION_KEYS) {
        let sql = format!(
            "SELECT {columns} FROM {table} WHERE tenant_id=? AND workspace_id=? AND ({})",
            vec!["(asset_id=? AND version=?)"; batch.len()].join(" OR ")
        );
        let mut query = native_turso::query(&sql)
            .bind(scope.tenant_id)
            .bind(scope.workspace_id);
        for (asset, version) in batch {
            query = query.bind(*asset).bind(*version);
        }
        rows.extend(query.fetch_all(&mut *conn).await.map_err(turso_err)?);
    }
    Ok(rows)
}

#[cfg(test)]
pub(super) struct CurrentChunk {
    pub hit: SearchHit,
}

/// Current root visibility only. Callers apply local support and Learning as
/// separate batch layers; startup health intentionally only needs this layer.
#[cfg(all(test, feature = "local-storage"))]
pub(super) async fn current_chunks(
    conn: &mut Connection,
    scope: Scope,
    ids: Option<&[Uuid]>,
    asset_filter: Option<Uuid>,
) -> StorageResult<Vec<CurrentChunk>> {
    Ok(current_chunks_measured(conn, scope, ids, asset_filter)
        .await?
        .0)
}

// Counts are lengths of already-read sets, for native regression evidence.
// No extra query or payload clone is performed to obtain them.
#[cfg(all(test, feature = "local-storage"))]
pub(super) async fn current_chunks_measured(
    conn: &mut Connection,
    scope: Scope,
    ids: Option<&[Uuid]>,
    asset_filter: Option<Uuid>,
) -> StorageResult<(Vec<CurrentChunk>, [usize; 4])> {
    let raw = current_rows(conn, scope, ids, asset_filter).await?;
    let mut chunks = Vec::new();
    for index in raw.eligible() {
        let row = &raw.rows[index];
        let position = raw.positions[index];
        let asset_row = &raw.assets[&position];
        let version_row = &raw.versions[&position];
        let asset: Uuid = row.try_get("asset_id").map_err(turso_err)?;
        let version: i32 = row.try_get("version").map_err(turso_err)?;
        let kind: String = asset_row.try_get("kind").map_err(turso_err)?;
        let title: String = version_row.try_get("title").map_err(turso_err)?;
        let source: Uuid = version_row.try_get("source_event_id").map_err(turso_err)?;
        let text: String = row.try_get("locator").map_err(turso_err)?;
        let locator: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        let source_event_id = if locator["parser"] == crate::memory_semantics::PARSER {
            locator["source_event_id"]
                .as_str()
                .and_then(|value| Uuid::parse_str(value).ok())
                .unwrap_or(source)
        } else {
            source
        };
        chunks.push(CurrentChunk {
            hit: SearchHit {
                identity: None,
                chunk_id: row.try_get("id").map_err(turso_err)?,
                asset_id: asset,
                version,
                kind,
                title,
                content: row.try_get("content").map_err(turso_err)?,
                locator,
                source_event_id,
                score: 0.0,
            },
        });
    }
    Ok((chunks, raw.counts))
}

// Domain-local borrowed projections, not a cache or configurable query mode.
// Each payload row has one owner; metadata is shared by integer positions.
pub(super) struct RawCurrentRows {
    rows: Vec<native_turso::Row>,
    assets: HashMap<usize, native_turso::Row>,
    versions: HashMap<usize, native_turso::Row>,
    positions: Vec<usize>,
    #[cfg(all(test, feature = "local-storage"))]
    counts: [usize; 4],
}

impl RawCurrentRows {
    fn eligible(&self) -> Vec<usize> {
        self.positions
            .iter()
            .enumerate()
            .filter_map(|(index, position)| {
                (self.assets.contains_key(position) && self.versions.contains_key(position))
                    .then_some(index)
            })
            .collect()
    }

    pub(super) fn version_key(&self, index: usize) -> StorageResult<VersionKey> {
        let row = &self.rows[index];
        Ok((
            row.try_get("asset_id").map_err(turso_err)?,
            row.try_get("version").map_err(turso_err)?,
        ))
    }

    pub(super) fn search_terms(&self, index: usize) -> StorageResult<Option<String>> {
        self.rows[index].try_get("search_terms").map_err(turso_err)
    }

    pub(super) fn hit(&self, index: usize, score: f64) -> StorageResult<SearchHit> {
        let row = &self.rows[index];
        let position = self.positions[index];
        let text: String = row.try_get("locator").map_err(turso_err)?;
        let locator: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        let override_source = if locator["parser"] == crate::memory_semantics::PARSER {
            locator["source_event_id"]
                .as_str()
                .and_then(|value| Uuid::parse_str(value).ok())
        } else {
            None
        };
        let source_event_id = match override_source {
            Some(source) => source,
            None => self.versions[&position]
                .try_get("source_event_id")
                .map_err(turso_err)?,
        };
        Ok(SearchHit {
            identity: None,
            chunk_id: row.try_get("id").map_err(turso_err)?,
            asset_id: row.try_get("asset_id").map_err(turso_err)?,
            version: row.try_get("version").map_err(turso_err)?,
            kind: self.assets[&position].try_get("kind").map_err(turso_err)?,
            title: self.versions[&position]
                .try_get("title")
                .map_err(turso_err)?,
            content: row.try_get("content").map_err(turso_err)?,
            locator,
            source_event_id,
            score,
        })
    }
}

pub(super) async fn current_rows(
    conn: &mut Connection,
    scope: Scope,
    ids: Option<&[Uuid]>,
    asset_filter: Option<Uuid>,
) -> StorageResult<RawCurrentRows> {
    let columns = "id,asset_id,version,content,locator,search_terms";
    let rows = if let Some(ids) = ids {
        let ids = unique_ids(ids.iter().copied());
        let mut rows = Vec::new();
        for batch in ids.chunks(READ_KEYS) {
            let sql = format!(
                "SELECT {columns} FROM oc_chunks WHERE tenant_id=? AND workspace_id=? AND (? IS NULL OR asset_id=?) AND id IN ({})",
                vec!["?"; batch.len()].join(",")
            );
            let mut query = native_turso::query(&sql)
                .bind(scope.tenant_id)
                .bind(scope.workspace_id)
                .bind(asset_filter)
                .bind(asset_filter);
            for id in batch {
                query = query.bind(*id);
            }
            rows.extend(query.fetch_all(&mut *conn).await.map_err(turso_err)?);
        }
        rows
    } else {
        let mut sql =
            format!("SELECT {columns} FROM oc_chunks WHERE tenant_id=? AND workspace_id=?");
        if asset_filter.is_some() {
            sql.push_str(" AND asset_id=?");
        }
        let mut query = native_turso::query(&sql)
            .bind(scope.tenant_id)
            .bind(scope.workspace_id);
        if let Some(asset) = asset_filter {
            query = query.bind(asset);
        }
        query.fetch_all(&mut *conn).await.map_err(turso_err)?
    };
    // This is conservative storage-identity deduplication, not SQL equality:
    // e.g. Integer(1), Real(1.0), and Text("1") remain separate requests.
    #[derive(Eq, Hash, PartialEq)]
    enum StoredKey {
        Null,
        Integer(i64),
        Real(u64),
        Text(String),
        Blob(Vec<u8>),
    }
    fn identity(key: &AssociationKey) -> StoredKey {
        match &key.0 {
            turso::Value::Null => StoredKey::Null,
            turso::Value::Integer(value) => StoredKey::Integer(*value),
            turso::Value::Real(value) => StoredKey::Real(value.to_bits()),
            turso::Value::Text(value) => StoredKey::Text(value.clone()),
            turso::Value::Blob(value) => StoredKey::Blob(value.clone()),
        }
    }
    let mut requests = Vec::new();
    let mut request_positions = HashMap::new();
    let mut positions = Vec::with_capacity(rows.len());
    for row in &rows {
        let asset: AssociationKey = row.try_get("asset_id").map_err(turso_err)?;
        let version: AssociationKey = row.try_get("version").map_err(turso_err)?;
        let key = (identity(&asset), identity(&version));
        let position = *request_positions.entry(key).or_insert_with(|| {
            let position = requests.len();
            requests.push(vec![asset, version]);
            position
        });
        positions.push(position);
    }
    drop(request_positions);
    // Keep payload metadata raw until native current/deleted/root predicates
    // have excluded the row. A hidden title or kind is never a decode error.
    let assets = associate_query(
        conn,
        scope,
        "SELECT kind,(id=? AND current_version=?) AS m0,(id=? AND current_version=?) AS m1,(id=? AND current_version=?) AS m2,(id=? AND current_version=?) AS m3,(id=? AND current_version=?) AS m4,(id=? AND current_version=?) AS m5,(id=? AND current_version=?) AS m6,(id=? AND current_version=?) AS m7 FROM oc_assets WHERE tenant_id=? AND workspace_id=? AND NOT deleted AND ((id=? AND current_version=?) OR (id=? AND current_version=?) OR (id=? AND current_version=?) OR (id=? AND current_version=?) OR (id=? AND current_version=?) OR (id=? AND current_version=?) OR (id=? AND current_version=?) OR (id=? AND current_version=?))",
        2,
        &requests,
    )
    .await?;
    #[cfg(all(test, feature = "local-storage"))]
    let asset_count = assets.len();
    let assets: HashMap<_, _> = assets.into_iter().collect();
    let versions = associate_rows(
        conn,
        scope,
        "oc_versions",
        "title,source_event_id",
        MATCH_VERSION_SELECT,
        MATCH_VERSION_WHERE,
        "1",
        2,
        &requests,
    )
    .await?;
    #[cfg(all(test, feature = "local-storage"))]
    let version_count = versions.len();
    let sources = versions
        .iter()
        .map(|(_, row)| Ok(vec![row.try_get("source_event_id").map_err(turso_err)?]))
        .collect::<StorageResult<Vec<_>>>()?;
    let events = associate_rows(
        conn,
        scope,
        "oc_events",
        "id",
        MATCH_EVENT_SELECT,
        MATCH_EVENT_WHERE,
        "state='active'",
        1,
        &sources,
    )
    .await?;
    let active: HashSet<_> = events.into_iter().map(|(index, _)| index).collect();
    let versions: HashMap<_, _> = versions
        .into_iter()
        .enumerate()
        .filter_map(|(index, (request, row))| active.contains(&index).then_some((request, row)))
        .collect();
    #[cfg(all(test, feature = "local-storage"))]
    let counts = [rows.len(), requests.len(), asset_count, version_count];
    Ok(RawCurrentRows {
        rows,
        assets,
        versions,
        positions,
        #[cfg(all(test, feature = "local-storage"))]
        counts,
    })
}

// Root-only consumers must not decode asset kind, current version, or title.
// Request positions retain native association results until the caller consumes
// its own projection. This also serves exact historical reads without a current gate.
pub(super) async fn live_root_rows(
    conn: &mut Connection,
    scope: Scope,
    requests: &[Vec<AssociationKey>],
) -> StorageResult<Vec<(usize, native_turso::Row)>> {
    let asset_keys: Vec<_> = requests.iter().map(|key| vec![key[0].clone()]).collect();
    let assets = associate_rows(
        conn,
        scope,
        "oc_assets",
        "id",
        MATCH_EVENT_SELECT,
        MATCH_EVENT_WHERE,
        "NOT deleted",
        1,
        &asset_keys,
    )
    .await?;
    let allowed: HashSet<_> = assets.into_iter().map(|(index, _)| index).collect();
    let versions = associate_rows(
        conn,
        scope,
        "oc_versions",
        "source_event_id,version",
        MATCH_VERSION_SELECT,
        MATCH_VERSION_WHERE,
        "1",
        2,
        requests,
    )
    .await?;
    let sources = versions
        .iter()
        .map(|(_, row)| Ok(vec![row.try_get("source_event_id").map_err(turso_err)?]))
        .collect::<StorageResult<Vec<_>>>()?;
    let events = associate_rows(
        conn,
        scope,
        "oc_events",
        "id",
        MATCH_EVENT_SELECT,
        MATCH_EVENT_WHERE,
        "state='active'",
        1,
        &sources,
    )
    .await?;
    let active: HashSet<_> = events.into_iter().map(|(index, _)| index).collect();
    Ok(versions
        .into_iter()
        .enumerate()
        .filter_map(|(index, (request, row))| {
            (allowed.contains(&request) && active.contains(&index)).then_some((request, row))
        })
        .collect())
}

// Root-only raw projection used by native candidates; never reads hit payload.
async fn current_root_rows(
    conn: &mut Connection,
    scope: Scope,
    ids: &[AssociationKey],
    asset_filter: Option<Uuid>,
) -> StorageResult<Vec<(native_turso::Row, native_turso::Row)>> {
    let filter = asset_filter
        .map(AssociationKey::from_uuid)
        .unwrap_or(AssociationKey(turso::Value::Null));
    let keys: Vec<_> = ids
        .iter()
        .map(|id| vec![id.clone(), filter.clone(), filter.clone()])
        .collect();
    // Scoped rowid only removes repeated matches of this same physical row
    // inside this snapshot. Every business relationship is still native SQL.
    let mut seen = HashSet::new();
    let mut rows = Vec::new();
    for (_, row) in associate_query(
        conn,
        scope,
        "SELECT rowid AS read_rowid,id,asset_id,version,(id=? AND (? IS NULL OR asset_id=?)) AS m0,(id=? AND (? IS NULL OR asset_id=?)) AS m1,(id=? AND (? IS NULL OR asset_id=?)) AS m2,(id=? AND (? IS NULL OR asset_id=?)) AS m3,(id=? AND (? IS NULL OR asset_id=?)) AS m4,(id=? AND (? IS NULL OR asset_id=?)) AS m5,(id=? AND (? IS NULL OR asset_id=?)) AS m6,(id=? AND (? IS NULL OR asset_id=?)) AS m7 FROM oc_chunks WHERE tenant_id=? AND workspace_id=? AND 1 AND ((id=? AND (? IS NULL OR asset_id=?)) OR (id=? AND (? IS NULL OR asset_id=?)) OR (id=? AND (? IS NULL OR asset_id=?)) OR (id=? AND (? IS NULL OR asset_id=?)) OR (id=? AND (? IS NULL OR asset_id=?)) OR (id=? AND (? IS NULL OR asset_id=?)) OR (id=? AND (? IS NULL OR asset_id=?)) OR (id=? AND (? IS NULL OR asset_id=?)))",
        3,
        &keys,
    )
    .await?
    {
        if seen.insert(row.try_get::<i64>("read_rowid").map_err(turso_err)?) {
            rows.push(row);
        }
    }
    let requests = rows
        .iter()
        .map(|row| {
            Ok(vec![
                row.try_get("asset_id").map_err(turso_err)?,
                row.try_get("version").map_err(turso_err)?,
            ])
        })
        .collect::<StorageResult<Vec<_>>>()?;
    let current = associate_query(
        conn,
        scope,
        "SELECT id,(id=? AND current_version=?) AS m0,(id=? AND current_version=?) AS m1,(id=? AND current_version=?) AS m2,(id=? AND current_version=?) AS m3,(id=? AND current_version=?) AS m4,(id=? AND current_version=?) AS m5,(id=? AND current_version=?) AS m6,(id=? AND current_version=?) AS m7 FROM oc_assets WHERE tenant_id=? AND workspace_id=? AND NOT deleted AND ((id=? AND current_version=?) OR (id=? AND current_version=?) OR (id=? AND current_version=?) OR (id=? AND current_version=?) OR (id=? AND current_version=?) OR (id=? AND current_version=?) OR (id=? AND current_version=?) OR (id=? AND current_version=?))",
        2,
        &requests,
    )
    .await?;
    let current: HashSet<_> = current.into_iter().map(|(index, _)| index).collect();
    let mut roots: HashMap<_, _> = live_root_rows(conn, scope, &requests)
        .await?
        .into_iter()
        .collect();
    Ok(rows
        .into_iter()
        .enumerate()
        .filter_map(|(index, row)| {
            if current.contains(&index) {
                roots.remove(&index).map(|root| (row, root))
            } else {
                None
            }
        })
        .collect())
}

#[cfg(all(test, feature = "local-storage"))]
pub(super) async fn current_chunk_roots(
    conn: &mut Connection,
    scope: Scope,
    ids: &[Uuid],
) -> StorageResult<HashMap<Uuid, SourceVersion>> {
    let keys: Vec<_> = ids.iter().copied().map(AssociationKey::from_uuid).collect();
    let mut roots = HashMap::new();
    for (row, root) in current_root_rows(conn, scope, &keys, None).await? {
        roots.insert(
            row.try_get("id").map_err(turso_err)?,
            SourceVersion {
                source_id: root.try_get("source_event_id").map_err(turso_err)?,
                version: root.try_get("version").map_err(turso_err)?,
            },
        );
    }
    Ok(roots)
}

struct LocalSupport {
    visible: HashMap<VersionKey, bool>,
    sources: HashMap<VersionKey, Vec<native_turso::Row>>,
}

struct SupportPositions {
    visible: Vec<bool>,
    sources: HashMap<usize, Vec<native_turso::Row>>,
}

// The old native query decoded the entire source column as String before
// parsing UUIDs. Delay both phases until local and asset Learning guards pass.
fn strict_support_sources(rows: &[native_turso::Row]) -> StorageResult<Vec<Uuid>> {
    let sources: Vec<String> = rows
        .iter()
        .map(|row| row.try_get("raw_source").map_err(turso_err))
        .collect::<StorageResult<_>>()?;
    sources
        .iter()
        .map(|source| Uuid::parse_str(source).map_err(|_| StorageError::NotFound))
        .collect()
}

// Member boundaries are read by serde's lossless IgnoredAny deserializer.
// Do not materialize a Value: that collapses duplicate object keys and rejects
// native-valid out-of-range numbers. Native JSON remains the semantic oracle.
pub(super) fn support_members(kind: &str, text: &str) -> StorageResult<Vec<String>> {
    fn invalid() -> StorageError {
        StorageError::Backend("invalid normalized support JSON".into())
    }
    fn prefix(text: &str) -> StorageResult<usize> {
        let mut values =
            serde_json::Deserializer::from_str(text).into_iter::<serde::de::IgnoredAny>();
        values.next().ok_or_else(invalid)?.map_err(|_| invalid())?;
        Ok(values.byte_offset())
    }
    let text = text.trim();
    let object = kind == "object";
    let (start, end) = if object { ('{', '}') } else { ('[', ']') };
    if !object && kind != "array" {
        if prefix(text)? != text.len() {
            return Err(invalid());
        }
        return Ok(vec![text.to_owned()]);
    }
    let mut remaining = text
        .strip_prefix(start)
        .and_then(|text| text.strip_suffix(end))
        .ok_or_else(invalid)?
        .trim();
    let mut members = Vec::new();
    while !remaining.is_empty() {
        if object {
            if !remaining.starts_with('"') {
                return Err(invalid());
            }
            let length = prefix(remaining)?;
            remaining = remaining[length..]
                .trim_start()
                .strip_prefix(':')
                .ok_or_else(invalid)?
                .trim_start();
        }
        let length = prefix(remaining)?;
        members.push(remaining[..length].to_owned());
        remaining = remaining[length..].trim_start();
        if remaining.is_empty() {
            break;
        }
        remaining = remaining
            .strip_prefix(',')
            .ok_or_else(invalid)?
            .trim_start();
        if remaining.is_empty() {
            return Err(invalid());
        }
    }
    Ok(members)
}

impl TursoTx {
    // Test-only typed projection; production historical views retain raw rows
    // to decode their own selected tuple in its original order.
    #[cfg(all(test, feature = "local-storage"))]
    pub(super) async fn active_version_roots(
        &mut self,
        versions: &[VersionKey],
    ) -> StorageResult<HashMap<VersionKey, Uuid>> {
        let requests: Vec<_> = versions
            .iter()
            .map(|(asset, version)| {
                vec![
                    AssociationKey(turso::Value::Blob(asset.as_bytes().to_vec())),
                    AssociationKey(turso::Value::Integer(i64::from(*version))),
                ]
            })
            .collect();
        let mut roots = HashMap::new();
        for (index, row) in live_root_rows(&mut self.tx, self.scope, &requests).await? {
            roots.insert(
                versions[index],
                row.try_get("source_event_id").map_err(turso_err)?,
            );
        }
        Ok(roots)
    }

    async fn support_snapshot(&mut self, versions: &[VersionKey]) -> StorageResult<LocalSupport> {
        // Native extraction retains JSON5, SQL NULL and malformed JSON behavior.
        // Exact version predicates avoid evaluating unrelated historical JSON.
        let rows = version_rows(
            &mut self.tx,
            self.scope,
            "oc_chunks",
            "asset_id,version,json_extract(locator,'$.parser') IS 'memory-semantics:v1' AS semantic,CASE WHEN json_extract(locator,'$.parser') IS 'memory-semantics:v1' THEN json_type(locator,'$.support_sources') END AS support_type,CASE WHEN json_extract(locator,'$.parser') IS 'memory-semantics:v1' THEN json_quote(json_extract(locator,'$.support_sources')) END AS support_json",
            versions,
        )
        .await?;
        self.evaluate_support_rows(rows, versions).await
    }

    pub(super) async fn local_current_positions(
        &mut self,
        raw: &RawCurrentRows,
    ) -> StorageResult<Vec<usize>> {
        let eligible = raw.eligible();
        let requests = eligible
            .iter()
            .map(|index| Ok(vec![raw.rows[*index].try_get("id").map_err(turso_err)?]))
            .collect::<StorageResult<Vec<_>>>()?;
        let allowed = self.local_support_positions(&requests).await?;
        Ok(eligible
            .into_iter()
            .enumerate()
            .filter_map(|(index, row)| allowed.contains(&index).then_some(row))
            .collect())
    }

    async fn local_support_positions(
        &mut self,
        requests: &[Vec<AssociationKey>],
    ) -> StorageResult<HashSet<usize>> {
        let rows = associate_rows(
            &mut self.tx,
            self.scope,
            "oc_chunks",
            "json_extract(locator,'$.parser') IS 'memory-semantics:v1' AS semantic,CASE WHEN json_extract(locator,'$.parser') IS 'memory-semantics:v1' THEN json_type(locator,'$.support_sources') END AS support_type,CASE WHEN json_extract(locator,'$.parser') IS 'memory-semantics:v1' THEN json_quote(json_extract(locator,'$.support_sources')) END AS support_json",
            MATCH_EVENT_SELECT,
            MATCH_EVENT_WHERE,
            "1",
            1,
            requests,
        )
        .await?;
        let mut members = Vec::new();
        for (position, row) in rows {
            if !row.try_get::<bool>("semantic").map_err(turso_err)? {
                continue;
            }
            let Some(kind) = row
                .try_get::<Option<String>>("support_type")
                .map_err(turso_err)?
            else {
                continue;
            };
            let normalized: String = row.try_get("support_json").map_err(turso_err)?;
            members.extend(
                support_members(&kind, &normalized)?
                    .into_iter()
                    .map(|value| (position, value)),
            );
        }
        let result = self
            .evaluate_support_members(members, requests.len())
            .await?;
        Ok(result
            .visible
            .into_iter()
            .enumerate()
            .filter_map(|(index, visible)| visible.then_some(index))
            .collect())
    }

    pub(super) async fn current_summary_terms(
        &mut self,
        raw: &RawCurrentRows,
        positions: &[usize],
    ) -> StorageResult<HashMap<usize, native_turso::Row>> {
        let requests = positions
            .iter()
            .map(|index| Ok(vec![raw.rows[*index].try_get("id").map_err(turso_err)?]))
            .collect::<StorageResult<Vec<_>>>()?;
        let rows = associate_query(
            &mut self.tx,
            self.scope,
            "SELECT group_concat(search_terms,' ') AS search_terms,(chunk_id=?) AS m0,(chunk_id=?) AS m1,(chunk_id=?) AS m2,(chunk_id=?) AS m3,(chunk_id=?) AS m4,(chunk_id=?) AS m5,(chunk_id=?) AS m6,(chunk_id=?) AS m7 FROM oc_summaries WHERE tenant_id=? AND workspace_id=? AND ((chunk_id=?) OR (chunk_id=?) OR (chunk_id=?) OR (chunk_id=?) OR (chunk_id=?) OR (chunk_id=?) OR (chunk_id=?) OR (chunk_id=?)) GROUP BY chunk_id",
            1,
            &requests,
        )
        .await?;
        Ok(rows
            .into_iter()
            .map(|(index, row)| (positions[index], row))
            .collect())
    }

    async fn evaluate_support_rows(
        &mut self,
        rows: Vec<native_turso::Row>,
        keys: &[VersionKey],
    ) -> StorageResult<LocalSupport> {
        let mut order = Vec::new();
        let mut positions = HashMap::new();
        for key in keys {
            positions.entry(*key).or_insert_with(|| {
                let position = order.len();
                order.push(*key);
                position
            });
        }
        let mut members = Vec::new();
        for row in rows {
            if !row.try_get::<bool>("semantic").map_err(turso_err)? {
                continue;
            }
            let Some(kind) = row
                .try_get::<Option<String>>("support_type")
                .map_err(turso_err)?
            else {
                continue;
            };
            let key = (
                row.try_get::<Uuid>("asset_id").map_err(turso_err)?,
                row.try_get::<i32>("version").map_err(turso_err)?,
            );
            let position = *positions
                .get(&key)
                .ok_or_else(|| StorageError::Backend("inconsistent support row position".into()))?;
            let normalized: String = row.try_get("support_json").map_err(turso_err)?;
            members.extend(
                support_members(&kind, &normalized)?
                    .into_iter()
                    .map(|value| (position, value)),
            );
        }
        let result = self.evaluate_support_members(members, order.len()).await?;
        Ok(LocalSupport {
            visible: result
                .visible
                .into_iter()
                .enumerate()
                .map(|(index, visible)| (order[index], visible))
                .collect(),
            sources: result
                .sources
                .into_iter()
                .map(|(index, rows)| (order[index], rows))
                .collect(),
        })
    }

    // Shared existing native normalization, indexed by request position rather
    // than requiring a business UUID/version before local visibility is known.
    async fn evaluate_support_members(
        &mut self,
        members: Vec<(usize, String)>,
        count: usize,
    ) -> StorageResult<SupportPositions> {
        let mut visible = vec![true; count];
        let mut sources_by_version: HashMap<usize, Vec<native_turso::Row>> = HashMap::new();
        let mut normalized = Vec::new();
        for batch in members.chunks(READ_KEYS) {
            let values: Vec<_> = batch.iter().map(|(_, value)| value.as_str()).collect();
            let rows = native_turso::query(
                "SELECT key,value AS raw_source,upper(replace(value,'-','')) AS source FROM json_each(?)",
            )
            .bind(format!("[{}]", values.join(",")))
            .fetch_all(&mut *self.tx)
            .await
            .map_err(turso_err)?;
            for row in rows {
                let position: i64 = row.try_get("key").map_err(turso_err)?;
                let key = usize::try_from(position)
                    .ok()
                    .and_then(|index| batch.get(index))
                    .map(|(key, _)| *key)
                    .ok_or_else(|| {
                        StorageError::Backend("invalid normalized support index".into())
                    })?;
                if let Some(source) = row.try_get::<Option<String>>("source").map_err(turso_err)? {
                    normalized.push((key, source));
                } else {
                    visible[key] = false;
                }
                // Keep native value types for the later String decoding phase.
                // A numeric SQL value can normalize to an active UUID's hex.
                sources_by_version.entry(key).or_default().push(row);
            }
        }
        let mut sources: Vec<_> = normalized
            .iter()
            .map(|(_, source)| source.clone())
            .collect();
        sources.sort_unstable();
        sources.dedup();
        let mut active = HashSet::new();
        for batch in sources.chunks(READ_KEYS) {
            let sql = format!(
                "SELECT hex(id) AS source FROM oc_events WHERE tenant_id=? AND workspace_id=? AND state='active' AND hex(id) IN ({})",
                vec!["?"; batch.len()].join(","),
            );
            let mut query = native_turso::query(&sql)
                .bind(self.scope.tenant_id)
                .bind(self.scope.workspace_id);
            for source in batch {
                query = query.bind(source);
            }
            let rows = query.fetch_all(&mut *self.tx).await.map_err(turso_err)?;
            for row in rows {
                active.insert(row.try_get::<String>("source").map_err(turso_err)?);
            }
        }
        for (key, source) in normalized {
            if !active.contains(&source) {
                visible[key] = false;
            }
        }
        Ok(SupportPositions {
            visible,
            sources: sources_by_version,
        })
    }

    pub(super) async fn local_support_visibility(
        &mut self,
        versions: &[VersionKey],
    ) -> StorageResult<HashMap<VersionKey, bool>> {
        Ok(self.support_snapshot(versions).await?.visible)
    }

    /// Complete semantic support for an exact version, including historical
    /// versions. This does not establish asset/root existence or currentness.
    pub(super) async fn semantic_version_support_visible(
        &mut self,
        asset: Uuid,
        version: i32,
    ) -> StorageResult<bool> {
        let key = (asset, version);
        let local = self.support_snapshot(&[key]).await?;
        let visible = local.visible.get(&key).copied().ok_or_else(|| {
            StorageError::Unavailable("inconsistent local support visibility".into())
        })?;
        if !visible || !self.learning_asset_visible(asset).await? {
            return Ok(false);
        }
        // Decode every native String before parsing any UUID. In particular,
        // a later non-text value must not be hidden by an earlier bad UUID.
        let rows = local.sources.get(&key).map_or(&[][..], Vec::as_slice);
        let mut sources = strict_support_sources(rows)?;
        let mut seen = HashSet::new();
        sources.retain(|source| seen.insert(*source));
        // Keep the exact source guard's strict native event-kind decoding.
        // Source checks retain their existing order and error propagation.
        match self.require_learning_sources_visible(&sources).await {
            Ok(()) => Ok(true),
            Err(StorageError::NotFound) => Ok(false),
            Err(error) => Err(error),
        }
    }

    #[cfg(test)]
    pub(super) async fn guard_retrieval_chunks(
        &mut self,
        mut chunks: Vec<CurrentChunk>,
    ) -> StorageResult<Vec<CurrentChunk>> {
        let versions: Vec<_> = chunks
            .iter()
            .map(|chunk| (chunk.hit.asset_id, chunk.hit.version))
            .collect();
        let visible = self.visible_version_keys(&versions).await?;
        chunks.retain(|chunk| visible.contains(&(chunk.hit.asset_id, chunk.hit.version)));
        Ok(chunks)
    }

    pub(super) async fn visible_version_keys(
        &mut self,
        versions: &[VersionKey],
    ) -> StorageResult<HashSet<VersionKey>> {
        if versions.is_empty() {
            return Ok(HashSet::new());
        }
        let local = self.support_snapshot(versions).await?;
        let versions: Vec<_> = versions
            .iter()
            .copied()
            .filter(|key| local.visible.get(key) == Some(&true))
            .collect();
        let assets = unique_ids(versions.iter().map(|key| key.0));
        let learning = self.learning_assets_visible(&assets).await?;
        let versions: Vec<_> = versions
            .into_iter()
            .filter(|key| learning.get(&key.0) == Some(&true))
            .collect();
        let mut version_sources = HashMap::new();
        for key in &versions {
            if version_sources.contains_key(key) {
                continue;
            }
            let rows = local.sources.get(key).map_or(&[][..], Vec::as_slice);
            version_sources.insert(*key, strict_support_sources(rows)?);
        }
        let source_visibility = self.learning_source_sets_visible(&version_sources).await?;
        Ok(versions
            .into_iter()
            .filter(|key| source_visibility.get(key) == Some(&true))
            .collect())
    }

    pub(crate) async fn visible_hits(
        &mut self,
        ids: &[Uuid],
    ) -> StorageResult<HashMap<Uuid, SearchHit>> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let raw = current_rows(&mut self.tx, self.scope, Some(ids), None).await?;
        let positions = self.local_current_positions(&raw).await?;
        let keys = positions
            .iter()
            .map(|index| raw.version_key(*index))
            .collect::<StorageResult<Vec<_>>>()?;
        let visible = self.visible_version_keys(&keys).await?;
        let mut hits = HashMap::new();
        for (index, key) in positions.into_iter().zip(keys) {
            if visible.contains(&key) {
                let hit = raw.hit(index, 0.0)?;
                hits.insert(hit.chunk_id, hit);
            }
        }
        Ok(hits)
    }

    /// Called after visible_hits in the same transaction; it only enriches
    /// already-authorized assets and is not a standalone visibility decision.
    #[cfg(feature = "local-storage")]
    pub(crate) async fn retrieval_identities(
        &mut self,
        assets: &[Uuid],
    ) -> StorageResult<HashMap<Uuid, Option<crate::memory_identity::MemoryIdentity>>> {
        let mut identities: HashMap<_, _> = assets.iter().copied().map(|id| (id, None)).collect();
        if identities.is_empty() {
            return Ok(identities);
        }
        let rows = rows_by_ids(
            &mut self.tx,
            self.scope,
            "oc_assets",
            "id,current_version,NOT deleted AS undeleted",
            "id",
            assets,
        )
        .await?;
        let mut versions = Vec::new();
        for row in rows {
            if !row.try_get::<bool>("undeleted").map_err(turso_err)? {
                return Err(StorageError::NotFound);
            }
            let version = row
                .try_get::<Option<i32>>("current_version")
                .map_err(turso_err)?
                .ok_or(StorageError::NotFound)?;
            versions.push((row.try_get::<Uuid>("id").map_err(turso_err)?, version));
        }
        if versions.len() != identities.len() {
            return Err(StorageError::NotFound);
        }
        // asset_view also reads the first semantic status. Decode only that
        // row per version, preserving errors for NULL/non-string first status
        // without rejecting non-first status values the old LIMIT 1 did not read.
        let rows = version_rows(
            &mut self.tx, self.scope, "oc_chunks",
            "asset_id,version,ordinal,json_extract(locator,'$.parser') IS 'memory-semantics:v1' AS semantic,json_extract(locator,'$.semantic_status') AS status",
            &versions,
        )
        .await?;
        let mut first: HashMap<VersionKey, (i64, native_turso::Row)> = HashMap::new();
        for row in rows {
            if !row.try_get::<bool>("semantic").map_err(turso_err)? {
                continue;
            }
            let key = (
                row.try_get("asset_id").map_err(turso_err)?,
                row.try_get("version").map_err(turso_err)?,
            );
            let ordinal: i64 = row.try_get("ordinal").map_err(turso_err)?;
            if first
                .get(&key)
                .is_none_or(|(previous, _)| ordinal < *previous)
            {
                first.insert(key, (ordinal, row));
            }
        }
        for (_, row) in first.into_values() {
            let _: String = row.try_get("status").map_err(turso_err)?;
        }
        if !identity_table_present(&mut self.tx).await? {
            return Ok(identities);
        }
        for row in rows_by_ids(
            &mut self.tx,
            self.scope,
            "oc_memory_identities",
            "asset_id,identity_json",
            "asset_id",
            assets,
        )
        .await?
        {
            let value: String = row.try_get("identity_json").map_err(turso_err)?;
            let identity = serde_json::from_str(&value)
                .map_err(|_| StorageError::Unavailable("invalid stored memory identity".into()))?;
            identities.insert(row.try_get("asset_id").map_err(turso_err)?, Some(identity));
        }
        Ok(identities)
    }

    pub(super) async fn graph_rows_for_sources(
        &mut self,
        roots: &HashSet<VersionKey>,
    ) -> StorageResult<Vec<Value>> {
        let requests: Vec<_> = roots
            .iter()
            .map(|(source, version)| {
                vec![
                    AssociationKey::from_uuid(*source),
                    AssociationKey(turso::Value::Integer(i64::from(*version))),
                ]
            })
            .collect();
        let versions = associate_query(
            &mut self.tx,
            self.scope,
            "SELECT asset_id,version,(source_event_id=? AND version=?) AS m0,(source_event_id=? AND version=?) AS m1,(source_event_id=? AND version=?) AS m2,(source_event_id=? AND version=?) AS m3,(source_event_id=? AND version=?) AS m4,(source_event_id=? AND version=?) AS m5,(source_event_id=? AND version=?) AS m6,(source_event_id=? AND version=?) AS m7 FROM oc_versions WHERE tenant_id=? AND workspace_id=? AND ((source_event_id=? AND version=?) OR (source_event_id=? AND version=?) OR (source_event_id=? AND version=?) OR (source_event_id=? AND version=?) OR (source_event_id=? AND version=?) OR (source_event_id=? AND version=?) OR (source_event_id=? AND version=?) OR (source_event_id=? AND version=?))",
            2,
            &requests,
        )
        .await?;
        let requests = versions
            .into_iter()
            .map(|(_, row)| {
                Ok(vec![
                    row.try_get("asset_id").map_err(turso_err)?,
                    row.try_get("version").map_err(turso_err)?,
                ])
            })
            .collect::<StorageResult<Vec<_>>>()?;
        let chunks = associate_rows(
            &mut self.tx,
            self.scope,
            "oc_chunks",
            "id",
            MATCH_VERSION_SELECT,
            MATCH_VERSION_WHERE,
            "1",
            2,
            &requests,
        )
        .await?;
        let ids = chunks
            .into_iter()
            .map(|(_, row)| row.try_get("id").map_err(turso_err))
            .collect::<StorageResult<Vec<AssociationKey>>>()?;
        let current = current_root_rows(&mut self.tx, self.scope, &ids, None).await?;
        let requests = current
            .iter()
            .map(|(row, _)| Ok(vec![row.try_get("id").map_err(turso_err)?]))
            .collect::<StorageResult<Vec<_>>>()?;
        let locators = associate_rows(
            &mut self.tx,
            self.scope,
            "oc_chunks",
            "locator,hex(id) AS order_key",
            MATCH_EVENT_SELECT,
            MATCH_EVENT_WHERE,
            "1",
            1,
            &requests,
        )
        .await?;
        let mut current: HashMap<_, _> = current.into_iter().enumerate().collect();
        let mut rows = Vec::new();
        for (index, locator) in locators {
            if let Some((row, root)) = current.remove(&index) {
                let order: String = locator.try_get("order_key").map_err(turso_err)?;
                rows.push((order, row, root, locator));
            }
        }
        // UUID BLOB byte order and its fixed-width uppercase hex order agree.
        // This is only an ordering key, never a business-key equality substitute.
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        let mut candidates = rows.into_iter();
        let mut evidence = Vec::new();
        while evidence.len() < 20 {
            let batch: Vec<_> = candidates.by_ref().take(20 - evidence.len()).collect();
            if batch.is_empty() {
                break;
            }
            let requests = batch
                .iter()
                .map(|(_, row, _, _)| Ok(vec![row.try_get("id").map_err(turso_err)?]))
                .collect::<StorageResult<Vec<_>>>()?;
            let local = self.local_support_positions(&requests).await?;
            let batch: Vec<_> = batch
                .into_iter()
                .enumerate()
                .filter_map(|(index, row)| local.contains(&index).then_some(row))
                .collect();
            let versions = batch
                .iter()
                .map(|(_, row, _, _)| {
                    Ok((
                        row.try_get("asset_id").map_err(turso_err)?,
                        row.try_get("version").map_err(turso_err)?,
                    ))
                })
                .collect::<StorageResult<Vec<VersionKey>>>()?;
            let visible = self.visible_version_keys(&versions).await?;
            for ((_, row, root, locator), (asset, version)) in batch.into_iter().zip(versions) {
                if !visible.contains(&(asset, version)) {
                    continue;
                }
                let text: String = locator.try_get("locator").map_err(turso_err)?;
                let locator: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
                let source_override = if locator["parser"] == crate::memory_semantics::PARSER {
                    locator["source_event_id"]
                        .as_str()
                        .and_then(|value| Uuid::parse_str(value).ok())
                } else {
                    None
                };
                let source: Uuid = match source_override {
                    Some(source) => source,
                    None => root.try_get("source_event_id").map_err(turso_err)?,
                };
                let chunk: Uuid = row.try_get("id").map_err(turso_err)?;
                evidence.push(json!({"asset_id":asset,"version":version,"chunk_id":chunk,
                    "source_event_id":source,"locator":locator}));
            }
        }
        Ok(evidence)
    }

    async fn published_positions(
        &mut self,
        requests: &[Vec<AssociationKey>],
    ) -> StorageResult<HashSet<usize>> {
        Ok(associate_query(
            &mut self.tx,
            self.scope,
            "SELECT artifact_id,(artifact_id=? AND source_id=? AND version=? AND generation=? AND surface=?) AS m0,(artifact_id=? AND source_id=? AND version=? AND generation=? AND surface=?) AS m1,(artifact_id=? AND source_id=? AND version=? AND generation=? AND surface=?) AS m2,(artifact_id=? AND source_id=? AND version=? AND generation=? AND surface=?) AS m3,(artifact_id=? AND source_id=? AND version=? AND generation=? AND surface=?) AS m4,(artifact_id=? AND source_id=? AND version=? AND generation=? AND surface=?) AS m5,(artifact_id=? AND source_id=? AND version=? AND generation=? AND surface=?) AS m6,(artifact_id=? AND source_id=? AND version=? AND generation=? AND surface=?) AS m7 FROM oc_artifact_ledger WHERE tenant_id=? AND workspace_id=? AND state='committed' AND ((artifact_id=? AND source_id=? AND version=? AND generation=? AND surface=?) OR (artifact_id=? AND source_id=? AND version=? AND generation=? AND surface=?) OR (artifact_id=? AND source_id=? AND version=? AND generation=? AND surface=?) OR (artifact_id=? AND source_id=? AND version=? AND generation=? AND surface=?) OR (artifact_id=? AND source_id=? AND version=? AND generation=? AND surface=?) OR (artifact_id=? AND source_id=? AND version=? AND generation=? AND surface=?) OR (artifact_id=? AND source_id=? AND version=? AND generation=? AND surface=?) OR (artifact_id=? AND source_id=? AND version=? AND generation=? AND surface=?))",
            5,
            requests,
        )
        .await?.into_iter().map(|(index, _)| index).collect())
    }

    pub(super) async fn index_candidate_ids(
        &mut self,
        ids: &[AssociationKey],
        asset_filter: Option<Uuid>,
        surface: &str,
        generation: i64,
    ) -> StorageResult<Vec<Uuid>> {
        let rows = current_root_rows(&mut self.tx, self.scope, ids, asset_filter).await?;
        let requests = rows
            .iter()
            .map(|(row, root)| {
                Ok(vec![
                    row.try_get("id").map_err(turso_err)?,
                    root.try_get("source_event_id").map_err(turso_err)?,
                    root.try_get("version").map_err(turso_err)?,
                    AssociationKey(turso::Value::Integer(generation)),
                    AssociationKey(turso::Value::Text(surface.to_owned())),
                ])
            })
            .collect::<StorageResult<Vec<_>>>()?;
        let published = self.published_positions(&requests).await?;
        let selected: Vec<_> = rows
            .into_iter()
            .enumerate()
            .filter_map(|(index, row)| published.contains(&index).then_some(row))
            .collect();
        let requests = selected
            .iter()
            .map(|(row, _)| Ok(vec![row.try_get("id").map_err(turso_err)?]))
            .collect::<StorageResult<Vec<_>>>()?;
        let local = self.local_support_positions(&requests).await?;
        let mut candidates = Vec::new();
        for (index, (row, _)) in selected.into_iter().enumerate() {
            if local.contains(&index) {
                let key: VersionKey = (
                    row.try_get("asset_id").map_err(turso_err)?,
                    row.try_get("version").map_err(turso_err)?,
                );
                candidates.push((row, key));
            }
        }
        let versions: Vec<_> = candidates.iter().map(|(_, key)| *key).collect();
        let visible = self.visible_version_keys(&versions).await?;
        let mut ids = Vec::new();
        for (row, key) in candidates {
            if visible.contains(&key) {
                ids.push(row.try_get("id").map_err(turso_err)?);
            }
        }
        Ok(unique_ids(ids))
    }

    #[cfg(feature = "local-storage")]
    pub(super) async fn supported_candidate_sources(
        &mut self,
        ids: &[Uuid],
    ) -> StorageResult<HashMap<Uuid, SourceVersion>> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let keys: Vec<_> = ids.iter().copied().map(AssociationKey::from_uuid).collect();
        let rows = current_root_rows(&mut self.tx, self.scope, &keys, None).await?;
        let requests = rows
            .iter()
            .map(|(row, _)| Ok(vec![row.try_get("id").map_err(turso_err)?]))
            .collect::<StorageResult<Vec<_>>>()?;
        let local = self.local_support_positions(&requests).await?;
        let mut roots = HashMap::new();
        for (index, (row, root)) in rows.into_iter().enumerate() {
            if local.contains(&index) {
                roots.insert(
                    row.try_get("id").map_err(turso_err)?,
                    SourceVersion {
                        source_id: root.try_get("source_event_id").map_err(turso_err)?,
                        version: root.try_get("version").map_err(turso_err)?,
                    },
                );
            }
        }
        Ok(roots)
    }

    pub(super) async fn indexed_hits(
        &mut self,
        keys: &[NativeHitKey],
        surface: &str,
    ) -> StorageResult<HashMap<NativeHitKey, SearchHit>> {
        let requests: Vec<_> = keys
            .iter()
            .map(|(id, source, generation)| {
                vec![
                    AssociationKey::from_uuid(*id),
                    AssociationKey::from_uuid(source.source_id),
                    AssociationKey(turso::Value::Integer(i64::from(source.version))),
                    AssociationKey(turso::Value::Integer(*generation)),
                    AssociationKey(turso::Value::Text(surface.to_owned())),
                ]
            })
            .collect();
        let published = self.published_positions(&requests).await?;
        let keys: Vec<_> = keys
            .iter()
            .enumerate()
            .filter_map(|(index, key)| published.contains(&index).then_some(*key))
            .collect();
        let ids = unique_ids(keys.iter().map(|key| key.0));
        let raw = current_rows(&mut self.tx, self.scope, Some(&ids), None).await?;
        let mut by_id: HashMap<Uuid, Vec<NativeHitKey>> = HashMap::new();
        for key in keys {
            by_id.entry(key.0).or_default().push(key);
        }
        let mut requests = Vec::new();
        let mut positions = Vec::new();
        for index in raw.eligible() {
            let id: Uuid = raw.rows[index].try_get("id").map_err(turso_err)?;
            for key in by_id.get(&id).into_iter().flatten() {
                requests.push(vec![
                    raw.rows[index].try_get("asset_id").map_err(turso_err)?,
                    raw.rows[index].try_get("version").map_err(turso_err)?,
                    AssociationKey::from_uuid(key.1.source_id),
                    AssociationKey(turso::Value::Integer(i64::from(key.1.version))),
                ]);
                positions.push((index, *key));
            }
        }
        let matching = associate_query(
            &mut self.tx,
            self.scope,
            "SELECT asset_id,(asset_id=? AND version=? AND source_event_id=? AND version=?) AS m0,(asset_id=? AND version=? AND source_event_id=? AND version=?) AS m1,(asset_id=? AND version=? AND source_event_id=? AND version=?) AS m2,(asset_id=? AND version=? AND source_event_id=? AND version=?) AS m3,(asset_id=? AND version=? AND source_event_id=? AND version=?) AS m4,(asset_id=? AND version=? AND source_event_id=? AND version=?) AS m5,(asset_id=? AND version=? AND source_event_id=? AND version=?) AS m6,(asset_id=? AND version=? AND source_event_id=? AND version=?) AS m7 FROM oc_versions WHERE tenant_id=? AND workspace_id=? AND 1 AND ((asset_id=? AND version=? AND source_event_id=? AND version=?) OR (asset_id=? AND version=? AND source_event_id=? AND version=?) OR (asset_id=? AND version=? AND source_event_id=? AND version=?) OR (asset_id=? AND version=? AND source_event_id=? AND version=?) OR (asset_id=? AND version=? AND source_event_id=? AND version=?) OR (asset_id=? AND version=? AND source_event_id=? AND version=?) OR (asset_id=? AND version=? AND source_event_id=? AND version=?) OR (asset_id=? AND version=? AND source_event_id=? AND version=?))",
            4,
            &requests,
        )
        .await?;
        let matched: Vec<_> = matching
            .into_iter()
            .map(|(index, _)| positions[index])
            .collect();
        let requests = matched
            .iter()
            .map(|(index, _)| Ok(vec![raw.rows[*index].try_get("id").map_err(turso_err)?]))
            .collect::<StorageResult<Vec<_>>>()?;
        let local = self.local_support_positions(&requests).await?;
        let mut selected = Vec::new();
        for (index, (position, key)) in matched.into_iter().enumerate() {
            if local.contains(&index) {
                selected.push((position, key, raw.version_key(position)?));
            }
        }
        let versions: Vec<_> = selected.iter().map(|(_, _, key)| *key).collect();
        let visible = self.visible_version_keys(&versions).await?;
        let mut hits = HashMap::new();
        for (position, key, version) in selected {
            if visible.contains(&version) {
                hits.insert(key, raw.hit(position, 0.0)?);
            }
        }
        Ok(hits)
    }

    pub(super) async fn ready_vector_keys(
        &mut self,
        keys: &[NativeHitKey],
        profile: &str,
        dimension: Option<usize>,
    ) -> StorageResult<Vec<NativeHitKey>> {
        Ok(self
            .ready_vector_keys_with_testmetrics(keys, profile, dimension)
            .await?
            .0)
    }

    // Counts come from actual fetches and returned row lengths, with no extra
    // query or payload copy. They verify batching and historical-read bounds.
    pub(super) async fn ready_vector_keys_with_testmetrics(
        &mut self,
        keys: &[NativeHitKey],
        profile: &str,
        dimension: Option<usize>,
    ) -> StorageResult<(Vec<NativeHitKey>, usize, usize)> {
        let mut ready = Vec::new();
        let mut queries = 0;
        let mut returned_rows = 0;
        // Keep even the empty-input prepare behavior. Common predicates occur
        // once: repeating them inside every slot overflowed Windows' 1 MiB stack.
        for batch_index in 0..keys.len().div_ceil(4).max(1) {
            let start = batch_index * 4;
            let batch = &keys[start..(start + 4).min(keys.len())];
            let mut query = native_turso::query(
                "SELECT (artifact_id=? AND generation=?) AS m0,(artifact_id=? AND generation=?) AS m1,(artifact_id=? AND generation=?) AS m2,(artifact_id=? AND generation=?) AS m3 FROM oc_index_entries WHERE tenant_id=? AND workspace_id=? AND state='ready' AND artifact_id IN (?,?,?,?) AND generation IN (?,?,?,?) AND model_id=? AND (? IS NULL OR (field='content' AND dimension=?))",
            );
            for slot in 0..4 {
                query = query
                    .bind(batch.get(slot).map(|key| key.0))
                    .bind(batch.get(slot).map(|key| key.2));
            }
            query = query
                .bind(self.scope.tenant_id)
                .bind(self.scope.workspace_id);
            for slot in 0..4 {
                query = query.bind(batch.get(slot).map(|key| key.0));
            }
            for slot in 0..4 {
                query = query.bind(batch.get(slot).map(|key| key.2));
            }
            let dimension = dimension.map(|value| value as i64);
            query = query.bind(profile).bind(dimension).bind(dimension);
            queries += 1;
            let rows = query.fetch_all(&mut *self.tx).await.map_err(turso_err)?;
            returned_rows += rows.len();
            let mut matched = [false; 4];
            for row in rows {
                for (slot, name) in ["m0", "m1", "m2", "m3"]
                    .iter()
                    .take(batch.len())
                    .enumerate()
                {
                    matched[slot] |= row
                        .try_get::<Option<bool>>(*name)
                        .map_err(turso_err)?
                        .unwrap_or(false);
                }
            }
            ready.extend(
                batch
                    .iter()
                    .enumerate()
                    .filter_map(|(slot, key)| matched[slot].then_some(*key)),
            );
        }
        Ok((ready, queries, returned_rows))
    }
}

#[cfg(test)]
#[path = "visibility/predicate_tests.rs"]
mod json_tests;

pub(super) const MATCH_VERSION_SELECT: &str = "(asset_id=? AND version=?) AS m0,(asset_id=? AND version=?) AS m1,(asset_id=? AND version=?) AS m2,(asset_id=? AND version=?) AS m3,(asset_id=? AND version=?) AS m4,(asset_id=? AND version=?) AS m5,(asset_id=? AND version=?) AS m6,(asset_id=? AND version=?) AS m7";

pub(super) const MATCH_VERSION_WHERE: &str = "(asset_id=? AND version=?) OR (asset_id=? AND version=?) OR (asset_id=? AND version=?) OR (asset_id=? AND version=?) OR (asset_id=? AND version=?) OR (asset_id=? AND version=?) OR (asset_id=? AND version=?) OR (asset_id=? AND version=?)";

pub(super) const MATCH_EVENT_SELECT: &str = "(id=?) AS m0,(id=?) AS m1,(id=?) AS m2,(id=?) AS m3,(id=?) AS m4,(id=?) AS m5,(id=?) AS m6,(id=?) AS m7";

pub(super) const MATCH_EVENT_WHERE: &str =
    "(id=?) OR (id=?) OR (id=?) OR (id=?) OR (id=?) OR (id=?) OR (id=?) OR (id=?)";

#[cfg(feature = "local-storage")]
pub(super) const MATCH_PUBLISHED_SELECT: &str = "(asset_id=? AND version=? AND source_event_id=?) AS m0,(asset_id=? AND version=? AND source_event_id=?) AS m1,(asset_id=? AND version=? AND source_event_id=?) AS m2,(asset_id=? AND version=? AND source_event_id=?) AS m3,(asset_id=? AND version=? AND source_event_id=?) AS m4,(asset_id=? AND version=? AND source_event_id=?) AS m5,(asset_id=? AND version=? AND source_event_id=?) AS m6,(asset_id=? AND version=? AND source_event_id=?) AS m7";

#[cfg(feature = "local-storage")]
pub(super) const MATCH_PUBLISHED_WHERE: &str = "(asset_id=? AND version=? AND source_event_id=?) OR (asset_id=? AND version=? AND source_event_id=?) OR (asset_id=? AND version=? AND source_event_id=?) OR (asset_id=? AND version=? AND source_event_id=?) OR (asset_id=? AND version=? AND source_event_id=?) OR (asset_id=? AND version=? AND source_event_id=?) OR (asset_id=? AND version=? AND source_event_id=?) OR (asset_id=? AND version=? AND source_event_id=?)";

// B-domain raw association. A fixed eight-way projection keeps native SQL
// affinity/equality without UUID decoding or a deeply nested generated tree.
pub(super) const ASSOCIATION_KEYS: usize = 8;

pub(super) async fn associate_rows(
    conn: &mut Connection,
    scope: Scope,
    table: &str,
    columns: &str,
    projections: &str,
    alternatives: &str,
    gate: &str,
    width: usize,
    requests: &[Vec<AssociationKey>],
) -> StorageResult<Vec<(usize, native_turso::Row)>> {
    let sql = format!(
        "SELECT {columns},{projections} FROM {table} WHERE tenant_id=? AND workspace_id=? AND ({gate}) AND ({alternatives})"
    );
    associate_query(conn, scope, &sql, width, requests).await
}

/// Execute a complete fixed association statement using the shared native
/// matcher. Separate full templates avoid multiplying unrelated SQL fragments.
pub(super) async fn associate_query(
    conn: &mut Connection,
    scope: Scope,
    sql: &str,
    width: usize,
    requests: &[Vec<AssociationKey>],
) -> StorageResult<Vec<(usize, native_turso::Row)>> {
    let mut result = Vec::new();
    // Even an empty input prepares all original table/column references. A
    // missing table must not be hidden by an empty earlier relation.
    for batch_index in 0..requests.len().div_ceil(ASSOCIATION_KEYS).max(1) {
        let start = batch_index * ASSOCIATION_KEYS;
        let batch = &requests[start..(start + ASSOCIATION_KEYS).min(requests.len())];
        let mut query = native_turso::query(sql);
        for slot in 0..ASSOCIATION_KEYS {
            for key in 0..width {
                query = query.bind(batch.get(slot).map_or_else(
                    || AssociationKey(turso::Value::Null),
                    |request| request[key].clone(),
                ));
            }
        }
        query = query.bind(scope.tenant_id).bind(scope.workspace_id);
        for slot in 0..ASSOCIATION_KEYS {
            for key in 0..width {
                query = query.bind(batch.get(slot).map_or_else(
                    || AssociationKey(turso::Value::Null),
                    |request| request[key].clone(),
                ));
            }
        }
        for row in query.fetch_all(&mut *conn).await.map_err(turso_err)? {
            for (slot, name) in ["m0", "m1", "m2", "m3", "m4", "m5", "m6", "m7"]
                .iter()
                .take(batch.len())
                .enumerate()
            {
                if row.try_get::<Option<bool>>(*name).map_err(turso_err)? == Some(true) {
                    result.push((
                        batch_index * ASSOCIATION_KEYS + slot,
                        native_turso::FromRow::from_row(&row).map_err(turso_err)?,
                    ));
                }
            }
        }
    }
    Ok(result)
}

#[cfg(feature = "local-storage")]
pub(super) async fn context_current_chunk_rows(
    conn: &mut Connection,
    scope: Scope,
    kind: &str,
) -> StorageResult<Vec<(Uuid, Uuid, String, String)>> {
    Ok(context_current_chunk_rows_measured(conn, scope, kind)
        .await?
        .0)
}

#[cfg(feature = "local-storage")]
type ContextChunkRows = Vec<(Uuid, Uuid, String, String)>;

// Local read-set metrics let regressions verify payload scope without timing
// thresholds or an extra query. The counters do not affect query decisions.
#[cfg(feature = "local-storage")]
pub(super) async fn context_current_chunk_rows_measured(
    conn: &mut Connection,
    scope: Scope,
    kind: &str,
) -> StorageResult<(ContextChunkRows, [usize; 3])> {
    // Start from the unique scoped asset key, not every historical chunk. Keep
    // keys native until all current-version/root predicates have been applied.
    let assets = native_turso::query("SELECT id,current_version FROM oc_assets WHERE tenant_id=? AND workspace_id=? AND NOT deleted AND kind=? AND current_version IS NOT NULL")
        .bind(scope.tenant_id)
        .bind(scope.workspace_id)
        .bind(kind)
        .fetch_all(&mut *conn)
        .await
        .map_err(turso_err)?;
    let keys = assets
        .iter()
        .map(|row| {
            Ok(vec![
                row.try_get("id").map_err(turso_err)?,
                row.try_get("current_version").map_err(turso_err)?,
            ])
        })
        .collect::<StorageResult<Vec<_>>>()?;
    let versions = associate_rows(
        conn,
        scope,
        "oc_versions",
        "source_event_id",
        MATCH_VERSION_SELECT,
        MATCH_VERSION_WHERE,
        "1",
        2,
        &keys,
    )
    .await?;
    let sources = versions
        .iter()
        .map(|(_, row)| Ok(vec![row.try_get("source_event_id").map_err(turso_err)?]))
        .collect::<StorageResult<Vec<_>>>()?;
    let events = associate_rows(
        conn,
        scope,
        "oc_events",
        "id",
        MATCH_EVENT_SELECT,
        MATCH_EVENT_WHERE,
        "state='active'",
        1,
        &sources,
    )
    .await?;
    let asset_count = assets.len();
    let version_count = versions.len();
    let matched: HashSet<_> = events
        .into_iter()
        .map(|(index, _)| versions[index].0)
        .collect();
    // This deduplicates only positions in the unique asset read set. Native
    // SQL, not Rust key equality, decides every business-key association.
    let keys: Vec<_> = keys
        .into_iter()
        .enumerate()
        .filter_map(|(index, keys)| matched.contains(&index).then_some(keys))
        .collect();
    let mut result = Vec::new();
    for batch_index in 0..keys.len().div_ceil(ASSOCIATION_KEYS).max(1) {
        let start = batch_index * ASSOCIATION_KEYS;
        let batch = &keys[start..(start + ASSOCIATION_KEYS).min(keys.len())];
        // A row belongs to one current asset key, so no per-request projection
        // or cloning of search_terms/locator is needed. Historical payloads
        // never enter the Rust read set. Empty input still prepares this table.
        let mut query = native_turso::query("SELECT id,asset_id,search_terms,locator FROM oc_chunks WHERE tenant_id=? AND workspace_id=? AND ((asset_id=? AND version=?) OR (asset_id=? AND version=?) OR (asset_id=? AND version=?) OR (asset_id=? AND version=?) OR (asset_id=? AND version=?) OR (asset_id=? AND version=?) OR (asset_id=? AND version=?) OR (asset_id=? AND version=?))")
            .bind(scope.tenant_id)
            .bind(scope.workspace_id);
        for slot in 0..ASSOCIATION_KEYS {
            for key in 0..2 {
                query = query.bind(batch.get(slot).map_or_else(
                    || AssociationKey(turso::Value::Null),
                    |request| request[key].clone(),
                ));
            }
        }
        for row in query.fetch_all(&mut *conn).await.map_err(turso_err)? {
            result.push((
                row.try_get("id").map_err(turso_err)?,
                row.try_get("asset_id").map_err(turso_err)?,
                row.try_get("search_terms").map_err(turso_err)?,
                row.try_get("locator").map_err(turso_err)?,
            ));
        }
    }
    let chunk_count = result.len();
    Ok((result, [asset_count, version_count, chunk_count]))
}
