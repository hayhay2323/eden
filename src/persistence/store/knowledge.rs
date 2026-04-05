use crate::persistence::agent_graph::{
    KnowledgeEventHistoryRecord, KnowledgeEventStateRecord, KnowledgeLinkHistoryRecord,
    KnowledgeLinkStateRecord, KnowledgeNodeHistoryRecord, KnowledgeNodeStateRecord,
    MacroEventHistoryRecord, MacroEventStateRecord,
};

use super::super::store_helpers::{
    fetch_market_history_records, fetch_market_history_records_for_node,
    fetch_market_state_records, fetch_market_state_records_for_node,
    fetch_optional_market_record_by_field, sync_market_state_checked, upsert_batch_checked,
    StoreError,
};
use super::EdenStore;

impl EdenStore {
    pub async fn write_macro_event_history(
        &self,
        records: &[MacroEventHistoryRecord],
    ) -> Result<(), StoreError> {
        upsert_batch_checked(&self.db, "macro_event_history", records, |record| {
            &record.record_id
        })
        .await
    }

    pub async fn write_knowledge_link_history(
        &self,
        records: &[KnowledgeLinkHistoryRecord],
    ) -> Result<(), StoreError> {
        upsert_batch_checked(&self.db, "knowledge_link_history", records, |record| {
            &record.record_id
        })
        .await
    }

    pub async fn recent_macro_event_history(
        &self,
        market: &str,
        since_tick: Option<u64>,
        limit: usize,
    ) -> Result<Vec<MacroEventHistoryRecord>, StoreError> {
        fetch_market_history_records(
            &self.db,
            "macro_event_history",
            market,
            "tick_number",
            since_tick,
            limit,
        )
        .await
    }

    pub async fn recent_knowledge_link_history(
        &self,
        market: &str,
        since_tick: Option<u64>,
        limit: usize,
    ) -> Result<Vec<KnowledgeLinkHistoryRecord>, StoreError> {
        fetch_market_history_records(
            &self.db,
            "knowledge_link_history",
            market,
            "tick_number",
            since_tick,
            limit,
        )
        .await
    }

    pub async fn sync_macro_event_state(
        &self,
        market: &str,
        records: &[MacroEventStateRecord],
    ) -> Result<(), StoreError> {
        sync_market_state_checked(&self.db, "macro_event_state", market, records, |record| {
            &record.state_id
        })
        .await
    }

    pub async fn sync_knowledge_link_state(
        &self,
        market: &str,
        records: &[KnowledgeLinkStateRecord],
    ) -> Result<(), StoreError> {
        sync_market_state_checked(
            &self.db,
            "knowledge_link_state",
            market,
            records,
            |record| &record.state_id,
        )
        .await
    }

    pub async fn current_macro_event_state(
        &self,
        market: &str,
        limit: usize,
    ) -> Result<Vec<MacroEventStateRecord>, StoreError> {
        fetch_market_state_records(
            &self.db,
            "macro_event_state",
            market,
            "latest_tick_number",
            limit,
        )
        .await
    }

    pub async fn current_knowledge_link_state(
        &self,
        market: &str,
        limit: usize,
    ) -> Result<Vec<KnowledgeLinkStateRecord>, StoreError> {
        fetch_market_state_records(
            &self.db,
            "knowledge_link_state",
            market,
            "latest_tick_number",
            limit,
        )
        .await
    }

    pub async fn write_knowledge_event_history(
        &self,
        records: &[KnowledgeEventHistoryRecord],
    ) -> Result<(), StoreError> {
        upsert_batch_checked(&self.db, "knowledge_event_history", records, |record| {
            &record.record_id
        })
        .await
    }

    pub async fn sync_knowledge_event_state(
        &self,
        market: &str,
        records: &[KnowledgeEventStateRecord],
    ) -> Result<(), StoreError> {
        sync_market_state_checked(
            &self.db,
            "knowledge_event_state",
            market,
            records,
            |record| &record.state_id,
        )
        .await
    }

    pub async fn current_knowledge_event_state(
        &self,
        market: &str,
        limit: usize,
    ) -> Result<Vec<KnowledgeEventStateRecord>, StoreError> {
        fetch_market_state_records(
            &self.db,
            "knowledge_event_state",
            market,
            "latest_tick_number",
            limit,
        )
        .await
    }

