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

pub(super) async fn chunk_ids_for_sources(
    conn: &mut Connection,
    scope: Scope,
    roots: &HashSet<VersionKey>,
) -> StorageResult<Vec<Uuid>> {
    let sources = unique_ids(roots.iter().map(|(source, _)| *source));
    let rows = rows_by_ids(
        conn,
        scope,
        "oc_versions",
        "asset_id,version,source_event_id",
        "source_event_id",
        &sources,
    )
    .await?;
    let mut versions = Vec::new();
    for row in rows {
        let source = row.try_get("source_event_id").map_err(turso_err)?;
        let version = row.try_get("version").map_err(turso_err)?;
        if roots.contains(&(source, version)) {
            versions.push((row.try_get("asset_id").map_err(turso_err)?, version));
        }
    }
    let rows = version_rows(conn, scope, "oc_chunks", "id", &versions).await?;
    rows.into_iter()
        .map(|row| row.try_get("id").map_err(turso_err))
        .collect()
}

pub(super) struct CurrentChunk {
    pub hit: SearchHit,
    pub root: SourceVersion,
    pub search_terms: String,
}

/// Current root visibility only. Callers apply local support and Learning as
/// separate batch layers; startup health intentionally only needs this layer.
pub(super) async fn current_chunks(
    conn: &mut Connection,
    scope: Scope,
    ids: Option<&[Uuid]>,
    asset_filter: Option<Uuid>,
) -> StorageResult<Vec<CurrentChunk>> {
    Ok(current_chunks_measured(conn, scope, ids, asset_filter).await?.0)
}

// Counts are lengths of already-read sets, for native regression evidence.
// No extra query or payload clone is performed to obtain them.
pub(super) async fn current_chunks_measured(
    conn: &mut Connection,
    scope: Scope,
    ids: Option<&[Uuid]>,
    asset_filter: Option<Uuid>,
) -> StorageResult<(Vec<CurrentChunk>, [usize; 4])> {
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
    let counts = [rows.len(), requests.len(), asset_count, version_count];
    let mut chunks = Vec::new();
    for (index, row) in rows.into_iter().enumerate() {
        let position = positions[index];
        let (Some(asset_row), Some(version_row)) = (assets.get(&position), versions.get(&position))
        else {
            continue;
        };
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
            root: SourceVersion {
                source_id: source,
                version,
            },
            search_terms: row.try_get("search_terms").map_err(turso_err)?,
        });
    }
    Ok((chunks, counts))
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

#[cfg(feature = "local-storage")]
pub(super) async fn current_chunk_roots(
    conn: &mut Connection,
    scope: Scope,
    ids: &[Uuid],
) -> StorageResult<HashMap<Uuid, SourceVersion>> {
    let rows = rows_by_ids(conn, scope, "oc_chunks", "id,asset_id,version", "id", ids).await?;
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
    let mut roots = HashMap::new();
    for (index, root) in live_root_rows(conn, scope, &requests).await? {
        if current.contains(&index) {
            // Original SELECT: c.id, v.source_event_id, v.version.
            let id = rows[index].try_get("id").map_err(turso_err)?;
            let source_id = root.try_get("source_event_id").map_err(turso_err)?;
            let version = root.try_get("version").map_err(turso_err)?;
            roots.insert(id, SourceVersion { source_id, version });
        }
    }
    Ok(roots)
}

