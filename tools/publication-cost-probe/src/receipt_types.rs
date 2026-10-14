const RECEIPT_METADATA_FORMAT: &str = "message-receipt:v1";

#[derive(Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ReceiptMetadata {
    format: String,
    chunks: Vec<ReceiptChunk>,
}

#[derive(Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ReceiptChunk {
    // Keep native json_quote output as a token, not a Rust interpretation of ID.
    id_json: String,
    #[serde(deserialize_with = "receipt_dimension")]
    dimension: Option<i64>,
}

fn receipt_dimension<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    serde::Deserialize::deserialize(deserializer)
}

fn invalid_plan() -> StorageError {
    StorageError::Unavailable("invalid message publication plan".into())
}

impl ReceiptMetadata {
    fn empty() -> Self {
        Self {
            format: RECEIPT_METADATA_FORMAT.into(),
            chunks: Vec::new(),
        }
    }

    fn decode(text: Option<&str>, count: usize) -> StorageResult<Self> {
        // Missing/null/empty chunks retain their original zero-ID behavior.
        if count == 0 {
            return Ok(Self::empty());
        }
        let metadata: Self =
            serde_json::from_str(text.ok_or_else(invalid_plan)?).map_err(|_| invalid_plan())?;
        if metadata.format != RECEIPT_METADATA_FORMAT || metadata.chunks.len() != count {
            return Err(invalid_plan());
        }
        Ok(metadata)
    }

    fn ids_json(&self) -> String {
        format!(
            "[{}]",
            self.chunks
                .iter()
                .map(|chunk| chunk.id_json.as_str())
                .collect::<Vec<_>>()
                .join(",")
        )
    }
}

