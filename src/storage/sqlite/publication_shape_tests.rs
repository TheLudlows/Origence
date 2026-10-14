//! Native Turso JSON projections used by receipt readiness.
//!
//! These fixtures deliberately retain malformed persisted shapes. They freeze
//! the engine boundary before a production rewrite; they do not introduce a
//! new policy for rejecting those shapes. Every query has at most one relation.

use crate::storage::native_turso as db;

async fn database() -> (tempfile::TempDir, db::Pool) {
    let dir = tempfile::tempdir().unwrap();
    let pool = db::Pool::open(dir.path().join("publication-shapes.db"))
        .await
        .unwrap();
    (dir, pool)
}

fn payload(chunks: Option<&str>) -> String {
    match chunks {
        Some(chunks) => format!("{{\"publication\":{{\"memories\":[{{\"chunks\":{chunks}}}]}}}}"),
        None => r#"{"publication":{"memories":[{}]}}"#.into(),
    }
}

#[tokio::test]
async fn receipt_json_shape_matrix_preserves_native_iteration() {
    let (_dir, pool) = database().await;
    // (name, raw chunks JSON, native JSON type, array length, rows, distinct IDs)
    // Raw strings retain duplicate object keys, unlike a serde_json map.
    let cases = [
        ("missing", None, None, None, 0, 0),
        ("null", Some("null"), Some("null"), Some(0), 1, 0),
        ("empty array", Some("[]"), Some("array"), Some(0), 0, 0),
        ("empty object", Some("{}"), Some("object"), Some(0), 0, 0),
        (
            "object values",
            Some(r#"{"a":{"id":"first"},"b":{"id":"second"}}"#),
            Some("object"),
            Some(0),
            2,
            2,
        ),
        (
            "duplicate object keys",
            Some(r#"{"a":{"id":"first"},"a":{"id":"second"}}"#),
            Some("object"),
            Some(0),
            2,
            2,
        ),
        (
            "string containing object JSON",
            Some(r#""{\"id\":\"encoded\"}""#),
            Some("text"),
            Some(0),
            1,
            1,
        ),
        (
            "string containing null JSON",
            Some(r#""null""#),
            Some("text"),
            Some(0),
            1,
            0,
        ),
        ("integer", Some("17"), Some("integer"), Some(0), 1, 0),
        ("real", Some("1.5"), Some("real"), Some(0), 1, 0),
        ("true", Some("true"), Some("true"), Some(0), 1, 0),
        ("false", Some("false"), Some("false"), Some(0), 1, 0),
    ];
    for (name, chunks, kind, length, count, distinct) in cases {
        let payload = payload(chunks);
        let actual: (Option<String>, Option<i64>) = db::query_as(
            "SELECT json_type(?,'$.publication.memories[0].chunks'),json_array_length(?,'$.publication.memories[0].chunks')",
        )
        .bind(&payload)
        .bind(&payload)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(actual.0.as_deref(), kind, "{name}: type");
        assert_eq!(actual.1, length, "{name}: array length");
        let actual: (i64, i64) = db::query_as(
            "SELECT count(*),count(DISTINCT json_extract(value,'$.id')) FROM json_each(?,'$.publication.memories[0].chunks')",
        )
        .bind(payload)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(actual, (count, distinct), "{name}: native iteration");
    }
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn receipt_json_invalid_element_errors_before_state_interpretation() {
    let (_dir, pool) = database().await;
    for chunks in [
        r#""not-json""#,
        r#"{"a":"not-json"}"#,
        r#"{"a":"not-json","a":{"id":"later"}}"#,
        r#"["not-json"]"#,
        r#"[{"id":"same"},{"id":"same"},"not-json"]"#,
        r#"[{"id":null},"not-json"]"#,
    ] {
        for state in ["pending", "failed", "completed"] {
            // This is the old first projection's eager evaluation, not a test
            // implementation of message_batch_status or a new rejection rule.
            let result: db::Result<(String, i64)> = db::query_as(
                "SELECT ?,count(DISTINCT json_extract(value,'$.id')) FROM json_each(?,'$.publication.memories[0].chunks')",
            )
            .bind(state)
            .bind(payload(Some(chunks)))
            .fetch_one(&pool)
            .await;
            assert!(
                matches!(result, Err(db::Error::Native(_))),
                "{state}, {chunks}: {result:?}"
            );
        }
    }
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn receipt_json_distinct_uses_native_value_types_before_normalization() {
    let (_dir, pool) = database().await;
    for (chunks, expected) in [
        (
            r#"[{"id":1},{"id":1.0},{"id":true},{"id":"1"},{"id":null},{}]"#,
            2,
        ),
        (r#"[{"id":0},{"id":-0.0},{"id":false},{"id":"0"}]"#, 2),
        (r#"[{"id":{"a":1}},{"id":"{\"a\":1}"}]"#, 1),
        (r#"[{"id":[1,2]},{"id":"[1,2]"}]"#, 1),
        (r#"[{"id":{"a":1,"b":2}},{"id":{"b":2,"a":1}}]"#, 2),
        (r#"[{"id":"a-b"},{"id":"AB"},{"id":"a-b"}]"#, 2),
    ] {
        let actual: i64 =
            db::query_scalar("SELECT count(DISTINCT json_extract(value,'$.id')) FROM json_each(?)")
                .bind(chunks)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(actual, expected, "{chunks}");
    }
    let normalized: i64 = db::query_scalar(
        "SELECT count(DISTINCT upper(replace(json_extract(value,'$.id'),'-',''))) FROM json_each(?)",
    )
    .bind(r#"[{"id":"a-b"},{"id":"AB"}]"#)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        normalized, 1,
        "normalization must not define raw distinctness"
    );
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn receipt_json_array_element_requires_native_second_parse() {
    let (_dir, pool) = database().await;
    let chunks = r#"[{"id":"plain","embedding":[1,2]},"{\"id\":\"encoded\",\"embedding\":[1]}"]"#;
    let actual: (Option<String>, Option<String>, Option<i64>, Option<i64>) =
        db::query_as(
            "SELECT json_extract(?,'$[1].id'),json_extract(json_extract(?,'$[1]'),'$.id'),json_array_length(?,'$[1].embedding'),json_array_length(json_extract(?,'$[1]'),'$.embedding')",
        )
        .bind(chunks)
        .bind(chunks)
        .bind(chunks)
        .bind(chunks)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(actual, (None, Some("encoded".into()), None, Some(1)));
    let ids: Vec<String> = db::query_scalar("SELECT json_extract(value,'$.id') FROM json_each(?)")
        .bind(chunks)
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(ids, ["plain", "encoded"]);
    let direct: Option<String> = db::query_scalar("SELECT json_extract(?,'$[0].id')")
        .bind(r#"["not-json"]"#)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(direct, None);
    let nested: db::Result<Option<String>> =
        db::query_scalar("SELECT json_extract(json_extract(?,'$[0]'),'$.id')")
            .bind(r#"["not-json"]"#)
            .fetch_one(&pool)
            .await;
    assert!(matches!(nested, Err(db::Error::Native(_))));
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn receipt_json_embedding_dimension_is_only_native_array_length() {
    let (_dir, pool) = database().await;
    let dimensions: Vec<Option<i64>> = db::query_scalar(
        "SELECT json_array_length(value,'$.embedding') FROM json_each(?)",
    )
    .bind(r#"[{},{"embedding":null},{"embedding":[]},{"embedding":{}},{"embedding":true},{"embedding":42},{"embedding":"not-json"},{"embedding":"[1]"},{"embedding":[null,"bad",{}]}]"#)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        dimensions,
        [
            None,
            Some(0),
            Some(0),
            Some(0),
            Some(0),
            Some(0),
            Some(0),
            Some(0),
            Some(3)
        ]
    );
    for length in [0usize, 1, 4096, 4097] {
        let chunks = serde_json::json!([{"id":"chunk","embedding":vec![0; length]}]).to_string();
        let result: (i64, bool) = db::query_as(
            "SELECT json_array_length(value,'$.embedding'),json_array_length(value,'$.embedding') BETWEEN 1 AND 4096 FROM json_each(?)",
        )
        .bind(chunks)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(result, (length as i64, (1..=4096).contains(&length)));
    }
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn receipt_json_quoted_id_metadata_retains_native_distinct_on_fixtures() {
    let (_dir, pool) = database().await;
    for chunks in [
        r#"[{"id":null},{},{"id":1},{"id":1.0},{"id":true},{"id":"1"}]"#,
        r#"[{"id":{"a":1}},{"id":"{\"a\":1}"},{"id":[1,2]},{"id":"[1,2]"}]"#,
        r#"[{"id":9007199254740993},{"id":9007199254740992.0},{"id":9223372036854775807},{"id":-9223372036854775808},{"id":1e300},{"id":-1e300},{"id":5e-324},{"id":1e-999}]"#,
        r#"[{"id":"a-b"},{"id":"AB"},{"id":"a-b"}]"#,
    ] {
        let expected: i64 =
            db::query_scalar("SELECT count(DISTINCT json_extract(value,'$.id')) FROM json_each(?)")
                .bind(chunks)
                .fetch_one(&pool)
                .await
                .unwrap();
        let tokens: Vec<String> =
            db::query_scalar("SELECT json_quote(json_extract(value,'$.id')) FROM json_each(?)")
                .bind(chunks)
                .fetch_all(&pool)
                .await
                .unwrap();
        // Preserve native tokens verbatim: no serde map, UUID decoding, or
        // String conversion of the original ID values before native DISTINCT.
        let metadata = format!("[{}]", tokens.join(","));
        let actual: i64 = db::query_scalar("SELECT count(DISTINCT value) FROM json_each(?)")
            .bind(metadata)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(actual, expected, "{chunks}");
    }
    pool.shutdown().await.unwrap();
}
