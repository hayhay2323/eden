use rust_decimal::Decimal;
use rust_decimal_macros::dec;

/// Risk-based position sizing.
///
/// Given account equity, risk budget, and stop distance, compute the number
/// of shares to buy such that a stop-loss exit loses at most `risk_per_trade`
/// percent of equity.
#[derive(Debug, Clone)]
pub struct PositionSizer {
    /// Maximum fraction of equity to risk on a single trade (e.g. 0.01 = 1%).
    pub risk_per_trade: Decimal,
    /// Maximum fraction of equity in a single position (e.g. 0.10 = 10%).
    pub max_position_pct: Decimal,
    /// Maximum number of concurrent positions.
    pub max_positions: usize,
    /// Maximum fraction of equity in a single sector (e.g. 0.30 = 30%).
    pub max_sector_pct: Decimal,
    /// HK board lot size lookup. Defaults to 100 if unknown.
    pub default_lot_size: u64,
}

impl Default for PositionSizer {
    fn default() -> Self {
        Self {
            risk_per_trade: dec!(0.01),   // risk 1% per trade
            max_position_pct: dec!(0.10), // max 10% of equity per position
            max_positions: 7,
            max_sector_pct: dec!(0.30),   // max 30% per sector
            default_lot_size: 100,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SizeResult {
    /// Number of shares to buy (rounded down to lot size).
    pub quantity: u64,
    /// Dollar value of the position.
    pub notional: Decimal,
    /// Fraction of equity this position represents.
    pub equity_pct: Decimal,
    /// Dollar amount at risk (quantity * stop_distance).
    pub risk_amount: Decimal,
    /// Reason if quantity is zero.
    pub reject_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PortfolioState {
    pub equity: Decimal,
    pub position_count: usize,
    pub sector_exposure: std::collections::HashMap<String, Decimal>,
}

impl PositionSizer {
    /// Compute the number of shares to trade.
    ///
    /// - `entry_price`: intended entry price
    /// - `stop_price`: intended stop-loss price
    /// - `lot_size`: board lot size for this symbol (HK stocks trade in lots)
    /// - `sector`: sector of the symbol
    /// - `portfolio`: current portfolio state
    pub fn compute(
        &self,
        entry_price: Decimal,
        stop_price: Decimal,
        lot_size: u64,
        sector: &str,
        portfolio: &PortfolioState,
    ) -> SizeResult {
        let lot_size = if lot_size == 0 { self.default_lot_size } else { lot_size };

        // Check position count limit
        if portfolio.position_count >= self.max_positions {
            return SizeResult {
                quantity: 0,
                notional: Decimal::ZERO,
                equity_pct: Decimal::ZERO,
                risk_amount: Decimal::ZERO,
                reject_reason: Some(format!(
                    "已達最大持倉數 {}",
                    self.max_positions
                )),
            };
        }

        if entry_price <= Decimal::ZERO || portfolio.equity <= Decimal::ZERO {
            return SizeResult {
                quantity: 0,
                notional: Decimal::ZERO,
                equity_pct: Decimal::ZERO,
                risk_amount: Decimal::ZERO,
                reject_reason: Some("無效價格或權益".into()),
            };
        }

        // Stop distance per share
        let stop_distance = (entry_price - stop_price).abs();
        if stop_distance <= Decimal::ZERO {
            return SizeResult {
                quantity: 0,
                notional: Decimal::ZERO,
                equity_pct: Decimal::ZERO,
                risk_amount: Decimal::ZERO,
                reject_reason: Some("止損距離為零".into()),
            };
        }

        // 1) Risk-based sizing: risk_amount = equity * risk_per_trade
        //    shares = risk_amount / stop_distance
        let risk_budget = portfolio.equity * self.risk_per_trade;
        let shares_by_risk = risk_budget / stop_distance;

        // 2) Max position size: notional <= equity * max_position_pct
        //    shares = (equity * max_position_pct) / entry_price
        let max_notional = portfolio.equity * self.max_position_pct;
        let shares_by_notional = max_notional / entry_price;

        // 3) Sector limit: remaining sector budget
        let current_sector_exposure = portfolio
            .sector_exposure
            .get(sector)
            .copied()
            .unwrap_or(Decimal::ZERO);
        let sector_budget = (portfolio.equity * self.max_sector_pct) - current_sector_exposure;
        let shares_by_sector = if sector_budget > Decimal::ZERO {
            sector_budget / entry_price
        } else {
            return SizeResult {
                quantity: 0,
                notional: Decimal::ZERO,
                equity_pct: Decimal::ZERO,
                risk_amount: Decimal::ZERO,
                reject_reason: Some(format!(
                    "板塊 {} 已達上限 ({:.0}%)",
                    sector,
                    (current_sector_exposure / portfolio.equity * dec!(100)).round()
                )),
            };
        };

        // Take the minimum of all constraints
        let shares_raw = shares_by_risk
            .min(shares_by_notional)
            .min(shares_by_sector);

        // Round down to lot size
        let shares_i64 = shares_raw.floor().to_string().parse::<i64>().unwrap_or(0);
        let lots = (shares_i64 as u64) / lot_size;
        let quantity = lots * lot_size;

        if quantity == 0 {
            return SizeResult {
                quantity: 0,
                notional: Decimal::ZERO,
                equity_pct: Decimal::ZERO,
                risk_amount: Decimal::ZERO,
                reject_reason: Some("計算後數量不足一手".into()),
            };
        }

        let q = Decimal::from(quantity);
        let notional = q * entry_price;
        let equity_pct = notional / portfolio.equity;
        let risk_amount = q * stop_distance;

        SizeResult {
            quantity,
            notional,
            equity_pct,
            risk_amount,
            reject_reason: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn portfolio(equity: Decimal, positions: usize) -> PortfolioState {
        PortfolioState {
            equity,
            position_count: positions,
            sector_exposure: HashMap::new(),
        }
    }

    #[test]
    fn basic_sizing() {
        let sizer = PositionSizer::default();
        let p = portfolio(dec!(100000), 0);
        // entry=100, stop=95 → stop_distance=5
        // risk_budget = 100000 * 0.01 = 1000
        // shares_by_risk = 1000 / 5 = 200
        // max_notional = 100000 * 0.10 = 10000 → shares = 100
        // → min(200, 100) = 100, rounded to lots of 100 = 100
        let r = sizer.compute(dec!(100), dec!(95), 100, "tech", &p);
        assert_eq!(r.quantity, 100);
        assert!(r.reject_reason.is_none());
    }

    #[test]
    fn max_positions_reached() {
        let sizer = PositionSizer::default();
        let p = portfolio(dec!(100000), 7);
        let r = sizer.compute(dec!(100), dec!(95), 100, "tech", &p);
        assert_eq!(r.quantity, 0);
        assert!(r.reject_reason.unwrap().contains("最大持倉"));
    }

    #[test]
    fn sector_limit() {
        let sizer = PositionSizer::default();
        let mut p = portfolio(dec!(100000), 2);
        // Already 30000 in tech = 30% → at limit
        p.sector_exposure.insert("tech".into(), dec!(30000));
        let r = sizer.compute(dec!(100), dec!(95), 100, "tech", &p);
        assert_eq!(r.quantity, 0);
        assert!(r.reject_reason.unwrap().contains("板塊"));
    }

    #[test]
    fn lot_size_rounding() {
        let sizer = PositionSizer::default();
        let p = portfolio(dec!(100000), 0);
        // entry=550, stop=530 → distance=20
        // risk = 1000 / 20 = 50 shares
        // max_notional = 10000 / 550 = 18.18 shares
        // min = 18, lot_size=100 → 0 lots → reject
        let r = sizer.compute(dec!(550), dec!(530), 100, "tech", &p);
        assert_eq!(r.quantity, 0); // can't fill a single lot
    }
}
