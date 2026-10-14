use super::*;
use crate::storage::native_turso::Pool;

async fn database() -> (tempfile::TempDir, Pool) {
    let dir = tempfile::tempdir().unwrap();
    let pool = Pool::open(dir.path().join("startup.db")).await.unwrap();
    // Diagnostic fixtures allow raw and missing keys. Affinities match the
    // authority schema; unrelated metadata is deliberately not decoded.
    for sql in [
        "CREATE TABLE oc_chunks(tenant_id BLOB,workspace_id BLOB,id BLOB,asset_id BLOB,version INTEGER,content TEXT,locator TEXT)",
        "CREATE TABLE oc_assets(tenant_id BLOB,workspace_id BLOB,id BLOB,current_version INTEGER,deleted INTEGER,kind TEXT)",
        "CREATE TABLE oc_versions(tenant_id BLOB,workspace_id BLOB,asset_id BLOB,version INTEGER,source_event_id BLOB,title TEXT)",
        "CREATE TABLE oc_events(tenant_id BLOB,workspace_id BLOB,id BLOB,state TEXT,kind TEXT)",
        "CREATE TABLE oc_artifact_ledger(tenant_id BLOB,workspace_id BLOB,artifact_id BLOB,source_id BLOB,version INTEGER,generation INTEGER,artifact_type TEXT,surface TEXT,state TEXT)",
        "CREATE TABLE oc_index_entries(tenant_id BLOB,workspace_id BLOB,artifact_id BLOB,generation INTEGER,field TEXT,state TEXT,model_id TEXT,dimension INTEGER)",
    ] {
        db::query(sql).execute(&pool).await.unwrap();
    }
    (dir, pool)
}

fn scope() -> Scope {
    Scope { tenant_id: Uuid::new_v4(), workspace_id: Uuid::new_v4() }
}

fn chunk(n: u128) -> Uuid { Uuid::from_u128(10_000 + n) }
fn asset(n: u128) -> Uuid { Uuid::from_u128(20_000 + n) }
fn source(n: u128) -> Uuid { Uuid::from_u128(30_000 + n) }

async fn seed(conn: &mut Connection, scope: Scope, n: u128, surface: &str) {
    db::query("INSERT INTO oc_chunks VALUES(?,?,?,?,1,x'31','{malformed')")
        .bind(scope.tenant_id).bind(scope.workspace_id).bind(chunk(n)).bind(asset(n))
        .execute(&mut *conn).await.unwrap();
    db::query("INSERT INTO oc_assets VALUES(?,?,?,1,0,x'31')")
        .bind(scope.tenant_id).bind(scope.workspace_id).bind(asset(n))
        .execute(&mut *conn).await.unwrap();
    db::query("INSERT INTO oc_versions VALUES(?,?,?,1,?,x'31')")
        .bind(scope.tenant_id).bind(scope.workspace_id).bind(asset(n)).bind(source(n))
        .execute(&mut *conn).await.unwrap();
    db::query("INSERT INTO oc_events VALUES(?,?,?,'active',x'31')")
        .bind(scope.tenant_id).bind(scope.workspace_id).bind(source(n))
        .execute(&mut *conn).await.unwrap();
    db::query("INSERT INTO oc_artifact_ledger VALUES(?,?,?,?,1,7,'chunk',?,'committed')")
        .bind(scope.tenant_id).bind(scope.workspace_id).bind(chunk(n)).bind(source(n)).bind(surface)
        .execute(&mut *conn).await.unwrap();
}

async fn index(conn: &mut Connection, scope: Scope, n: u128, model: &str, dimension: i64) {
    db::query("INSERT INTO oc_index_entries VALUES(?,?,?,7,'content','ready',?,?)")
        .bind(scope.tenant_id).bind(scope.workspace_id).bind(chunk(n)).bind(model).bind(dimension)
        .execute(&mut *conn).await.unwrap();
}

