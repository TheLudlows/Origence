use anyhow::{Result, ensure};
use arrow_array::{
    FixedSizeListArray, Int32Array, Int64Array, RecordBatch, RecordBatchIterator,
    RecordBatchReader, StringArray, types::Float32Type,
};
use arrow_schema::{DataType, Field, Schema};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::HashSet, sync::Arc};

pub const FRESH_ROWS: usize = 16;
pub const CANDIDATE_LIMIT: usize = 100;
pub const ELIGIBILITY_BATCH: usize = 512;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub rows: usize,
    pub dimension: usize,
    pub queries_per_bucket: usize,
    pub partitions: u32,
    pub nprobes: usize,
    pub refine_factor: u32,
    pub index_kind: String,
    pub seed: u64,
}

impl Config {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            [10_000, 100_000, 1_000_000].contains(&self.rows),
            "unsupported row rung"
        );
        ensure!(
            [128, 384, 768, 1024].contains(&self.dimension),
            "unsupported dimension"
        );
        ensure!(
            (1..=20).contains(&self.queries_per_bucket),
            "query count out of bounds"
        );
        ensure!(
            (2..=256).contains(&self.partitions),
            "partition count out of bounds"
        );
        ensure!(
            (1..=self.partitions as usize).contains(&self.nprobes),
            "nprobes out of bounds"
        );
        ensure!(
            (1..=20).contains(&self.refine_factor),
            "refine factor out of bounds"
        );
        ensure!(
            ["ivf_flat", "ivf_pq", "ivf_hnsw_pq"].contains(&self.index_kind.as_str()),
            "unknown index"
        );
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct Row {
    pub id: String,
    pub source: String,
    pub tenant: String,
    pub workspace: String,
    pub version: i32,
    pub generation: i64,
    pub visible: bool,
    pub fresh: bool,
    pub vector: Vec<f32>,
}

fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e3779b97f4a7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
    x ^ (x >> 31)
}

// No model/provider, external input, or clock-seeded data. Clusters are intentionally
// easy synthetic geometry: passing this fixture cannot establish embedding recall.
pub fn make_row(i: usize, config: &Config, fresh: bool) -> Row {
    let logical = i / 2;
    let historical = !fresh && i.is_multiple_of(2);
    let category = logical % 23;
    let cluster = logical % 16;
    let vector = (0..config.dimension)
        .map(|d| {
            let center = (mix(config.seed ^ ((cluster * config.dimension + d) as u64)) >> 40)
                as f32
                / (1_u32 << 24) as f32;
            let noise = (mix(config.seed ^ ((i * config.dimension + d) as u64) ^ 0xa55a) >> 40)
                as f32
                / (1_u32 << 24) as f32;
            center * 2.0 + noise * 0.12
        })
        .collect();
    Row {
        // Historic and current artifact IDs are distinct in this fixture. Reused-ID
        // stale-native/final-verification adversaries are injected separately.
        id: format!("artifact-{i:09}"),
        source: format!("source-{:07}", logical / 8),
        tenant: if !fresh && category == 0 {
            "tenant-b"
        } else {
            "tenant-a"
        }
        .into(),
        workspace: if !fresh && category == 1 {
            "workspace-b"
        } else {
            "workspace-a"
        }
        .into(),
        version: if historical { 1 } else { 2 },
        generation: if historical { 0 } else { 1 },
        visible: fresh || category != 2,
        fresh,
        vector,
    }
}

pub fn schema(dimension: usize) -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("tenant", DataType::Utf8, false),
        Field::new("workspace", DataType::Utf8, false),
        Field::new("id", DataType::Utf8, false),
        Field::new("source_id", DataType::Utf8, false),
        Field::new("version", DataType::Int32, false),
        Field::new("generation", DataType::Int64, false),
        Field::new(
            "vector",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                dimension as i32,
            ),
            true,
        ),
    ]))
}

pub fn reader(rows: &[Row], dimension: usize) -> Result<Box<dyn RecordBatchReader + Send>> {
    let schema = schema(dimension);
    let strings = |get: fn(&Row) -> &str| {
        Arc::new(StringArray::from(rows.iter().map(get).collect::<Vec<_>>()))
    };
    let vectors = rows
        .iter()
        .map(|r| Some(r.vector.iter().copied().map(Some).collect::<Vec<_>>()));
    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            strings(|r| &r.tenant),
            strings(|r| &r.workspace),
            strings(|r| &r.id),
            strings(|r| &r.source),
            Arc::new(Int32Array::from(
                rows.iter().map(|r| r.version).collect::<Vec<_>>(),
            )),
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.generation).collect::<Vec<_>>(),
            )),
            Arc::new(
                FixedSizeListArray::from_iter_primitive::<Float32Type, _, _>(
                    vectors,
                    dimension as i32,
                ),
            ),
        ],
    )?;
    Ok(Box::new(RecordBatchIterator::new(
        vec![Ok(batch)].into_iter(),
        schema,
    )))
}

#[derive(Debug, Clone)]
pub struct Bucket {
    pub name: &'static str,
    pub tenant: &'static str,
    pub workspace: &'static str,
    pub generation: i64,
    pub version: i32,
    pub source: Option<String>,
    pub stride: usize,
    pub cap: Option<usize>,
    pub fresh_only: bool,
}

