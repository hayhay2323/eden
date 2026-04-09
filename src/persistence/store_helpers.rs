use serde::de::DeserializeOwned;
use serde::Serialize;
use surrealdb::engine::local::Db;
use surrealdb::Surreal;

pub(super) type StoreError = Box<dyn std::error::Error + Send + Sync>;

pub(super) fn upsert_json_query<T: Serialize>(
    table: &str,
    id: &str,
    value: &T,
) -> Result<String, StoreError> {
    let mut content_value = serde_json::to_value(value)?;
    strip_nulls(&mut content_value);
    let content = serde_json::to_string(&content_value)?;
    let escaped_id = id.replace('\\', "\\\\").replace('\'', "\\'");
    Ok(format!(
        "UPSERT type::thing('{table}', '{escaped_id}') CONTENT {content};",
    ))
}

pub(super) async fn upsert_record_checked<T: Serialize>(
    db: &Surreal<Db>,
    table: &str,
    id: &str,
    value: &T,
) -> Result<(), StoreError> {
    exec_query_checked(db, upsert_json_query(table, id, value)?).await
}

pub(super) async fn upsert_batch_checked<T, F>(
    db: &Surreal<Db>,
    table: &str,
    records: &[T],
    mut id_for: F,
) -> Result<(), StoreError>
where
    T: Serialize,
    F: FnMut(&T) -> &str,
{
    if records.is_empty() {
        return Ok(());
    }

    let mut query = String::new();
    for record in records {
        query.push_str(&upsert_json_query(table, id_for(record), record)?);
    }
    exec_query_checked(db, query).await
}

pub(super) async fn sync_market_state_checked<T, F>(
    db: &Surreal<Db>,
    table: &str,
    market: &str,
    records: &[T],
    mut id_for: F,
) -> Result<(), StoreError>
where
    T: Serialize,
    F: FnMut(&T) -> &str,
{
    let mut query = format!("BEGIN TRANSACTION; DELETE {table} WHERE market = $market;");
    for record in records {
        query.push_str(&upsert_json_query(table, id_for(record), record)?);
    }
    query.push_str("COMMIT TRANSACTION;");
    db.query(query)
        .bind(("market", market.to_string()))
        .await?
        .check()?;
    Ok(())
}

pub(super) fn take_records<T: DeserializeOwned>(
    mut result: surrealdb::Response,
) -> Result<Vec<T>, StoreError> {
    Ok(result.take(0)?)
}

pub(super) async fn fetch_latest_timestamp_field(
    db: &Surreal<Db>,
    table: &str,
    field: &str,
) -> Result<Option<time::OffsetDateTime>, StoreError> {
    #[derive(serde::Deserialize)]
    struct TimestampRow {
        value: String,
    }

    let query = format!("SELECT {field} AS value FROM {table} ORDER BY {field} DESC LIMIT 1");
    let result = db.query(query).await?;
    let mut rows: Vec<TimestampRow> = take_records(result)?;
    let Some(row) = rows.pop() else {
        return Ok(None);
    };
    Ok(Some(time::OffsetDateTime::parse(
        &row.value,
        &time::format_description::well_known::Rfc3339,
    )?))
}

pub(super) async fn fetch_recent_tick_window<T: DeserializeOwned>(
    db: &Surreal<Db>,
    table: &str,
    limit: usize,
) -> Result<Vec<T>, StoreError> {
    let mut records = fetch_ordered_records(db, table, "tick_number", false, limit).await?;
    records.reverse();
    Ok(records)
}

pub(super) async fn fetch_market_state_records<T: DeserializeOwned>(
    db: &Surreal<Db>,
    table: &str,
    market: &str,
    order_field: &str,
    limit: usize,
) -> Result<Vec<T>, StoreError> {
    let query = format!(
        "SELECT * FROM {table} WHERE market = $market ORDER BY {order_field} DESC LIMIT $limit"
    );
    let result = db
        .query(query)
        .bind(("market", market.to_string()))
        .bind(("limit", limit))
        .await?;
    take_records(result)
}

