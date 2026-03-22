use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use longport::quote::{
    CalcIndex, MarketTemperature, Period, PushEvent, PushEventDetail, QuoteContext,
    SecurityBrokers, SecurityCalcIndex, SecurityDepth, SecurityQuote, SubFlags, Trade,
    TradeSessions,
};
use longport::{Config, Market};
use rust_decimal::Decimal;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TrySendError;
use tokio::time::Duration;

use eden::action::narrative::NarrativeSnapshot;
use eden::action::workflow::{ActionDescriptor, ActionWorkflowSnapshot, SuggestedAction};
#[cfg(feature = "persistence")]
use eden::cases::build_case_list;
use eden::external::polymarket::{
    fetch_polymarket_snapshot, load_polymarket_configs, PolymarketMarketConfig, PolymarketSnapshot,
};
use eden::graph::decision::{DecisionSnapshot, OrderDirection, StructuralFingerprint};
use eden::graph::graph::BrainGraph;
use eden::graph::insights::{ConflictHistory, GraphInsights};
use eden::graph::tracker::PositionTracker;
use eden::graph::validation::{SignalScorecard, SignalType};
use eden::live_snapshot::{
    ensure_snapshot_parent, snapshot_path, spawn_write_snapshot, LiveBackwardChain,
    LiveCausalLeader, LiveEvent, LiveEvidence, LiveHypothesisTrack, LiveLineageMetric, LiveMarket,
    LiveMarketRegime, LivePressure, LiveScorecard, LiveSignal, LiveSnapshot, LiveStressSnapshot,
    LiveTacticalCase,
};
use eden::logic::tension::TensionSnapshot;
use eden::ontology::links::LinkSnapshot;
use eden::ontology::objects::{BrokerId, Symbol};
use eden::ontology::reasoning::HypothesisTrack;
use eden::ontology::snapshot::{self, RawSnapshot};
use eden::ontology::store;
use eden::ontology::TacticalSetup;
use eden::persistence::action_workflow::{ActionWorkflowEventRecord, ActionWorkflowRecord};
#[cfg(feature = "persistence")]
use eden::persistence::case_realized_outcome::CaseRealizedOutcomeRecord;
#[cfg(feature = "persistence")]
use eden::persistence::case_reasoning_assessment::CaseReasoningAssessmentRecord;
use eden::pipeline::dimensions::DimensionSnapshot;
use eden::pipeline::reasoning::{path_has_family, path_is_mixed_multi_hop, ReasoningSnapshot};
use eden::pipeline::signals::{
    DerivedSignalSnapshot, EventSnapshot, MarketEventKind, ObservationSnapshot, SignalScope,
};
use eden::pipeline::world::WorldSnapshots;
use eden::runtime_loop::{next_tick, spawn_periodic_fetch, TickState};
use eden::temporal::analysis::{compute_dynamics, compute_polymarket_dynamics};
use eden::temporal::buffer::TickHistory;
use eden::temporal::causality::{compute_causal_timelines, CausalTimeline};
#[cfg(feature = "persistence")]
use eden::temporal::causality::{CausalFlipEvent, CausalTimelinePoint};
#[cfg(feature = "persistence")]
use eden::temporal::lineage::compute_case_realized_outcomes;
use eden::temporal::lineage::compute_lineage_stats;
use eden::temporal::record::TickRecord;

#[cfg(feature = "persistence")]
use eden::persistence::hypothesis_track::HypothesisTrackRecord;
#[cfg(feature = "persistence")]
use eden::persistence::lineage_metric_row::{
    row_matches_filters, rows_from_lineage_stats, snapshot_records_from_rows,
};
#[cfg(feature = "persistence")]
use eden::persistence::lineage_snapshot::LineageSnapshotRecord;
#[cfg(feature = "persistence")]
use eden::persistence::store::EdenStore;
#[cfg(feature = "persistence")]
use eden::persistence::tactical_setup::TacticalSetupRecord;
use eden::temporal::lineage::{LineageAlignmentFilter, LineageFilters, LineageSortKey};

#[cfg(feature = "persistence")]
const CASE_OUTCOME_RESOLUTION_LAG: u64 = 15;
const WATCHLIST: &[&str] = &[
    // ── User Holdings ──
    "981.HK",  // SMIC
    "2259.HK", // Zijin Gold International
    // ── Tech: Internet, Software, Platforms ──
    "700.HK",  // Tencent
    "9988.HK", // Alibaba
    "3690.HK", // Meituan
    "9618.HK", // JD.com
    "1810.HK", // Xiaomi
    "9888.HK", // Baidu
    "268.HK",  // Kingdee
    "9999.HK", // NetEase
    "9698.HK", // Trip.com
    "1024.HK", // Kuaishou
    "772.HK",  // China Literature
    "780.HK",  // Tongcheng Travel
    "3888.HK", // Kingsoft
    "9626.HK", // Bilibili
    "6618.HK", // JD Health
    "241.HK",  // Alibaba Health
    "9898.HK", // Weibo
    "6060.HK", // ZhongAn Online
    "2013.HK", // Weimob
    "1797.HK", // NetEase Cloud Music
    "992.HK",  // Lenovo
    "909.HK",  // Ming Yuan Cloud
    "2018.HK", // AAC Technologies
    "2382.HK", // Sunny Optical
    "285.HK",  // BYD Electronic
    "6690.HK", // Haier Smart Home
    "1691.HK", // JS Global Lifestyle
    "2038.HK", // FIT Hon Teng
    "669.HK",  // Techtronic Industries
    "1833.HK", // Ping An Healthcare
    "6855.HK", // Asiainfo Technologies
    "522.HK",  // ASM Pacific
    "6098.HK", // CG Services
    "9969.HK", // iQIYI
    "2096.HK", // Sinohealth
    // ── Semiconductor ──
    "1347.HK", // Hua Hong Semiconductor
    "2518.HK", // ASMPT
    "1385.HK", // Shanghai Fudan Micro
    // ── Finance: Banks, Brokerages, Exchanges ──
    "5.HK",    // HSBC
    "388.HK",  // HKEX
    "1398.HK", // ICBC
    "3988.HK", // Bank of China
    "939.HK",  // CCB
    "1288.HK", // ABC
    "2388.HK", // BOC Hong Kong
    "11.HK",   // Hang Seng Bank
    "3328.HK", // Bank of Communications
    "1658.HK", // Postal Savings Bank
    "6881.HK", // China Galaxy Securities
    "6030.HK", // CITIC Securities
    "3908.HK", // China International Capital
    "6886.HK", // Huatai Securities
    "3968.HK", // CM Bank
    "1988.HK", // Minsheng Bank
    "998.HK",  // CITIC Bank
    "1963.HK", // Bank of Chongqing
    "6818.HK", // China Everbright Bank
    // "2066.HK",  // Shenwan Hongyuan — delisted/invalid on Longport capital_flow
    // "6837.HK",  // Haitong Securities — delisted/invalid on Longport capital_flow
    "1776.HK", // GF Securities
    "1359.HK", // China Cinda
    "6199.HK", // Lufax
    "2799.HK", // China Huarong
    "3618.HK", // Chongqing Rural Commercial
    "1916.HK", // China Resources Bank
    "2611.HK", // Guotai Junan International
    "3698.HK", // Huishang Bank
    "6196.HK", // Bank of Zhengzhou
    "1461.HK", // Bank of Guizhou
    "2356.HK", // Dah Sing Banking
    "440.HK",  // Dah Sing Financial
    "23.HK",   // Bank of East Asia
    "1111.HK", // Chong Hing Bank
    "6178.HK", // Everbright Securities
    // ── Energy: Oil, Gas, Coal ──
    "883.HK",  // CNOOC
    "857.HK",  // PetroChina
    "386.HK",  // Sinopec
    "1088.HK", // China Shenhua Energy
    "2688.HK", // ENN Energy
    "384.HK",  // China Gas Holdings
    "1193.HK", // China Resources Gas
    "135.HK",  // Kunlun Energy
    "1171.HK", // Yankuang Energy
    "3983.HK", // China BlueChemical
    "467.HK",  // United Energy Group
    "2883.HK", // China Oilfield Services
    "3899.HK", // CIMC Enric
    "1083.HK", // Towngas China
    // ── Telecom ──
    "941.HK",  // China Mobile
    "762.HK",  // China Unicom
    "728.HK",  // China Telecom
    "6823.HK", // HKT Trust
    // ── Property: Developers, REITs ──
    "16.HK",   // SHK Properties
    "1109.HK", // China Resources Land
    "688.HK",  // China Overseas Land
    "1113.HK", // CK Asset
    "17.HK",   // New World Development
    "12.HK",   // Henderson Land
    "101.HK",  // Hang Lung Properties
    "823.HK",  // Link REIT
    "1997.HK", // Wharf REIC
    "960.HK",  // Longfor Group
    "3383.HK", // Agile Group
    "884.HK",  // CIFI Holdings
    "2202.HK", // China Vanke
    "1030.HK", // Future Land
    "123.HK",  // Yuexiu Property
    "119.HK",  // Poly Property
    "3900.HK", // Greentown China
    "81.HK",   // China Overseas Grand Oceans
    "83.HK",   // Sino Land
    "14.HK",   // Hysan Development
    "1972.HK", // Swire Properties
    "778.HK",  // Fortune REIT
    "405.HK",  // Yuexiu REIT
    "1908.HK", // C&D International
    "9979.HK", // Greentown Management
    "813.HK",  // Shimao Group
    "2007.HK", // Country Garden
    // ── Consumer: Food, Beverage, Retail, Sportswear ──
    "1929.HK", // Chow Tai Fook
    "2020.HK", // Anta Sports
    "6862.HK", // Haidilao
    "9633.HK", // Nongfu Spring
    "2319.HK", // China Mengniu Dairy
    "291.HK",  // China Resources Beer
    "168.HK",  // Tsingtao Brewery
    "322.HK",  // Tingyi (Master Kong)
    "151.HK",  // Want Want China
    "2331.HK", // Li Ning
    "9987.HK", // Yum China
    "220.HK",  // Uni-President China
    "6186.HK", // China Feihe
    "1044.HK", // Hengan International
    // "3799.HK",  // Dali Foods — delisted/invalid on Longport capital_flow
    "6969.HK", // Smoore International
    "9922.HK", // Jiumaojiu
    "1458.HK", // Zhou Hei Ya
    "6808.HK", // Sun Art Retail
    // "3331.HK",  // Vinda International — delisted/invalid on Longport capital_flow
    "1910.HK", // Samsonite
    "9992.HK", // Pop Mart
    "6993.HK", // Blue Moon Group
    "3998.HK", // Bosideng
    "9660.HK", // Mao Geping
    "6110.HK", // Topsports International
    "116.HK",  // Chow Sang Sang
    "590.HK",  // Luk Fook Holdings
    "1579.HK", // Yihai International
    "9869.HK", // Soulgate
    "9995.HK", // RLX Technology
    "3319.HK", // A-Living Smart City
    // ── Healthcare: Pharma, Biotech ──
    "2269.HK", // WuXi Bio
    "1177.HK", // Sino Biopharmaceutical
    "2359.HK", // WuXi AppTec
    "1093.HK", // CSPC Pharmaceutical
    "6160.HK", // BeiGene
    "2616.HK", // China Resources Pharmaceutical
    "3692.HK", // Hansoh Pharmaceutical
    "1801.HK", // Innovent Biologics
    "2196.HK", // Shanghai Fosun Pharma
    "6185.HK", // CanSino Biologics
    "1513.HK", // Livzon Pharmaceutical
    "570.HK",  // China Traditional Chinese Medicine
    "867.HK",  // China Medical System
    "6622.HK", // Zhaoke Ophthalmology
    "2607.HK", // Shanghai Pharmaceuticals
    "3320.HK", // China Resources Medical
    "2142.HK", // Simcere Pharmaceutical
    "1066.HK", // Weigao Group
    "2186.HK", // Luye Pharma
    "1530.HK", // 3SBio
    "9926.HK", // Akeso
    // ── Utilities: Power, Gas, Water ──
    "2.HK",    // CLP Holdings
    "3.HK",    // HK & China Gas
    "6.HK",    // Power Assets
    "836.HK",  // China Resources Power
    "1038.HK", // CK Infrastructure
    "902.HK",  // Huaneng Power
    "1071.HK", // Huadian Power
    "816.HK",  // Huadian Fuxin
    "1816.HK", // CGN Power
    "579.HK",  // Beijing Jingneng Clean
    "956.HK",  // China Suntien Green Energy
    "371.HK",  // Beijing Enterprises Water
    "270.HK",  // Guangdong Investment
    "855.HK",  // China Water Affairs
    "2380.HK", // China Power International
    "1798.HK", // Datang New Energy
    // ── Insurance ──
    "2318.HK", // Ping An
    "1299.HK", // AIA
    "2628.HK", // China Life
    "2601.HK", // CPIC
    "966.HK",  // China Taiping
    "1339.HK", // PICC
    "1508.HK", // China Reinsurance
    // ── Auto: EVs, Traditional Auto ──
    "9868.HK", // XPeng
    "2015.HK", // Li Auto
    "1211.HK", // BYD
    "175.HK",  // Geely Auto
    "2333.HK", // Great Wall Motor
    "9863.HK", // Zeekr
    "2238.HK", // GAC Group
    "1958.HK", // BAIC Motor
    "489.HK",  // Dongfeng Motor
    "2488.HK", // Leapmotor
    // ── Materials: Mining, Metals, Cement, Gold ──
    "2899.HK", // Zijin Mining
    "914.HK",  // Anhui Conch Cement
    "2600.HK", // Aluminum Corp of China
    "358.HK",  // Jiangxi Copper
    "3323.HK", // China National Building Material
    "1818.HK", // Zhaojin Mining
    "3993.HK", // China Molybdenum
    "1138.HK", // China Resources Cement
    "1208.HK", // MMG Limited
    "323.HK",  // Maanshan Iron & Steel
    "347.HK",  // Angang Steel
    "1787.HK", // Shandong Gold Mining
    "6865.HK", // Flat Glass Group
    "3606.HK", // Fuyao Glass
    "546.HK",  // Fufeng Group
    // ── Industrial: Construction, Railways, Infrastructure ──
    "1186.HK", // China Railway Construction
    "390.HK",  // China Railway Group
    "1766.HK", // CRRC
    "1800.HK", // China Communications Construction
    "3311.HK", // China State Construction Intl
    "1072.HK", // Dongfang Electric
    "2727.HK", // Shanghai Electric
    "1157.HK", // Zoomlion Heavy
    "3339.HK", // Lonking Holdings
    "696.HK",  // TravelSky Technology
    "1880.HK", // China Railway Signal
    "586.HK",  // China Conch Venture
    "177.HK",  // Jiangsu Expressway
    "576.HK",  // Zhejiang Expressway
    "548.HK",  // Shenzhen Expressway
    "107.HK",  // Sichuan Expressway
    "995.HK",  // Anhui Expressway
    // ── Conglomerate & Gaming ──
    "1.HK",    // CK Hutchison
    "19.HK",   // Swire Pacific
    "4.HK",    // Wharf Holdings
    "267.HK",  // CITIC Limited
    "27.HK",   // Galaxy Entertainment
    "10.HK",   // Hang Lung Group
    "66.HK",   // MTR Corporation
    "683.HK",  // Kerry Properties
    "659.HK",  // NWS Holdings
    "880.HK",  // SJM Holdings
    "1128.HK", // Wynn Macau
    "2282.HK", // MGM China
    "6883.HK", // Melco International
    "1928.HK", // Sands China
    // ── Media & Entertainment ──
    "1060.HK", // Alibaba Pictures
    "2400.HK", // XD Inc
    "799.HK",  // IGG Inc
    "777.HK",  // NetDragon Websoft
    // ── Logistics & Transport ──
    "2057.HK", // ZTO Express
    "2618.HK", // JD Logistics
    // "6139.HK",  // Kerry Logistics — delisted/invalid on Longport capital_flow
    "316.HK",  // Orient Overseas (Intl)
    "144.HK",  // China Merchants Port
    "1199.HK", // COSCO Shipping
    "1919.HK", // COSCO Shipping Holdings
    "1308.HK", // SITC International
    "2343.HK", // Pacific Basin Shipping
    "598.HK",  // Sinotrans
    "2866.HK", // COSCO Shipping Development
    "152.HK",  // Shenzhen International
    "694.HK",  // Beijing Capital Airport
    "753.HK",  // Air China
    "670.HK",  // China Eastern Airlines
    "1055.HK", // China Southern Airlines
    // ── Additional Tech ──
    "9961.HK", // Trip.com (ADR)
    "1478.HK", // Q Technology
    "1357.HK", // Meitu
    "9901.HK", // New Oriental Education
    "9911.HK", // NewBorn Town
    "6058.HK", // OneConnect Financial
    "3918.HK", // Nagacorp
    "1877.HK", // Shanghai Junshi Bio
    "1516.HK", // SinoMedia
    "1022.HK", // Fly Leasing
    // ── Additional Finance ──
    "1336.HK", // New China Life
    "3958.HK", // Orient Securities
    "1375.HK", // Central China Securities
    "3903.HK", // Hanhua Financial
    "412.HK",  // China Shandong Hi-Speed Financial
    "2858.HK", // Yixin Group
    "6099.HK", // China Merchants Securities
    "1539.HK", // Yestar Healthcare
    // ── Additional Energy ──
    "1258.HK", // China Yurun Food
    // ── Additional Property ──
    "3377.HK", // Sino-Ocean Group
    "1638.HK", // Kaisa Group
    "6158.HK", // COLI Property Services
    "345.HK",  // Vitasoy
    "272.HK",  // Shui On Land
    "35.HK",   // FE Consort International
    // "2868.HK",  // Beijing Capital Land — delisted/invalid on Longport capital_flow
    "127.HK",  // Chinese Estates
    "1238.HK", // Powerlong Real Estate
    // ── Additional Consumer ──
    "6127.HK", // Yi Feng Pharmacy
    "336.HK",  // Huabao International
    "1382.HK", // Pacific Textiles
    "6049.HK", // Poly Culture
    // "1212.HK",  // Lifestyle International — delisted/invalid on Longport capital_flow
    "848.HK",  // Maoye International
    "2888.HK", // Standard Chartered HK
    "1618.HK", // Metallurgical Corp China
    "763.HK",  // ZTE Corporation
    "552.HK",  // China Communications Services
    // ── Additional Healthcare ──
    "6978.HK", // Yadea Group
    "1548.HK", // Genscript Biotech
    "1302.HK", // Kindstar Globalgene
    "2126.HK", // Grand Pharma
    "6616.HK", // Gene Harbour Biosciences
    "1858.HK", // Chunbo (healthcare)
    "3613.HK", // Beijing Health
    "1317.HK", // Maple Leaf Education
    "9688.HK", // ZJLD Group
    // ── Additional Materials ──
    "2208.HK", // Xinjiang Goldwind Tech
    "1733.HK", // EEKA Fashion
    "1164.HK", // CGN Mining
    "691.HK",  // Shanshui Cement
    "2009.HK", // BBMG
    // ── Additional Industrial ──
    "1133.HK", // Harbin Electric
    "1882.HK", // Haitian International
    "3898.HK", // China Yida
    "2039.HK", // CIMC Vehicles
    // ── Additional Telecom/IT Services ──
    "354.HK", // China Software International
    // ── Additional Conglomerate & Gaming ──
    "142.HK", // First Pacific
    "242.HK", // Shun Tak Holdings
    "493.HK", // GOME Retail
    "551.HK", // Yue Yuen Industrial
    "303.HK", // VTech Holdings
    "179.HK", // Johnson Electric
    "69.HK",  // Shangri-La Asia
    "293.HK", // Cathay Pacific
    "189.HK", // Dongyue Group
    "215.HK", // Hutchison Telecom HK
    // ── Additional Utilities ──
    // ── Additional Logistics ──
    "636.HK",  // Kerry Logistics Network
    "3378.HK", // Xiamen C&D
    // ── Education ──
    "1765.HK", // Hope Education
    "6068.HK", // No Bull Financial
    "2001.HK", // New Higher Education
    "839.HK",  // China Education Group
    // ── REITs ──
    "87001.HK", // Hui Xian REIT
    "808.HK",   // Prosperus Real Estate
    "435.HK",   // Sunlight REIT
    "2778.HK",  // Champion REIT
    // ── Misc Large/Mid-Cap ──
    "2357.HK", // AVIC International
    // ── AI 六小虎 / 半導體 / 光纖 / 近期熱門 ──
    "2513.HK", // 智譜 AI (Zhipu) — AI LLM
    "100.HK",  // MiniMax — AI LLM
    "6082.HK", // 壁仞科技 Biren Technology — 國產 GPU
    "3896.HK", // 兆易創新 GigaDevice — 存儲芯片
    "6809.HK", // 澜起科技 Montage Technology — 內存接口芯片
    "600.HK",  // 愛芯元智 Aixin — AI 視覺芯片
    "6869.HK", // 長飛光纖 Yangtze Optical Fibre — AI 光互連
    // ── 更多 AI / 雲 / SaaS ──
    // ── 更多半導體產業鏈 ──
    // ── 更多新能源 ──
    "1799.HK", // Xinyi Solar
    "968.HK",  // Xinyi Glass
    // ── 更多券商/資管 ──
    // ── 港股近期活躍大市值 ──
    // ── 更多消費/餐飲 ──
    // ── 更多醫藥/CXO ──
    // ── 更多基建/軍工 ──
    // ── 更多地產 ──
    // ── 更多銀行 ──
    // ── 更多保險 ──
    // ── 新增：近期港股熱門標的 ──
    "1686.HK", // Sunevision (數據中心)
    "1361.HK", // 361 Degrees
    "2168.HK", // Kaisa Prosperity
    "9996.HK", // Satu Holdings
               // ── 新增：教育 ──
               // ── 新增：REITs ──
];

