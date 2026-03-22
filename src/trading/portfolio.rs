use std::collections::HashMap;

use longport::trade::TradeContext;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use super::position_sizer::PortfolioState;

/// Live portfolio state fetched from the broker.
#[derive(Debug, Clone)]
pub struct LivePosition {
    pub symbol: String,
    pub quantity: i64,
    pub cost_price: Decimal,
    pub currency: String,
    pub market: String,
}

/// Fetch current portfolio state from Longport.
pub async fn fetch_portfolio_state(
    ctx: &TradeContext,
    sector_lookup: &dyn Fn(&str) -> String,
) -> Result<(PortfolioState, Vec<LivePosition>), String> {
    // Get account balance
    let balances = ctx
        .account_balance(None)
        .await
        .map_err(|e| format!("failed to fetch balance: {e}"))?;

    let equity = balances
        .iter()
        .map(|b| b.net_assets)
        .max()
        .unwrap_or(Decimal::ZERO);

    // Get positions
    let positions_resp = ctx
        .stock_positions(None)
        .await
        .map_err(|e| format!("failed to fetch positions: {e}"))?;

    let mut positions = Vec::new();
    let mut sector_exposure: HashMap<String, Decimal> = HashMap::new();
    let mut position_count = 0_usize;

    for channel in &positions_resp.channels {
        for pos in &channel.positions {
            let qty_str = pos.quantity.to_string();
            let qty: i64 = qty_str.parse().unwrap_or(0);
            if qty == 0 {
                continue;
            }

            position_count += 1;
            let notional = pos.cost_price * Decimal::from(qty.unsigned_abs());
            let sector = sector_lookup(&pos.symbol);
            *sector_exposure.entry(sector).or_insert(Decimal::ZERO) += notional;

            positions.push(LivePosition {
                symbol: pos.symbol.clone(),
                quantity: qty,
                cost_price: pos.cost_price,
                currency: pos.currency.clone(),
                market: if pos.symbol.ends_with(".HK") {
                    "HK".into()
                } else {
                    "US".into()
                },
            });
        }
    }

    let state = PortfolioState {
        equity,
        position_count,
        sector_exposure,
    };

    Ok((state, positions))
}

/// Check if we already hold a position in the given symbol.
pub fn has_position(positions: &[LivePosition], symbol: &str) -> bool {
    positions.iter().any(|p| p.symbol == symbol && p.quantity != 0)
}

/// Get the current quantity for a symbol (positive = long, negative = short).
pub fn position_quantity(positions: &[LivePosition], symbol: &str) -> i64 {
    positions
        .iter()
        .find(|p| p.symbol == symbol)
        .map(|p| p.quantity)
        .unwrap_or(0)
}
