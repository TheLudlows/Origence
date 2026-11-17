//! Owner-bound graph support. Candidate projections never grant visibility.
use super::*;
use crate::{
    graph::{entity_id, relation_id},
    types::GraphExtraction,
};

const MAX_GRAPH_ROWS: usize = 1000;
const MAX_GRAPH_TERMS_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug)]
pub struct GraphPublication {
    pub source: SourceVersion,
    pub generation: i64,
    pub graph: GraphExtraction,
}
#[derive(Clone, Debug, PartialEq)]
pub struct GraphEntityCandidate {
    pub id: Uuid,
    pub name: String,
    pub source: SourceVersion,
    pub generation: i64,
}
#[derive(Clone, Debug, PartialEq)]
pub struct GraphEdgeCandidate {
    pub id: Uuid,
    pub from: Uuid,
    pub to: Uuid,
    pub predicate: String,
    pub from_name: String,
    pub to_name: String,
    pub source: SourceVersion,
    pub generation: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(super) struct Projection {
    entities: Vec<String>,
    edges: Vec<Edge>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
struct Edge {
    from: Uuid,
    to: Uuid,
    predicate: String,
    from_name: String,
    to_name: String,
}
fn normalized(s: &str) -> String {
    s.trim().to_lowercase()
}
// O(n) bounded Unicode scalar grams, not O(n²) substring materialization.
fn grams(name: &str) -> Vec<String> {
    let chars: Vec<_> = name.chars().collect();
    let mut keys = std::collections::BTreeSet::new();
    for n in 1..=3 {
        for window in chars.windows(n) {
            keys.insert(format!("g{}", hash(window.iter().collect::<String>())));
        }
    }
    keys.into_iter().collect()
}
fn name_valid(name: &str) -> bool {
    !name.is_empty() && name.len() <= 768 && !name.contains('\0') && normalized(name) == name
}

pub(super) fn prepare(
    publication: Option<GraphPublication>,
    entries: &[FullTextEntry],
) -> StorageResult<Option<(Uuid, Projection)>> {
    let Some(p) = publication else {
        return Ok(None);
    };
    if entries.is_empty()
        || p.source.version <= 0
        || p.generation <= 0
        || entries
            .iter()
            .any(|e| e.source != p.source || e.generation != p.generation)
    {
        return Err(invalid("graph publication requires matching source chunks"));
    }
    crate::graph::parse_graph_extraction(
        &serde_json::to_value(&p.graph).map_err(|_| inconsistent())?,
    )
    .map_err(|_| invalid("invalid graph publication"))?;
    let entities = p
        .graph
        .entities
        .iter()
        .map(|e| normalized(&e.name))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let edges = p
        .graph
        .relations
        .iter()
        .map(|e| {
            let from_name = normalized(&e.source);
            let to_name = normalized(&e.target);
            Edge {
                from: entity_id(&from_name),
                to: entity_id(&to_name),
                predicate: normalized(&e.predicate),
                from_name,
                to_name,
            }
        })
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let projection = Projection { entities, edges };
    if projection.entities.is_empty() && projection.edges.is_empty() {
        return Ok(None);
    }
    Ok(Some((
        entries.iter().map(|e| e.id).min().unwrap(),
        projection,
    )))
}
fn projection_keys(p: &Projection) -> Vec<String> {
    p.entities
        .iter()
        .flat_map(|n| grams(n))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}
fn projection_endpoints(p: &Projection) -> Vec<String> {
    p.edges
        .iter()
        .flat_map(|e| [e.from.to_string(), e.to.to_string()])
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}
pub(super) fn attach(
    docs: &mut BTreeMap<String, Document>,
    prepared: Option<(Uuid, Projection)>,
) -> StorageResult<()> {
    if let Some((anchor, p)) = prepared {
        let doc = docs
            .values_mut()
            .find(|d| d.chunk_id == anchor)
            .ok_or_else(inconsistent)?;
        doc.graph_keys = Some(projection_keys(&p));
        doc.endpoint_ids = Some(projection_endpoints(&p));
        doc.graph = Some(p);
    }
    Ok(())
}
fn set_filter(field: &str, values: impl IntoIterator<Item = String>) -> Value {
    json!({"StringSet":{"field":field,"filter":values.into_iter().collect::<Vec<_>>()}})
}
pub(super) fn validate_document(doc: &Document) -> StorageResult<()> {
    if doc.kind != "chunk" {
        return Err(inconsistent());
    }
    let Some(p) = &doc.graph else {
        return if doc.graph_keys.is_none() && doc.endpoint_ids.is_none() {
            Ok(())
        } else {
            Err(inconsistent())
        };
    };
    if p.entities.len() > 20
        || p.edges.len() > 20
        || p.entities.is_empty()
        || p.entities.iter().any(|n| !name_valid(n))
        || p.entities.windows(2).any(|w| w[0] >= w[1])
        || p.edges.windows(2).any(|w| w[0] >= w[1])
    {
        return Err(inconsistent());
    }
    for e in &p.edges {
        if !name_valid(&e.predicate)
            || entity_id(&e.from_name) != e.from
            || entity_id(&e.to_name) != e.to
            || !p.entities.contains(&e.from_name)
            || !p.entities.contains(&e.to_name)
        {
            return Err(inconsistent());
        }
    }
    if doc.graph_keys.as_ref() != Some(&projection_keys(p))
        || doc.endpoint_ids.as_ref() != Some(&projection_endpoints(p))
    {
        return Err(inconsistent());
    }
    Ok(())
}
impl AnySeekFullTextStore {
    async fn graph_rows(&self, scope: Scope, extra: Vec<Value>) -> StorageResult<Vec<Document>> {
        let mut filters = scope_filters(scope);
        filters.push(filter("kind", ["chunk".into()]));
        filters.extend(extra);
        let mut fields = META.to_vec();
        fields.extend(["graph", "graph_keys", "endpoint_ids"]);
        let (status,result)=self.request(Method::POST,"/query",Some(json!({"query":"","filter":filters,"fields":fields,"length":MAX_GRAPH_ROWS+1,"count":true}))).await?;
        if !status.is_success() {
            return Err(unavailable());
        }
        let hits = result["hits"].as_array().ok_or_else(unavailable)?;
        let count = result["count_total"].as_u64().ok_or_else(unavailable)?;
        if count > MAX_GRAPH_ROWS as u64 || hits.len() > MAX_GRAPH_ROWS {
            return Err(invalid("graph candidate bound exceeded"));
        }
        if count != hits.len() as u64 {
            return Err(unavailable());
        }
        let mut out = BTreeMap::new();
        for hit in hits {
            let (id, doc) = decode_record(&hit["record"], scope)?;
            if doc.graph.is_none() || out.insert(id, doc).is_some() {
                return Err(inconsistent());
            }
        }
        Ok(out.into_values().collect())
    }
    /// Indexed substring candidates, followed by exact normalized substring matching.
    pub async fn graph_seed_candidates(
        &self,
        scope: Scope,
        terms: &str,
    ) -> StorageResult<Vec<GraphEntityCandidate>> {
        // Origence validates the raw query separately; segmentation can expand it.
        if terms.len() > MAX_GRAPH_TERMS_BYTES {
            return Err(invalid("graph query is too large"));
        }
        let terms: Vec<_> = terms
            .split_whitespace()
            .map(normalized)
            .filter(|t| !t.is_empty())
            .collect();
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        let query = terms
            .iter()
            .map(|t| format!("g{}", hash(t.chars().take(3).collect::<String>())))
            .collect::<std::collections::BTreeSet<_>>();
        let rows = self
            .graph_rows(scope, vec![set_filter("graph_keys", query)])
            .await?;
        entity_candidates(rows, |name| terms.iter().any(|t| name.contains(t)))
    }
    /// Union exact incoming/outgoing endpoints; no approximate text interpretation.
    pub async fn graph_neighbors(
        &self,
        scope: Scope,
        seeds: &[Uuid],
    ) -> StorageResult<Vec<GraphEdgeCandidate>> {
        if seeds.len() > 20 {
            return Err(invalid("graph seed bound exceeded"));
        }
        if seeds.is_empty() {
            return Ok(Vec::new());
        }
        let rows = self
            .graph_rows(
                scope,
                vec![set_filter(
                    "endpoint_ids",
                    seeds.iter().map(Uuid::to_string),
                )],
            )
            .await?;
        let mut out = Vec::new();
        for doc in rows {
            let source = SourceVersion {
                source_id: doc.source_id,
                version: doc.source_version.parse().map_err(|_| inconsistent())?,
            };
            let generation = doc.generation.parse().map_err(|_| inconsistent())?;
            for e in doc.graph.ok_or_else(inconsistent)?.edges {
                if !seeds.contains(&e.from) && !seeds.contains(&e.to) {
                    continue;
                }
                out.push(GraphEdgeCandidate {
                    id: relation_id(e.from, &e.predicate, e.to),
                    from: e.from,
                    to: e.to,
                    predicate: e.predicate,
                    from_name: e.from_name,
                    to_name: e.to_name,
                    source,
                    generation,
                });
                if out.len() > MAX_GRAPH_ROWS {
                    return Err(invalid("graph expanded candidate bound exceeded"));
                }
            }
        }
        out.sort_by_key(|e| (e.id, e.source.source_id, e.source.version, e.generation));
        if out.windows(2).any(|w| {
            w[0].id == w[1].id && w[0].source == w[1].source && w[0].generation == w[1].generation
        }) {
            return Err(inconsistent());
        }
        Ok(out)
    }
}
fn entity_candidates(
    rows: Vec<Document>,
    matches: impl Fn(&str) -> bool,
) -> StorageResult<Vec<GraphEntityCandidate>> {
    let mut out = Vec::new();
    for doc in rows {
        let source = SourceVersion {
            source_id: doc.source_id,
            version: doc.source_version.parse().map_err(|_| inconsistent())?,
        };
        let generation = doc.generation.parse().map_err(|_| inconsistent())?;
        for name in doc.graph.ok_or_else(inconsistent)?.entities {
            if matches(&name) {
                out.push(GraphEntityCandidate {
                    id: entity_id(&name),
                    name,
                    source,
                    generation,
                });
                if out.len() > MAX_GRAPH_ROWS {
                    return Err(invalid("graph expanded candidate bound exceeded"));
                }
            }
        }
    }
    out.sort_by_key(|e| (e.id, e.source.source_id, e.source.version, e.generation));
    if out.windows(2).any(|w| {
        w[0].id == w[1].id && w[0].source == w[1].source && w[0].generation == w[1].generation
    }) {
        return Err(inconsistent());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Scope, GraphPublication, Vec<FullTextEntry>) {
        let scope = Scope {
            tenant_id: Uuid::new_v4(),
            workspace_id: Uuid::new_v4(),
        };
        let source = SourceVersion {
            source_id: Uuid::new_v4(),
            version: 1,
        };
        let graph=crate::graph::parse_graph_extraction(&json!({"entities":[{"name":" Atlas数据库 ","entity_type":"project","description":"desc"},{"name":"支付","entity_type":"team","description":""}],"relations":[{"source":" Atlas数据库 ","predicate":" OWNED_BY ","target":"支付"}]})).unwrap();
        (
            scope,
            GraphPublication {
                source,
                generation: 1,
                graph,
            },
            vec![FullTextEntry {
                id: Uuid::new_v4(),
                source,
                generation: 1,
                search_terms: "chunk".into(),
                payload: crate::storage::ChunkPayload {
                    content: "chunk".into(),
                    locator: json!({}),
                    summary: None,
                },
            }],
        )
    }
    #[test]
    fn bounded_grams_preserve_every_unicode_substring_candidate() {
        let name = "atlas数据库é😃";
        let chars: Vec<_> = name.chars().collect();
        let keys = grams(name);
        assert!(keys.len() <= 3 * chars.len());
        for i in 0..chars.len() {
            for j in i + 1..=chars.len() {
                let term: String = chars[i..j].iter().collect();
                let key = format!("g{}", hash(term.chars().take(3).collect::<String>()));
                assert!(keys.contains(&key));
            }
        }
    }
    #[test]
    fn attached_support_is_canonical_and_exact() {
        let (_, p, mut entries) = fixture();
        let mut other = entries[0].clone();
        other.id = Uuid::from_u128(1);
        entries.push(other);
        let (anchor, projection) = prepare(Some(p.clone()), &entries).unwrap().unwrap();
        assert_eq!(anchor, Uuid::from_u128(1));
        assert_eq!(projection.entities, vec!["atlas数据库", "支付"]);
        assert_eq!(projection.edges.len(), 1);
        assert!(projection_keys(&projection).len() < 100);
        let mut looped = p.clone();
        looped.graph.relations[0].target = looped.graph.relations[0].source.clone();
        let (_, projection) = prepare(Some(looped), &entries).unwrap().unwrap();
        assert_eq!(projection_endpoints(&projection).len(), 1);
        assert!(prepare(Some(p.clone()), &[]).is_err());
        let (scope, _, _) = fixture();
        let mut docs = BTreeMap::new();
        for entry in &entries {
            let doc:Document=serde_json::from_value(json!({"format":FORMAT,"kind":"chunk","tenant":scope.tenant_id,"workspace":scope.workspace_id,"chunk_id":entry.id,"source_id":entry.source.source_id,"source_version":"1","generation":"1","publication":"test","terms":"unchanged","profile":"test","dimension":"2","vector":[1.0,0.0]})).unwrap();
            docs.insert(record_id(&doc), doc);
        }
        let ids = docs.keys().cloned().collect::<Vec<_>>();
        attach(&mut docs, prepare(Some(p), &entries).unwrap()).unwrap();
        assert_eq!(ids, docs.keys().cloned().collect::<Vec<_>>());
        assert_eq!(docs.len(), 2);
        for doc in docs.values() {
            assert_eq!(doc.terms.as_deref(), Some("unchanged"));
            assert_eq!(doc.vector, Some(vec![1.0, 0.0]));
            validate_document(doc).unwrap();
        }
        let mut anchor = docs.values().find(|d| d.graph.is_some()).unwrap().clone();
        anchor.graph_keys = Some(vec![]);
        assert!(validate_document(&anchor).is_err());
    }
}