#[derive(Debug)]
enum CliCommand {
    Live,
    UsLive,
    Polymarket {
        json: bool,
    },
    CausalTimeline {
        leaf_scope_key: String,
        limit: usize,
    },
    CausalFlips {
        limit: usize,
    },
    Lineage {
        limit: usize,
        filters: LineageFilters,
        view: LineageViewOptions,
    },
    LineageHistory {
        snapshots: usize,
        filters: LineageFilters,
        view: LineageViewOptions,
    },
    LineageRows {
        rows: usize,
        filters: LineageFilters,
        view: LineageViewOptions,
    },
}

const CLI_USAGE: &str =
    "usage: eden us\n       eden polymarket [--json]\n       eden causal timeline <leaf_scope_key> [limit]\n       eden causal flips [limit]\n       eden lineage [limit] [--label <value>] [--bucket <value>] [--family <value>] [--session <value>] [--regime <value>] [--top <n>] [--sort net|conv|external] [--alignment all|confirm|contradict] [--json]\n       eden lineage history [snapshots] [--label <value>] [--bucket <value>] [--family <value>] [--session <value>] [--regime <value>] [--top <n>] [--sort net|conv|external] [--alignment all|confirm|contradict] [--latest-only] [--json]\n       eden lineage rows [rows] [--label <value>] [--bucket <value>] [--family <value>] [--session <value>] [--regime <value>] [--top <n>] [--sort net|conv|external] [--alignment all|confirm|contradict] [--latest-only] [--json]";

#[derive(Debug, Clone, Copy, Default)]
struct LineageViewOptions {
    top: usize,
    latest_only: bool,
    json: bool,
    sort_by: LineageSortKey,
    alignment: LineageAlignmentFilter,
}

fn parse_cli_command(args: &[String]) -> Result<CliCommand, String> {
    const DEFAULT_LIMIT: usize = 120;

    if args.len() <= 1 {
        return Ok(CliCommand::Live);
    }

    match args.get(1).map(|value| value.as_str()) {
        Some("us") => Ok(CliCommand::UsLive),
        Some("polymarket") => Ok(CliCommand::Polymarket {
            json: args.iter().any(|arg| arg == "--json"),
        }),
        Some("causal") => match args.get(2).map(|value| value.as_str()) {
            Some("timeline") => {
                let leaf_scope_key = args.get(3).cloned().ok_or_else(|| {
                    "usage: eden causal timeline <leaf_scope_key> [limit]".to_string()
                })?;
                let limit = parse_optional_limit(args.get(4), DEFAULT_LIMIT)?;
                Ok(CliCommand::CausalTimeline {
                    leaf_scope_key,
                    limit,
                })
            }
            Some("flips") => {
                let limit = parse_optional_limit(args.get(3), DEFAULT_LIMIT)?;
                Ok(CliCommand::CausalFlips { limit })
            }
            _ => Err(CLI_USAGE.into()),
        },
        Some("lineage") => parse_lineage_cli_command(&args[2..], DEFAULT_LIMIT),
        _ => Err(CLI_USAGE.into()),
    }
}

fn parse_lineage_cli_command(args: &[String], default_limit: usize) -> Result<CliCommand, String> {
    if matches!(args.first().map(|value| value.as_str()), Some("rows")) {
        let (rows, filters, view) = parse_lineage_arguments(&args[1..], default_limit)?;
        return Ok(CliCommand::LineageRows {
            rows,
            filters,
            view,
        });
    }
    if matches!(args.first().map(|value| value.as_str()), Some("history")) {
        let (snapshots, filters, view) = parse_lineage_arguments(&args[1..], default_limit)?;
        return Ok(CliCommand::LineageHistory {
            snapshots,
            filters,
            view,
        });
    }

    let (limit, filters, view) = parse_lineage_arguments(args, default_limit)?;
    if view.latest_only {
        return Err("--latest-only is only valid for `eden lineage history`".into());
    }
    Ok(CliCommand::Lineage {
        limit,
        filters,
        view,
    })
}

fn parse_lineage_arguments(
    args: &[String],
    default_limit: usize,
) -> Result<(usize, LineageFilters, LineageViewOptions), String> {
    let mut index = 0usize;
    let mut limit = default_limit;
    let mut filters = LineageFilters::default();
    let mut view = LineageViewOptions {
        top: 5,
        latest_only: false,
        json: false,
        sort_by: LineageSortKey::NetReturn,
        alignment: LineageAlignmentFilter::All,
    };

    if let Some(value) = args.get(index) {
        if !value.starts_with("--") {
            limit = parse_optional_limit(Some(value), default_limit)?;
            index += 1;
        }
    }

    while index < args.len() {
        let flag = args[index].as_str();
        match flag {
            "--latest-only" => {
                view.latest_only = true;
                index += 1;
                continue;
            }
            "--json" => {
                view.json = true;
                index += 1;
                continue;
            }
            "--label" | "--bucket" | "--family" | "--session" | "--regime" | "--top" | "--sort"
            | "--alignment" => {}
            _ => return Err(format!("unknown lineage flag: {}", flag)),
        }

        let value = args.get(index + 1).ok_or_else(|| match flag {
            "--label" => "missing value for --label".to_string(),
            "--bucket" => "missing value for --bucket".to_string(),
            "--family" => "missing value for --family".to_string(),
            "--session" => "missing value for --session".to_string(),
            "--regime" => "missing value for --regime".to_string(),
            "--top" => "missing value for --top".to_string(),
            "--sort" => "missing value for --sort".to_string(),
            "--alignment" => "missing value for --alignment".to_string(),
            _ => format!("unknown lineage flag: {}", flag),
        })?;

        match flag {
            "--label" => filters.label = Some(value.clone()),
            "--bucket" => filters.bucket = Some(value.clone()),
            "--family" => filters.family = Some(value.clone()),
            "--session" => filters.session = Some(value.clone()),
            "--regime" => filters.market_regime = Some(value.clone()),
            "--top" => {
                view.top = value
                    .parse::<usize>()
                    .map_err(|_| format!("invalid top value: {}", value))?;
                if view.top == 0 {
                    return Err("--top must be greater than 0".into());
                }
            }
            "--sort" => {
                view.sort_by = match value.as_str() {
                    "net" | "net_return" => LineageSortKey::NetReturn,
                    "conv" | "convergence" => LineageSortKey::ConvergenceScore,
                    "external" | "ext" => LineageSortKey::ExternalDelta,
                    _ => return Err(format!("invalid sort value: {}", value)),
                };
            }
            "--alignment" => {
                view.alignment = match value.as_str() {
                    "all" => LineageAlignmentFilter::All,
                    "confirm" => LineageAlignmentFilter::Confirm,
                    "contradict" => LineageAlignmentFilter::Contradict,
                    _ => return Err(format!("invalid alignment value: {}", value)),
                };
            }
            _ => return Err(format!("unknown lineage flag: {}", flag)),
        }
        index += 2;
    }

    Ok((limit, filters, view))
}

fn parse_optional_limit(arg: Option<&String>, default: usize) -> Result<usize, String> {
    match arg {
        None => Ok(default),
        Some(value) => value
            .parse::<usize>()
            .map_err(|_| format!("invalid limit: {}", value))
            .and_then(|limit| {
                if limit == 0 {
                    Err("limit must be greater than 0".into())
                } else {
                    Ok(limit)
                }
            }),
    }
}

fn summarize_hk_scorecard(scorecard: &SignalScorecard) -> LiveScorecard {
    let stats = scorecard.stats();
    let total_signals = stats.iter().map(|item| item.total).sum::<usize>();
    let resolved_signals = stats.iter().map(|item| item.resolved).sum::<usize>();
    let hits = stats.iter().map(|item| item.hits).sum::<usize>();
    let misses = resolved_signals.saturating_sub(hits);
    let hit_rate = if resolved_signals == 0 {
        Decimal::ZERO
    } else {
        Decimal::from(hits as i64) / Decimal::from(resolved_signals as i64)
    };
    let mean_return = if resolved_signals == 0 {
        Decimal::ZERO
    } else {
        stats
            .iter()
            .map(|item| item.mean_return * Decimal::from(item.resolved as i64))
            .sum::<Decimal>()
            / Decimal::from(resolved_signals as i64)
    };

    LiveScorecard {
        total_signals,
        resolved_signals,
        hits,
        misses,
        hit_rate,
        mean_return,
    }
}

fn build_hk_lineage_metrics(
    stats: &eden::temporal::lineage::LineageStats,
) -> Vec<LiveLineageMetric> {
    stats
        .promoted_outcomes
        .iter()
        .take(6)
        .map(|item| LiveLineageMetric {
            template: item.label.clone(),
            total: item.total,
            resolved: item.resolved,
            hits: item.hits,
            hit_rate: item.hit_rate,
            mean_return: item.mean_return,
        })
        .collect()
}

fn extract_symbol_scope(scope: &eden::ReasoningScope) -> Option<&Symbol> {
    match scope {
        eden::ReasoningScope::Symbol(symbol) => Some(symbol),
        _ => None,
    }
}

fn symbol_string_from_scope(scope: &eden::ReasoningScope) -> String {
    extract_symbol_scope(scope)
        .map(|symbol| symbol.0.clone())
        .unwrap_or_default()
}

fn sector_name_for_symbol(
    store: &std::sync::Arc<eden::ontology::store::ObjectStore>,
    symbol: &Symbol,
) -> Option<String> {
    let sector_id = store.stocks.get(symbol)?.sector_id.as_ref()?;
    store
        .sectors
        .get(sector_id)
        .map(|sector| sector.name.clone())
}

fn build_hk_backward_chains(snapshot: &eden::BackwardReasoningSnapshot) -> Vec<LiveBackwardChain> {
    snapshot
        .investigations
        .iter()
        .filter_map(|item| {
            let symbol = extract_symbol_scope(&item.leaf_scope)?;
            let leading = item.leading_cause.as_ref()?;

            let mut evidence = leading
                .supporting_evidence
                .iter()
                .map(|e| LiveEvidence {
                    source: e.channel.clone(),
                    description: e.statement.clone(),
                    weight: e.weight,
                    direction: e.weight,
                })
                .collect::<Vec<_>>();
            evidence.extend(leading.contradicting_evidence.iter().map(|e| LiveEvidence {
                source: e.channel.clone(),
                description: e.statement.clone(),
                weight: e.weight,
                direction: -e.weight,
            }));

            Some(LiveBackwardChain {
                symbol: symbol.0.clone(),
                conclusion: format!("{} — 主因: {}", item.leaf_label, leading.explanation),
                primary_driver: leading.explanation.clone(),
                confidence: leading.confidence,
                evidence,
            })
        })
        .take(10)
        .collect()
}

fn hk_causal_leader_streak(timeline: &CausalTimeline) -> u64 {
    let Some(latest) = timeline.points.last() else {
        return 0;
    };
    let latest_id = latest.leading_cause_id.as_deref();
    timeline
        .points
        .iter()
        .rev()
        .take_while(|point| point.leading_cause_id.as_deref() == latest_id)
        .count() as u64
}

fn build_hk_causal_leaders(
    timelines: &std::collections::HashMap<String, CausalTimeline>,
) -> Vec<LiveCausalLeader> {
    let mut items = timelines
        .values()
        .filter(|timeline| timeline.leaf_scope_key.ends_with(".HK"))
        .filter_map(|timeline| {
            let current_leader = timeline.latest_point()?.leading_explanation.clone()?;
            Some(LiveCausalLeader {
                symbol: timeline.leaf_scope_key.clone(),
                current_leader,
                leader_streak: hk_causal_leader_streak(timeline),
                flips: timeline.flip_events.len(),
            })
        })
        .collect::<Vec<_>>();
    items.sort_by(|a, b| b.leader_streak.cmp(&a.leader_streak));
    items.truncate(10);
    items
}

fn build_hk_live_snapshot(
    tick: u64,
    timestamp: String,
    store: &std::sync::Arc<eden::ontology::store::ObjectStore>,
    brain: &BrainGraph,
    decision: &DecisionSnapshot,
    graph_insights: &GraphInsights,
    reasoning_snapshot: &ReasoningSnapshot,
    event_snapshot: &EventSnapshot,
    observation_snapshot: &ObservationSnapshot,
    scorecard: &SignalScorecard,
    dim_snapshot: &DimensionSnapshot,
    latest: &TickRecord,
    tracker: &PositionTracker,
    causal_timelines: &std::collections::HashMap<String, CausalTimeline>,
    lineage_stats: &eden::temporal::lineage::LineageStats,
) -> LiveSnapshot {
    let hypothesis_map: HashMap<&str, &eden::Hypothesis> = reasoning_snapshot
        .hypotheses
        .iter()
        .map(|item| (item.hypothesis_id.as_str(), item))
        .collect();

    let mut top_signals = latest
        .signals
        .iter()
        .map(|(symbol, signal)| {
            let dims = dim_snapshot.dimensions.get(symbol);
            LiveSignal {
                symbol: symbol.0.clone(),
                sector: sector_name_for_symbol(store, symbol),
                composite: signal.composite,
                mark_price: signal.mark_price,
                dimension_composite: None,
                capital_flow_direction: signal.capital_flow_direction,
                price_momentum: dims
                    .map(|item| item.activity_momentum)
                    .unwrap_or(Decimal::ZERO),
                volume_profile: dims
                    .map(|item| item.candlestick_conviction)
                    .unwrap_or(Decimal::ZERO),
                pre_post_market_anomaly: Decimal::ZERO,
                valuation: dims
                    .map(|item| item.valuation_support)
                    .unwrap_or(Decimal::ZERO),
                cross_stock_correlation: Some(signal.cross_stock_correlation),
                sector_coherence: signal.sector_coherence,
                cross_market_propagation: None,
            }
        })
        .filter(|signal| signal.composite.abs() > Decimal::new(3, 2))
        .collect::<Vec<_>>();
    top_signals.sort_by(|a, b| b.composite.abs().cmp(&a.composite.abs()));
    top_signals.truncate(20);

    let tactical_cases = reasoning_snapshot
        .tactical_setups
        .iter()
        .filter(|item| item.action == "enter" || item.action == "review")
        .take(10)
        .map(|item| LiveTacticalCase {
            setup_id: item.setup_id.clone(),
            symbol: symbol_string_from_scope(&item.scope),
            title: item.title.clone(),
            action: item.action.clone(),
            confidence: item.confidence,
            confidence_gap: item.confidence_gap,
            heuristic_edge: item.heuristic_edge,
            entry_rationale: item.entry_rationale.clone(),
            family_label: hypothesis_map
                .get(item.hypothesis_id.as_str())
                .map(|hypothesis| hypothesis.family_label.clone()),
            counter_label: item
                .runner_up_hypothesis_id
                .as_ref()
                .and_then(|id| hypothesis_map.get(id.as_str()))
                .map(|hypothesis| hypothesis.family_label.clone()),
        })
        .collect::<Vec<_>>();

    let hypothesis_tracks = reasoning_snapshot
        .hypothesis_tracks
        .iter()
        .filter(|item| item.status.as_str() != "stable")
        .take(10)
        .map(|item| LiveHypothesisTrack {
            symbol: symbol_string_from_scope(&item.scope),
            title: item.title.clone(),
            status: item.status.as_str().to_string(),
            age_ticks: item.age_ticks,
            confidence: item.confidence,
        })
        .collect::<Vec<_>>();

    let pressures = graph_insights
        .pressures
        .iter()
        .take(10)
        .map(|item| LivePressure {
            symbol: item.symbol.0.clone(),
            sector: sector_name_for_symbol(store, &item.symbol),
            capital_flow_pressure: item.net_pressure,
            momentum: Decimal::ZERO,
            pressure_delta: item.pressure_delta,
            pressure_duration: item.pressure_duration,
            accelerating: item.accelerating,
        })
        .collect::<Vec<_>>();

    let events = event_snapshot
        .events
        .iter()
        .take(8)
        .map(|item| LiveEvent {
            kind: format!("{:?}", item.value.kind),
            magnitude: item.value.magnitude,
            summary: item.value.summary.clone(),
        })
        .collect::<Vec<_>>();

    LiveSnapshot {
        tick,
        timestamp,
        market: LiveMarket::Hk,
        stock_count: store.stocks.len(),
        edge_count: brain.graph.edge_count(),
        hypothesis_count: reasoning_snapshot.hypotheses.len(),
        observation_count: observation_snapshot.observations.len(),
        active_positions: tracker.active_count(),
        market_regime: LiveMarketRegime {
            bias: decision.market_regime.bias.as_str().to_string(),
            confidence: decision.market_regime.confidence,
            breadth_up: decision.market_regime.breadth_up,
            breadth_down: decision.market_regime.breadth_down,
            average_return: decision.market_regime.average_return,
            directional_consensus: Some(decision.market_regime.directional_consensus),
            pre_market_sentiment: None,
        },
        stress: LiveStressSnapshot {
            composite_stress: graph_insights.stress.composite_stress,
            sector_synchrony: Some(graph_insights.stress.sector_synchrony),
            pressure_consensus: Some(graph_insights.stress.pressure_consensus),
            momentum_consensus: None,
            pressure_dispersion: None,
            volume_anomaly: None,
        },
        scorecard: summarize_hk_scorecard(scorecard),
        tactical_cases,
        hypothesis_tracks,
        top_signals: top_signals.clone(),
        convergence_scores: top_signals,
        pressures,
        backward_chains: build_hk_backward_chains(&latest.backward_reasoning),
        causal_leaders: build_hk_causal_leaders(causal_timelines),
        events,
        cross_market_signals: Vec::new(),
        cross_market_anomalies: Vec::new(),
        lineage: build_hk_lineage_metrics(lineage_stats),
    }
}