pub(super) async fn fetch_ordered_records<T: DeserializeOwned>(
    db: &Surreal<Db>,
    table: &str,
    order_field: &str,
    ascending: bool,
    limit: usize,
) -> Result<Vec<T>, StoreError> {
    let direction = if ascending { "ASC" } else { "DESC" };
    let query = format!("SELECT * FROM {table} ORDER BY {order_field} {direction} LIMIT $limit");
    let result = db.query(query).bind(("limit", limit)).await?;
    take_records(result)
}

pub(super) async fn fetch_ordered_records_custom<T: DeserializeOwned>(
    db: &Surreal<Db>,
    query: &str,
    limit: usize,
) -> Result<Vec<T>, StoreError> {
    let result = db.query(query).bind(("limit", limit)).await?;
    take_records(result)
}

pub(super) async fn fetch_ranked_records<T: DeserializeOwned>(
    db: &Surreal<Db>,
    table: &str,
    rank_limit: usize,
    max_rows: usize,
    order_clause: &str,
) -> Result<Vec<T>, StoreError> {
    let query = format!(
        "SELECT * FROM {table} WHERE rank < $rank_limit ORDER BY {order_clause} LIMIT $max_rows"
    );
    let result = db
        .query(query)
        .bind(("rank_limit", rank_limit))
        .bind(("max_rows", max_rows))
        .await?;
    take_records(result)
}

pub(super) async fn fetch_market_state_records_for_node<T: DeserializeOwned>(
    db: &Surreal<Db>,
    table: &str,
    market: &str,
    node_clause: &str,
    node_id: &str,
    order_field: &str,
    limit: usize,
) -> Result<Vec<T>, StoreError> {
    let query = format!(
        "SELECT * FROM {table} WHERE market = $market AND {node_clause} ORDER BY {order_field} DESC LIMIT $limit"
    );
    let result = db
        .query(query)
        .bind(("market", market.to_string()))
        .bind(("node_id", node_id.to_string()))
        .bind(("limit", limit))
        .await?;
    take_records(result)
}

pub(super) async fn fetch_market_history_records<T: DeserializeOwned>(
    db: &Surreal<Db>,
    table: &str,
    market: &str,
    order_field: &str,
    since_tick: Option<u64>,
    limit: usize,
) -> Result<Vec<T>, StoreError> {
    let query = if since_tick.is_some() {
        format!(
            "SELECT * FROM {table} WHERE market = $market AND tick_number >= $since_tick ORDER BY {order_field} DESC LIMIT $limit"
        )
    } else {
        format!(
            "SELECT * FROM {table} WHERE market = $market ORDER BY {order_field} DESC LIMIT $limit"
        )
    };
    let mut req = db
        .query(query)
        .bind(("market", market.to_string()))
        .bind(("limit", limit));
    if let Some(since_tick) = since_tick {
        req = req.bind(("since_tick", since_tick));
    }
    let result = req.await?;
    take_records(result)
}

pub(super) async fn fetch_optional_record_by_field<T: DeserializeOwned>(
    db: &Surreal<Db>,
    table: &str,
    field: &str,
    value: &str,
) -> Result<Option<T>, StoreError> {
    let query = format!("SELECT * FROM {table} WHERE {field} = $value LIMIT 1");
    let result = db.query(query).bind(("value", value.to_string())).await?;
    let mut records: Vec<T> = take_records(result)?;
    Ok(records.pop())
}

pub(super) async fn fetch_optional_market_record_by_field<T: DeserializeOwned>(
    db: &Surreal<Db>,
    table: &str,
    market: &str,
    field: &str,
    value: &str,
) -> Result<Option<T>, StoreError> {
    let query =
        format!("SELECT * FROM {table} WHERE market = $market AND {field} = $value LIMIT 1");
    let result = db
        .query(query)
        .bind(("market", market.to_string()))
        .bind(("value", value.to_string()))
        .await?;
    let mut records: Vec<T> = take_records(result)?;
    Ok(records.pop())
}

