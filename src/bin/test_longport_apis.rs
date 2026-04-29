//! Test all Longport SDK APIs to see what data we can actually get.
//! Run: cargo run --bin test_longport_apis

use longport::quote::{AdjustType, CalcIndex, Period, QuoteContext, TradeSessions};
use longport::Config;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    let config = Arc::new(Config::from_env()?);
    let (ctx, _) = QuoteContext::try_new(config).await?;

    let hk_sym = "700.HK";
    let us_sym = "AAPL.US";

    println!("\n============================================================");
    println!("=== Longport SDK API Test ===");
    println!("============================================================\n");

    // 1. Quote
    print_section("1. Quote");
    match ctx.quote([hk_sym, us_sym]).await {
        Ok(quotes) => {
            for q in &quotes {
                println!(
                    "  {}: last={} vol={} turnover={}",
                    q.symbol, q.last_done, q.volume, q.turnover
                );
                if let Some(pre) = &q.pre_market_quote {
                    println!("    pre_market: last={} vol={}", pre.last_done, pre.volume);
                }
                if let Some(post) = &q.post_market_quote {
                    println!(
                        "    post_market: last={} vol={}",
                        post.last_done, post.volume
                    );
                }
            }
        }
        Err(e) => println!("  ERROR: {e}"),
    }

    // 2. Depth
    print_section("2. Depth");
    for sym in [hk_sym, us_sym] {
        match ctx.depth(sym).await {
            Ok(depth) => {
                let ask_levels = depth.asks.len();
                let bid_levels = depth.bids.len();
                let has_data = depth.asks.iter().any(|a| a.volume > 0);
                println!(
                    "  {}: asks={} bids={} has_data={}",
                    sym, ask_levels, bid_levels, has_data
                );
                if has_data {
                    if let Some(a) = depth.asks.first() {
                        println!(
                            "    best_ask: price={:?} vol={} orders={}",
                            a.price, a.volume, a.order_num
                        );
                    }
                    if let Some(b) = depth.bids.first() {
                        println!(
                            "    best_bid: price={:?} vol={} orders={}",
                            b.price, b.volume, b.order_num
                        );
                    }
                }
            }
            Err(e) => println!("  {} ERROR: {e}", sym),
        }
    }

    // 3. Broker Queue
    print_section("3. Broker Queue");
    match ctx.brokers(hk_sym).await {
        Ok(brokers) => {
            println!(
                "  {}: ask_levels={} bid_levels={}",
                hk_sym,
                brokers.ask_brokers.len(),
                brokers.bid_brokers.len()
            );
        }
        Err(e) => println!("  ERROR: {e}"),
    }

    // 4. Trades
    print_section("4. Trades");
    for sym in [hk_sym, us_sym] {
        match ctx.trades(sym, 5).await {
            Ok(trades) => {
                println!("  {}: {} trades returned", sym, trades.len());
                if let Some(t) = trades.first() {
                    println!(
                        "    first: price={} vol={} direction={:?} session={:?}",
                        t.price, t.volume, t.direction, t.trade_session
                    );
                }
            }
            Err(e) => println!("  {} ERROR: {e}", sym),
        }
    }

    // 5. History Candlesticks
    print_section("5. History Candlesticks");
    for sym in [hk_sym, us_sym] {
        match ctx
            .history_candlesticks_by_offset(
                sym,
                Period::Day,
                AdjustType::ForwardAdjust,
                true,
                None,
                5,
                TradeSessions::Intraday,
            )
            .await
        {
            Ok(candles) => {
                println!("  {} daily: {} candles", sym, candles.len());
                for c in &candles {
                    println!(
                        "    {} O={} H={} L={} C={} V={}",
                        c.timestamp, c.open, c.high, c.low, c.close, c.volume
                    );
                }
            }
            Err(e) => println!("  {} daily ERROR: {e}", sym),
        }
        match ctx
            .history_candlesticks_by_offset(
                sym,
                Period::Week,
                AdjustType::ForwardAdjust,
                true,
                None,
                3,
                TradeSessions::Intraday,
            )
            .await
        {
            Ok(candles) => println!("  {} weekly: {} candles", sym, candles.len()),
            Err(e) => println!("  {} weekly ERROR: {e}", sym),
        }
        match ctx
            .history_candlesticks_by_offset(
                sym,
                Period::FiveMinute,
                AdjustType::ForwardAdjust,
                true,
                None,
                5,
                TradeSessions::Intraday,
            )
            .await
        {
            Ok(candles) => println!("  {} 5min: {} candles", sym, candles.len()),
            Err(e) => println!("  {} 5min ERROR: {e}", sym),
        }
    }

    // 6. Intraday
    print_section("6. Intraday");
    for sym in [hk_sym, us_sym] {
        match ctx.intraday(sym, TradeSessions::Intraday).await {
            Ok(lines) => {
                println!("  {}: {} intraday points", sym, lines.len());
                if let Some(first) = lines.first() {
                    println!(
                        "    first: price={} vol={} turnover={} avg_price={}",
                        first.price, first.volume, first.turnover, first.avg_price
                    );
                }
                if let Some(last) = lines.last() {
                    println!(
                        "    last:  price={} vol={} turnover={} avg_price={}",
                        last.price, last.volume, last.turnover, last.avg_price
                    );
                }
            }
            Err(e) => println!("  {} ERROR: {e}", sym),
        }
    }

    // 7. CalcIndex - all available fields
    print_section("7. CalcIndex (all fields)");
    let all_calc = vec![
        CalcIndex::TurnoverRate,
        CalcIndex::VolumeRatio,
        CalcIndex::PeTtmRatio,
        CalcIndex::PbRatio,
        CalcIndex::Amplitude,
        CalcIndex::FiveMinutesChangeRate,
        CalcIndex::DividendRatioTtm,
        CalcIndex::YtdChangeRate,
        CalcIndex::FiveDayChangeRate,
        CalcIndex::TenDayChangeRate,
        CalcIndex::HalfYearChangeRate,
        CalcIndex::TotalMarketValue,
        CalcIndex::CapitalFlow,
        CalcIndex::ChangeRate,
        CalcIndex::ExpiryDate,
        CalcIndex::StrikePrice,
        CalcIndex::UpperStrikePrice,
        CalcIndex::LowerStrikePrice,
        CalcIndex::ImpliedVolatility,
        CalcIndex::WarrantDelta,
        CalcIndex::CallPrice,
        CalcIndex::ToCallPrice,
        CalcIndex::EffectiveLeverage,
        CalcIndex::LeverageRatio,
        CalcIndex::ConversionRatio,
        CalcIndex::BalancePoint,
        CalcIndex::OpenInterest,
        CalcIndex::Delta,
        CalcIndex::Gamma,
        CalcIndex::Theta,
        CalcIndex::Vega,
        CalcIndex::Rho,
    ];

    // Stock CalcIndex
    for sym in [hk_sym, us_sym] {
        match ctx.calc_indexes([sym], all_calc.clone()).await {
            Ok(results) => {
                for r in &results {
                    println!("  {} CalcIndex:", r.symbol);
                    println!(
                        "    turnover_rate={:?} volume_ratio={:?} pe_ttm={:?} pb={:?}",
                        r.turnover_rate, r.volume_ratio, r.pe_ttm_ratio, r.pb_ratio
                    );
                    println!(
                        "    amplitude={:?} 5min_change={:?} div_yield={:?} change_rate={:?}",
                        r.amplitude,
                        r.five_minutes_change_rate,
                        r.dividend_ratio_ttm,
                        r.change_rate
                    );
                    println!(
                        "    ytd={:?} 5d={:?} 10d={:?} half_yr={:?}",
                        r.ytd_change_rate,
                        r.five_day_change_rate,
                        r.ten_day_change_rate,
                        r.half_year_change_rate,
                    );
                    println!(
                        "    total_mkt_val={:?} capital_flow={:?}",
                        r.total_market_value, r.capital_flow
                    );
                }
            }
            Err(e) => println!("  {} CalcIndex ERROR: {e}", sym),
        }
    }

    // Option CalcIndex (IV, Greeks)
    let option_sym = "AAPL260417C255000.US";
    match ctx.calc_indexes([option_sym], all_calc.clone()).await {
        Ok(results) => {
            for r in &results {
                println!("  {} Option CalcIndex:", r.symbol);
                println!(
                    "    implied_vol={:?} delta={:?} gamma={:?} theta={:?} vega={:?} rho={:?}",
                    r.implied_volatility, r.delta, r.gamma, r.theta, r.vega, r.rho
                );
                println!(
                    "    open_interest={:?} expiry={:?} strike={:?}",
                    r.open_interest, r.expiry_date, r.strike_price
                );
            }
        }
        Err(e) => println!("  {} option CalcIndex ERROR: {e}", option_sym),
    }

    // 8. Option Chain
    print_section("8. Option Chain");
    match ctx.option_chain_expiry_date_list(us_sym).await {
        Ok(dates) => {
            println!("  {} option expiry dates: {}", us_sym, dates.len());
            for d in dates.iter().take(5) {
                println!("    {}", d);
            }
            if dates.len() > 5 {
                println!("    ... ({} total)", dates.len());
            }
        }
        Err(e) => println!("  {} option expiry ERROR: {e}", us_sym),
    }
    match ctx.option_chain_expiry_date_list(hk_sym).await {
        Ok(dates) => println!("  {} option expiry dates: {}", hk_sym, dates.len()),
        Err(e) => println!("  {} option expiry: {e}", hk_sym),
    }

    let expiry = time::Date::from_calendar_date(2026, time::Month::April, 17)?;
    match ctx.option_chain_info_by_date(us_sym, expiry).await {
        Ok(chain) => {
            println!("  {} 2026-04-17 chain: {} strikes", us_sym, chain.len());
            if let Some(atm) = chain
                .iter()
                .find(|c| c.price.to_string().starts_with("255"))
            {
                println!(
                    "    ATM price={} call={} put={}",
                    atm.price, atm.call_symbol, atm.put_symbol
                );
            }
        }
        Err(e) => println!("  {} chain ERROR: {e}", us_sym),
    }

    // 9. Warrant (HK)
    print_section("9. Warrant");
    match ctx
        .warrant_list(
            hk_sym,
            longport::quote::WarrantSortBy::LastDone,
            longport::quote::SortOrderType::Descending,
            None,
            None,
            None,
            None,
            None,
        )
        .await
    {
        Ok(warrants) => {
            println!("  {} warrants: {} found", hk_sym, warrants.len());
            for w in warrants.iter().take(3) {
                println!(
                    "    {} name={} type={:?} strike={:?} expiry={} iv={:?} outstanding={:?}",
                    w.symbol,
                    w.name,
                    w.warrant_type,
                    w.strike_price,
                    w.expiry_date,
                    w.implied_volatility,
                    w.outstanding_ratio
                );
            }
        }
        Err(e) => println!("  {} warrant ERROR: {e}", hk_sym),
    }

    // 10. Capital Flow
    print_section("10. Capital Flow");
    for sym in [hk_sym, us_sym] {
        match ctx.capital_flow(sym).await {
            Ok(flows) => {
                println!("  {}: {} data points", sym, flows.len());
                if let Some(last) = flows.last() {
                    println!("    latest: inflow={}", last.inflow);
                }
            }
            Err(e) => println!("  {} ERROR: {e}", sym),
        }
    }

    // 11. Capital Distribution
    print_section("11. Capital Distribution");
    for sym in [hk_sym, us_sym] {
        match ctx.capital_distribution(sym).await {
            Ok(dist) => {
                println!(
                    "  {}: large_in={} large_out={} medium_in={} medium_out={} small_in={} small_out={}",
                    sym,
                    dist.capital_in.large, dist.capital_out.large,
                    dist.capital_in.medium, dist.capital_out.medium,
                    dist.capital_in.small, dist.capital_out.small,
                );
            }
            Err(e) => println!("  {} ERROR: {e}", sym),
        }
    }

    // 12. Static Info
    print_section("12. Static Info");
    match ctx.static_info([hk_sym, us_sym]).await {
        Ok(infos) => {
            for info in &infos {
                println!(
                    "  {}: exchange={} lot_size={} derivatives={:?} board={:?}",
                    info.symbol, info.exchange, info.lot_size, info.stock_derivatives, info.board
                );
            }
        }
        Err(e) => println!("  ERROR: {e}"),
    }

    // 13. Security List
    print_section("13. Security List");
    // Longport docs: this endpoint currently only supports
    // `market=US` and `category=Overnight`.
    match ctx
        .security_list(
            longport::Market::US,
            longport::quote::SecurityListCategory::Overnight,
        )
        .await
    {
        Ok(list) => {
            println!("  US overnight securities: {} found", list.len());
            for security in list.iter().take(3) {
                println!(
                    "    {} {} / {} / {}",
                    security.symbol, security.name_en, security.name_cn, security.name_hk
                );
            }
        }
        Err(e) => println!("  US security_list ERROR: {e}"),
    }

    // 14. Participants (Broker Info)
    print_section("14. Participants");
    match ctx.participants().await {
        Ok(brokers) => println!("  Total broker participants: {}", brokers.len()),
        Err(e) => println!("  ERROR: {e}"),
    }

    // 15. Trading Days
    print_section("15. Trading Days");
    let start = time::Date::from_calendar_date(2026, time::Month::April, 1)?;
    let end = time::Date::from_calendar_date(2026, time::Month::April, 10)?;
    match ctx.trading_days(longport::Market::HK, start, end).await {
        Ok(days) => {
            println!("  HK trading days Apr 1-10: {:?}", days.trading_days);
            println!("  HK half days: {:?}", days.half_trading_days);
        }
        Err(e) => println!("  ERROR: {e}"),
    }

    println!("\n============================================================");
    println!("=== Test Complete ===");
    println!("============================================================");

    Ok(())
}

fn print_section(name: &str) {
    println!("\n--- {} ---", name);
}