#[cfg(feature = "persistence")]
async fn open_query_store() -> Result<EdenStore, Box<dyn std::error::Error>> {
    let eden_db_path = std::env::var("EDEN_DB_PATH").unwrap_or_else(|_| "data/eden.db".into());
    EdenStore::open(&eden_db_path).await
}

#[cfg(feature = "persistence")]
async fn run_cli_query(command: CliCommand) -> Result<(), Box<dyn std::error::Error>> {
    let store = open_query_store().await?;

    match command {
        CliCommand::Live => Ok(()),
        CliCommand::UsLive => Ok(()),
        CliCommand::Polymarket { json } => {
            let configs = load_polymarket_configs()
                .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?;
            if configs.is_empty() {
                println!("No Polymarket markets configured. Set POLYMARKET_MARKETS first.");
                return Ok(());
            }
            let snapshot = fetch_polymarket_snapshot(&configs)
                .await
                .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "configs": configs,
                        "snapshot": snapshot,
                    }))?
                );
            } else {
                print_polymarket_snapshot(&configs, &snapshot);
            }
            Ok(())
        }
        CliCommand::CausalTimeline {
            leaf_scope_key,
            limit,
        } => {
            let Some(timeline) = store.recent_causal_timeline(&leaf_scope_key, limit).await? else {
                println!("No causal timeline found for {}", leaf_scope_key);
                return Ok(());
            };
            print_causal_timeline(&timeline);
            Ok(())
        }
        CliCommand::CausalFlips { limit } => {
            let records = store.recent_tick_window(limit).await?;
            let mut history = TickHistory::new(records.len().max(1));
            for record in records {
                history.push(record);
            }
            let timelines = compute_causal_timelines(&history);
            print_causal_flips(timelines.values().collect());
            Ok(())
        }
        CliCommand::Lineage {
            limit,
            filters,
            view,
        } => {
            let stats = store.recent_lineage_stats(limit).await?;
            let stats = stats
                .filtered(&filters)
                .aligned(view.alignment)
                .sorted_by(view.sort_by)
                .truncated(view.top);
            if view.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "window_size": limit,
                        "filters": filters,
                        "top": view.top,
                        "sort_by": view.sort_by,
                        "alignment": view.alignment,
                        "stats": stats,
                    }))?
                );
            } else {
                print_lineage_report(&stats, limit, &filters, view.top);
            }
            Ok(())
        }
        CliCommand::LineageHistory {
            snapshots,
            filters,
            view,
        } => {
            let rows = store
                .recent_ranked_lineage_metric_rows(snapshots, view.top)
                .await?;
            let rows = select_lineage_rows(
                &rows,
                &filters,
                snapshots.saturating_mul(view.top.max(1)),
                view.latest_only,
                view.sort_by,
                view.alignment,
            );
            let records = snapshot_records_from_rows(&rows, &filters, view.latest_only);
            if view.json {
                println!("{}", serde_json::to_string_pretty(&records)?);
            } else {
                print_lineage_history(&records, &filters, view.top);
            }
            Ok(())
        }
        CliCommand::LineageRows {
            rows,
            filters,
            view,
        } => {
            let ranked_rows = store
                .recent_ranked_lineage_metric_rows(rows.max(1), view.top)
                .await?;
            let rows = select_lineage_rows(
                &ranked_rows,
                &filters,
                rows,
                view.latest_only,
                view.sort_by,
                view.alignment,
            );
            if view.json {
                println!("{}", serde_json::to_string_pretty(&rows)?);
            } else {
                print_lineage_rows(&rows);
            }
            Ok(())
        }
    }
}

#[cfg(not(feature = "persistence"))]
async fn run_cli_query(command: CliCommand) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        CliCommand::Live => Ok(()),
        CliCommand::UsLive => Ok(()),
        CliCommand::Polymarket { json } => {
            let configs = load_polymarket_configs()
                .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?;
            if configs.is_empty() {
                println!("No Polymarket markets configured. Set POLYMARKET_MARKETS first.");
                return Ok(());
            }
            let snapshot = fetch_polymarket_snapshot(&configs)
                .await
                .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "configs": configs,
                        "snapshot": snapshot,
                    }))?
                );
            } else {
                print_polymarket_snapshot(&configs, &snapshot);
            }
            Ok(())
        }
        CliCommand::CausalTimeline {
            leaf_scope_key,
            limit,
        } => {
            let _ = (leaf_scope_key, limit);
            Err("causal query commands require building with --features persistence".into())
        }
        CliCommand::CausalFlips { limit } => {
            let _ = limit;
            Err("causal query commands require building with --features persistence".into())
        }
        CliCommand::Lineage {
            limit,
            filters,
            view,
        } => {
            let _ = (limit, filters, view);
            Err("lineage query commands require building with --features persistence".into())
        }
        CliCommand::LineageHistory {
            snapshots,
            filters,
            view,
        } => {
            let _ = (snapshots, filters, view);
            Err("lineage query commands require building with --features persistence".into())
        }
        CliCommand::LineageRows {
            rows,
            filters,
            view,
        } => {
            let _ = (rows, filters, view);
            Err("lineage query commands require building with --features persistence".into())
        }
    }
}

#[cfg(feature = "persistence")]
fn print_causal_timeline(timeline: &CausalTimeline) {
    println!(
        "Causal Timeline  {}  scope={}  points={}  flips={}",
        timeline.leaf_label,
        timeline.leaf_scope_key,
        timeline.points.len(),
        timeline.flip_events.len(),
    );

    let sequence = timeline.recent_leader_sequence(8);
    if !sequence.is_empty() {
        println!("leader_sequence={}", sequence.join(" -> "));
    }
    if let Some(flip) = timeline.latest_flip() {
        println!(
            "latest_flip#{}  {} -> {}  style={}  gap={:+}",
            flip.tick_number,
            flip.from_explanation,
            flip.to_explanation,
            flip.style,
            flip.cause_gap.unwrap_or(Decimal::ZERO).round_dp(3),
        );
        println!("latest_flip_summary={}", flip.summary);
    }

    println!("\nRecent Points");
    for point in timeline.points.iter().rev().take(8).rev() {
        print_causal_timeline_point(point);
    }
}

#[cfg(feature = "persistence")]
fn print_causal_timeline_point(point: &CausalTimelinePoint) {
    println!(
        "  tick#{}  state={}  lead={}  gap={:+}  d_support={:+}  d_against={:+}",
        point.tick_number,
        point.contest_state,
        point.leading_explanation.as_deref().unwrap_or("none"),
        point.cause_gap.unwrap_or(Decimal::ZERO).round_dp(3),
        point
            .leading_support_delta
            .unwrap_or(Decimal::ZERO)
            .round_dp(3),
        point
            .leading_contradict_delta
            .unwrap_or(Decimal::ZERO)
            .round_dp(3),
    );
    if let Some(summary) = &point.leader_transition_summary {
        println!("          {}", summary);
    }
}

#[cfg(feature = "persistence")]
fn print_causal_flips(timelines: Vec<&CausalTimeline>) {
    let mut flips = timelines
        .into_iter()
        .flat_map(|timeline| {
            timeline.flip_events.iter().map(move |flip| {
                (
                    timeline.leaf_label.as_str(),
                    timeline.leaf_scope_key.as_str(),
                    flip,
                )
            })
        })
        .collect::<Vec<_>>();
    flips.sort_by(|a, b| b.2.tick_number.cmp(&a.2.tick_number));

    let sudden = flips
        .iter()
        .filter(|(_, _, flip)| {
            matches!(
                flip.style,
                eden::temporal::causality::CausalFlipStyle::Sudden
            )
        })
        .count();
    let erosion = flips.len().saturating_sub(sudden);

    println!(
        "Causal Flips  total={}  sudden={}  erosion_driven={}",
        flips.len(),
        sudden,
        erosion,
    );
    for (leaf_label, leaf_scope_key, flip) in flips.iter().take(20) {
        print_causal_flip_event(leaf_label, leaf_scope_key, flip);
    }
}

#[cfg(feature = "persistence")]
fn print_causal_flip_event(leaf_label: &str, leaf_scope_key: &str, flip: &CausalFlipEvent) {
    println!(
        "  {}  scope={}  tick#{}  {} -> {}  style={}  gap={:+}",
        leaf_label,
        leaf_scope_key,
        flip.tick_number,
        flip.from_explanation,
        flip.to_explanation,
        flip.style,
        flip.cause_gap.unwrap_or(Decimal::ZERO).round_dp(3),
    );
    println!("          {}", flip.summary);
}

fn print_polymarket_snapshot(configs: &[PolymarketMarketConfig], snapshot: &PolymarketSnapshot) {
    let pct = Decimal::new(100, 0);
    println!(
        "Polymarket  configured={}  fetched={}  priors={}",
        configs.len(),
        snapshot.fetched_at,
        snapshot.priors.len(),
    );

    for config in configs {
        println!(
            "  config  slug={}  scope={:?}  bias={}  threshold={:.0}%  targets=[{}]",
            config.slug,
            config.scope(),
            config.bias.as_str(),
            (config.conviction_threshold * pct).round_dp(0),
            if config.target_scopes.is_empty() {
                "*".into()
            } else {
                config.target_scopes.join(", ")
            },
        );
    }

    for prior in &snapshot.priors {
        println!(
            "  prior   {}  outcome={}  prob={:.0}%  scope={:?}  bias={}  active={}  closed={}  material={}  targets=[{}]",
            prior.label,
            prior.selected_outcome,
            (prior.probability * pct).round_dp(0),
            prior.scope,
            prior.bias.as_str(),
            prior.active,
            prior.closed,
            prior.is_material(),
            if prior.target_scopes.is_empty() {
                "*".into()
            } else {
                prior.target_scopes.join(", ")
            },
        );
    }
}

#[cfg(feature = "persistence")]
fn print_lineage_report(
    stats: &eden::temporal::lineage::LineageStats,
    limit: usize,
    filters: &LineageFilters,
    top: usize,
) {
    let pct = Decimal::new(100, 0);
    println!("Lineage Evaluation  window={} ticks", limit);
    if !filters.is_empty() {
        println!(
            "filters  label={}  bucket={}  family={}  session={}  regime={}",
            filters.label.as_deref().unwrap_or("*"),
            filters.bucket.as_deref().unwrap_or("*"),
            filters.family.as_deref().unwrap_or("*"),
            filters.session.as_deref().unwrap_or("*"),
            filters.market_regime.as_deref().unwrap_or("*"),
        );
    }
    println!("top={}", top);

    if !stats.based_on.is_empty()
        || !stats.blocked_by.is_empty()
        || !stats.promoted_by.is_empty()
        || !stats.falsified_by.is_empty()
    {
        println!("\nTop Labels");
        for (label, count) in stats.based_on.iter().take(5) {
            println!("  based_on      x{:<3} {}", count, label);
        }
        for (label, count) in stats.promoted_by.iter().take(5) {
            println!("  promoted_by   x{:<3} {}", count, label);
        }
        for (label, count) in stats.blocked_by.iter().take(5) {
            println!("  blocked_by    x{:<3} {}", count, label);
        }
        for (label, count) in stats.falsified_by.iter().take(5) {
            println!("  falsified_by  x{:<3} {}", count, label);
        }
    }

    print_lineage_outcome_group("Promoted Outcomes", &stats.promoted_outcomes, pct);
    print_lineage_outcome_group("Blocked Outcomes", &stats.blocked_outcomes, pct);
    print_lineage_outcome_group("Falsified Outcomes", &stats.falsified_outcomes, pct);
    print_lineage_context_group("Promoted Contexts", &stats.promoted_contexts, pct);
    print_lineage_context_group("Blocked Contexts", &stats.blocked_contexts, pct);
    print_lineage_context_group("Falsified Contexts", &stats.falsified_contexts, pct);
}

#[cfg(feature = "persistence")]
fn print_lineage_history(records: &[LineageSnapshotRecord], filters: &LineageFilters, top: usize) {
    if records.is_empty() {
        println!("No lineage snapshots found.");
        return;
    }

    for record in records {
        println!(
            "\n=== Lineage Snapshot  tick#{}  at={}  window={} ===",
            record.tick_number, record.recorded_at, record.window_size
        );
        print_lineage_report(&record.stats, record.window_size, filters, top);
    }
}

#[cfg(feature = "persistence")]
fn select_lineage_rows(
    rows: &[eden::persistence::lineage_metric_row::LineageMetricRowRecord],
    filters: &LineageFilters,
    limit: usize,
    latest_only: bool,
    sort_by: LineageSortKey,
    alignment: LineageAlignmentFilter,
) -> Vec<eden::persistence::lineage_metric_row::LineageMetricRowRecord> {
    let mut filtered_rows = rows
        .iter()
        .cloned()
        .filter(|row| {
            row_matches_filters(row, filters)
                && matches_lineage_alignment(
                    row.mean_external_delta
                        .parse::<Decimal>()
                        .unwrap_or(Decimal::ZERO),
                    alignment,
                )
        })
        .collect::<Vec<_>>();

    filtered_rows.sort_by(|a, b| {
        lineage_row_metric(b, sort_by)
            .cmp(&lineage_row_metric(a, sort_by))
            .then_with(|| a.rank.cmp(&b.rank))
            .then_with(|| a.label.cmp(&b.label))
    });

    if latest_only {
        if let Some(snapshot_id) = filtered_rows.first().map(|row| row.snapshot_id.clone()) {
            filtered_rows.retain(|row| row.snapshot_id == snapshot_id);
        }
    }

    filtered_rows.truncate(limit);
    filtered_rows
}

#[cfg(feature = "persistence")]
fn lineage_row_metric(
    row: &eden::persistence::lineage_metric_row::LineageMetricRowRecord,
    sort_by: LineageSortKey,
) -> Decimal {
    match sort_by {
        LineageSortKey::NetReturn => row.mean_net_return.parse().unwrap_or(Decimal::ZERO),
        LineageSortKey::ConvergenceScore => {
            row.mean_convergence_score.parse().unwrap_or(Decimal::ZERO)
        }
        LineageSortKey::ExternalDelta => row.mean_external_delta.parse().unwrap_or(Decimal::ZERO),
    }
}

#[cfg(feature = "persistence")]
fn matches_lineage_alignment(value: Decimal, alignment: LineageAlignmentFilter) -> bool {
    match alignment {
        LineageAlignmentFilter::All => true,
        LineageAlignmentFilter::Confirm => value > Decimal::ZERO,
        LineageAlignmentFilter::Contradict => value < Decimal::ZERO,
    }
}

#[cfg(feature = "persistence")]
fn print_lineage_rows(rows: &[eden::persistence::lineage_metric_row::LineageMetricRowRecord]) {
    if rows.is_empty() {
        println!("No lineage rows matched the provided filters.");
        return;
    }

    let pct = Decimal::new(100, 0);
    for row in rows {
        println!(
            "  tick#{}  bucket={}  rank={}  label={}  family={}  session={}  regime={}  resolved={}  conv={:.0}%  net={:+.2}%  mfe={:+.2}%  mae={:+.2}%  follow={:.0}%  retain={:.0}%  invalid={:.0}%  ext_delta={:+.2}%  ext_follow={:.0}%",
            row.tick_number,
            row.bucket,
            row.rank + 1,
            row.label,
            row.family.as_deref().unwrap_or("-"),
            row.session.as_deref().unwrap_or("-"),
            row.market_regime.as_deref().unwrap_or("-"),
            row.resolved,
            (row.mean_convergence_score.parse::<Decimal>().unwrap_or(Decimal::ZERO) * pct).round_dp(0),
            (row.mean_net_return.parse::<Decimal>().unwrap_or(Decimal::ZERO) * pct).round_dp(2),
            (row.mean_mfe.parse::<Decimal>().unwrap_or(Decimal::ZERO) * pct).round_dp(2),
            (row.mean_mae.parse::<Decimal>().unwrap_or(Decimal::ZERO) * pct).round_dp(2),
            (row.follow_through_rate.parse::<Decimal>().unwrap_or(Decimal::ZERO) * pct).round_dp(0),
            (row.structure_retention_rate.parse::<Decimal>().unwrap_or(Decimal::ZERO) * pct)
                .round_dp(0),
            (row.invalidation_rate.parse::<Decimal>().unwrap_or(Decimal::ZERO) * pct).round_dp(0),
            (row.mean_external_delta.parse::<Decimal>().unwrap_or(Decimal::ZERO) * pct)
                .round_dp(2),
            (row
                .external_follow_through_rate
                .parse::<Decimal>()
                .unwrap_or(Decimal::ZERO)
                * pct)
                .round_dp(0),
        );
    }
}