impl Bucket {
    pub fn validate_cardinality(&self, count: usize) -> Result<()> {
        let valid = match self.name {
            "empty" => count == 0,
            "one" => count == 1,
            "lt_10" => count == 7,
            "lt_100" => count == 37,
            "source_version" => count == 8,
            "fresh_unindexed" => count == FRESH_ROWS,
            "dense_current" | "historical_generation" => count > ELIGIBILITY_BATCH,
            _ => count > 0,
        };
        ensure!(
            valid,
            "fixture bucket {} has unexpected cardinality {}",
            self.name,
            count
        );
        Ok(())
    }

    pub fn accepts(&self, r: &Row) -> bool {
        r.visible
            && r.tenant == self.tenant
            && r.workspace == self.workspace
            && r.generation == self.generation
            && r.version == self.version
            && self.source.as_ref().is_none_or(|s| s == &r.source)
            && (!self.fresh_only || r.fresh)
    }
    pub fn eligible(&self, rows: &[Row]) -> Vec<usize> {
        rows.iter()
            .enumerate()
            .filter(|(_, r)| self.accepts(r))
            .step_by(self.stride)
            .take(self.cap.unwrap_or(usize::MAX))
            .map(|(i, _)| i)
            .collect()
    }
    // Exactly one merged AND expression reaches only_if(). Repeated only_if calls
    // replace the filter in LanceDB 0.40.0 and must not be used as conjunctions.
    pub fn filter(&self, rows: &[Row], ids: &[usize]) -> String {
        let mut terms = vec![
            format!("tenant = '{}'", quote(self.tenant)),
            format!("workspace = '{}'", quote(self.workspace)),
            format!("generation = {}", self.generation),
            format!("version = {}", self.version),
        ];
        if let Some(source) = &self.source {
            terms.push(format!("source_id = '{}'", quote(source)));
        }
        terms.push(if ids.is_empty() {
            "1 = 0".into()
        } else {
            format!(
                "id IN ({})",
                ids.iter()
                    .map(|&i| format!("'{}'", quote(&rows[i].id)))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        });
        terms
            .into_iter()
            .map(|t| format!("({t})"))
            .collect::<Vec<_>>()
            .join(" AND ")
    }
}

pub fn quote(s: &str) -> String {
    s.replace('\'', "''")
}

pub fn buckets(include_fresh: bool) -> Vec<Bucket> {
    let base = Bucket {
        name: "dense_current",
        tenant: "tenant-a",
        workspace: "workspace-a",
        generation: 1,
        version: 2,
        source: None,
        stride: 1,
        cap: None,
        fresh_only: false,
    };
    let mut out = vec![
        Bucket {
            name: "empty",
            cap: Some(0),
            ..base.clone()
        },
        Bucket {
            name: "one",
            cap: Some(1),
            ..base.clone()
        },
        Bucket {
            name: "lt_10",
            cap: Some(7),
            ..base.clone()
        },
        Bucket {
            name: "lt_100",
            cap: Some(37),
            ..base.clone()
        },
        base.clone(),
        Bucket {
            name: "sparse",
            stride: 97,
            ..base.clone()
        },
        Bucket {
            name: "other_tenant",
            tenant: "tenant-b",
            ..base.clone()
        },
        Bucket {
            name: "other_workspace",
            workspace: "workspace-b",
            ..base.clone()
        },
        Bucket {
            name: "source_version",
            source: Some("source-0000001".into()),
            ..base.clone()
        },
        Bucket {
            name: "historical_generation",
            generation: 0,
            version: 1,
            ..base.clone()
        },
    ];
    if include_fresh {
        out.push(Bucket {
            name: "fresh_unindexed",
            fresh_only: true,
            ..base
        });
    }
    out
}

pub fn query_vector(rows: &[Row], eligible: &[usize], query: usize, dimension: usize) -> Vec<f32> {
    let Some(&index) = eligible.get((query * 997) % eligible.len().max(1)) else {
        return vec![0.25; dimension];
    };
    rows[index]
        .vector
        .iter()
        .enumerate()
        .map(|(d, v)| v + ((d + query) % 7) as f32 * 0.0001)
        .collect()
}

pub fn unique_ids(rows: &[Row]) -> bool {
    let ids: HashSet<_> = rows.iter().map(|r| &r.id).collect();
    ids.len() == rows.len()
}

// Canonical fixture digest includes provenance, visibility and full vector bytes.
// Lengths/integers use little-endian fixed widths so the digest is architecture-independent.
pub fn digest(rows: &[Row]) -> String {
    let mut hash = Sha256::new();
    hash.update((rows.len() as u64).to_le_bytes());
    for row in rows {
        for value in [&row.id, &row.source, &row.tenant, &row.workspace] {
            hash.update((value.len() as u64).to_le_bytes());
            hash.update(value.as_bytes());
        }
        hash.update(row.version.to_le_bytes());
        hash.update(row.generation.to_le_bytes());
        hash.update([u8::from(row.visible), u8::from(row.fresh)]);
        hash.update((row.vector.len() as u64).to_le_bytes());
        for value in &row.vector {
            hash.update(value.to_bits().to_le_bytes());
        }
    }
    format!("{:x}", hash.finalize())
}
