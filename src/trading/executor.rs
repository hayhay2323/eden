use longport::trade::{OrderSide, OrderType, SubmitOrderOptions, TimeInForceType, TradeContext};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use time::OffsetDateTime;
use tokio::sync::Mutex;

use super::portfolio::{fetch_portfolio_state, has_position, LivePosition};
use super::position_sizer::{PortfolioState, PositionSizer, SizeResult};

/// A signal from Eden's pipeline that the executor can act on.
#[derive(Debug, Clone)]
pub struct TradeSignal {
    pub symbol: String,
    pub sector: String,
    pub direction: SignalDirection,
    pub confidence: Decimal,
    pub mechanism: String,
    pub entry_price: Decimal,
    pub stop_price: Decimal,
    pub lot_size: u64,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalDirection {
    Long,
    Short,
    Exit,
}

/// Result of executing a trade signal.
#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub symbol: String,
    pub action: String,
    pub order_id: Option<String>,
    pub quantity: Decimal,
    pub price: Decimal,
    pub success: bool,
    pub message: String,
    pub timestamp: OffsetDateTime,
}

/// Configuration for the trading executor.
#[derive(Debug, Clone)]
pub struct ExecutorConfig {
    pub sizer: PositionSizer,
    /// Minimum confidence to act on a signal.
    pub min_confidence: Decimal,
    /// Whether to actually submit orders (false = dry run, log only).
    pub live: bool,
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self {
            sizer: PositionSizer::default(),
            min_confidence: dec!(0.65),
            live: false, // default to dry run
        }
    }
}

/// The main trading executor. Takes Eden signals and executes them.
pub struct Executor {
    pub config: ExecutorConfig,
    execution_lock: Mutex<()>,
}

impl Executor {
    pub fn new(config: ExecutorConfig) -> Self {
        Self {
            config,
            execution_lock: Mutex::new(()),
        }
    }

    /// Process a batch of signals against the current portfolio.
    pub async fn process_signals(
        &self,
        ctx: &TradeContext,
        signals: &[TradeSignal],
        sector_lookup: &dyn Fn(&str) -> String,
    ) -> Vec<ExecutionResult> {
        let _guard = self.execution_lock.lock().await;

        // Fetch current portfolio
        let (portfolio_state, positions) = match fetch_portfolio_state(ctx, sector_lookup).await {
            Ok(state) => state,
            Err(e) => {
                eprintln!("[executor] 無法獲取持倉: {e}");
                return vec![];
            }
        };

        eprintln!(
            "[executor] 權益: {} | 持倉: {} | 購買力: --",
            portfolio_state.equity, portfolio_state.position_count
        );

        let mut portfolio_state = portfolio_state;
        let mut positions = positions;
        let mut results = Vec::new();

        for signal in signals {
            let result = self
                .process_single_signal(ctx, signal, &portfolio_state, &positions)
                .await;
            if result.success {
                apply_successful_signal(
                    &mut portfolio_state,
                    &mut positions,
                    signal,
                    &result,
                    sector_lookup,
                );
            }
            results.push(result);
        }

        results
    }

    async fn process_single_signal(
        &self,
        ctx: &TradeContext,
        signal: &TradeSignal,
        portfolio_state: &super::position_sizer::PortfolioState,
        positions: &[LivePosition],
    ) -> ExecutionResult {
        let now = OffsetDateTime::now_utc();

        // Check confidence threshold
        if signal.confidence < self.config.min_confidence {
            return ExecutionResult {
                symbol: signal.symbol.clone(),
                action: "skip".into(),
                order_id: None,
                quantity: Decimal::ZERO,
                price: signal.entry_price,
                success: false,
                message: format!(
                    "信心 {} < 閾值 {}",
                    signal.confidence, self.config.min_confidence
                ),
                timestamp: now,
            };
        }

        match signal.direction {
            SignalDirection::Long | SignalDirection::Short => {
                self.handle_entry(ctx, signal, portfolio_state, positions, now)
                    .await
            }
            SignalDirection::Exit => self.handle_exit(ctx, signal, positions, now).await,
        }
    }