#[tokio::test]
async fn full_text_stops_after_first_matching_page_without_decoding_payloads() {
    let (_dir, pool) = database().await;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let scope = scope();
    for n in 0..140 {
        seed(&mut tx, scope, n, "full_text").await;
    }
    // The very first valid row can have the minimum native rowid. Later pages
    // contain the same invalid kind/title/content values and must stay unread.
    db::query("UPDATE oc_artifact_ledger SET rowid=? WHERE artifact_id=?")
        .bind(i64::MIN).bind(chunk(0)).execute(&mut *tx).await.unwrap();
    let mut pages = Pages::default();
    assert!(full_text_in_pages(&mut tx, &mut pages).await.unwrap());
    assert_eq!((pages.pages_read, pages.rows_read), (1, PAGE_SIZE));
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn full_text_reads_later_pages_until_a_match_or_exhaustion() {
    let (_dir, pool) = database().await;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let scope = scope();
    for n in 0..130 {
        seed(&mut tx, scope, n, "full_text").await;
    }
    db::query("UPDATE oc_assets SET deleted=1 WHERE id<>?")
        .bind(asset(129)).execute(&mut *tx).await.unwrap();
    let mut pages = Pages::default();
    assert!(full_text_in_pages(&mut tx, &mut pages).await.unwrap());
    assert_eq!((pages.pages_read, pages.rows_read), (5, 130));
    db::query("UPDATE oc_assets SET deleted=1").execute(&mut *tx).await.unwrap();
    let mut pages = Pages::default();
    assert!(!full_text_in_pages(&mut tx, &mut pages).await.unwrap());
    assert_eq!((pages.pages_read, pages.rows_read), (5, 130));
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn vector_profiles_cover_all_pages_and_filter_before_strict_decode() {
    let (_dir, pool) = database().await;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let scope = scope();
    for n in 0..140 {
        seed(&mut tx, scope, n, "vector").await;
        index(&mut tx, scope, n, if n < 130 { "alpha" } else { "beta" }, 3).await;
    }
    // These rows share the exact artifact but fail one native index predicate.
    for sql in [
        "INSERT INTO oc_index_entries VALUES(?,?,?,8,'content','ready',x'31',x'31')",
        "INSERT INTO oc_index_entries VALUES(?,?,?,7,'summary','ready',x'31',x'31')",
        "INSERT INTO oc_index_entries VALUES(?,?,?,7,'content','pending',x'31',x'31')",
        "INSERT INTO oc_index_entries VALUES(?,?,?,7,NULL,'ready',x'31',x'31')",
        "INSERT INTO oc_index_entries VALUES(?,?,?,NULL,'content','ready',x'31',x'31')",
    ] {
        db::query(sql).bind(scope.tenant_id).bind(scope.workspace_id).bind(chunk(0))
            .execute(&mut *tx).await.unwrap();
    }
    let mut pages = Pages::default();
    assert_eq!(profiles_in_pages(&mut tx, &mut pages).await.unwrap(),
        vec![("alpha".into(), 3), ("beta".into(), 3)]);
    assert_eq!((pages.pages_read, pages.rows_read), (5, 140));
    // Same-transaction writes are observed by a new call, with no cross-call cache.
    db::query("UPDATE oc_artifact_ledger SET state='withdrawn'")
        .execute(&mut *tx).await.unwrap();
    assert!(vector_profiles(&mut tx).await.unwrap().is_empty());
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn startup_exact_keys_preserve_scope_current_source_and_native_nulls() {
    let (_dir, pool) = database().await;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let local = scope();
    let foreign = Scope { tenant_id: local.tenant_id, workspace_id: Uuid::new_v4() };
    seed(&mut tx, local, 0, "full_text").await;
    // A same-UUID foreign ledger cannot borrow local metadata.
    db::query("UPDATE oc_artifact_ledger SET workspace_id=?")
        .bind(foreign.workspace_id).execute(&mut *tx).await.unwrap();
    assert!(!has_full_text(&mut tx).await.unwrap());
    db::query("UPDATE oc_artifact_ledger SET workspace_id=?")
        .bind(local.workspace_id).execute(&mut *tx).await.unwrap();
    assert!(has_full_text(&mut tx).await.unwrap());
    db::query("UPDATE oc_artifact_ledger SET tenant_id=?")
        .bind(Uuid::new_v4()).execute(&mut *tx).await.unwrap();
    assert!(!has_full_text(&mut tx).await.unwrap());
    db::query("UPDATE oc_artifact_ledger SET tenant_id=?")
        .bind(local.tenant_id).execute(&mut *tx).await.unwrap();
    for sql in [
        "UPDATE oc_assets SET current_version=2",
        "UPDATE oc_assets SET current_version=NULL",
        "UPDATE oc_assets SET current_version=1,deleted=2",
        "UPDATE oc_assets SET deleted=NULL",
    ] {
        db::query(sql).execute(&mut *tx).await.unwrap();
        assert!(!has_full_text(&mut tx).await.unwrap());
    }
    db::query("UPDATE oc_assets SET current_version=1,deleted=0").execute(&mut *tx).await.unwrap();
    for sql in [
        "UPDATE oc_artifact_ledger SET source_id=x'31'",
        "UPDATE oc_artifact_ledger SET source_id=NULL",
    ] {
        db::query(sql).execute(&mut *tx).await.unwrap();
        assert!(!has_full_text(&mut tx).await.unwrap());
    }
    db::query("UPDATE oc_artifact_ledger SET source_id=?")
        .bind(source(0)).execute(&mut *tx).await.unwrap();
    for state in ["retracted", "missing"] {
        db::query("UPDATE oc_events SET state=?").bind(state).execute(&mut *tx).await.unwrap();
        assert!(!has_full_text(&mut tx).await.unwrap());
    }
    db::query("UPDATE oc_events SET state='active'").execute(&mut *tx).await.unwrap();
    // Matching raw keys are SQL values, not UUIDs: the former EXISTS also
    // accepts these byte-identical short BLOBs. Integer affinity keeps '01'=1.
    db::query("UPDATE oc_events SET id=x'31'").execute(&mut *tx).await.unwrap();
    db::query("UPDATE oc_versions SET source_event_id=x'31',version='01'")
        .execute(&mut *tx).await.unwrap();
    db::query("UPDATE oc_artifact_ledger SET source_id=x'31',version='01'")
        .execute(&mut *tx).await.unwrap();
    assert!(has_full_text(&mut tx).await.unwrap());
    db::query("UPDATE oc_artifact_ledger SET generation=x'31'").execute(&mut *tx).await.unwrap();
    assert!(has_full_text(&mut tx).await.unwrap(), "FTS existence does not inspect generation");
    db::query("UPDATE oc_artifact_ledger SET source_id='1'").execute(&mut *tx).await.unwrap();
    assert!(!has_full_text(&mut tx).await.unwrap(), "TEXT must not equal a BLOB key");
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn selected_profile_decode_precedes_dimension_validation_across_pages() {
    let (_dir, pool) = database().await;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let scope = scope();
    for n in 0..33 {
        seed(&mut tx, scope, n, "vector").await;
    }
    index(&mut tx, scope, 0, "invalid dimension", 0).await;
    db::query("INSERT INTO oc_index_entries VALUES(?,?,?,7,'content','ready',x'31',3)")
        .bind(scope.tenant_id).bind(scope.workspace_id).bind(chunk(32))
        .execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let mut tx = pool.begin().await.unwrap();
    assert!(matches!(vector_profiles(&mut tx).await, Err(StorageError::Backend(_))));
    tx.rollback().await.unwrap();
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    db::query("DELETE FROM oc_index_entries WHERE artifact_id=?")
        .bind(chunk(32)).execute(&mut *tx).await.unwrap();
    assert!(matches!(vector_profiles(&mut tx).await, Err(StorageError::Conflict(_))));
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn empty_startup_still_prepares_every_original_relation() {
    for missing in ["oc_chunks", "oc_assets", "oc_versions", "oc_events", "oc_artifact_ledger", "oc_index_entries"] {
        let (_dir, pool) = database().await;
        db::query(&format!("DROP TABLE {missing}")).execute(&pool).await.unwrap();
        let mut tx = pool.begin().await.unwrap();
        if missing == "oc_index_entries" {
            assert!(!has_full_text(&mut tx).await.unwrap());
        } else {
            assert!(matches!(has_full_text(&mut tx).await, Err(StorageError::Backend(_))));
        }
        tx.rollback().await.unwrap();
        let mut tx = pool.begin().await.unwrap();
        assert!(matches!(vector_profiles(&mut tx).await, Err(StorageError::Backend(_))));
        tx.rollback().await.unwrap();
        pool.shutdown().await.unwrap();
    }
    // Only the vector query referenced generation in the original ledger SQL.
    let (_dir, pool) = database().await;
    db::query("DROP TABLE oc_artifact_ledger").execute(&pool).await.unwrap();
    db::query("CREATE TABLE oc_artifact_ledger(tenant_id BLOB,workspace_id BLOB,artifact_id BLOB,source_id BLOB,version INTEGER,artifact_type TEXT,surface TEXT,state TEXT)")
        .execute(&pool).await.unwrap();
    let mut tx = pool.begin().await.unwrap();
    assert!(!has_full_text(&mut tx).await.unwrap());
    tx.rollback().await.unwrap();
    let mut tx = pool.begin().await.unwrap();
    assert!(matches!(vector_profiles(&mut tx).await, Err(StorageError::Backend(_))));
    tx.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn startup_pages_share_the_existing_transaction_snapshot() {
    let (_dir, pool) = database().await;
    let mut writer = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    seed(&mut writer, scope(), 0, "full_text").await;
    writer.commit().await.unwrap();
    let mut reader = pool.begin().await.unwrap();
    assert!(has_full_text(&mut reader).await.unwrap());
    let mut writer = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    db::query("UPDATE oc_events SET state='retracted'")
        .execute(&mut *writer).await.unwrap();
    writer.commit().await.unwrap();
    assert!(has_full_text(&mut reader).await.unwrap());
    reader.rollback().await.unwrap();
    let mut reader = pool.begin().await.unwrap();
    assert!(!has_full_text(&mut reader).await.unwrap());
    reader.rollback().await.unwrap();
    pool.shutdown().await.unwrap();
}
