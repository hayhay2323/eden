use super::*;

pub(super) fn macro_notice_relevant(notice: &AgentNotice) -> bool {
    notice.significance >= Decimal::new(45, 2)
        || matches!(
            notice.kind.as_str(),
            "market_event"
                | "cross_market_signal"
                | "sector_divergence"
                | "invalidation"
                | "broker_movement"
        )
}

pub(super) fn macro_headline_relevant(headline: &str) -> bool {
    let normalized = headline.to_ascii_lowercase();
    [
        "trump",
        "white house",
        "fed",
        "rate",
        "iran",
        "tariff",
        "sanction",
        "ceasefire",
        "talks",
        "opec",
        "oil",
        "beijing",
        "hkma",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

pub(super) fn macro_event_candidate_from_headline(
    tick: u64,
    market: LiveMarket,
    market_regime: &LiveMarketRegime,
    stress: &LiveStressSnapshot,
    headline: &str,
    sectors: &[AgentSectorFlow],
    symbols: &[AgentSymbolState],
    cross_market_signals: &[LiveCrossMarketSignal],
) -> AgentMacroEventCandidate {
    let temp_notice = AgentNotice {
        notice_id: format!("wake:{tick}"),
        tick,
        kind: "wake_headline".into(),
        symbol: None,
        sector: None,
        title: headline.into(),
        summary: headline.into(),
        significance: Decimal::new(60, 2),
    };
    macro_event_candidate_from_notice(
        tick,
        market,
        market_regime,
        stress,
        &temp_notice,
        sectors,
        symbols,
        cross_market_signals,
    )
}

pub(super) fn macro_event_candidate_from_notice(
    tick: u64,
    market: LiveMarket,
    market_regime: &LiveMarketRegime,
    stress: &LiveStressSnapshot,
    notice: &AgentNotice,
    sectors: &[AgentSectorFlow],
    symbols: &[AgentSymbolState],
    cross_market_signals: &[LiveCrossMarketSignal],
) -> AgentMacroEventCandidate {
    let headline = notice.title.clone();
    let event_type = classify_macro_event_type(notice);
    let jurisdictions = extract_macro_jurisdictions(&notice.summary);
    let entities = extract_macro_entities(notice, &notice.summary);
    let impact = route_macro_event_impact(
        market,
        market_regime,
        stress,
        notice,
        &event_type,
        &jurisdictions,
        sectors,
        symbols,
        cross_market_signals,
    );
    let authority_level = macro_event_authority_level(notice, &event_type, &headline);
    let novelty_score = (notice.significance
        + if notice.kind == "market_event" {
            Decimal::new(10, 2)
        } else {
            Decimal::ZERO
        })
    .min(Decimal::ONE);
    let confidence = (notice.significance
        + if impact.primary_scope == "market" {
            Decimal::new(10, 2)
        } else {
            Decimal::ZERO
        }
        + if !impact.affected_symbols.is_empty() {
            Decimal::new(5, 2)
        } else {
            Decimal::ZERO
        })
    .min(Decimal::ONE);

    AgentMacroEventCandidate {
        candidate_id: format!("macro_candidate:{tick}:{}", notice.notice_id),
        tick,
        market,
        source_kind: if notice.kind == "market_event" {
            "live_event".into()
        } else {
            "agent_notice".into()
        },
        source_name: "eden_internal".into(),
        event_type,
        authority_level,
        headline,
        summary: notice.summary.clone(),
        confidence: confidence.round_dp(4),
        novelty_score: novelty_score.round_dp(4),
        jurisdictions,
        entities,
        impact,
    }
}

pub(super) fn world_monitor_candidate_from_record(
    tick: u64,
    market: LiveMarket,
    market_regime: &LiveMarketRegime,
    stress: &LiveStressSnapshot,
    sectors: &[AgentSectorFlow],
    symbols: &[AgentSymbolState],
    cross_market_signals: &[LiveCrossMarketSignal],
    record: WorldMonitorEventRecord,
) -> Option<AgentMacroEventCandidate> {
    let headline = if record.title.trim().is_empty() {
        record.summary.trim().to_string()
    } else {
        record.title.trim().to_string()
    };
    if headline.is_empty() {
        return None;
    }

    let summary = if record.summary.trim().is_empty() {
        headline.clone()
    } else {
        record.summary.trim().to_string()
    };
    let text = format!("{headline} {summary}");
    let event_type = classify_world_monitor_event_type(&record, &text);
    let mut jurisdictions = record.countries.clone();
    for value in extract_macro_jurisdictions(&text) {
        push_unique(&mut jurisdictions, value);
    }
    let mut entities = record.entities.clone();
    for value in extract_symbols(&text) {
        push_unique(&mut entities, value);
    }
    let impact = route_world_monitor_impact(
        market,
        market_regime,
        stress,
        sectors,
        symbols,
        cross_market_signals,
        &record,
        &text,
        &event_type,
        &jurisdictions,
    );
    let confidence = world_monitor_confidence(&record, &impact);
    let novelty_score = world_monitor_novelty(&record);
    Some(AgentMacroEventCandidate {
        candidate_id: format!(
            "macro_candidate:{tick}:wm:{}",
            if record.id.trim().is_empty() {
                headline.replace(' ', "_").to_ascii_lowercase()
            } else {
                record.id.clone()
            }
        ),
        tick,
        market,
        source_kind: "world_monitor".into(),
        source_name: record
            .source
            .clone()
            .unwrap_or_else(|| "world_monitor".into()),
        event_type,
        authority_level: world_monitor_authority_level(&record),
        headline,
        summary,
        confidence,
        novelty_score,
        jurisdictions,
        entities,
        impact,
    })
}

fn classify_world_monitor_event_type(record: &WorldMonitorEventRecord, text: &str) -> String {
    let normalized = text.to_ascii_lowercase();
    let tags = record
        .topics
        .iter()
        .chain(record.market_tags.iter())
        .map(|item| item.to_ascii_lowercase())
        .collect::<Vec<_>>();
    if tags.iter().any(|tag| {
        matches!(
            tag.as_str(),
            "macro" | "rates" | "central-bank" | "central_bank" | "monetary-policy"
        )
    }) || normalized.contains("fed")
        || normalized.contains("rate")
    {
        "rates_macro".into()
    } else if tags.iter().any(|tag| {
        matches!(
            tag.as_str(),
            "geopolitics" | "policy" | "war" | "sanctions" | "tariffs"
        )
    }) || normalized.contains("trump")
        || normalized.contains("iran")
        || normalized.contains("tariff")
        || normalized.contains("sanction")
        || normalized.contains("ceasefire")
    {
        "geopolitical_policy".into()
    } else if tags
        .iter()
        .any(|tag| matches!(tag.as_str(), "oil" | "energy" | "shipping" | "commodities"))
        || normalized.contains("oil")
        || normalized.contains("opec")
        || normalized.contains("shipping")
    {
        "commodity_logistics".into()
    } else if tags
        .iter()
        .any(|tag| tag.contains("equities") || tag.contains("stocks"))
    {
        "market_structure".into()
    } else {
        "macro_event".into()
    }
}

fn route_world_monitor_impact(
    market: LiveMarket,
    market_regime: &LiveMarketRegime,
    stress: &LiveStressSnapshot,
    sectors: &[AgentSectorFlow],
    symbols: &[AgentSymbolState],
    cross_market_signals: &[LiveCrossMarketSignal],
    record: &WorldMonitorEventRecord,
    text: &str,
    event_type: &str,
    jurisdictions: &[String],
) -> AgentEventImpact {
    let mut affected_markets = Vec::new();
    for tag in record
        .market_tags
        .iter()
        .map(|item| item.to_ascii_lowercase())
    {
        if tag.contains("us") || tag.contains("nyse") || tag.contains("nasdaq") {
            push_unique(&mut affected_markets, "US Equities".into());
        }
        if tag.contains("hk") || tag.contains("hang seng") || tag.contains("hkex") {
            push_unique(&mut affected_markets, "HK Equities".into());
        }
    }
    let extracted_symbols = extract_symbols(text);
    for entity in record.entities.iter().chain(extracted_symbols.iter()) {
        if entity.ends_with(".US") {
            push_unique(&mut affected_markets, "US Equities".into());
        }
        if entity.ends_with(".HK") {
            push_unique(&mut affected_markets, "HK Equities".into());
        }
    }
    if affected_markets.is_empty()
        && matches!(
            event_type,
            "geopolitical_policy" | "rates_macro" | "commodity_logistics" | "macro_event"
        )
    {
        push_unique(&mut affected_markets, "US Equities".into());
        push_unique(&mut affected_markets, "HK Equities".into());
    }
    if affected_markets.is_empty() {
        push_unique(&mut affected_markets, impact_market_label(market));
    }

    let mut affected_sectors = Vec::new();
    for tag in record.market_tags.iter().chain(record.topics.iter()) {
        if let Some(flow) = sectors.iter().find(|item| {
            item.sector.eq_ignore_ascii_case(tag)
                || tag
                    .to_ascii_lowercase()
                    .contains(&item.sector.to_ascii_lowercase())
        }) {
            push_unique(&mut affected_sectors, flow.sector.clone());
        }
    }

    let mut affected_symbols = record
        .entities
        .iter()
        .filter(|item| item.ends_with(".HK") || item.ends_with(".US"))
        .cloned()
        .collect::<Vec<_>>();
    for symbol in extract_symbols(text) {
        push_unique(&mut affected_symbols, symbol);
    }
    if affected_symbols.is_empty() && !affected_sectors.is_empty() {
        for sector in &affected_sectors {
            if let Some(flow) = sectors.iter().find(|item| &item.sector == sector) {
                for leader in &flow.leaders {
                    push_unique(&mut affected_symbols, leader.clone());
                }
            }
        }
    }
    if affected_symbols.is_empty() && !symbols.is_empty() && affected_markets.len() > 1 {
        for symbol in symbols.iter().take(2) {
            push_unique(&mut affected_symbols, symbol.symbol.clone());
        }
    }

    let primary_scope = if affected_markets.len() > 1
        || matches!(
            event_type,
            "geopolitical_policy" | "rates_macro" | "commodity_logistics"
        ) {
        "market"
    } else if !affected_sectors.is_empty() {
        "sector"
    } else if !affected_symbols.is_empty() {
        "symbol"
    } else {
        "market"
    };
    let mut secondary_scopes = Vec::new();
    if primary_scope == "market" && !affected_sectors.is_empty() {
        secondary_scopes.push("sector".into());
    }
    if (primary_scope == "market" || primary_scope == "sector") && !affected_symbols.is_empty() {
        secondary_scopes.push("symbol".into());
    }
    let preferred_expression = match primary_scope {
        "market" => {
            if cross_market_signals.is_empty() {
                "index".into()
            } else {
                "index_then_dual_listed".into()
            }
        }
        "sector" => "sector_basket".into(),
        _ => "single_name".into(),
    };
    let mut decisive_factors = vec![format!(
        "source_tier={} event_type={} breadth_up={:.0}% breadth_down={:.0}%",
        record.source_tier.unwrap_or(4),
        event_type,
        (market_regime.breadth_up * Decimal::from(100)).round_dp(0),
        (market_regime.breadth_down * Decimal::from(100)).round_dp(0)
    )];
    if !jurisdictions.is_empty() {
        decisive_factors.push(format!("jurisdictions={}", jurisdictions.join(", ")));
    }
    if let Some(sync) = stress.sector_synchrony {
        decisive_factors.push(format!(
            "sector_synchrony={:.0}%",
            (sync * Decimal::from(100)).round_dp(0)
        ));
    }
    AgentEventImpact {
        primary_scope: primary_scope.into(),
        secondary_scopes,
        affected_markets,
        affected_sectors,
        affected_symbols,
        preferred_expression,
        requires_market_confirmation: primary_scope == "market",
        decisive_factors,
    }
}

fn world_monitor_authority_level(record: &WorldMonitorEventRecord) -> String {
    match record.source_tier.unwrap_or(4) {
        1 => "wire_or_official".into(),
        2 => "major_media".into(),
        3 => "specialty".into(),
        _ => "aggregator".into(),
    }
}

fn world_monitor_confidence(
    record: &WorldMonitorEventRecord,
    impact: &AgentEventImpact,
) -> Decimal {
    let tier_score = match record.source_tier.unwrap_or(4) {
        1 => Decimal::new(85, 2),
        2 => Decimal::new(72, 2),
        3 => Decimal::new(58, 2),
        _ => Decimal::new(45, 2),
    };
    let routing_bonus = if impact.primary_scope == "market" {
        Decimal::new(10, 2)
    } else if impact.primary_scope == "sector" {
        Decimal::new(6, 2)
    } else {
        Decimal::new(3, 2)
    };
    (tier_score + routing_bonus).min(Decimal::ONE).round_dp(4)
}

fn world_monitor_novelty(record: &WorldMonitorEventRecord) -> Decimal {
    let mut novelty = Decimal::new(55, 2);
    if !record.topics.is_empty() {
        novelty += Decimal::new(10, 2);
    }
    if !record.market_tags.is_empty() {
        novelty += Decimal::new(10, 2);
    }
    if !record.entities.is_empty() {
        novelty += Decimal::new(5, 2);
    }
    novelty.min(Decimal::ONE).round_dp(4)
}

fn classify_macro_event_type(notice: &AgentNotice) -> String {
    let normalized = format!("{} {}", notice.title, notice.summary).to_ascii_lowercase();
    if normalized.contains("trump")
        || normalized.contains("white house")
        || normalized.contains("tariff")
        || normalized.contains("sanction")
        || normalized.contains("talks")
        || normalized.contains("iran")
        || normalized.contains("ceasefire")
    {
        "geopolitical_policy".into()
    } else if normalized.contains("fed")
        || normalized.contains("rate")
        || normalized.contains("yield")
        || normalized.contains("inflation")
        || normalized.contains("cpi")
    {
        "rates_macro".into()
    } else if normalized.contains("oil")
        || normalized.contains("opec")
        || normalized.contains("shipping")
        || normalized.contains("maritime")
    {
        "commodity_logistics".into()
    } else if notice.kind == "cross_market_signal" {
        "cross_market_propagation".into()
    } else if notice.kind == "sector_divergence" {
        "sector_rotation".into()
    } else if notice.kind == "broker_movement" {
        "informed_flow".into()
    } else if notice.kind == "invalidation" {
        "thesis_failure".into()
    } else if notice.kind == "transition" || notice.kind == "wake_headline" {
        "thesis_transition".into()
    } else {
        "market_structure".into()
    }
}

fn extract_macro_jurisdictions(text: &str) -> Vec<String> {
    let normalized = text.to_ascii_lowercase();
    let mut values = Vec::new();
    for (needle, label) in [
        ("hong kong", "Hong Kong"),
        (" hk ", "Hong Kong"),
        ("china", "China"),
        ("beijing", "China"),
        ("us ", "United States"),
        ("u.s.", "United States"),
        ("america", "United States"),
        ("white house", "United States"),
        ("federal reserve", "United States"),
        ("iran", "Iran"),
        ("middle east", "Middle East"),
        ("europe", "Europe"),
        ("eu ", "Europe"),
    ] {
        if normalized.contains(needle) {
            push_unique(&mut values, label.into());
        }
    }
    values
}

fn extract_macro_entities(notice: &AgentNotice, text: &str) -> Vec<String> {
    let mut values = Vec::new();
    if let Some(symbol) = notice.symbol.as_ref() {
        push_unique(&mut values, symbol.clone());
    }
    for symbol in extract_symbols(text) {
        push_unique(&mut values, symbol);
    }
    for entity in [
        "Trump",
        "White House",
        "Federal Reserve",
        "HKMA",
        "OPEC",
        "Iran",
    ] {
        if text
            .to_ascii_lowercase()
            .contains(&entity.to_ascii_lowercase())
        {
            push_unique(&mut values, entity.into());
        }
    }
    values
}

fn route_macro_event_impact(
    market: LiveMarket,
    market_regime: &LiveMarketRegime,
    stress: &LiveStressSnapshot,
    notice: &AgentNotice,
    event_type: &str,
    jurisdictions: &[String],
    sectors: &[AgentSectorFlow],
    symbols: &[AgentSymbolState],
    cross_market_signals: &[LiveCrossMarketSignal],
) -> AgentEventImpact {
    let mut affected_markets = vec![impact_market_label(market)];
    let mut affected_sectors = notice.sector.clone().into_iter().collect::<Vec<_>>();
    let mut affected_symbols = notice.symbol.clone().into_iter().collect::<Vec<_>>();
    for symbol in extract_symbols(&notice.summary) {
        push_unique(&mut affected_symbols, symbol);
    }
    if matches!(
        event_type,
        "geopolitical_policy" | "rates_macro" | "commodity_logistics"
    ) {
        push_unique(&mut affected_markets, "US Equities".into());
        push_unique(&mut affected_markets, "HK Equities".into());
    }
    if notice.kind == "sector_divergence" {
        if let Some(sector) = &notice.sector {
            if let Some(flow) = sectors.iter().find(|item| &item.sector == sector) {
                for leader in &flow.leaders {
                    push_unique(&mut affected_symbols, leader.clone());
                }
            }
        }
    }
    if notice.kind == "cross_market_signal" {
        for signal in cross_market_signals.iter().take(3) {
            push_unique(&mut affected_symbols, signal.us_symbol.clone());
            push_unique(&mut affected_symbols, signal.hk_symbol.clone());
        }
        push_unique(&mut affected_markets, "US Equities".into());
        push_unique(&mut affected_markets, "HK Equities".into());
    }
    if affected_sectors.is_empty() && affected_symbols.is_empty() && !sectors.is_empty() {
        for flow in sectors.iter().take(2) {
            push_unique(&mut affected_sectors, flow.sector.clone());
        }
    }
    if affected_symbols.is_empty() {
        for symbol in symbols.iter().take(2) {
            push_unique(&mut affected_symbols, symbol.symbol.clone());
        }
    }

    let primary_scope = if matches!(
        event_type,
        "geopolitical_policy" | "rates_macro" | "commodity_logistics" | "cross_market_propagation"
    ) || market_regime.breadth_up >= Decimal::new(75, 2)
        || market_regime.breadth_down >= Decimal::new(75, 2)
    {
        "market"
    } else if !affected_sectors.is_empty() {
        "sector"
    } else {
        "symbol"
    };
    let mut secondary_scopes = Vec::new();
    if primary_scope == "market" && !affected_sectors.is_empty() {
        secondary_scopes.push("sector".into());
    }
    if (primary_scope == "market" || primary_scope == "sector") && !affected_symbols.is_empty() {
        secondary_scopes.push("symbol".into());
    }

    let preferred_expression = match primary_scope {
        "market" => {
            if cross_market_signals.is_empty() {
                "index".into()
            } else {
                "index_then_dual_listed".into()
            }
        }
        "sector" => "sector_basket".into(),
        _ => "single_name".into(),
    };

    let mut decisive_factors = vec![format!(
        "event_type={} breadth_up={:.0}% breadth_down={:.0}%",
        event_type,
        (market_regime.breadth_up * Decimal::from(100)).round_dp(0),
        (market_regime.breadth_down * Decimal::from(100)).round_dp(0)
    )];
    if !jurisdictions.is_empty() {
        decisive_factors.push(format!("jurisdictions={}", jurisdictions.join(", ")));
    }
    if let Some(sync) = stress.sector_synchrony {
        decisive_factors.push(format!(
            "sector_synchrony={:.0}% composite_stress={:.0}%",
            (sync * Decimal::from(100)).round_dp(0),
            (stress.composite_stress * Decimal::from(100)).round_dp(0)
        ));
    }
    dedupe_strings(&mut affected_markets);
    dedupe_strings(&mut affected_sectors);
    dedupe_strings(&mut affected_symbols);
    dedupe_strings(&mut secondary_scopes);

    AgentEventImpact {
        primary_scope: primary_scope.into(),
        secondary_scopes,
        affected_markets,
        affected_sectors,
        affected_symbols,
        preferred_expression,
        requires_market_confirmation: primary_scope == "market",
        decisive_factors,
    }
}

fn macro_event_authority_level(notice: &AgentNotice, event_type: &str, headline: &str) -> String {
    let normalized = format!("{} {}", notice.kind, headline).to_ascii_lowercase();
    if normalized.contains("white house")
        || normalized.contains("federal reserve")
        || normalized.contains("hkma")
        || normalized.contains("state dept")
    {
        "official".into()
    } else if matches!(
        event_type,
        "geopolitical_policy" | "rates_macro" | "commodity_logistics"
    ) {
        "derived_macro".into()
    } else {
        "derived_market".into()
    }
}

pub(super) fn macro_market_confirmation(
    market_regime: &LiveMarketRegime,
    stress: &LiveStressSnapshot,
) -> Decimal {
    let breadth = market_regime.breadth_up.max(market_regime.breadth_down);
    let return_strength = market_regime.average_return.abs().min(Decimal::ONE);
    let sync = stress.sector_synchrony.unwrap_or(Decimal::ZERO);
    ((breadth * Decimal::new(4, 1))
        + (return_strength * Decimal::new(3, 0))
        + (sync * Decimal::new(3, 1)))
    .min(Decimal::ONE)
    .round_dp(4)
}

pub(super) fn impact_market_label(market: LiveMarket) -> String {
    match market {
        LiveMarket::Hk => "HK Equities".into(),
        LiveMarket::Us => "US Equities".into(),
    }
}