    async fn handle_entry(
        &self,
        ctx: &TradeContext,
        signal: &TradeSignal,
        portfolio_state: &super::position_sizer::PortfolioState,
        positions: &[LivePosition],
        now: OffsetDateTime,
    ) -> ExecutionResult {
        // Already have a position?
        if has_position(positions, &signal.symbol) {
            return ExecutionResult {
                symbol: signal.symbol.clone(),
                action: "skip".into(),
                order_id: None,
                quantity: Decimal::ZERO,
                price: signal.entry_price,
                success: false,
                message: "已有持倉".into(),
                timestamp: now,
            };
        }

        // Size the position
        let size = self.config.sizer.compute(
            signal.entry_price,
            signal.stop_price,
            signal.lot_size,
            &signal.sector,
            portfolio_state,
        );

        if size.quantity == 0 {
            return ExecutionResult {
                symbol: signal.symbol.clone(),
                action: "reject".into(),
                order_id: None,
                quantity: Decimal::ZERO,
                price: signal.entry_price,
                success: false,
                message: size.reject_reason.unwrap_or_else(|| "倉位計算為零".into()),
                timestamp: now,
            };
        }

        let side = match signal.direction {
            SignalDirection::Long => OrderSide::Buy,
            SignalDirection::Short => OrderSide::Sell,
            _ => unreachable!(),
        };

        let action_label = match signal.direction {
            SignalDirection::Long => "買入",
            SignalDirection::Short => "賣空",
            _ => unreachable!(),
        };

        eprintln!(
            "[executor] {} {} {} 股 @ {} | 止損 {} | 風險 {} | 機制: {}",
            action_label,
            signal.symbol,
            size.quantity,
            signal.entry_price,
            signal.stop_price,
            size.risk_amount,
            signal.mechanism,
        );

        if !self.config.live {
            return ExecutionResult {
                symbol: signal.symbol.clone(),
                action: format!("dry-run:{}", action_label),
                order_id: None,
                quantity: Decimal::from(size.quantity),
                price: signal.entry_price,
                success: true,
                message: format!(
                    "[模擬] {} {} 股 @ {} | 機制: {} | 理由: {}",
                    action_label,
                    size.quantity,
                    signal.entry_price,
                    signal.mechanism,
                    signal.reason
                ),
                timestamp: now,
            };
        }

        // Submit the order
        self.submit_entry_order(ctx, signal, &size, side, action_label, now)
            .await
    }

    async fn submit_entry_order(
        &self,
        ctx: &TradeContext,
        signal: &TradeSignal,
        size: &SizeResult,
        side: OrderSide,
        action_label: &str,
        now: OffsetDateTime,
    ) -> ExecutionResult {
        let order = SubmitOrderOptions::new(
            &signal.symbol,
            OrderType::ELO,
            side,
            size.quantity.into(),
            TimeInForceType::Day,
        )
        .submitted_price(signal.entry_price)
        .remark(format!(
            "Eden:{} conf={} mech={}",
            action_label, signal.confidence, signal.mechanism
        ));

        match ctx.submit_order(order).await {
            Ok(resp) => {
                eprintln!(
                    "[executor] ✓ 訂單已提交: {} {} {} 股 | order_id={}",
                    action_label, signal.symbol, size.quantity, resp.order_id
                );
                ExecutionResult {
                    symbol: signal.symbol.clone(),
                    action: action_label.into(),
                    order_id: Some(resp.order_id.clone()),
                    quantity: Decimal::from(size.quantity),
                    price: signal.entry_price,
                    success: true,
                    message: format!("訂單 {} 已提交", resp.order_id),
                    timestamp: now,
                }
            }
            Err(e) => {
                eprintln!(
                    "[executor] ✗ 下單失敗: {} {} — {}",
                    action_label, signal.symbol, e
                );
                ExecutionResult {
                    symbol: signal.symbol.clone(),
                    action: format!("fail:{}", action_label),
                    order_id: None,
                    quantity: Decimal::from(size.quantity),
                    price: signal.entry_price,
                    success: false,
                    message: format!("下單失敗: {e}"),
                    timestamp: now,
                }
            }
        }
    }

