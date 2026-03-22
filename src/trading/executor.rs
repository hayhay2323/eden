use longport::trade::{
    OrderSide, OrderType, SubmitOrderOptions, TimeInForceType, TradeContext,
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use time::OffsetDateTime;

use super::portfolio::{fetch_portfolio_state, has_position, LivePosition};
use super::position_sizer::{PositionSizer, SizeResult};

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
    pub quantity: u64,
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
}

impl Executor {
    pub fn new(config: ExecutorConfig) -> Self {
        Self { config }
    }

    /// Process a batch of signals against the current portfolio.
    pub async fn process_signals(
        &self,
        ctx: &TradeContext,
        signals: &[TradeSignal],
        sector_lookup: &dyn Fn(&str) -> String,
    ) -> Vec<ExecutionResult> {
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

        let mut results = Vec::new();

        for signal in signals {
            let result = self
                .process_single_signal(ctx, signal, &portfolio_state, &positions)
                .await;
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
                quantity: 0,
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
                quantity: 0,
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
                quantity: 0,
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
                quantity: size.quantity,
                price: signal.entry_price,
                success: true,
                message: format!(
                    "[模擬] {} {} 股 @ {} | 機制: {} | 理由: {}",
                    action_label, size.quantity, signal.entry_price, signal.mechanism, signal.reason
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
                    quantity: size.quantity,
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
                    quantity: size.quantity,
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
                quantity: 0,
                price: signal.entry_price,
                success: false,
                message: "無持倉可平".into(),
                timestamp: now,
            };
        };

        let qty = position.quantity.unsigned_abs();
        if qty == 0 {
            return ExecutionResult {
                symbol: signal.symbol.clone(),
                action: "skip".into(),
                order_id: None,
                quantity: 0,
                price: signal.entry_price,
                success: false,
                message: "持倉數量為零".into(),
                timestamp: now,
            };
        }

        // Reverse the position direction to exit
        let side = if position.quantity > 0 {
            OrderSide::Sell
        } else {
            OrderSide::Buy
        };
        let action_label = if position.quantity > 0 {
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
            qty.into(),
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
