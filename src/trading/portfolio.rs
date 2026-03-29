use std::collections::HashMap;

use longport::trade::TradeContext;
use rust_decimal::Decimal;

use super::position_sizer::PortfolioState;

fn aggregate_equity(balances: &[longport::trade::AccountBalance]) -> Decimal {
    let Some(primary_currency) = balances.first().map(|balance| balance.currency.as_str()) else {
        return Decimal::ZERO;
    };

    balances
        .iter()
        .filter(|balance| balance.currency == primary_currency)
        .map(|balance| balance.net_assets)
        .sum()
}

/// Live portfolio state fetched from the broker.
#[derive(Debug, Clone)]
pub struct LivePosition {
    pub symbol: String,
    pub quantity: Decimal,
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

    let equity = aggregate_equity(&balances);

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
            let qty = pos.quantity;
            if qty == Decimal::ZERO {
                continue;
            }

            position_count += 1;
            let notional = pos.cost_price * qty.abs();
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
    positions
        .iter()
        .any(|p| p.symbol == symbol && p.quantity != Decimal::ZERO)
}

/// Get the current quantity for a symbol (positive = long, negative = short).
pub fn position_quantity(positions: &[LivePosition], symbol: &str) -> Decimal {
    positions
        .iter()
        .find(|p| p.symbol == symbol)
        .map(|p| p.quantity)
        .unwrap_or(Decimal::ZERO)
}

#[cfg(test)]
mod tests {
    use super::*;
    use longport::trade::AccountBalance;

    fn balance(currency: &str, net_assets: Decimal) -> AccountBalance {
        AccountBalance {
            total_cash: Decimal::ZERO,
            max_finance_amount: Decimal::ZERO,
            remaining_finance_amount: Decimal::ZERO,
            risk_level: 0,
            margin_call: Decimal::ZERO,
            currency: currency.into(),
            cash_infos: vec![],
            net_assets,
            init_margin: Decimal::ZERO,
            maintenance_margin: Decimal::ZERO,
            buy_power: Decimal::ZERO,
            frozen_transaction_fees: vec![],
        }
    }

    #[test]
    fn has_position_treats_fractional_quantity_as_open_position() {
        let positions = vec![LivePosition {
            symbol: "AAPL.US".into(),
            quantity: Decimal::new(5, 1),
            cost_price: Decimal::new(100, 0),
            currency: "USD".into(),
            market: "US".into(),
        }];

        assert!(has_position(&positions, "AAPL.US"));
        assert_eq!(position_quantity(&positions, "AAPL.US"), Decimal::new(5, 1));
    }

    #[test]
    fn aggregate_equity_uses_primary_currency_only() {
        let balances = vec![
            balance("HKD", Decimal::new(1_000_000, 0)),
            balance("USD", Decimal::new(20_000, 0)),
            balance("HKD", Decimal::new(500_000, 0)),
        ];

        assert_eq!(aggregate_equity(&balances), Decimal::new(1_500_000, 0));
    }
}