    async fn handle_exit(
        &self,
        ctx: &TradeContext,
        signal: &TradeSignal,
        positions: &[LivePosition],
        now: OffsetDateTime,
    ) -> ExecutionResult {
        let position = positions.iter().find(|p| p.symbol == signal.symbol);
        let Some(position) = position else {
            return ExecutionResult {
                symbol: signal.symbol.clone(),
                action: "skip".into(),
                order_id: None,
                quantity: Decimal::ZERO,
                price: signal.entry_price,
                success: false,
                message: "無持倉可平".into(),
                timestamp: now,
            };
        };

        let qty = position.quantity.abs();
        if qty == Decimal::ZERO {
            return ExecutionResult {
                symbol: signal.symbol.clone(),
                action: "skip".into(),
                order_id: None,
                quantity: Decimal::ZERO,
                price: signal.entry_price,
                success: false,
                message: "持倉數量為零".into(),
                timestamp: now,
            };
        }

        // Reverse the position direction to exit
        let side = if position.quantity > Decimal::ZERO {
            OrderSide::Sell
        } else {
            OrderSide::Buy
        };
        let action_label = if position.quantity > Decimal::ZERO {
            "平多"
        } else {
            "平空"
        };

        eprintln!(
            "[executor] {} {} {} 股 @ {} | 理由: {}",
            action_label, signal.symbol, qty, signal.entry_price, signal.reason,
        );

        if !self.config.live {
            return ExecutionResult {
                symbol: signal.symbol.clone(),
                action: format!("dry-run:{}", action_label),
                order_id: None,
                quantity: qty,
                price: signal.entry_price,
                success: true,
                message: format!(
                    "[模擬] {} {} 股 @ {} | 理由: {}",
                    action_label, qty, signal.entry_price, signal.reason
                ),
                timestamp: now,
            };
        }

        let order = SubmitOrderOptions::new(
            &signal.symbol,
            OrderType::ELO,
            side,
            qty,
            TimeInForceType::Day,
        )
        .submitted_price(signal.entry_price)
        .remark(format!("Eden:{} mech={}", action_label, signal.mechanism));

        match ctx.submit_order(order).await {
            Ok(resp) => {
                eprintln!(
                    "[executor] ✓ 平倉訂單: {} {} {} 股 | order_id={}",
                    action_label, signal.symbol, qty, resp.order_id
                );
                ExecutionResult {
                    symbol: signal.symbol.clone(),
                    action: action_label.into(),
                    order_id: Some(resp.order_id.clone()),
                    quantity: qty,
                    price: signal.entry_price,
                    success: true,
                    message: format!("平倉訂單 {} 已提交", resp.order_id),
                    timestamp: now,
                }
            }
            Err(e) => {
                eprintln!(
                    "[executor] ✗ 平倉失敗: {} {} — {}",
                    action_label, signal.symbol, e
                );
                ExecutionResult {
                    symbol: signal.symbol.clone(),
                    action: format!("fail:{}", action_label),
                    order_id: None,
                    quantity: qty,
                    price: signal.entry_price,
                    success: false,
                    message: format!("平倉失敗: {e}"),
                    timestamp: now,
                }
            }
        }
    }
}

fn apply_successful_signal(
    portfolio_state: &mut PortfolioState,
    positions: &mut Vec<LivePosition>,
    signal: &TradeSignal,
    result: &ExecutionResult,
    sector_lookup: &dyn Fn(&str) -> String,
) {
    match signal.direction {
        SignalDirection::Long | SignalDirection::Short => {
            if has_position(positions, &signal.symbol) || result.quantity == Decimal::ZERO {
                return;
            }

            let signed_quantity = match signal.direction {
                SignalDirection::Long => result.quantity,
                SignalDirection::Short => -result.quantity,
                SignalDirection::Exit => unreachable!(),
            };
            let sector = sector_lookup(&signal.symbol);

            let position = LivePosition {
                symbol: signal.symbol.clone(),
                quantity: signed_quantity,
                cost_price: result.price,
                currency: if signal.symbol.ends_with(".HK") {
                    "HKD".into()
                } else {
                    "USD".into()
                },
                market: if signal.symbol.ends_with(".HK") {
                    "HK".into()
                } else {
                    "US".into()
                },
            };
            let notional = notional_from_position(&position);
            positions.push(position);
            portfolio_state.position_count += 1;
            *portfolio_state
                .sector_exposure
                .entry(sector)
                .or_insert(Decimal::ZERO) += notional;
        }
        SignalDirection::Exit => {
            let Some(index) = positions
                .iter()
                .position(|position| position.symbol == signal.symbol)
            else {
                return;
            };
            let position = positions.remove(index);
            portfolio_state.position_count = portfolio_state.position_count.saturating_sub(1);

            let sector = sector_lookup(&signal.symbol);
            let notional = notional_from_position(&position);
            if let Some(exposure) = portfolio_state.sector_exposure.get_mut(&sector) {
                *exposure = (*exposure - notional).max(Decimal::ZERO);
            }
        }
    }
}