    pub async fn recent_knowledge_event_history(
        &self,
        market: &str,
        since_tick: Option<u64>,
        limit: usize,
    ) -> Result<Vec<KnowledgeEventHistoryRecord>, StoreError> {
        fetch_market_history_records(
            &self.db,
            "knowledge_event_history",
            market,
            "tick_number",
            since_tick,
            limit,
        )
        .await
    }

    pub async fn current_knowledge_event_state_for_node(
        &self,
        market: &str,
        node_id: &str,
        limit: usize,
    ) -> Result<Vec<KnowledgeEventStateRecord>, StoreError> {
        fetch_market_state_records_for_node(
            &self.db,
            "knowledge_event_state",
            market,
            "(subject_node_id = $node_id OR object_node_id = $node_id)",
            node_id,
            "latest_tick_number",
            limit,
        )
        .await
    }

    pub async fn recent_knowledge_event_history_for_node(
        &self,
        market: &str,
        node_id: &str,
        since_tick: Option<u64>,
        limit: usize,
    ) -> Result<Vec<KnowledgeEventHistoryRecord>, StoreError> {
        fetch_market_history_records_for_node(
            &self.db,
            "knowledge_event_history",
            market,
            "(subject_node_id = $node_id OR object_node_id = $node_id)",
            node_id,
            "tick_number",
            since_tick,
            limit,
        )
        .await
    }

    pub async fn write_knowledge_node_history(
        &self,
        records: &[KnowledgeNodeHistoryRecord],
    ) -> Result<(), StoreError> {
        upsert_batch_checked(&self.db, "knowledge_node_history", records, |record| {
            &record.record_id
        })
        .await
    }

    pub async fn sync_knowledge_node_state(
        &self,
        market: &str,
        records: &[KnowledgeNodeStateRecord],
    ) -> Result<(), StoreError> {
        sync_market_state_checked(
            &self.db,
            "knowledge_node_state",
            market,
            records,
            |record| &record.state_id,
        )
        .await
    }

    pub async fn current_knowledge_node_state(
        &self,
        market: &str,
        limit: usize,
    ) -> Result<Vec<KnowledgeNodeStateRecord>, StoreError> {
        fetch_market_state_records(
            &self.db,
            "knowledge_node_state",
            market,
            "latest_tick_number",
            limit,
        )
        .await
    }

    pub async fn knowledge_node_state_by_id(
        &self,
        market: &str,
        node_id: &str,
    ) -> Result<Option<KnowledgeNodeStateRecord>, StoreError> {
        fetch_optional_market_record_by_field(
            &self.db,
            "knowledge_node_state",
            market,
            "node_id",
            node_id,
        )
        .await
    }

    pub async fn recent_knowledge_node_history(
        &self,
        market: &str,
        since_tick: Option<u64>,
        limit: usize,
    ) -> Result<Vec<KnowledgeNodeHistoryRecord>, StoreError> {
        fetch_market_history_records(
            &self.db,
            "knowledge_node_history",
            market,
            "tick_number",
            since_tick,
            limit,
        )
        .await
    }

    pub async fn recent_knowledge_node_history_for_id(
        &self,
        market: &str,
        node_id: &str,
        since_tick: Option<u64>,
        limit: usize,
    ) -> Result<Vec<KnowledgeNodeHistoryRecord>, StoreError> {
        fetch_market_history_records_for_node(
            &self.db,
            "knowledge_node_history",
            market,
            "node_id = $node_id",
            node_id,
            "tick_number",
            since_tick,
            limit,
        )
        .await
    }

    pub async fn current_knowledge_link_state_for_node(
        &self,
        market: &str,
        node_id: &str,
        limit: usize,
    ) -> Result<Vec<KnowledgeLinkStateRecord>, StoreError> {
        fetch_market_state_records_for_node(
            &self.db,
            "knowledge_link_state",
            market,
            "(source_node_id = $node_id OR target_node_id = $node_id)",
            node_id,
            "latest_tick_number",
            limit,
        )
        .await
    }

    pub async fn recent_knowledge_link_history_for_node(
        &self,
        market: &str,
        node_id: &str,
        since_tick: Option<u64>,
        limit: usize,
    ) -> Result<Vec<KnowledgeLinkHistoryRecord>, StoreError> {
        fetch_market_history_records_for_node(
            &self.db,
            "knowledge_link_history",
            market,
            "(source_node_id = $node_id OR target_node_id = $node_id)",
            node_id,
            "tick_number",
            since_tick,
            limit,
        )
        .await
    }
}