struct LocalSupport {
    visible: HashMap<VersionKey, bool>,
    sources: HashMap<VersionKey, Vec<native_turso::Row>>,
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
        let requests: Vec<_> = versions.iter().map(|(asset, version)| {
            vec![AssociationKey(turso::Value::Blob(asset.as_bytes().to_vec())),
                 AssociationKey(turso::Value::Integer(i64::from(*version)))]
        }).collect();
        let mut roots = HashMap::new();
        for (index, row) in live_root_rows(&mut self.tx, self.scope, &requests).await? {
            roots.insert(versions[index], row.try_get("source_event_id").map_err(turso_err)?);
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
            "asset_id,version,json_extract(locator,'$.parser') IS 'memory-semantics:v1' AS semantic,json_type(locator,'$.support_sources') AS support_type,json_quote(json_extract(locator,'$.support_sources')) AS support_json",
            versions,
        )
        .await?;
        self.evaluate_support_rows(rows, versions).await
    }

    #[cfg(feature = "local-storage")]
    pub(super) async fn local_chunk_support_visibility(
        &mut self,
        ids: &[Uuid],
    ) -> StorageResult<HashMap<Uuid, bool>> {
        let rows = rows_by_ids(
            &mut self.tx,
            self.scope,
            "oc_chunks",
            "id AS asset_id,version,json_extract(locator,'$.parser') IS 'memory-semantics:v1' AS semantic,json_type(locator,'$.support_sources') AS support_type,json_quote(json_extract(locator,'$.support_sources')) AS support_json",
            "id",
            ids,
        ).await?;
        let mut keys = Vec::new();
        for row in &rows {
            keys.push((
                row.try_get("asset_id").map_err(turso_err)?,
                row.try_get("version").map_err(turso_err)?,
            ));
        }
        Ok(self
            .evaluate_support_rows(rows, &keys)
            .await?
            .visible
            .into_iter()
            .map(|((id, _), visible)| (id, visible))
            .collect())
    }

    async fn evaluate_support_rows(
        &mut self,
        rows: Vec<native_turso::Row>,
        keys: &[VersionKey],
    ) -> StorageResult<LocalSupport> {
        let mut visible: HashMap<_, _> = keys.iter().copied().map(|key| (key, true)).collect();
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
            let normalized: String = row.try_get("support_json").map_err(turso_err)?;
            members.extend(
                support_members(&kind, &normalized)?
                    .into_iter()
                    .map(|value| (key, value)),
            );
        }
        let mut sources_by_version: HashMap<VersionKey, Vec<native_turso::Row>> = HashMap::new();
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
                    visible.insert(key, false);
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
                visible.insert(key, false);
            }
        }
        Ok(LocalSupport {
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

    pub(super) async fn retrieval_chunks(
        &mut self,
        ids: Option<&[Uuid]>,
        asset_filter: Option<Uuid>,
    ) -> StorageResult<Vec<CurrentChunk>> {
        if ids.is_some_and(<[Uuid]>::is_empty) {
            return Ok(Vec::new());
        }
        let chunks = current_chunks(&mut self.tx, self.scope, ids, asset_filter).await?;
        self.guard_retrieval_chunks(chunks).await
    }

    pub(super) async fn guard_retrieval_chunks(
        &mut self,
        mut chunks: Vec<CurrentChunk>,
    ) -> StorageResult<Vec<CurrentChunk>> {
        if chunks.is_empty() {
            return Ok(chunks);
        }
        let versions: Vec<_> = chunks
            .iter()
            .map(|chunk| (chunk.hit.asset_id, chunk.hit.version))
            .collect();
        let local = self.support_snapshot(&versions).await?;
        chunks.retain(|chunk| {
            local.visible.get(&(chunk.hit.asset_id, chunk.hit.version)) == Some(&true)
        });
        let assets = unique_ids(chunks.iter().map(|chunk| chunk.hit.asset_id));
        let learning = self.learning_assets_visible(&assets).await?;
        chunks.retain(|chunk| learning.get(&chunk.hit.asset_id) == Some(&true));
        // Preserve the separate Learning layer's strict UUID provenance rule.
        // It runs only after local support and asset visibility have succeeded.
        let mut version_sources: HashMap<VersionKey, Vec<Uuid>> = HashMap::new();
        for chunk in &chunks {
            let key = (chunk.hit.asset_id, chunk.hit.version);
            if version_sources.contains_key(&key) {
                continue;
            }
            let rows = local.sources.get(&key).map_or(&[][..], Vec::as_slice);
            version_sources.insert(key, strict_support_sources(rows)?);
        }
        let source_visibility = self.learning_source_sets_visible(&version_sources).await?;
        chunks.retain(|chunk| {
            source_visibility.get(&(chunk.hit.asset_id, chunk.hit.version)) == Some(&true)
        });
        Ok(chunks)
    }

    pub(crate) async fn visible_hits(
        &mut self,
        ids: &[Uuid],
    ) -> StorageResult<HashMap<Uuid, SearchHit>> {
        Ok(self
            .retrieval_chunks(Some(ids), None)
            .await?
            .into_iter()
            .map(|chunk| (chunk.hit.chunk_id, chunk.hit))
            .collect())
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
        ).await?;
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

    pub(super) async fn committed_publications(
        &mut self,
        ids: &[Uuid],
        surface: &str,
    ) -> StorageResult<HashSet<NativeHitKey>> {
        let rows = rows_by_ids(
            &mut self.tx,
            self.scope,
            "oc_artifact_ledger",
            "artifact_id,source_id,version,generation,surface,state",
            "artifact_id",
            ids,
        )
        .await?;
        let mut publications = HashSet::new();
        for row in rows {
            if row.try_get::<String>("state").map_err(turso_err)? == "committed"
                && row.try_get::<String>("surface").map_err(turso_err)? == surface
            {
                publications.insert((
                    row.try_get("artifact_id").map_err(turso_err)?,
                    SourceVersion {
                        source_id: row.try_get("source_id").map_err(turso_err)?,
                        version: row.try_get("version").map_err(turso_err)?,
                    },
                    row.try_get("generation").map_err(turso_err)?,
                ));
            }
        }
        Ok(publications)
    }

    pub(super) async fn ready_vector_publications(
        &mut self,
        ids: &[Uuid],
        profile: &str,
        dimension: Option<usize>,
    ) -> StorageResult<HashSet<(Uuid, i64)>> {
        let rows = rows_by_ids(
            &mut self.tx,
            self.scope,
            "oc_index_entries",
            "artifact_id,field,model_id,dimension,generation,state",
            "artifact_id",
            ids,
        )
        .await?;
        let mut ready = HashSet::new();
        for row in rows {
            if row.try_get::<String>("state").map_err(turso_err)? != "ready"
                || row.try_get::<String>("model_id").map_err(turso_err)? != profile
            {
                continue;
            }
            if let Some(dimension) = dimension
                && (row.try_get::<String>("field").map_err(turso_err)? != "content"
                    || row.try_get::<i64>("dimension").map_err(turso_err)? != dimension as i64)
            {
                continue;
            }
            ready.insert((
                row.try_get("artifact_id").map_err(turso_err)?,
                row.try_get("generation").map_err(turso_err)?,
            ));
        }
        Ok(ready)
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