#[cfg(feature = "persistence")]
fn print_lineage_outcome_group(
    title: &str,
    items: &[eden::temporal::lineage::LineageOutcome],
    pct: Decimal,
) {
    if items.is_empty() {
        return;
    }
    println!("\n{}", title);
    for item in items.iter().take(5) {
        println!(
            "  {}  resolved={}  hit={:.0}%  conv={:.0}%  gross={:+.2}%  net={:+.2}%  mfe={:+.2}%  mae={:+.2}%  follow={:.0}%  retain={:.0}%  invalid={:.0}%  ext_delta={:+.2}%  ext_follow={:.0}%",
            item.label,
            item.resolved,
            (item.hit_rate * pct).round_dp(0),
            (item.mean_convergence_score * pct).round_dp(0),
            (item.mean_return * pct).round_dp(2),
            (item.mean_net_return * pct).round_dp(2),
            (item.mean_mfe * pct).round_dp(2),
            (item.mean_mae * pct).round_dp(2),
            (item.follow_through_rate * pct).round_dp(0),
            (item.structure_retention_rate * pct).round_dp(0),
            (item.invalidation_rate * pct).round_dp(0),
            (item.mean_external_delta * pct).round_dp(2),
            (item.external_follow_through_rate * pct).round_dp(0),
        );
    }
}

#[cfg(feature = "persistence")]
fn print_lineage_context_group(
    title: &str,
    items: &[eden::temporal::lineage::ContextualLineageOutcome],
    pct: Decimal,
) {
    if items.is_empty() {
        return;
    }
    println!("\n{}", title);
    for item in items.iter().take(5) {
        println!(
            "  {}  family={}  session={}  regime={}  resolved={}  conv={:.0}%  net={:+.2}%  mfe={:+.2}%  mae={:+.2}%  follow={:.0}%  retain={:.0}%  invalid={:.0}%  ext_delta={:+.2}%  ext_follow={:.0}%",
            item.label,
            item.family,
            item.session,
            item.market_regime,
            item.resolved,
            (item.mean_convergence_score * pct).round_dp(0),
            (item.mean_net_return * pct).round_dp(2),
            (item.mean_mfe * pct).round_dp(2),
            (item.mean_mae * pct).round_dp(2),
            (item.follow_through_rate * pct).round_dp(0),
            (item.structure_retention_rate * pct).round_dp(0),
            (item.invalidation_rate * pct).round_dp(0),
            (item.mean_external_delta * pct).round_dp(2),
            (item.external_follow_through_rate * pct).round_dp(0),
        );
    }
}

/// Debounce window: after receiving a push event, wait this long for more
/// before running the pipeline. Batches rapid-fire events without adding latency.
const DEBOUNCE_MS: u64 = 2000;
const LINEAGE_WINDOW: usize = 50;
const TRADE_BUFFER_CAP_PER_SYMBOL: usize = 2_000;
const POLYMARKET_WARNING_INTERVAL: std::time::Duration = std::time::Duration::from_secs(300);

/// Live market state accumulated from WebSocket push events.
struct LiveState {
    depths: HashMap<Symbol, SecurityDepth>,
    brokers: HashMap<Symbol, SecurityBrokers>,
    quotes: HashMap<Symbol, SecurityQuote>,
    trades: HashMap<Symbol, Vec<Trade>>,
    candlesticks: HashMap<Symbol, Vec<longport::quote::Candlestick>>,
    push_count: u64,
    dirty: bool, // true if new pushes since last pipeline run
}

impl LiveState {
    fn new() -> Self {
        Self {
            depths: HashMap::new(),
            brokers: HashMap::new(),
            quotes: HashMap::new(),
            trades: HashMap::new(),
            candlesticks: HashMap::new(),
            push_count: 0,
            dirty: false,
        }
    }

    fn apply(&mut self, event: PushEvent) {
        let symbol = Symbol(event.symbol);
        self.push_count += 1;
        self.dirty = true;
        match event.detail {
            PushEventDetail::Depth(depth) => {
                self.depths.insert(
                    symbol,
                    SecurityDepth {
                        asks: depth.asks,
                        bids: depth.bids,
                    },
                );
            }
            PushEventDetail::Brokers(brokers) => {
                self.brokers.insert(
                    symbol,
                    SecurityBrokers {
                        ask_brokers: brokers.ask_brokers,
                        bid_brokers: brokers.bid_brokers,
                    },
                );
            }
            PushEventDetail::Quote(quote) => {
                let existing = self.quotes.get(&symbol);
                self.quotes.insert(
                    symbol.clone(),
                    SecurityQuote {
                        symbol: symbol.0,
                        last_done: quote.last_done,
                        prev_close: existing.map(|q| q.prev_close).unwrap_or(Decimal::ZERO),
                        open: quote.open,
                        high: quote.high,
                        low: quote.low,
                        timestamp: quote.timestamp,
                        volume: quote.volume,
                        turnover: quote.turnover,
                        trade_status: quote.trade_status,
                        pre_market_quote: None,
                        post_market_quote: None,
                        overnight_quote: None,
                    },
                );
            }
            PushEventDetail::Trade(push_trades) => {
                let entry = self.trades.entry(symbol).or_default();
                append_trades_with_cap(entry, push_trades.trades);
            }
            PushEventDetail::Candlestick(candle) => {
                let entry = self.candlesticks.entry(symbol).or_default();
                entry.push(candle.candlestick);
                // Keep last 60 candles (1 hour of 1-min data)
                if entry.len() > 60 {
                    entry.drain(..entry.len() - 60);
                }
            }
        }
    }

    /// Merge live push state with REST-fetched capital data into a RawSnapshot.
    /// Consumes accumulated trades (they're per-tick, not cumulative).
    fn to_raw_snapshot(&mut self, rest: &RestSnapshot) -> RawSnapshot {
        let trades = std::mem::take(&mut self.trades);
        self.dirty = false;
        RawSnapshot {
            timestamp: time::OffsetDateTime::now_utc(),
            brokers: self.brokers.clone(),
            calc_indexes: rest.calc_indexes.clone(),
            candlesticks: self.candlesticks.clone(),
            depths: self.depths.clone(),
            market_temperature: rest.market_temperature.clone(),
            quotes: self.quotes.clone(),
            trades,
            capital_flows: rest.capital_flows.clone(),
            capital_distributions: rest.capital_distributions.clone(),
        }
    }
}

fn append_trades_with_cap(buffer: &mut Vec<Trade>, mut trades: Vec<Trade>) {
    buffer.append(&mut trades);
    if buffer.len() > TRADE_BUFFER_CAP_PER_SYMBOL {
        buffer.drain(..buffer.len() - TRADE_BUFFER_CAP_PER_SYMBOL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eden::action::narrative::Regime;
    use eden::ontology::domain::{Event, ProvenanceMetadata, ProvenanceSource};
    use eden::ontology::reasoning::{HypothesisTrack, HypothesisTrackStatus, ReasoningScope};
    use eden::pipeline::dimensions::SymbolDimensions;
    use eden::pipeline::signals::{MarketEventRecord, SignalScope};
    use rust_decimal_macros::dec;

    fn sym(value: &str) -> Symbol {
        Symbol(value.into())
    }

    fn event_snapshot(scope: SignalScope, kind: MarketEventKind, summary: &str) -> EventSnapshot {
        EventSnapshot {
            timestamp: time::OffsetDateTime::UNIX_EPOCH,
            events: vec![Event::new(
                MarketEventRecord {
                    scope,
                    kind,
                    magnitude: dec!(0.8),
                    summary: summary.into(),
                },
                ProvenanceMetadata::new(
                    ProvenanceSource::Computed,
                    time::OffsetDateTime::UNIX_EPOCH,
                ),
            )],
        }
    }

    #[test]
    fn temporal_event_blocks_auto_confirmation() {
        let suggestion = eden::graph::decision::OrderSuggestion {
            symbol: sym("700.HK"),
            direction: OrderDirection::Buy,
            convergence: eden::graph::decision::ConvergenceScore {
                symbol: sym("700.HK"),
                institutional_alignment: dec!(0.5),
                sector_coherence: Some(dec!(0.4)),
                cross_stock_correlation: dec!(0.3),
                composite: dec!(0.6),
            },
            suggested_quantity: 100,
            price_low: Some(dec!(350)),
            price_high: Some(dec!(351)),
            estimated_cost: dec!(0.002),
            heuristic_edge: dec!(0.598),
            requires_confirmation: false,
            convergence_score: dec!(0.6),
            effective_confidence: dec!(0.6),
            external_confirmation: None,
            external_conflict: None,
            external_support_slug: None,
            external_support_probability: None,
            external_conflict_slug: None,
            external_conflict_probability: None,
        };

        let (snapshots, _, _) = build_action_workflows(
            time::OffsetDateTime::UNIX_EPOCH,
            &[suggestion],
            &[],
            &HashMap::new(),
            &event_snapshot(
                SignalScope::Symbol(sym("700.HK")),
                MarketEventKind::InstitutionalFlip,
                "institutional alignment flipped",
            ),
            &[],
            &[],
        );

        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].stage.as_str(), "suggest");
    }

    #[test]
    fn live_state_caps_trade_buffer_per_symbol() {
        let mut buffered = Vec::new();
        for _ in 0..(TRADE_BUFFER_CAP_PER_SYMBOL + 10) {
            append_trades_with_cap(
                &mut buffered,
                vec![Trade {
                    price: dec!(10),
                    volume: 1,
                    timestamp: time::OffsetDateTime::UNIX_EPOCH,
                    trade_type: String::new(),
                    direction: longport::quote::TradeDirection::Neutral,
                    trade_session: longport::quote::TradeSession::Intraday,
                }],
            );
        }
        assert_eq!(buffered.len(), TRADE_BUFFER_CAP_PER_SYMBOL);
    }

    #[test]
    fn active_position_reviews_on_market_stress_shift() {
        let fingerprint = StructuralFingerprint {
            symbol: sym("700.HK"),
            entry_timestamp: time::OffsetDateTime::UNIX_EPOCH,
            entry_composite: dec!(0.6),
            entry_regime: Regime::CoherentBullish,
            institutional_directions: vec![],
            sector_mean_coherence: Some(dec!(0.4)),
            correlated_stocks: vec![],
            entry_dimensions: SymbolDimensions::default(),
        };

        let (snapshots, _, _) = build_action_workflows(
            time::OffsetDateTime::UNIX_EPOCH,
            &[],
            &[fingerprint],
            &HashMap::new(),
            &event_snapshot(
                SignalScope::Market,
                MarketEventKind::StressRegimeShift,
                "market stress shifted sharply",
            ),
            &[],
            &[],
        );

        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].stage.as_str(), "review");
    }

    #[test]
    fn track_review_reason_overrides_position_monitoring() {
        let fingerprint = StructuralFingerprint {
            symbol: sym("700.HK"),
            entry_timestamp: time::OffsetDateTime::UNIX_EPOCH,
            entry_composite: dec!(0.6),
            entry_regime: Regime::CoherentBullish,
            institutional_directions: vec![],
            sector_mean_coherence: Some(dec!(0.4)),
            correlated_stocks: vec![],
            entry_dimensions: SymbolDimensions::default(),
        };
        let track = HypothesisTrack {
            track_id: "track:700.HK".into(),
            setup_id: "setup:700.HK:review".into(),
            hypothesis_id: "hyp:700.HK:flow".into(),
            runner_up_hypothesis_id: Some("hyp:700.HK:risk".into()),
            scope: ReasoningScope::Symbol(sym("700.HK")),
            title: "Long 700.HK".into(),
            action: "review".into(),
            status: HypothesisTrackStatus::Weakening,
            age_ticks: 3,
            status_streak: 2,
            confidence: dec!(0.55),
            previous_confidence: Some(dec!(0.64)),
            confidence_change: dec!(-0.09),
            confidence_gap: dec!(0.07),
            previous_confidence_gap: Some(dec!(0.14)),
            confidence_gap_change: dec!(-0.07),
            heuristic_edge: dec!(0.03),
            policy_reason: "confidence or gap weakened materially".into(),
            transition_reason: Some(
                "downgraded from enter to review because confidence or gap weakened materially"
                    .into(),
            ),
            first_seen_at: time::OffsetDateTime::UNIX_EPOCH,
            last_updated_at: time::OffsetDateTime::UNIX_EPOCH,
            invalidated_at: None,
        };

        let (snapshots, _, _) = build_action_workflows(
            time::OffsetDateTime::UNIX_EPOCH,
            &[],
            &[fingerprint],
            &HashMap::new(),
            &EventSnapshot {
                timestamp: time::OffsetDateTime::UNIX_EPOCH,
                events: vec![],
            },
            &[track],
            &[],
        );

        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].stage.as_str(), "review");
        assert!(snapshots[0]
            .note
            .as_deref()
            .unwrap_or_default()
            .contains("downgraded from enter to review"));
    }

    #[test]
    fn parse_lineage_cli_command_with_default_limit() {
        let args = vec!["eden".to_string(), "lineage".to_string()];
        let command = parse_cli_command(&args).expect("lineage command parses");
        match command {
            CliCommand::Lineage {
                limit,
                filters,
                view,
            } => {
                assert_eq!(limit, 120);
                assert!(filters.is_empty());
                assert_eq!(view.top, 5);
                assert!(!view.json);
            }
            _ => panic!("expected lineage command"),
        }
    }

    #[test]
    fn parse_lineage_cli_command_with_explicit_limit() {
        let args = vec!["eden".to_string(), "lineage".to_string(), "42".to_string()];
        let command = parse_cli_command(&args).expect("lineage command parses");
        match command {
            CliCommand::Lineage {
                limit,
                filters,
                view,
            } => {
                assert_eq!(limit, 42);
                assert!(filters.is_empty());
                assert_eq!(view.top, 5);
            }
            _ => panic!("expected lineage command"),
        }
    }

    #[test]
    fn parse_lineage_cli_command_with_filters() {
        let args = vec![
            "eden".to_string(),
            "lineage".to_string(),
            "60".to_string(),
            "--label".to_string(),
            "review -> enter".to_string(),
            "--family".to_string(),
            "Directed Flow".to_string(),
            "--session".to_string(),
            "opening".to_string(),
            "--regime".to_string(),
            "risk_on".to_string(),
            "--top".to_string(),
            "8".to_string(),
            "--sort".to_string(),
            "conv".to_string(),
            "--alignment".to_string(),
            "confirm".to_string(),
            "--json".to_string(),
        ];
        let command = parse_cli_command(&args).expect("lineage command parses");
        match command {
            CliCommand::Lineage {
                limit,
                filters,
                view,
            } => {
                assert_eq!(limit, 60);
                assert_eq!(filters.label.as_deref(), Some("review -> enter"));
                assert_eq!(filters.bucket.as_deref(), None);
                assert_eq!(filters.family.as_deref(), Some("Directed Flow"));
                assert_eq!(filters.session.as_deref(), Some("opening"));
                assert_eq!(filters.market_regime.as_deref(), Some("risk_on"));
                assert_eq!(view.top, 8);
                assert_eq!(view.sort_by, LineageSortKey::ConvergenceScore);
                assert_eq!(view.alignment, LineageAlignmentFilter::Confirm);
                assert!(view.json);
            }
            _ => panic!("expected lineage command"),
        }
    }

    #[test]
    fn parse_lineage_history_cli_command() {
        let args = vec![
            "eden".to_string(),
            "lineage".to_string(),
            "history".to_string(),
            "12".to_string(),
            "--label".to_string(),
            "review -> enter".to_string(),
            "--bucket".to_string(),
            "promoted_contexts".to_string(),
            "--latest-only".to_string(),
            "--top".to_string(),
            "3".to_string(),
            "--sort".to_string(),
            "external".to_string(),
            "--alignment".to_string(),
            "contradict".to_string(),
        ];
        let command = parse_cli_command(&args).expect("lineage history parses");
        match command {
            CliCommand::LineageHistory {
                snapshots,
                filters,
                view,
            } => {
                assert_eq!(snapshots, 12);
                assert_eq!(filters.label.as_deref(), Some("review -> enter"));
                assert_eq!(filters.bucket.as_deref(), Some("promoted_contexts"));
                assert!(view.latest_only);
                assert_eq!(view.top, 3);
                assert_eq!(view.sort_by, LineageSortKey::ExternalDelta);
                assert_eq!(view.alignment, LineageAlignmentFilter::Contradict);
            }
            _ => panic!("expected lineage command"),
        }
    }

    #[test]
    fn parse_lineage_rejects_latest_only_outside_history() {
        let args = vec![
            "eden".to_string(),
            "lineage".to_string(),
            "--latest-only".to_string(),
        ];
        let error = parse_cli_command(&args).expect_err("latest-only should fail outside history");
        assert!(error.contains("--latest-only"));
    }

    #[test]
    fn parse_lineage_rows_cli_command() {
        let args = vec![
            "eden".to_string(),
            "lineage".to_string(),
            "rows".to_string(),
            "25".to_string(),
            "--bucket".to_string(),
            "promoted_contexts".to_string(),
            "--latest-only".to_string(),
            "--sort".to_string(),
            "net".to_string(),
            "--json".to_string(),
        ];
        let command = parse_cli_command(&args).expect("lineage rows parses");
        match command {
            CliCommand::LineageRows {
                rows,
                filters,
                view,
            } => {
                assert_eq!(rows, 25);
                assert_eq!(filters.bucket.as_deref(), Some("promoted_contexts"));
                assert!(view.latest_only);
                assert_eq!(view.sort_by, LineageSortKey::NetReturn);
                assert!(view.json);
            }
            _ => panic!("expected lineage rows command"),
        }
    }

    #[test]
    fn parse_polymarket_cli_command() {
        let args = vec![
            "eden".to_string(),
            "polymarket".to_string(),
            "--json".to_string(),
        ];
        let command = parse_cli_command(&args).expect("polymarket parses");
        match command {
            CliCommand::Polymarket { json } => assert!(json),
            _ => panic!("expected polymarket command"),
        }
    }

    #[test]
    fn parse_us_cli_command() {
        let args = vec!["eden".to_string(), "us".to_string()];
        let command = parse_cli_command(&args).expect("us command parses");
        match command {
            CliCommand::UsLive => {}
            _ => panic!("expected us command"),
        }
    }
}

