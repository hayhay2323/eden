use super::*;

pub(super) fn canonical_pair(left: String, right: String) -> (String, String) {
    if left <= right {
        (left, right)
    } else {
        (right, left)
    }
}

pub(super) fn institution_key(institution_id: InstitutionId) -> String {
    institution_numeric_node_id(institution_id)
}

pub(super) fn institution_label(institution_id: InstitutionId) -> String {
    institution_id.0.to_string()
}

pub(super) fn stock_key(symbol: &Symbol) -> String {
    symbol_node_id(&symbol.0)
}

pub(super) fn stock_label(symbol: &Symbol) -> String {
    symbol.0.clone()
}

pub(super) fn sector_key(sector_id: &SectorId) -> String {
    sector_node_id(&sector_id.0)
}

pub(super) fn sector_label(sector_id: &SectorId) -> String {
    sector_id.0.clone()
}