fn notional_from_position(position: &LivePosition) -> Decimal {
    position.cost_price * position.quantity.abs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn sector_lookup(_: &str) -> String {
        "tech".into()
    }

    fn base_signal(symbol: &str, direction: SignalDirection) -> TradeSignal {
        TradeSignal {
            symbol: symbol.into(),
            sector: "tech".into(),
            direction,
            confidence: dec!(0.9),
            mechanism: "test".into(),
            entry_price: dec!(100),
            stop_price: dec!(95),
            lot_size: 100,
            reason: "unit test".into(),
        }
    }

    #[test]
    fn successful_entry_updates_portfolio_state_for_following_batch_items() {
        let mut portfolio = PortfolioState {
            equity: dec!(100000),
            position_count: 0,
            sector_exposure: HashMap::new(),
        };
        let mut positions = Vec::new();
        let signal = base_signal("AAPL.US", SignalDirection::Long);
        let result = ExecutionResult {
            symbol: signal.symbol.clone(),
            action: "dry-run:買入".into(),
            order_id: None,
            quantity: dec!(100),
            price: dec!(100),
            success: true,
            message: "ok".into(),
            timestamp: OffsetDateTime::UNIX_EPOCH,
        };

        apply_successful_signal(
            &mut portfolio,
            &mut positions,
            &signal,
            &result,
            &sector_lookup,
        );

        assert_eq!(portfolio.position_count, 1);
        assert!(has_position(&positions, "AAPL.US"));
        assert_eq!(portfolio.sector_exposure["tech"], dec!(10000));
    }

    #[test]
    fn successful_exit_releases_position_slot() {
        let mut portfolio = PortfolioState {
            equity: dec!(100000),
            position_count: 1,
            sector_exposure: HashMap::from([("tech".into(), dec!(10000))]),
        };
        let mut positions = vec![LivePosition {
            symbol: "AAPL.US".into(),
            quantity: dec!(100),
            cost_price: dec!(100),
            currency: "USD".into(),
            market: "US".into(),
        }];
        let signal = base_signal("AAPL.US", SignalDirection::Exit);
        let result = ExecutionResult {
            symbol: signal.symbol.clone(),
            action: "dry-run:平多".into(),
            order_id: None,
            quantity: dec!(100),
            price: dec!(101),
            success: true,
            message: "ok".into(),
            timestamp: OffsetDateTime::UNIX_EPOCH,
        };

        apply_successful_signal(
            &mut portfolio,
            &mut positions,
            &signal,
            &result,
            &sector_lookup,
        );

        assert_eq!(portfolio.position_count, 0);
        assert!(!has_position(&positions, "AAPL.US"));
        assert_eq!(portfolio.sector_exposure["tech"], Decimal::ZERO);
    }

    #[test]
    fn successful_entry_uses_position_notional_basis() {
        let mut portfolio = PortfolioState {
            equity: dec!(100000),
            position_count: 0,
            sector_exposure: HashMap::new(),
        };
        let mut positions = Vec::new();
        let signal = base_signal("AAPL.US", SignalDirection::Long);
        let result = ExecutionResult {
            symbol: signal.symbol.clone(),
            action: "dry-run:買入".into(),
            order_id: None,
            quantity: dec!(50.5),
            price: dec!(100),
            success: true,
            message: "ok".into(),
            timestamp: OffsetDateTime::UNIX_EPOCH,
        };

        apply_successful_signal(
            &mut portfolio,
            &mut positions,
            &signal,
            &result,
            &sector_lookup,
        );

        let position = positions.first().unwrap();
        assert_eq!(notional_from_position(position), dec!(5050));
        assert_eq!(portfolio.sector_exposure["tech"], dec!(5050));
    }
}