/// REST-only data that doesn't come via push.
struct RestSnapshot {
    calc_indexes: HashMap<Symbol, SecurityCalcIndex>,
    capital_flows: HashMap<Symbol, Vec<longport::quote::CapitalFlowLine>>,
    capital_distributions: HashMap<Symbol, longport::quote::CapitalDistributionResponse>,
    market_temperature: Option<MarketTemperature>,
    polymarket: PolymarketSnapshot,
}

impl RestSnapshot {
    fn empty() -> Self {
        Self {
            calc_indexes: HashMap::new(),
            capital_flows: HashMap::new(),
            capital_distributions: HashMap::new(),
            market_temperature: None,
            polymarket: PolymarketSnapshot::default(),
        }
    }
}

struct HkTickState<'a> {
    live: &'a mut LiveState,
    rest: &'a mut RestSnapshot,
    rest_updated: &'a mut bool,
}

impl TickState<PushEvent, RestSnapshot> for HkTickState<'_> {
    fn apply_push(&mut self, event: PushEvent) {
        self.live.apply(event);
    }

    fn apply_update(&mut self, update: RestSnapshot) {
        *self.rest = update;
        *self.rest_updated = true;
        self.live.dirty = true;
    }

    fn is_dirty(&self) -> bool {
        self.live.dirty
    }

    fn clear_dirty(&mut self) {
        self.live.dirty = false;
    }
}

async fn fetch_market_context(
    ctx: &QuoteContext,
    watchlist: &[Symbol],
) -> (
    HashMap<Symbol, SecurityCalcIndex>,
    Option<MarketTemperature>,
) {
    let calc_indexes = match ctx
        .calc_indexes(
            watchlist.iter().map(|s| s.0.clone()).collect::<Vec<_>>(),
            [
                CalcIndex::TurnoverRate,
                CalcIndex::VolumeRatio,
                CalcIndex::PeTtmRatio,
                CalcIndex::PbRatio,
                CalcIndex::Amplitude,
                CalcIndex::FiveMinutesChangeRate,
                CalcIndex::DividendRatioTtm,
            ],
        )
        .await
    {
        Ok(indexes) => indexes
            .into_iter()
            .map(|idx| (Symbol(idx.symbol.clone()), idx))
            .collect(),
        Err(e) => {
            eprintln!("Warning: calc_indexes failed: {}", e);
            HashMap::new()
        }
    };

    let market_temperature = match ctx.market_temperature(Market::HK).await {
        Ok(temp) => Some(temp),
        Err(e) => {
            eprintln!("Warning: market_temperature failed: {}", e);
            None
        }
    };

    (calc_indexes, market_temperature)
}

/// Fetch REST-only data that doesn't come via push.
/// Batches requests to stay under Longport's 10 req/s rate limit.
async fn fetch_rest_data(
    ctx: &QuoteContext,
    watchlist: &[Symbol],
    polymarket_configs: &[PolymarketMarketConfig],
) -> RestSnapshot {
    use futures::stream::{self, StreamExt};

    const BATCH_CONCURRENCY: usize = 8; // max concurrent requests per stream

    let flow_future = stream::iter(watchlist.iter().cloned())
        .map(|sym| {
            let ctx = ctx.clone();
            async move {
                match ctx.capital_flow(sym.0.clone()).await {
                    Ok(f) => Some((sym, f)),
                    Err(e) => {
                        eprintln!("Warning: capital_flow({}) failed: {}", sym, e);
                        None
                    }
                }
            }
        })
        .buffer_unordered(BATCH_CONCURRENCY)
        .collect::<Vec<_>>();

    let dist_future = stream::iter(watchlist.iter().cloned())
        .map(|sym| {
            let ctx = ctx.clone();
            async move {
                match ctx.capital_distribution(sym.0.clone()).await {
                    Ok(d) => Some((sym, d)),
                    Err(e) => {
                        eprintln!("Warning: capital_distribution({}) failed: {}", sym, e);
                        None
                    }
                }
            }
        })
        .buffer_unordered(BATCH_CONCURRENCY)
        .collect::<Vec<_>>();

    let market_context_future = fetch_market_context(ctx, watchlist);
    let polymarket_future = fetch_polymarket_snapshot(polymarket_configs);

    let (flow_results, dist_results, (calc_indexes, market_temperature), polymarket_snapshot) = tokio::join!(
        flow_future,
        dist_future,
        market_context_future,
        polymarket_future
    );

    RestSnapshot {
        calc_indexes,
        capital_flows: flow_results.into_iter().flatten().collect(),
        capital_distributions: dist_results.into_iter().flatten().collect(),
        market_temperature,
        polymarket: polymarket_snapshot.unwrap_or_else(|error| {
            rate_limited_polymarket_warning(&format!(
                "Warning: Polymarket refresh failed: {}",
                error
            ));
            PolymarketSnapshot::default()
        }),
    }
}

fn rate_limited_polymarket_warning(message: &str) {
    static LAST_WARNING_AT: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();
    let mutex = LAST_WARNING_AT.get_or_init(|| Mutex::new(None));
    let Ok(mut guard) = mutex.lock() else {
        eprintln!("{}", message);
        return;
    };
    let should_log = guard
        .map(|instant| instant.elapsed() >= POLYMARKET_WARNING_INTERVAL)
        .unwrap_or(true);
    if should_log {
        eprintln!("{}", message);
        *guard = Some(Instant::now());
    }
}

fn order_direction_label(direction: OrderDirection) -> &'static str {
    match direction {
        OrderDirection::Buy => "buy",
        OrderDirection::Sell => "sell",
    }
}

fn build_action_workflows(
    timestamp: time::OffsetDateTime,
    suggestions: &[eden::graph::decision::OrderSuggestion],
    active_fps: &[StructuralFingerprint],
    degradations: &HashMap<Symbol, eden::graph::decision::StructuralDegradation>,
    event_snapshot: &EventSnapshot,
    tracks: &[HypothesisTrack],
    setups: &[TacticalSetup],
) -> (
    Vec<ActionWorkflowSnapshot>,
    Vec<ActionWorkflowRecord>,
    Vec<ActionWorkflowEventRecord>,
) {
    let mut snapshots = Vec::new();
    let mut records = Vec::new();
    let mut events = Vec::new();

    for suggestion in suggestions {
        let track = symbol_track(&suggestion.symbol, tracks);
        let setup = symbol_setup(&suggestion.symbol, setups);
        let gate_reason =
            suggestion_gate_reason(&suggestion.symbol, suggestion, event_snapshot, track);
        let descriptor = ActionDescriptor::new(
            format!(
                "order:{}:{}",
                suggestion.symbol,
                order_direction_label(suggestion.direction)
            ),
            format!(
                "{} {}",
                order_direction_label(suggestion.direction).to_uppercase(),
                suggestion.symbol
            ),
            serde_json::json!({
                "symbol": suggestion.symbol,
                "direction": order_direction_label(suggestion.direction),
                "suggested_quantity": suggestion.suggested_quantity,
                "price_low": suggestion.price_low,
                "price_high": suggestion.price_high,
                "composite": suggestion.convergence.composite,
                "requires_confirmation": suggestion.requires_confirmation,
                "track_status": track.map(|track| track.status.as_str()),
                "track_streak": track.map(|track| track.status_streak),
                "decision_lineage": setup.map(|setup| serde_json::to_value(&setup.lineage).unwrap_or(serde_json::Value::Null)),
            }),
        );
        let suggested = SuggestedAction::new(
            descriptor,
            timestamp,
            Some("eden".into()),
            Some(gate_reason.clone().unwrap_or_else(|| {
                setup
                    .and_then(workflow_lineage_summary)
                    .unwrap_or_else(|| "generated from convergence pipeline".into())
            })),
        );
        let suggested_snapshot = ActionWorkflowSnapshot::from_state(&suggested);
        events.push(ActionWorkflowEventRecord::from_snapshot(
            &suggested_snapshot,
        ));

        if gate_reason.is_some() {
            snapshots.push(suggested_snapshot);
            records.push(ActionWorkflowRecord::from_state(&suggested));
        } else {
            let confirmed = suggested.clone().confirm(
                timestamp,
                Some("eden-auto".into()),
                Some(
                    setup
                        .and_then(workflow_lineage_summary)
                        .or_else(|| track.and_then(|track| track.transition_reason.clone()))
                        .unwrap_or_else(|| "auto-confirmed by structural consensus".into()),
                ),
            );
            events.push(ActionWorkflowEventRecord::from_transition(
                &suggested, &confirmed,
            ));
            snapshots.push(ActionWorkflowSnapshot::from_state(&confirmed));
            records.push(ActionWorkflowRecord::from_state(&confirmed));
        }
    }

    for fingerprint in active_fps {
        let track = symbol_track(&fingerprint.symbol, tracks);
        let setup = symbol_setup(&fingerprint.symbol, setups);
        let review_reason =
            position_review_reason(&fingerprint.symbol, degradations, event_snapshot, track);
        let descriptor = ActionDescriptor::new(
            format!("position:{}", fingerprint.symbol),
            format!("Position {}", fingerprint.symbol),
            serde_json::json!({
                "symbol": fingerprint.symbol,
                "entry_timestamp": fingerprint.entry_timestamp,
                "entry_composite": fingerprint.entry_composite,
                "entry_regime": fingerprint.entry_regime.to_string(),
                "track_status": track.map(|track| track.status.as_str()),
                "track_action": track.map(|track| track.action.as_str()),
                "decision_lineage": setup.map(|setup| serde_json::to_value(&setup.lineage).unwrap_or(serde_json::Value::Null)),
            }),
        );
        let suggested = SuggestedAction::new(
            descriptor,
            fingerprint.entry_timestamp,
            Some("tracker".into()),
            Some(
                setup
                    .and_then(workflow_lineage_summary)
                    .unwrap_or_else(|| "position entered".into()),
            ),
        );
        events.push(ActionWorkflowEventRecord::from_snapshot(
            &ActionWorkflowSnapshot::from_state(&suggested),
        ));
        let confirmed = suggested.clone().confirm(
            fingerprint.entry_timestamp,
            Some("tracker".into()),
            Some("position acknowledged".into()),
        );
        events.push(ActionWorkflowEventRecord::from_transition(
            &suggested, &confirmed,
        ));
        let executed = confirmed.clone().execute(
            fingerprint.entry_timestamp,
            Some("tracker".into()),
            Some("position active".into()),
        );
        events.push(ActionWorkflowEventRecord::from_transition(
            &confirmed, &executed,
        ));
        let monitored = executed.clone().monitor(
            timestamp,
            Some("eden".into()),
            Some("position still monitored".into()),
        );
        events.push(ActionWorkflowEventRecord::from_transition(
            &executed, &monitored,
        ));

        if let Some(reason) = review_reason {
            let reviewed = monitored
                .clone()
                .review(timestamp, Some("eden".into()), Some(reason));
            events.push(ActionWorkflowEventRecord::from_transition(
                &monitored, &reviewed,
            ));
            snapshots.push(ActionWorkflowSnapshot::from_state(&reviewed));
            records.push(ActionWorkflowRecord::from_state(&reviewed));
            continue;
        }

        snapshots.push(ActionWorkflowSnapshot::from_state(&monitored));
        records.push(ActionWorkflowRecord::from_state(&monitored));
    }

    (snapshots, records, events)
}

fn suggestion_gate_reason(
    symbol: &Symbol,
    suggestion: &eden::graph::decision::OrderSuggestion,
    event_snapshot: &EventSnapshot,
    track: Option<&HypothesisTrack>,
) -> Option<String> {
    if suggestion.requires_confirmation {
        return Some("manual confirmation required by decision policy".into());
    }

    if let Some(track) = track {
        if track.action != "enter" {
            return Some(
                track
                    .transition_reason
                    .clone()
                    .unwrap_or_else(|| track.policy_reason.clone()),
            );
        }
    }

    critical_event_reason(symbol, event_snapshot)
}

fn position_review_reason(
    symbol: &Symbol,
    degradations: &HashMap<Symbol, eden::graph::decision::StructuralDegradation>,
    event_snapshot: &EventSnapshot,
    track: Option<&HypothesisTrack>,
) -> Option<String> {
    if let Some(track) = track {
        if matches!(track.status.as_str(), "weakening" | "invalidated") || track.action == "review"
        {
            return Some(
                track
                    .transition_reason
                    .clone()
                    .unwrap_or_else(|| track.policy_reason.clone()),
            );
        }
    }

    if let Some(degradation) = degradations.get(symbol) {
        if degradation.composite_degradation >= Decimal::new(45, 2) {
            return Some(format!(
                "structural degradation reached {}",
                degradation.composite_degradation.round_dp(2)
            ));
        }
    }

    critical_event_reason(symbol, event_snapshot)
}

fn symbol_track<'a>(symbol: &Symbol, tracks: &'a [HypothesisTrack]) -> Option<&'a HypothesisTrack> {
    tracks.iter().find(|track| {
        matches!(
            &track.scope,
            eden::ReasoningScope::Symbol(track_symbol) if track_symbol == symbol
        ) && track.invalidated_at.is_none()
    })
}

fn symbol_setup<'a>(symbol: &Symbol, setups: &'a [TacticalSetup]) -> Option<&'a TacticalSetup> {
    setups.iter().find(|setup| {
        matches!(
            &setup.scope,
            eden::ReasoningScope::Symbol(setup_symbol) if setup_symbol == symbol
        )
    })
}

fn workflow_lineage_summary(setup: &TacticalSetup) -> Option<String> {
    if !setup.lineage.promoted_by.is_empty() {
        Some(format!(
            "promoted_by {}",
            setup.lineage.promoted_by.join(" + ")
        ))
    } else if !setup.lineage.blocked_by.is_empty() {
        Some(format!(
            "blocked_by {}",
            setup.lineage.blocked_by.join(" + ")
        ))
    } else if !setup.lineage.based_on.is_empty() {
        Some(format!("based_on {}", setup.lineage.based_on.join(" + ")))
    } else if !setup.lineage.falsified_by.is_empty() {
        Some(format!(
            "falsified_by {}",
            setup.lineage.falsified_by.join(" + ")
        ))
    } else {
        None
    }
}

fn critical_event_reason(symbol: &Symbol, event_snapshot: &EventSnapshot) -> Option<String> {
    event_snapshot.events.iter().find_map(|event| {
        let symbol_match = matches!(&event.value.scope, SignalScope::Symbol(event_symbol) if event_symbol == symbol);
        let market_match = matches!(event.value.scope, SignalScope::Market);
        let is_critical = matches!(
            event.value.kind,
            MarketEventKind::InstitutionalFlip
                | MarketEventKind::StressRegimeShift
                | MarketEventKind::MarketStressElevated
                | MarketEventKind::ManualReviewRequired
        );

        if is_critical && (symbol_match || market_match) {
            Some(event.value.summary.clone())
        } else {
            None
        }
    })
}

fn setup_action_priority(action: &str) -> i32 {
    match action {
        "enter" => 0,
        "review" => 1,
        "observe" => 2,
        _ => 3,
    }
}

fn select_propagation_preview<'a>(
    paths: &'a [eden::PropagationPath],
    limit: usize,
) -> Vec<&'a eden::PropagationPath> {
    let mut selected = Vec::new();

    for candidate in [
        paths
            .iter()
            .find(|path| path_has_family(path, "shared_holder")),
        paths.iter().find(|path| path_has_family(path, "rotation")),
        paths.iter().find(|path| path_is_mixed_multi_hop(path)),
    ]
    .into_iter()
    .flatten()
    {
        if !selected
            .iter()
            .any(|existing: &&eden::PropagationPath| existing.path_id == candidate.path_id)
        {
            selected.push(candidate);
        }
    }

    for path in paths {
        if selected.len() >= limit {
            break;
        }
        if selected
            .iter()
            .any(|existing: &&eden::PropagationPath| existing.path_id == path.path_id)
        {
            continue;
        }
        selected.push(path);
    }

    selected
}

fn best_multi_hop_by_len<'a>(
    paths: &'a [eden::PropagationPath],
    hop_len: usize,
) -> Option<&'a eden::PropagationPath> {
    paths.iter().find(|path| path.steps.len() == hop_len)
}

const MIN_READY_SYMBOLS_FOR_FULL_DISPLAY: usize = 35;
const MIN_BOOTSTRAP_TICKS: u64 = 3;
const MIN_DEGRADATION_AGE_SECS: i64 = 30;

struct ReadinessReport {
    ready_symbols: HashSet<Symbol>,
    quote_symbols: usize,
    order_book_symbols: usize,
    context_symbols: usize,
}

impl ReadinessReport {
    fn bootstrap_mode(&self, tick: u64) -> bool {
        tick <= MIN_BOOTSTRAP_TICKS || self.ready_symbols.len() < MIN_READY_SYMBOLS_FOR_FULL_DISPLAY
    }
}

fn compute_readiness(links: &LinkSnapshot) -> ReadinessReport {
    let quoted_symbols: HashSet<Symbol> = links
        .quotes
        .iter()
        .filter(|q| q.last_done > Decimal::ZERO)
        .map(|q| q.symbol.clone())
        .collect();
    let order_book_symbols: HashSet<Symbol> = links
        .order_books
        .iter()
        .filter(|ob| ob.total_bid_volume + ob.total_ask_volume > 0)
        .map(|ob| ob.symbol.clone())
        .collect();

    let mut context_symbols: HashSet<Symbol> = HashSet::new();
    context_symbols.extend(links.calc_indexes.iter().map(|obs| obs.symbol.clone()));
    context_symbols.extend(
        links
            .candlesticks
            .iter()
            .filter(|obs| obs.candle_count >= 2)
            .map(|obs| obs.symbol.clone()),
    );
    context_symbols.extend(links.capital_flows.iter().map(|obs| obs.symbol.clone()));
    context_symbols.extend(
        links
            .capital_breakdowns
            .iter()
            .map(|obs| obs.symbol.clone()),
    );

    let ready_symbols = quoted_symbols
        .iter()
        .filter(|symbol| order_book_symbols.contains(*symbol) && context_symbols.contains(*symbol))
        .cloned()
        .collect();

    ReadinessReport {
        ready_symbols,
        quote_symbols: quoted_symbols.len(),
        order_book_symbols: order_book_symbols.len(),
        context_symbols: context_symbols.len(),
    }
}