pub(super) async fn fetch_records_by_ids<T: DeserializeOwned>(
    db: &Surreal<Db>,
    table: &str,
    field: &str,
    ids: &[String],
) -> Result<Vec<T>, StoreError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    let query = format!("SELECT * FROM {table} WHERE {field} INSIDE $ids");
    let result = db.query(query).bind(("ids", ids.to_vec())).await?;
    take_records(result)
}

pub(super) async fn fetch_records_by_field_order<T: DeserializeOwned>(
    db: &Surreal<Db>,
    table: &str,
    field: &str,
    value: &str,
    order_field: &str,
    ascending: bool,
    limit: usize,
) -> Result<Vec<T>, StoreError> {
    let direction = if ascending { "ASC" } else { "DESC" };
    let query = format!(
        "SELECT * FROM {table} WHERE {field} = $value ORDER BY {order_field} {direction} LIMIT $limit"
    );
    let result = db
        .query(query)
        .bind(("value", value.to_string()))
        .bind(("limit", limit))
        .await?;
    take_records(result)
}

pub(super) async fn fetch_market_history_records_for_node<T: DeserializeOwned>(
    db: &Surreal<Db>,
    table: &str,
    market: &str,
    node_clause: &str,
    node_id: &str,
    order_field: &str,
    since_tick: Option<u64>,
    limit: usize,
) -> Result<Vec<T>, StoreError> {
    let query = if since_tick.is_some() {
        format!(
            "SELECT * FROM {table} WHERE market = $market AND {node_clause} AND tick_number >= $since_tick ORDER BY {order_field} DESC LIMIT $limit"
        )
    } else {
        format!(
            "SELECT * FROM {table} WHERE market = $market AND {node_clause} ORDER BY {order_field} DESC LIMIT $limit"
        )
    };
    let mut req = db
        .query(query)
        .bind(("market", market.to_string()))
        .bind(("node_id", node_id.to_string()))
        .bind(("limit", limit));
    if let Some(since_tick) = since_tick {
        req = req.bind(("since_tick", since_tick));
    }
    let result = req.await?;
    take_records(result)
}

pub(super) async fn fetch_records_in_time_range<T: DeserializeOwned>(
    db: &Surreal<Db>,
    table: &str,
    order_field: &str,
    from: time::OffsetDateTime,
    to: time::OffsetDateTime,
    limit: usize,
) -> Result<Vec<T>, StoreError> {
    let from_ts = from
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap();
    let to_ts = to
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap();
    let query = format!(
        "SELECT * FROM {table} WHERE timestamp >= <datetime>$from_ts AND timestamp <= <datetime>$to_ts ORDER BY {order_field} ASC LIMIT $limit"
    );
    let result = db
        .query(query)
        .bind(("from_ts", from_ts))
        .bind(("to_ts", to_ts))
        .bind(("limit", limit))
        .await?;
    take_records(result)
}

pub(super) async fn fetch_tick_archives_in_range(
    db: &Surreal<Db>,
    from: time::OffsetDateTime,
    to: time::OffsetDateTime,
) -> Result<Vec<crate::ontology::microstructure::TickArchive>, StoreError> {
    fetch_records_in_time_range(db, "tick_archive", "timestamp", from, to, 10_000).await
}

pub(super) async fn exec_query_checked(db: &Surreal<Db>, query: String) -> Result<(), StoreError> {
    db.query(query).await?.check()?;
    Ok(())
}

fn strip_nulls(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            map.retain(|_, item| {
                strip_nulls(item);
                !item.is_null()
            });
        }
        serde_json::Value::Array(items) => {
            for item in items.iter_mut() {
                strip_nulls(item);
            }
        }
        _ => {}
    }
}