fn filter_convergence_scores(
    convergence_scores: &HashMap<Symbol, eden::graph::decision::ConvergenceScore>,
    ready_symbols: &HashSet<Symbol>,
) -> HashMap<Symbol, eden::graph::decision::ConvergenceScore> {
    convergence_scores
        .iter()
        .filter(|(symbol, _)| ready_symbols.contains(*symbol))
        .map(|(symbol, score)| (symbol.clone(), score.clone()))
        .collect()
}

fn filter_order_suggestions(
    order_suggestions: &[eden::graph::decision::OrderSuggestion],
    ready_symbols: &HashSet<Symbol>,
) -> Vec<eden::graph::decision::OrderSuggestion> {
    order_suggestions
        .iter()
        .filter(|suggestion| ready_symbols.contains(&suggestion.symbol))
        .cloned()
        .collect()
}

fn filter_degradations(
    degradations: &HashMap<Symbol, eden::graph::decision::StructuralDegradation>,
    active_fingerprints: &[StructuralFingerprint],
    now: time::OffsetDateTime,
    ready_symbols: &HashSet<Symbol>,
) -> HashMap<Symbol, eden::graph::decision::StructuralDegradation> {
    let active_map: HashMap<&Symbol, &StructuralFingerprint> = active_fingerprints
        .iter()
        .map(|fingerprint| (&fingerprint.symbol, fingerprint))
        .collect();

    degradations
        .iter()
        .filter(|(symbol, _)| ready_symbols.contains(*symbol))
        .filter_map(|(symbol, degradation)| {
            active_map.get(symbol).and_then(|fingerprint| {
                let age_secs = (now - fingerprint.entry_timestamp).whole_seconds();
                if age_secs >= MIN_DEGRADATION_AGE_SECS {
                    Some((symbol.clone(), degradation.clone()))
                } else {
                    None
                }
            })
        })
        .collect()
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    let args = std::env::args().collect::<Vec<_>>();
    let command = match parse_cli_command(&args) {
        Ok(command) => command,
        Err(message) => {
            eprintln!("{}", message);
            std::process::exit(2);
        }
    };
    if matches!(command, CliCommand::UsLive) {
        if let Err(error) = eden::us::run().await {
            eprintln!("US runtime failed: {}", error);
            std::process::exit(1);
        }
        return;
    }

    if !matches!(command, CliCommand::Live) {
        if let Err(error) = run_cli_query(command).await {
            eprintln!("Query failed: {}", error);
            std::process::exit(1);
        }
        return;
    }

    let config = match Config::from_env() {
        Ok(config) => config,
        Err(error) => {
            eprintln!(
                "Live runtime failed to load Longport config from env: {}",
                error
            );
            std::process::exit(1);
        }
    };
    let (ctx, mut receiver) = match QuoteContext::try_new(Arc::new(config)).await {
        Ok(value) => value,
        Err(error) => {
            eprintln!("Live runtime failed to connect to Longport: {}", error);
            std::process::exit(1);
        }
    };

    println!("Connected to Longport. Initializing ObjectStore...");

    let store = store::initialize(&ctx, WATCHLIST).await;

    println!("\n=== ObjectStore Stats ===");
    println!("Institutions: {}", store.institutions.len());
    println!("Brokers:      {}", store.brokers.len());
    println!("Stocks:       {}", store.stocks.len());
    println!("Sectors:      {}", store.sectors.len());

    let test_broker = BrokerId(4497);
    if let Some(inst) = store.institution_for_broker(&test_broker) {
        println!("\nBroker {} → {} ({})", test_broker, inst.name_en, inst.id);
    }

    // ── Initialize persistence ──
    #[cfg(feature = "persistence")]
    let eden_store = {
        let eden_db_path = std::env::var("EDEN_DB_PATH").unwrap_or_else(|_| "data/eden.db".into());
        match EdenStore::open(&eden_db_path).await {
            Ok(store) => {
                println!("SurrealDB opened at {}", eden_db_path);
                Some(store)
            }
            Err(e) => {
                eprintln!(
                    "Warning: SurrealDB failed to open: {}. Running without persistence.",
                    e
                );
                None
            }
        }
    };
    #[cfg(not(feature = "persistence"))]
    println!("Persistence feature disabled; running without SurrealDB.");

    let watchlist_symbols: Vec<Symbol> = WATCHLIST.iter().map(|s| Symbol(s.to_string())).collect();
    let polymarket_configs = match load_polymarket_configs() {
        Ok(configs) => configs,
        Err(error) => {
            eprintln!("Warning: {}", error);
            vec![]
        }
    };
    if !polymarket_configs.is_empty() {
        println!(
            "Loaded {} Polymarket market priors from POLYMARKET_MARKETS.",
            polymarket_configs.len()
        );
    }

    // ── Subscribe to ALL real-time push types ──
    println!("\nSubscribing to WebSocket (DEPTH + BROKER + QUOTE + TRADE)...");
    if let Err(error) = ctx
        .subscribe(
            WATCHLIST,
            SubFlags::DEPTH | SubFlags::BROKER | SubFlags::QUOTE | SubFlags::TRADE,
        )
        .await
    {
        eprintln!(
            "Live runtime failed to subscribe to Longport streams: {}",
            error
        );
        std::process::exit(1);
    }
    println!("Subscribed to {} symbols × 4 channels.", WATCHLIST.len());

    // Subscribe to 1-minute candlesticks
    for symbol in WATCHLIST {
        if let Err(e) = ctx
            .subscribe_candlesticks(*symbol, Period::OneMinute, TradeSessions::Intraday)
            .await
        {
            eprintln!(
                "Warning: failed to subscribe candlestick for {}: {}",
                symbol, e
            );
        }
    }
    println!("Subscribed to 1-min candlesticks.");

    // ── Seed with a cheap bootstrap snapshot ──
    println!("Fetching bootstrap quotes...");
    let initial_quotes = snapshot::fetch_quotes_only(&ctx, &watchlist_symbols).await;

    let mut live = LiveState::new();
    live.quotes = initial_quotes;
    live.dirty = !live.quotes.is_empty();
    let mut rest = RestSnapshot::empty();

    let mut tracker = PositionTracker::new();
    let mut history = TickHistory::new(300); // ~10 min at 2s debounce
    let mut prev_insights: Option<GraphInsights> = None;
    let mut conflict_history = ConflictHistory::new();
    let mut scorecard = SignalScorecard::new(500, 15); // 500 events, resolve after 15 ticks (~30s)
    let live_snapshot_path = snapshot_path("EDEN_LIVE_SNAPSHOT_PATH", "data/live_snapshot.json");
    ensure_snapshot_parent(&live_snapshot_path).await;
    let pct = Decimal::new(100, 0);

    println!(
        "\nReal-time event-driven monitoring active (debounce: {}ms)\n",
        DEBOUNCE_MS,
    );

    // ── Spawn push event forwarder ──
    let (push_tx, mut push_rx) = mpsc::channel::<PushEvent>(10000);
    tokio::spawn(async move {
        let mut dropped_push_events = 0u64;
        while let Some(event) = receiver.recv().await {
            match push_tx.try_send(event) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) => {
                    dropped_push_events += 1;
                    if dropped_push_events == 1 || dropped_push_events % 100 == 0 {
                        eprintln!(
                            "Warning: dropped {} HK push events because debounce channel is full.",
                            dropped_push_events
                        );
                    }
                }
                Err(TrySendError::Closed(_)) => {
                    eprintln!("Warning: HK push event channel closed; stopping forwarder.");
                    break;
                }
            }
        }
    });

    let mut tick: u64 = 0;
    let debounce = Duration::from_millis(DEBOUNCE_MS);

    // Refresh heavy REST data in the background so bootstrap can reach the first tick quickly.
    let rest_ctx = ctx.clone();
    let rest_watchlist = watchlist_symbols.clone();
    let rest_polymarket = polymarket_configs.clone();
    let mut rest_rx = spawn_periodic_fetch(1, Duration::from_secs(60), move || {
        let rest_ctx = rest_ctx.clone();
        let rest_watchlist = rest_watchlist.clone();
        let rest_polymarket = rest_polymarket.clone();
        async move { fetch_rest_data(&rest_ctx, &rest_watchlist, &rest_polymarket).await }
    });
    let mut bootstrap_pending = live.dirty;

    loop {
        let mut rest_updated = false;
        let Some(tick_advance) = ({
            let mut tick_state = HkTickState {
                live: &mut live,
                rest: &mut rest,
                rest_updated: &mut rest_updated,
            };
            match next_tick(
                &mut bootstrap_pending,
                &mut push_rx,
                &mut rest_rx,
                debounce,
                &mut tick_state,
                &mut tick,
            )
            .await
            {
                Ok(result) => result,
                Err(()) => {
                    eprintln!("Push channel closed. Exiting.");
                    break;
                }
            }
        }) else {
            continue;
        };

        let now = tick_advance.now;
        let previous_polymarket = history
            .latest()
            .map(|tick| tick.polymarket_priors.clone())
            .unwrap_or_default();

        if tick_advance.received_update {
            for idx in rest.calc_indexes.values() {
                if let (Some(vr), Some(tr)) = (idx.volume_ratio, idx.turnover_rate) {
                    if vr > Decimal::TWO {
                        println!(
                            "  [VOLUME] {}  volume_ratio={:.1}  turnover_rate={:.2}%  5min_chg={:+.2}%",
                            idx.symbol,
                            vr,
                            tr * pct,
                            idx.five_minutes_change_rate.unwrap_or(Decimal::ZERO) * pct,
                        );
                    }
                }
            }

            if let Some(temp) = &rest.market_temperature {
                println!(
                    "  [MARKET] HK temperature={} valuation={} sentiment={} ({})",
                    temp.temperature, temp.valuation, temp.sentiment, temp.description,
                );
            }

            if !rest.polymarket.priors.is_empty() {
                for prior in rest.polymarket.priors.iter().take(3) {
                    let delta = previous_polymarket
                        .iter()
                        .find(|previous| previous.slug == prior.slug)
                        .map(|previous| prior.probability - previous.probability)
                        .unwrap_or(Decimal::ZERO);
                    println!(
                        "  [POLY] {}  outcome={}  prob={:.0}%  d_prob={:+.0}%  bias={}",
                        prior.label,
                        prior.selected_outcome,
                        (prior.probability * pct).round_dp(0),
                        (delta * pct).round_dp(0),
                        prior.bias.as_str(),
                    );
                }
            }
        }

        println!("══════════════════════════════════════════════════════════");
        println!(
            "  #{:<4}  {}  │  {} total pushes",
            tick,
            now.format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_else(|_| now.to_string()),
            live.push_count,
        );
        println!("══════════════════════════════════════════════════════════");

        // ── Build snapshot and run full pipeline ──
        let raw = live.to_raw_snapshot(&rest);

        // Show trade activity if any
        let trade_symbols: Vec<_> = raw
            .trades
            .iter()
            .filter(|(_, t)| !t.is_empty())
            .map(|(s, t)| (s.clone(), t.len(), t.iter().map(|t| t.volume).sum::<i64>()))
            .collect();

        let links = LinkSnapshot::compute(&raw, &store);
        let readiness = compute_readiness(&links);
        let dim_snapshot = DimensionSnapshot::compute(&links, &store);
        let tension_snapshot = TensionSnapshot::compute(&dim_snapshot);
        let narrative_snapshot = NarrativeSnapshot::compute(&tension_snapshot, &dim_snapshot);
        let brain = BrainGraph::compute(&narrative_snapshot, &dim_snapshot, &links, &store);

        let graph_insights = GraphInsights::compute(
            &brain,
            &store,
            prev_insights.as_ref(),
            &mut conflict_history,
            tick,
        );

        let active_fps = tracker.active_fingerprints();
        let mut decision = DecisionSnapshot::compute(&brain, &links, &active_fps, &store);
        if !rest.polymarket.is_empty() {
            decision.apply_polymarket_snapshot(&rest.polymarket, &store);
        }
        let ready_convergence_scores =
            filter_convergence_scores(&decision.convergence_scores, &readiness.ready_symbols);
        let ready_order_suggestions =
            filter_order_suggestions(&decision.order_suggestions, &readiness.ready_symbols);
        let aged_degradations = filter_degradations(
            &decision.degradations,
            &active_fps,
            now,
            &readiness.ready_symbols,
        );
        let observation_snapshot = ObservationSnapshot::from_links(&links);
        let event_snapshot =
            EventSnapshot::detect(&history, &links, &dim_snapshot, &graph_insights, &decision);
        let derived_signal_snapshot = DerivedSignalSnapshot::compute(
            &dim_snapshot,
            &graph_insights,
            &decision,
            &event_snapshot,
        );
        let previous_setups = history
            .latest()
            .map(|tick| tick.tactical_setups.as_slice())
            .unwrap_or(&[]);
        let previous_tracks = history
            .latest()
            .map(|tick| tick.hypothesis_tracks.as_slice())
            .unwrap_or(&[]);
        let reasoning_snapshot = ReasoningSnapshot::derive(
            &event_snapshot,
            &derived_signal_snapshot,
            &graph_insights,
            &decision,
            previous_setups,
            previous_tracks,
        );
        let world_snapshots = WorldSnapshots::derive(
            &event_snapshot,
            &derived_signal_snapshot,
            &graph_insights,
            &decision,
            &reasoning_snapshot,
            (!rest.polymarket.is_empty()).then_some(&rest.polymarket),
            history.latest().map(|tick| &tick.backward_reasoning),
        );
        let actionable_setups = reasoning_snapshot
            .tactical_setups
            .iter()
            .filter(|setup| setup.action == "enter")
            .collect::<Vec<_>>();
        let actionable_symbols: HashSet<Symbol> = actionable_setups
            .iter()
            .filter_map(|setup| match &setup.scope {
                eden::ReasoningScope::Symbol(symbol) => Some(symbol.clone()),
                _ => None,
            })
            .collect();
        let actionable_order_suggestions = ready_order_suggestions
            .iter()
            .filter(|suggestion| actionable_symbols.contains(&suggestion.symbol))
            .cloned()
            .collect::<Vec<_>>();

        // Auto-exit: remove positions whose composite is now zero
        let zero_syms: Vec<Symbol> = tracker
            .active_fingerprints()
            .iter()
            .filter(|fp| {
                readiness.ready_symbols.contains(&fp.symbol)
                    && decision
                        .convergence_scores
                        .get(&fp.symbol)
                        .map(|c| c.composite == Decimal::ZERO)
                        .unwrap_or(true) // also exit if symbol disappeared from convergence
            })
            .map(|fp| fp.symbol.clone())
            .collect();
        for sym in &zero_syms {
            tracker.exit(sym);
        }

        let newly_entered = tracker.auto_enter_allowed(
            &ready_convergence_scores,
            Some(&actionable_symbols),
            &brain,
        );
        let new_set: HashSet<&Symbol> = newly_entered.iter().collect();
        let (workflow_snapshots, workflow_records, workflow_events) = build_action_workflows(
            now,
            &actionable_order_suggestions,
            &tracker.active_fingerprints(),
            &aged_degradations,
            &event_snapshot,
            &reasoning_snapshot.hypothesis_tracks,
            &reasoning_snapshot.tactical_setups,
        );
        #[cfg(not(feature = "persistence"))]
        let _ = (&workflow_records, &workflow_events);

        // Refresh fingerprints every 30 ticks to prevent stale degradation baselines
        if tick % 30 == 0 && tracker.active_count() > 0 {
            tracker.refresh_all(&brain);
        }

        // ── Capture tick record into history ──
        let tick_record = TickRecord::capture(
            tick,
            now,
            &ready_convergence_scores,
            &dim_snapshot.dimensions,
            &links.order_books,
            &links.quotes,
            &links.trade_activities,
            &aged_degradations,
            &observation_snapshot,
            &event_snapshot,
            &derived_signal_snapshot,
            &workflow_snapshots,
            &rest.polymarket.priors,
            &reasoning_snapshot,
            &world_snapshots.world_state,
            &world_snapshots.backward_reasoning,
        );
        history.push(tick_record);

        // ── Persist to SurrealDB (non-blocking, fire-and-forget) ──
        #[cfg(feature = "persistence")]
        if let Some(ref store) = eden_store {
            if let Some(latest) = history.latest() {
                let record = latest.clone();
                let store_ref = store.clone();
                tokio::spawn(async move {
                    if let Err(e) = store_ref.write_tick(&record).await {
                        eprintln!("Warning: failed to write tick: {}", e);
                    }
                });
            }
            if tick % 30 == 0 {
                let presences = links.cross_stock_presences.clone();
                let store_ref = store.clone();
                tokio::spawn(async move {
                    if let Err(e) = store_ref.write_institution_states(&presences, now).await {
                        eprintln!("Warning: failed to write institution states: {}", e);
                    }
                });
            }
            if !workflow_records.is_empty() || !workflow_events.is_empty() {
                let workflow_records = workflow_records.clone();
                let workflow_events = workflow_events.clone();
                let store_ref = store.clone();
                tokio::spawn(async move {
                    for record in workflow_records {
                        if let Err(e) = store_ref.write_action_workflow(&record).await {
                            eprintln!("Warning: failed to write action workflow: {}", e);
                        }
                    }
                    for event in workflow_events {
                        if let Err(e) = store_ref.write_action_workflow_event(&event).await {
                            eprintln!("Warning: failed to write action workflow event: {}", e);
                        }
                    }
                });
            }
            if !reasoning_snapshot.tactical_setups.is_empty() {
                let tactical_setup_records = reasoning_snapshot
                    .tactical_setups
                    .iter()
                    .map(|setup| TacticalSetupRecord::from_setup(setup, now))
                    .collect::<Vec<_>>();
                let store_ref = store.clone();
                tokio::spawn(async move {
                    for record in tactical_setup_records {
                        if let Err(e) = store_ref.write_tactical_setup(&record).await {
                            eprintln!("Warning: failed to write tactical setup: {}", e);
                        }
                    }
                });
            }
            if !reasoning_snapshot.hypothesis_tracks.is_empty() {
                let hypothesis_track_records = reasoning_snapshot
                    .hypothesis_tracks
                    .iter()
                    .map(HypothesisTrackRecord::from_track)
                    .collect::<Vec<_>>();
                let store_ref = store.clone();
                tokio::spawn(async move {
                    for record in hypothesis_track_records {
                        if let Err(e) = store_ref.write_hypothesis_track(&record).await {
                            eprintln!("Warning: failed to write hypothesis track: {}", e);
                        }
                    }
                });
            }
        }

        // ── Compute temporal dynamics ──
        let dynamics = compute_dynamics(&history);
        let polymarket_dynamics = compute_polymarket_dynamics(&history);
        let causal_timelines = compute_causal_timelines(&history);
        let lineage_stats = compute_lineage_stats(&history, LINEAGE_WINDOW);

        if let Some(latest) = history.latest() {
            let live_snapshot = build_hk_live_snapshot(
                tick,
                now.format(&time::format_description::well_known::Rfc3339)
                    .unwrap_or_default(),
                &store,
                &brain,
                &decision,
                &graph_insights,
                &reasoning_snapshot,
                &event_snapshot,
                &observation_snapshot,
                &scorecard,
                &dim_snapshot,
                latest,
                &tracker,
                &causal_timelines,
                &lineage_stats,
            );
            #[cfg(feature = "persistence")]
            let reasoning_assessment_records = eden_store
                .as_ref()
                .map(|_| {
                    build_case_list(&live_snapshot)
                        .cases
                        .iter()
                        .map(|case| {
                            CaseReasoningAssessmentRecord::from_case_summary(case, now, "runtime")
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            spawn_write_snapshot(live_snapshot_path.clone(), live_snapshot);

            #[cfg(feature = "persistence")]
            if let Some(ref store) = eden_store {
                if !reasoning_assessment_records.is_empty() {
                    let assessment_records = reasoning_assessment_records.clone();
                    let store_ref = store.clone();
                    tokio::spawn(async move {
                        if let Err(e) = store_ref
                            .write_case_reasoning_assessments(&assessment_records)
                            .await
                        {
                            eprintln!("Warning: failed to write case reasoning assessments: {}", e);
                        }
                    });
                }

                let realized_outcomes = compute_case_realized_outcomes(
                    &history,
                    LINEAGE_WINDOW,
                    CASE_OUTCOME_RESOLUTION_LAG,
                )
                .into_iter()
                .map(|outcome| CaseRealizedOutcomeRecord::from_outcome(&outcome, "hk"))
                .collect::<Vec<_>>();
                if !realized_outcomes.is_empty() {
                    let realized_outcomes = realized_outcomes.clone();
                    let store_ref = store.clone();
                    tokio::spawn(async move {
                        if let Err(e) = store_ref
                            .write_case_realized_outcomes(&realized_outcomes)
                            .await
                        {
                            eprintln!("Warning: failed to write case realized outcomes: {}", e);
                        }
                    });
                }
            }
        }

        #[cfg(feature = "persistence")]
        if let Some(ref store) = eden_store {
            let lineage_snapshot =
                LineageSnapshotRecord::new(tick, now, LINEAGE_WINDOW, &lineage_stats);
            let lineage_rows = rows_from_lineage_stats(
                lineage_snapshot.record_id(),
                tick,
                now,
                LINEAGE_WINDOW,
                &lineage_stats,
            );
            let store_ref = store.clone();
            tokio::spawn(async move {
                if let Err(e) = store_ref.write_lineage_snapshot(&lineage_snapshot).await {
                    eprintln!("Warning: failed to write lineage snapshot: {}", e);
                }
                if let Err(e) = store_ref.write_lineage_metric_rows(&lineage_rows).await {
                    eprintln!("Warning: failed to write lineage metric rows: {}", e);
                }
            });
        }

        let bootstrap_mode = readiness.bootstrap_mode(tick);
        if bootstrap_mode {
            println!(
                "\n── Bootstrap ──\n  ready_symbols={}  quotes={}  order_books={}  context={}  workflows={}",
                readiness.ready_symbols.len(),
                readiness.quote_symbols,
                readiness.order_book_symbols,
                readiness.context_symbols,
                workflow_snapshots.len(),
            );
            if !reasoning_snapshot.propagation_paths.is_empty() {
                println!("\n── Bootstrap Propagation Preview ──");
                for path in select_propagation_preview(&reasoning_snapshot.propagation_paths, 5) {
                    println!(
                        "  hops={}  conf={:+}  {}",
                        path.steps.len(),
                        path.confidence.round_dp(3),
                        path.summary,
                    );
                }
                if let Some(path) = best_multi_hop_by_len(&reasoning_snapshot.propagation_paths, 2)
                {
                    println!(
                        "  best_2hop:    conf={:+}  {}",
                        path.confidence.round_dp(3),
                        path.summary,
                    );
                }
                if let Some(path) = best_multi_hop_by_len(&reasoning_snapshot.propagation_paths, 3)
                {
                    println!(
                        "  best_3hop:    conf={:+}  {}",
                        path.confidence.round_dp(3),
                        path.summary,
                    );
                }
            }
        } else {
            // ── Display: Convergence Scores ──
            println!("\n── Convergence Scores ──");
            let mut conv_syms: Vec<_> = ready_convergence_scores.iter().collect();
            conv_syms.sort_by(|a, b| b.1.composite.abs().cmp(&a.1.composite.abs()));
            for (sym, c) in &conv_syms {
                let dir = if c.composite > Decimal::ZERO {
                    "▲"
                } else if c.composite < Decimal::ZERO {
                    "▼"
                } else {
                    "—"
                };
                println!(
                    "  {:>8}  composite={}{:>+7}%  inst={:>+7}%  sector={:>+7}%  corr={:>+7}%",
                    sym,
                    dir,
                    (c.composite * pct).round_dp(1),
                    (c.institutional_alignment * pct).round_dp(1),
                    c.sector_coherence
                        .map(|s| format!("{:>+7}", (s * pct).round_dp(1)))
                        .unwrap_or_else(|| "    n/a".into()),
                    (c.cross_stock_correlation * pct).round_dp(1),
                );
            }

            // ── Display: Graph Structure ──
            graph_insights.display(&store);

            // ── Display: Semantic Layers ──
            println!(
                "\n── Semantic Layers ──\n  observations={}  events={}  derived_signals={}  workflows={}  hypotheses={}  paths={}  setups={}  tracks={}  clusters={}  world_entities={}  backward_cases={}",
                observation_snapshot.observations.len(),
                event_snapshot.events.len(),
                derived_signal_snapshot.signals.len(),
                workflow_snapshots.len(),
                reasoning_snapshot.hypotheses.len(),
                reasoning_snapshot.propagation_paths.len(),
                reasoning_snapshot.tactical_setups.len(),
                reasoning_snapshot.hypothesis_tracks.len(),
                reasoning_snapshot.case_clusters.len(),
                world_snapshots.world_state.entities.len(),
                world_snapshots.backward_reasoning.investigations.len(),
            );
            for event in event_snapshot.events.iter().take(5) {
                println!(
                    "  Event:        {:?}  {:?}  mag={:+}  {}",
                    event.value.scope,
                    event.value.kind,
                    event.value.magnitude.round_dp(2),
                    event.value.summary,
                );
            }
            for signal in derived_signal_snapshot.signals.iter().take(5) {
                println!(
                    "  Signal:       {:?}  {:?}  strength={:+}  {}",
                    signal.value.scope,
                    signal.value.kind,
                    signal.value.strength.round_dp(2),
                    signal.value.summary,
                );
            }
            for path in select_propagation_preview(&reasoning_snapshot.propagation_paths, 5) {
                println!(
                    "  Path:         hops={}  conf={:+}  {}",
                    path.steps.len(),
                    path.confidence.round_dp(3),
                    path.summary,
                );
            }
            if let Some(path) = best_multi_hop_by_len(&reasoning_snapshot.propagation_paths, 2) {
                println!(
                    "  best_2hop:    conf={:+}  {}",
                    path.confidence.round_dp(3),
                    path.summary,
                );
            }
            if let Some(path) = best_multi_hop_by_len(&reasoning_snapshot.propagation_paths, 3) {
                println!(
                    "  best_3hop:    conf={:+}  {}",
                    path.confidence.round_dp(3),
                    path.summary,
                );
            }
            for workflow in workflow_snapshots.iter().take(5) {
                println!(
                    "  Workflow:     {}  stage={}  {}",
                    workflow.workflow_id, workflow.stage, workflow.title,
                );
                if let Some(note) = &workflow.note {
                    println!("                why={}", note);
                }
            }
            let hypothesis_map: HashMap<_, _> = reasoning_snapshot
                .hypotheses
                .iter()
                .map(|hypothesis| (hypothesis.hypothesis_id.as_str(), hypothesis))
                .collect();
            let track_map: HashMap<_, _> = reasoning_snapshot
                .hypothesis_tracks
                .iter()
                .filter(|track| track.invalidated_at.is_none())
                .map(|track| (track.setup_id.as_str(), track))
                .collect();
            if !reasoning_snapshot.case_clusters.is_empty() {
                println!("\n── Top Tactical Clusters ──");
                for cluster in reasoning_snapshot.case_clusters.iter().take(5) {
                    println!(
                        "  {}  trend={}  members={}  avg_gap={:+}  avg_edge={:+}",
                        cluster.title,
                        cluster.trend,
                        cluster.member_count,
                        cluster.average_gap.round_dp(3),
                        cluster.average_edge.round_dp(3),
                    );
                    println!(
                        "                lead={}  strongest={}  weakest={}",
                        cluster.lead_statement, cluster.strongest_title, cluster.weakest_title,
                    );
                }
            }
            if !world_snapshots.world_state.entities.is_empty() {
                println!("\n── World State ──");
                for entity in world_snapshots.world_state.entities.iter().take(6) {
                    println!(
                        "  {}  layer={}  conf={:+}  local={:+}  propagated={:+}  regime={}",
                        entity.label,
                        entity.layer,
                        entity.confidence.round_dp(3),
                        entity.local_support.round_dp(3),
                        entity.propagated_support.round_dp(3),
                        entity.regime,
                    );
                    if let Some(driver) = entity.drivers.first() {
                        println!("                driver={}", driver);
                    }
                    println!(
                        "                provenance={:?}  trace={}  inputs={}",
                        entity.provenance.source,
                        entity.provenance.trace_id.as_deref().unwrap_or("-"),
                        entity
                            .provenance
                            .inputs
                            .iter()
                            .take(3)
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
            }
            println!(
                "\n── Market Gate ──\n  bias={}  conf={}  breadth_up={:.0}%  breadth_down={:.0}%  avg_return={:+.2}%  consensus={:+.2}",
                decision.market_regime.bias,
                (decision.market_regime.confidence * pct).round_dp(0),
                (decision.market_regime.breadth_up * pct).round_dp(0),
                (decision.market_regime.breadth_down * pct).round_dp(0),
                (decision.market_regime.average_return * pct).round_dp(2),
                decision.market_regime.directional_consensus.round_dp(2),
            );
            if let Some(leader_return) = decision.market_regime.leader_return {
                println!(
                    "                leader_return={:+.2}%",
                    (leader_return * pct).round_dp(2),
                );
            }
            if let Some(driver) = &decision.market_regime.external_driver {
                println!(
                    "                external={}  ext_bias={}  ext_conf={:.0}%",
                    driver,
                    decision
                        .market_regime
                        .external_bias
                        .map(|bias| bias.as_str())
                        .unwrap_or("neutral"),
                    (decision
                        .market_regime
                        .external_confidence
                        .unwrap_or(Decimal::ZERO)
                        * pct)
                        .round_dp(0),
                );
            }
            if let Some(best) = actionable_order_suggestions
                .iter()
                .max_by(|a, b| a.convergence_score.cmp(&b.convergence_score))
            {
                let direction = match best.direction {
                    OrderDirection::Buy => "long",
                    OrderDirection::Sell => "short",
                };
                println!(
                    "                best_convergence={}  {}={:.0}%",
                    best.symbol,
                    direction,
                    (best.convergence_score * pct).round_dp(0),
                );
            }
            let mut top_cases = reasoning_snapshot
                .tactical_setups
                .iter()
                .collect::<Vec<_>>();
            top_cases.sort_by(|a, b| {
                setup_action_priority(&a.action)
                    .cmp(&setup_action_priority(&b.action))
                    .then_with(|| b.confidence_gap.cmp(&a.confidence_gap))
                    .then_with(|| b.heuristic_edge.cmp(&a.heuristic_edge))
                    .then_with(|| b.confidence.cmp(&a.confidence))
            });
            if !top_cases.is_empty() {
                println!("\n── Top Tactical Cases ──");
                for setup in top_cases.iter().take(5) {
                    let primary = hypothesis_map.get(setup.hypothesis_id.as_str()).copied();
                    let runner_up = setup
                        .runner_up_hypothesis_id
                        .as_ref()
                        .and_then(|hypothesis_id| {
                            hypothesis_map.get(hypothesis_id.as_str()).copied()
                        })
                        .map(|hypothesis| hypothesis.statement.as_str())
                        .unwrap_or("none");
                    let track = track_map.get(setup.setup_id.as_str()).copied();
                    let status = track.map(|track| track.status.as_str()).unwrap_or("new");
                    let conf_delta = track
                        .map(|track| track.confidence_change.round_dp(3))
                        .unwrap_or(Decimal::ZERO);
                    println!(
                        "  {}  action={}  status={}  d_conf={:+}  gap={:+}  edge={:+}  family={}  winner={}  runner_up={}",
                        setup.title,
                        setup.action,
                        status,
                        conf_delta,
                        setup.confidence_gap.round_dp(3),
                        setup.heuristic_edge.round_dp(3),
                        primary
                            .map(|hypothesis| hypothesis.family_label.as_str())
                            .unwrap_or("unknown"),
                        primary
                            .map(|hypothesis| hypothesis.statement.as_str())
                            .unwrap_or("unknown"),
                        runner_up,
                    );
                    if let Some(hypothesis) = primary {
                        println!(
                            "                evidence local={:+}/{:+}  propagated={:+}/{:+}",
                            hypothesis.local_support_weight.round_dp(3),
                            hypothesis.local_contradict_weight.round_dp(3),
                            hypothesis.propagated_support_weight.round_dp(3),
                            hypothesis.propagated_contradict_weight.round_dp(3),
                        );
                        if let Some(invalidation) = hypothesis.invalidation_conditions.first() {
                            println!(
                                "                invalidates_on={}",
                                invalidation.description
                            );
                        }
                        println!(
                            "                provenance={:?}  trace={}  inputs={}",
                            hypothesis.provenance.source,
                            hypothesis.provenance.trace_id.as_deref().unwrap_or("-"),
                            hypothesis
                                .provenance
                                .inputs
                                .iter()
                                .take(3)
                                .cloned()
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                    }
                    if let Some(track) = track {
                        println!("                why={}", track.policy_reason);
                        if let Some(transition_reason) = &track.transition_reason {
                            println!("                transition={}", transition_reason);
                        }
                    }
                    if !setup.lineage.based_on.is_empty()
                        || !setup.lineage.blocked_by.is_empty()
                        || !setup.lineage.promoted_by.is_empty()
                        || !setup.lineage.falsified_by.is_empty()
                    {
                        println!(
                            "                lineage based_on=[{}] blocked_by=[{}] promoted_by=[{}] falsified_by=[{}]",
                            setup.lineage.based_on.join(", "),
                            setup.lineage.blocked_by.join(", "),
                            setup.lineage.promoted_by.join(", "),
                            setup.lineage.falsified_by.join(", "),
                        );
                    }
                }
            }
            let invalidated_cases = reasoning_snapshot
                .hypothesis_tracks
                .iter()
                .filter(|track| track.status.as_str() == "invalidated")
                .collect::<Vec<_>>();
            if !invalidated_cases.is_empty() {
                println!("\n── Recently Invalidated Cases ──");
                for track in invalidated_cases.iter().take(5) {
                    println!(
                        "  {}  action={}  last_conf={:+}  last_gap={:+}",
                        track.title,
                        track.action,
                        track.confidence.round_dp(3),
                        track.confidence_gap.round_dp(3),
                    );
                }
            }
            if !lineage_stats.based_on.is_empty()
                || !lineage_stats.blocked_by.is_empty()
                || !lineage_stats.promoted_by.is_empty()
                || !lineage_stats.falsified_by.is_empty()
            {
                println!("\n── Lineage Stats ──");
                if let Some((label, count)) = lineage_stats.based_on.first() {
                    println!("  top_based_on     {}  x{}", label, count);
                }
                if let Some((label, count)) = lineage_stats.blocked_by.first() {
                    println!("  top_blocked_by   {}  x{}", label, count);
                }
                if let Some((label, count)) = lineage_stats.promoted_by.first() {
                    println!("  top_promoted_by  {}  x{}", label, count);
                }
                if let Some((label, count)) = lineage_stats.falsified_by.first() {
                    println!("  top_falsified_by {}  x{}", label, count);
                }
                if let Some(item) = lineage_stats.promoted_outcomes.first() {
                    println!(
                        "  best_promoted    {}  resolved={}  hit_rate={:.0}%  gross={:+.2}%  net={:+.2}%  mfe={:+.2}%  mae={:+.2}%  follow={:.0}%  retain={:.0}%  invalid={:.0}%",
                        item.label,
                        item.resolved,
                        (item.hit_rate * pct).round_dp(0),
                        (item.mean_return * pct).round_dp(2),
                        (item.mean_net_return * pct).round_dp(2),
                        (item.mean_mfe * pct).round_dp(2),
                        (item.mean_mae * pct).round_dp(2),
                        (item.follow_through_rate * pct).round_dp(0),
                        (item.structure_retention_rate * pct).round_dp(0),
                        (item.invalidation_rate * pct).round_dp(0),
                    );
                }
                if let Some(item) = lineage_stats.blocked_outcomes.first() {
                    println!(
                        "  best_blocked     {}  resolved={}  hit_rate={:.0}%  gross={:+.2}%  net={:+.2}%  mfe={:+.2}%  mae={:+.2}%  follow={:.0}%  retain={:.0}%  invalid={:.0}%",
                        item.label,
                        item.resolved,
                        (item.hit_rate * pct).round_dp(0),
                        (item.mean_return * pct).round_dp(2),
                        (item.mean_net_return * pct).round_dp(2),
                        (item.mean_mfe * pct).round_dp(2),
                        (item.mean_mae * pct).round_dp(2),
                        (item.follow_through_rate * pct).round_dp(0),
                        (item.structure_retention_rate * pct).round_dp(0),
                        (item.invalidation_rate * pct).round_dp(0),
                    );
                }
                if let Some(item) = lineage_stats.falsified_outcomes.first() {
                    println!(
                        "  best_falsified   {}  resolved={}  hit_rate={:.0}%  gross={:+.2}%  net={:+.2}%  mfe={:+.2}%  mae={:+.2}%  follow={:.0}%  retain={:.0}%  invalid={:.0}%",
                        item.label,
                        item.resolved,
                        (item.hit_rate * pct).round_dp(0),
                        (item.mean_return * pct).round_dp(2),
                        (item.mean_net_return * pct).round_dp(2),
                        (item.mean_mfe * pct).round_dp(2),
                        (item.mean_mae * pct).round_dp(2),
                        (item.follow_through_rate * pct).round_dp(0),
                        (item.structure_retention_rate * pct).round_dp(0),
                        (item.invalidation_rate * pct).round_dp(0),
                    );
                }
                if let Some(item) = lineage_stats.promoted_contexts.first() {
                    println!(
                        "  ctx_promoted     {}  family={}  session={}  regime={}  net={:+.2}%  mfe={:+.2}%  mae={:+.2}%  follow={:.0}%  retain={:.0}%  invalid={:.0}%",
                        item.label,
                        item.family,
                        item.session,
                        item.market_regime,
                        (item.mean_net_return * pct).round_dp(2),
                        (item.mean_mfe * pct).round_dp(2),
                        (item.mean_mae * pct).round_dp(2),
                        (item.follow_through_rate * pct).round_dp(0),
                        (item.structure_retention_rate * pct).round_dp(0),
                        (item.invalidation_rate * pct).round_dp(0),
                    );
                }
                if let Some(item) = lineage_stats.blocked_contexts.first() {
                    println!(
                        "  ctx_blocked      {}  family={}  session={}  regime={}  net={:+.2}%  mfe={:+.2}%  mae={:+.2}%  follow={:.0}%  retain={:.0}%  invalid={:.0}%",
                        item.label,
                        item.family,
                        item.session,
                        item.market_regime,
                        (item.mean_net_return * pct).round_dp(2),
                        (item.mean_mfe * pct).round_dp(2),
                        (item.mean_mae * pct).round_dp(2),
                        (item.follow_through_rate * pct).round_dp(0),
                        (item.structure_retention_rate * pct).round_dp(0),
                        (item.invalidation_rate * pct).round_dp(0),
                    );
                }
                if let Some(item) = lineage_stats.falsified_contexts.first() {
                    println!(
                        "  ctx_falsified    {}  family={}  session={}  regime={}  net={:+.2}%  mfe={:+.2}%  mae={:+.2}%  follow={:.0}%  retain={:.0}%  invalid={:.0}%",
                        item.label,
                        item.family,
                        item.session,
                        item.market_regime,
                        (item.mean_net_return * pct).round_dp(2),
                        (item.mean_mfe * pct).round_dp(2),
                        (item.mean_mae * pct).round_dp(2),
                        (item.follow_through_rate * pct).round_dp(0),
                        (item.structure_retention_rate * pct).round_dp(0),
                        (item.invalidation_rate * pct).round_dp(0),
                    );
                }
            }
            if !world_snapshots.backward_reasoning.investigations.is_empty() {
                println!("\n── Backward Reasoning ──");
                for investigation in world_snapshots
                    .backward_reasoning
                    .investigations
                    .iter()
                    .take(5)
                {
                    println!(
                        "  {}  regime={}  contest={}  streak={}  prev_lead={}",
                        investigation.leaf_label,
                        investigation.leaf_regime,
                        investigation.contest_state,
                        investigation.leading_cause_streak,
                        investigation
                            .previous_leading_cause_id
                            .as_deref()
                            .unwrap_or("none"),
                    );
                    if let Some(summary) = &investigation.leader_transition_summary {
                        println!("                transition={}", summary);
                    }
                    if let Some(leading) = &investigation.leading_cause {
                        println!(
                            "                leading[{}|d{}]  score={:+}  net={:+}  support={:+}  against={:+}  conf={:+}  {}",
                            leading.layer,
                            leading.depth,
                            leading.competitive_score.round_dp(3),
                            leading.net_conviction.round_dp(3),
                            leading.support_weight.round_dp(3),
                            leading.contradict_weight.round_dp(3),
                            leading.confidence.round_dp(3),
                            leading.explanation,
                        );
                        if let Some(chain) = &leading.chain_summary {
                            println!("                lead_chain={}", chain);
                        }
                        println!(
                            "                lead_provenance={:?}  trace={}  inputs={}",
                            leading.provenance.source,
                            leading.provenance.trace_id.as_deref().unwrap_or("-"),
                            leading
                                .provenance
                                .inputs
                                .iter()
                                .take(3)
                                .cloned()
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                        for item in leading.supporting_evidence.iter().take(2) {
                            println!(
                                "                lead_support[{}]={:+}  {}",
                                item.channel,
                                item.weight.round_dp(3),
                                item.statement,
                            );
                        }
                        for item in leading.contradicting_evidence.iter().take(2) {
                            println!(
                                "                lead_against[{}]={:+}  {}",
                                item.channel,
                                item.weight.round_dp(3),
                                item.statement,
                            );
                        }
                    }
                    if investigation.leading_support_delta.is_some()
                        || investigation.leading_contradict_delta.is_some()
                    {
                        println!(
                            "                lead_deltas  d_support={:+}  d_against={:+}",
                            investigation
                                .leading_support_delta
                                .unwrap_or(Decimal::ZERO)
                                .round_dp(3),
                            investigation
                                .leading_contradict_delta
                                .unwrap_or(Decimal::ZERO)
                                .round_dp(3),
                        );
                    }
                    if let Some(runner_up) = &investigation.runner_up_cause {
                        println!(
                            "                runner_up[{}|d{}]  score={:+}  net={:+}  support={:+}  against={:+}  conf={:+}  gap={:+}  {}",
                            runner_up.layer,
                            runner_up.depth,
                            runner_up.competitive_score.round_dp(3),
                            runner_up.net_conviction.round_dp(3),
                            runner_up.support_weight.round_dp(3),
                            runner_up.contradict_weight.round_dp(3),
                            runner_up.confidence.round_dp(3),
                            investigation.cause_gap.unwrap_or(Decimal::ZERO).round_dp(3),
                            runner_up.explanation,
                        );
                        if let Some(chain) = &runner_up.chain_summary {
                            println!("                runner_chain={}", chain);
                        }
                        println!(
                            "                runner_provenance={:?}  trace={}  inputs={}",
                            runner_up.provenance.source,
                            runner_up.provenance.trace_id.as_deref().unwrap_or("-"),
                            runner_up
                                .provenance
                                .inputs
                                .iter()
                                .take(3)
                                .cloned()
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                        for item in runner_up.supporting_evidence.iter().take(2) {
                            println!(
                                "                runner_support[{}]={:+}  {}",
                                item.channel,
                                item.weight.round_dp(3),
                                item.statement,
                            );
                        }
                        for item in runner_up.contradicting_evidence.iter().take(2) {
                            println!(
                                "                runner_against[{}]={:+}  {}",
                                item.channel,
                                item.weight.round_dp(3),
                                item.statement,
                            );
                        }
                    }
                    if let Some(falsifier) = &investigation.leading_falsifier {
                        println!("                falsify_lead={}", falsifier);
                    }
                    for cause in investigation.candidate_causes.iter().skip(2).take(2) {
                        println!(
                            "                alt[{}|d{}]  score={:+}  conf={:+}  {}",
                            cause.layer,
                            cause.depth,
                            cause.competitive_score.round_dp(3),
                            cause.confidence.round_dp(3),
                            cause.explanation,
                        );
                        if let Some(chain) = &cause.chain_summary {
                            println!("                chain={}", chain);
                        }
                    }
                }
            }
            if !causal_timelines.is_empty() {
                println!("\n── Causal Memory ──");
                let mut timelines = causal_timelines.values().collect::<Vec<_>>();
                timelines.sort_by(|a, b| {
                    let a_flips = a.flip_events.len();
                    let b_flips = b.flip_events.len();
                    b_flips
                        .cmp(&a_flips)
                        .then_with(|| a.leaf_label.cmp(&b.leaf_label))
                });
                for timeline in timelines.iter().take(5) {
                    let sequence = timeline.recent_leader_sequence(4);
                    println!(
                        "  {}  scope={}  flips={}  latest_style={}",
                        timeline.leaf_label,
                        timeline.leaf_scope_key,
                        timeline.flip_events.len(),
                        timeline
                            .latest_flip_style()
                            .map(|style| style.to_string())
                            .unwrap_or_else(|| "none".into()),
                    );
                    if !sequence.is_empty() {
                        println!("                leaders={}", sequence.join(" -> "));
                    }
                    if let Some(latest_point) = timeline.latest_point() {
                        println!(
                            "                latest_state={}  latest_gap={:+}",
                            latest_point.contest_state,
                            latest_point.cause_gap.unwrap_or(Decimal::ZERO).round_dp(3),
                        );
                    }
                    if let Some(flip) = timeline.latest_flip() {
                        println!(
                            "                last_flip#{}  {} -> {}  style={}  gap={:+}",
                            flip.tick_number,
                            flip.from_explanation,
                            flip.to_explanation,
                            flip.style,
                            flip.cause_gap.unwrap_or(Decimal::ZERO).round_dp(3),
                        );
                        println!("                flip_why={}", flip.summary);
                    }
                }
            }
        }

        // ── Display: Temporal Dynamics ──
        if !bootstrap_mode && history.len() >= 2 {
            let mut dyn_syms: Vec<_> = dynamics.iter().collect();
            dyn_syms.sort_by(|a, b| b.1.composite_delta.abs().cmp(&a.1.composite_delta.abs()));
            println!("\n── Signal Dynamics (biggest movers) ──");
            for (sym, d) in dyn_syms.iter().take(10) {
                let accel = if d.composite_acceleration > Decimal::ZERO {
                    "accelerating"
                } else if d.composite_acceleration < Decimal::ZERO {
                    "decelerating"
                } else {
                    "steady"
                };
                println!(
                    "  {:>8}  delta={:>+7}%  conv={:>+7}%  {}  duration={} ticks  inst_delta={:>+7}%  bid_wall={:>+6}%  ask_wall={:>+6}%  buy_ratio={:>5}%",
                    sym,
                    (d.composite_delta * pct).round_dp(1),
                    (d.convergence_delta * pct).round_dp(1),
                    accel,
                    d.composite_duration,
                    (d.inst_alignment_delta * pct).round_dp(1),
                    (d.bid_wall_delta * pct).round_dp(1),
                    (d.ask_wall_delta * pct).round_dp(1),
                    (d.buy_ratio_trend * pct).round_dp(0),
                );
            }
        }

        if !bootstrap_mode && !polymarket_dynamics.is_empty() {
            println!("\n── Polymarket Dynamics ──");
            for prior in polymarket_dynamics.iter().take(6) {
                let accel = if prior.probability_acceleration > Decimal::ZERO {
                    "accelerating"
                } else if prior.probability_acceleration < Decimal::ZERO {
                    "decelerating"
                } else {
                    "steady"
                };
                println!(
                    "  {}  prob={:.0}%  delta={:+.0}%  {}",
                    prior.label,
                    (prior.current_probability * pct).round_dp(0),
                    (prior.probability_delta * pct).round_dp(0),
                    accel,
                );
            }
        }

        // ── Display: Order Suggestions ──
        if !bootstrap_mode && !actionable_order_suggestions.is_empty() {
            println!("\n── Order Suggestions ──");
            for s in &actionable_order_suggestions {
                let dir = match s.direction {
                    OrderDirection::Buy => "BUY ",
                    OrderDirection::Sell => "SELL",
                };
                let tag = if new_set.contains(&s.symbol) {
                    " [NEW]"
                } else {
                    ""
                };
                let confirm_tag = if s.requires_confirmation {
                    " [confirm]"
                } else {
                    ""
                };
                println!(
                    "  {:>8}  {}  qty={}  price=[{} - {}]  composite={:>+7}%  conv={:>+7}%{}{}",
                    s.symbol,
                    dir,
                    s.suggested_quantity,
                    s.price_low
                        .map(|p| p.to_string())
                        .unwrap_or_else(|| "?".into()),
                    s.price_high
                        .map(|p| p.to_string())
                        .unwrap_or_else(|| "?".into()),
                    (s.convergence.composite * pct).round_dp(1),
                    (s.convergence_score * pct).round_dp(1),
                    tag,
                    confirm_tag,
                );
                if let Some(reason) = &s.external_confirmation {
                    println!("                external_confirm={}", reason);
                }
                if let Some(reason) = &s.external_conflict {
                    println!("                external_conflict={}", reason);
                }
            }
        }

        // ── Signal Validation: record + resolve ──
        if !bootstrap_mode {
            // Build current price map from quotes
            let price_map: HashMap<Symbol, Decimal> = links
                .quotes
                .iter()
                .filter(|q| q.last_done > Decimal::ZERO)
                .map(|q| (q.symbol.clone(), q.last_done))
                .collect();

            // Record order suggestions
            for s in &actionable_order_suggestions {
                let signal_type = match s.direction {
                    OrderDirection::Buy => SignalType::OrderBuy,
                    OrderDirection::Sell => SignalType::OrderSell,
                };
                let price = price_map.get(&s.symbol).copied().unwrap_or(Decimal::ZERO);
                scorecard.record(
                    tick,
                    s.symbol.clone(),
                    signal_type,
                    s.convergence.composite,
                    price,
                );
            }

            // Record strong pressure signals (top 3 by magnitude)
            for p in graph_insights
                .pressures
                .iter()
                .filter(|p| readiness.ready_symbols.contains(&p.symbol))
                .take(3)
            {
                let (signal_type, strength) = if p.net_pressure > Decimal::ZERO {
                    (SignalType::PressureBullish, p.net_pressure)
                } else {
                    (SignalType::PressureBearish, p.net_pressure)
                };
                let price = price_map.get(&p.symbol).copied().unwrap_or(Decimal::ZERO);
                scorecard.record(tick, p.symbol.clone(), signal_type, strength, price);
            }

            // Resolve past signals
            scorecard.resolve(tick, &price_map);
        }

        // ── Display: Signal Scorecard ──
        if !bootstrap_mode {
            let stats = scorecard.stats();
            if !stats.is_empty() {
                println!("\n── Signal Scorecard ──");
                for s in &stats {
                    println!(
                        "  {:>6}  total={}  resolved={}  hits={}  hit_rate={}%  mean_return={:+}%",
                        s.signal_type,
                        s.total,
                        s.resolved,
                        s.hits,
                        (s.hit_rate * pct).round_dp(0),
                        (s.mean_return * pct).round_dp(2),
                    );
                }
                println!(
                    "  pending={} / {}",
                    scorecard.pending_count(),
                    scorecard.total_count(),
                );
            }
        }

        // ── Display: Structural Degradation ──
        if !bootstrap_mode && !aged_degradations.is_empty() {
            println!("\n── Structural Degradation (active positions) ──");
            let mut deg_syms: Vec<_> = aged_degradations.iter().collect();
            deg_syms.sort_by(|a, b| b.1.composite_degradation.cmp(&a.1.composite_degradation));
            for (sym, d) in &deg_syms {
                println!(
                    "  {:>8}  degradation={:>+7}%  inst_retain={:>+7}%  sector_chg={:>+7}%  corr_retain={:>+7}%  dim_drift={:>+7}%",
                    sym,
                    (d.composite_degradation * pct).round_dp(1),
                    (d.institution_retention * pct).round_dp(1),
                    (d.sector_coherence_change * pct).round_dp(1),
                    (d.correlation_retention * pct).round_dp(1),
                    (d.dimension_drift * pct).round_dp(1),
                );
            }
        }

        // ── Display: Trade Activity ──
        if !trade_symbols.is_empty() {
            println!("\n── Trade Ticks ──");
            let mut sorted = trade_symbols;
            sorted.sort_by(|a, b| b.2.cmp(&a.2));
            for (sym, count, vol) in sorted.iter().take(10) {
                // Find buy/sell breakdown from links
                if let Some(ta) = links.trade_activities.iter().find(|t| &t.symbol == sym) {
                    let buy_pct = if ta.total_volume > 0 {
                        ta.buy_volume as f64 / ta.total_volume as f64 * 100.0
                    } else {
                        0.0
                    };
                    println!(
                        "  {:>8}  {} ticks  vol={}  buy={:.0}%  vwap={}",
                        sym,
                        count,
                        vol,
                        buy_pct,
                        ta.vwap.round_dp(3),
                    );
                }
            }
        }

        // ── Display: Recent Candlesticks ──
        let mut candle_syms: Vec<_> = live
            .candlesticks
            .iter()
            .filter_map(|(sym, candles)| {
                let latest = candles.last()?;
                let range = latest.high - latest.low;
                Some((sym, candles.len(), latest.close, range, latest.volume))
            })
            .collect();
        candle_syms.sort_by(|a, b| b.4.cmp(&a.4));
        if !candle_syms.is_empty() {
            println!("\n── 1-Min Candles ──");
            for (sym, count, close, range, vol) in candle_syms.iter().take(10) {
                println!(
                    "  {:>8}  close={}  range={}  vol={}  ({} candles buffered)",
                    sym, close, range, vol, count,
                );
            }
        }

        // ── Display: Depth Profile ──
        let mut profiles: Vec<_> = links
            .order_books
            .iter()
            .filter(|ob| ob.bid_profile.active_levels > 0 || ob.ask_profile.active_levels > 0)
            .collect();
        profiles.sort_by(|a, b| {
            let a_imbal = (a.bid_profile.top3_volume_ratio - a.ask_profile.top3_volume_ratio).abs();
            let b_imbal = (b.bid_profile.top3_volume_ratio - b.ask_profile.top3_volume_ratio).abs();
            b_imbal.cmp(&a_imbal)
        });
        if !profiles.is_empty() {
            println!("\n── Depth Profile (top asymmetry) ──");
            for ob in profiles.iter().take(10) {
                println!(
                    "  {:>8}  bid[top3={:>5}% best={:>5}% lvls={}]  ask[top3={:>5}% best={:>5}% lvls={}]  spread={:?}",
                    ob.symbol,
                    (ob.bid_profile.top3_volume_ratio * pct).round_dp(1),
                    (ob.bid_profile.best_level_ratio * pct).round_dp(1),
                    ob.bid_profile.active_levels,
                    (ob.ask_profile.top3_volume_ratio * pct).round_dp(1),
                    (ob.ask_profile.best_level_ratio * pct).round_dp(1),
                    ob.ask_profile.active_levels,
                    ob.spread,
                );
            }
        }

        // ── Summary ──
        println!(
            "\n  Tracked: {} | New: {} | History: {}/{} ticks | Data: {} depths, {} brokers, {} quotes",
            tracker.active_count(),
            newly_entered.len(),
            history.len(),
            300,
            live.depths.len(),
            live.brokers.len(),
            live.quotes.len(),
        );
        println!();

        prev_insights = Some(graph_insights);
    }
}
