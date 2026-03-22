/// US stock watchlist and cross-market HK<->US mappings for dual-listed stocks.

/// Symbols use the Longport `.US` suffix convention.
pub const US_WATCHLIST: &[&str] = &[
    // ═══════════════════════════════════════════════════
    // ── Mega-cap Tech (FAANG+) ──
    // ═══════════════════════════════════════════════════
    "AAPL.US",  // Apple
    "MSFT.US",  // Microsoft
    "GOOGL.US", // Alphabet
    "AMZN.US",  // Amazon
    "META.US",  // Meta Platforms
    "NFLX.US",  // Netflix
    "CRM.US",   // Salesforce
    "ORCL.US",  // Oracle
    "ADBE.US",  // Adobe
    "NOW.US",   // ServiceNow
    "INTU.US",  // Intuit
    "SHOP.US",  // Shopify
    "SNOW.US",  // Snowflake
    "PLTR.US",  // Palantir
    "UBER.US",  // Uber
    "ABNB.US",  // Airbnb
    "DASH.US",  // DoorDash
    "SQ.US",    // Block (Square)
    "PYPL.US",  // PayPal
    "COIN.US",  // Coinbase
    "DDOG.US",  // Datadog
    "CRWD.US",  // CrowdStrike
    "ZS.US",    // Zscaler
    "NET.US",   // Cloudflare
    "MDB.US",   // MongoDB
    "PANW.US",  // Palo Alto Networks
    "WDAY.US",  // Workday
    "TEAM.US",  // Atlassian
    "HUBS.US",  // HubSpot
    "VEEV.US",  // Veeva Systems
    "DKNG.US",  // DraftKings
    "ROKU.US",  // Roku
    "TTD.US",   // The Trade Desk
    "SNAP.US",  // Snap
    "PINS.US",  // Pinterest
    "SPOT.US",  // Spotify
    "RBLX.US",  // Roblox
    "U.US",     // Unity Software
    "ZM.US",    // Zoom Video
    "DOCU.US",  // DocuSign
    "TWLO.US",  // Twilio
    "OKTA.US",  // Okta
    "MNDY.US",  // monday.com
    "PATH.US",  // UiPath
    "APP.US",   // AppLovin
    "BILL.US",  // Bill Holdings
    "GTLB.US",  // GitLab
    "IOT.US",   // Samsara
    // ═══════════════════════════════════════════════════
    // ── Semiconductors ──
    // ═══════════════════════════════════════════════════
    "NVDA.US", // NVIDIA
    "AMD.US",  // Advanced Micro Devices
    "AVGO.US", // Broadcom
    "QCOM.US", // Qualcomm
    "INTC.US", // Intel
    "MU.US",   // Micron Technology
    "AMAT.US", // Applied Materials
    "LRCX.US", // Lam Research
    "KLAC.US", // KLA Corporation
    "TSM.US",  // TSMC (ADR)
    "TXN.US",  // Texas Instruments
    "ADI.US",  // Analog Devices
    "MRVL.US", // Marvell Technology
    "ON.US",   // ON Semiconductor
    "NXPI.US", // NXP Semiconductors
    "MCHP.US", // Microchip Technology
    "SWKS.US", // Skyworks Solutions
    "MPWR.US", // Monolithic Power
    "ARM.US",  // Arm Holdings
    "ASML.US", // ASML (ADR)
    "SNPS.US", // Synopsys
    "CDNS.US", // Cadence Design
    "GFS.US",  // GlobalFoundries
    // ═══════════════════════════════════════════════════
    // ── China ADRs ──
    // ═══════════════════════════════════════════════════
    "BABA.US", // Alibaba
    "BIDU.US", // Baidu
    "NIO.US",  // NIO
    "XPEV.US", // XPeng
    "LI.US",   // Li Auto
    "PDD.US",  // PDD Holdings (Temu)
    "JD.US",   // JD.com
    "TCOM.US", // Trip.com
    "ZTO.US",  // ZTO Express
    "BILI.US", // Bilibili
    "NTES.US", // NetEase
    "TME.US",  // Tencent Music
    "WB.US",   // Weibo
    "IQ.US",   // iQIYI
    "VNET.US", // VNET Group
    "FUTU.US", // Futu Holdings
    "TIGR.US", // UP Fintech (Tiger)
    "MNSO.US", // Miniso
    "TAL.US",  // TAL Education
    "EDU.US",  // New Oriental Education
    "HTHT.US", // H World Group (Huazhu)
    "YMM.US",  // Full Truck Alliance
    "QFIN.US", // 360 Finance (Qifu)
    // "DADA.US",  // Dada Nexus — delisted
    "LX.US",   // LexinFintech
    "FINV.US", // FinVolution
    "GDS.US",  // GDS Holdings
    "KC.US",   // Kingsoft Cloud
    "ZLAB.US", // Zai Lab
    "BGNE.US", // BeiGene
    "LEGN.US", // Legend Biotech
    // ═══════════════════════════════════════════════════
    // ── EV & Autonomous ──
    // ═══════════════════════════════════════════════════
    "TSLA.US", // Tesla
    "RIVN.US", // Rivian
    "LCID.US", // Lucid Group
    "GM.US",   // General Motors
    "F.US",    // Ford Motor
    "TM.US",   // Toyota (ADR)
    "STLA.US", // Stellantis
    // ═══════════════════════════════════════════════════
    // ── Financials: Banks & Brokerages ──
    // ═══════════════════════════════════════════════════
    "JPM.US",  // JPMorgan Chase
    "GS.US",   // Goldman Sachs
    "MS.US",   // Morgan Stanley
    "BAC.US",  // Bank of America
    "WFC.US",  // Wells Fargo
    "C.US",    // Citigroup
    "USB.US",  // US Bancorp
    "PNC.US",  // PNC Financial
    "SCHW.US", // Charles Schwab
    "BK.US",   // Bank of New York Mellon
    "TFC.US",  // Truist Financial
    "COF.US",  // Capital One
    "AXP.US",  // American Express
    "BLK.US",  // BlackRock
    "ICE.US",  // Intercontinental Exchange
    "CME.US",  // CME Group
    "SPGI.US", // S&P Global
    "MCO.US",  // Moody's
    "MSCI.US", // MSCI
    "FIS.US",  // Fidelity National
    "MA.US",   // Mastercard
    "V.US",    // Visa
    "HOOD.US", // Robinhood
    "SOFI.US", // SoFi Technologies
    // ═══════════════════════════════════════════════════
    // ── Insurance ──
    // ═══════════════════════════════════════════════════
    // "BRK-B.US", // Berkshire Hathaway B — invalid symbol (hyphen)
    "PGR.US", // Progressive
    "TRV.US", // Travelers
    "ALL.US", // Allstate
    "MET.US", // MetLife
    "AIG.US", // AIG
    "AFL.US", // Aflac
    // ═══════════════════════════════════════════════════
    // ── Healthcare: Pharma & Biotech ──
    // ═══════════════════════════════════════════════════
    "UNH.US",  // UnitedHealth
    "JNJ.US",  // Johnson & Johnson
    "LLY.US",  // Eli Lilly
    "ABBV.US", // AbbVie
    "PFE.US",  // Pfizer
    "MRK.US",  // Merck
    "TMO.US",  // Thermo Fisher
    "ABT.US",  // Abbott Labs
    "DHR.US",  // Danaher
    "BMY.US",  // Bristol-Myers Squibb
    "AMGN.US", // Amgen
    "GILD.US", // Gilead Sciences
    "VRTX.US", // Vertex Pharmaceuticals
    "REGN.US", // Regeneron
    "ISRG.US", // Intuitive Surgical
    "SYK.US",  // Stryker
    "BSX.US",  // Boston Scientific
    "MDT.US",  // Medtronic
    "ZTS.US",  // Zoetis
    "EW.US",   // Edwards Lifesciences
    "A.US",    // Agilent Technologies
    "DXCM.US", // DexCom
    "IDXX.US", // IDEXX Laboratories
    "MRNA.US", // Moderna
    "BIIB.US", // Biogen
    "ALNY.US", // Alnylam Pharmaceuticals
    // "SGEN.US",  // Seagen — acquired by Pfizer, delisted
    "ILMN.US", // Illumina
    "CVS.US",  // CVS Health
    "CI.US",   // Cigna Group
    "ELV.US",  // Elevance Health
    "HCA.US",  // HCA Healthcare
    "HUM.US",  // Humana
    "MCK.US",  // McKesson
    // ═══════════════════════════════════════════════════
    // ── Consumer Discretionary: Retail ──
    // ═══════════════════════════════════════════════════
    "WMT.US",  // Walmart
    "COST.US", // Costco
    "TGT.US",  // Target
    "HD.US",   // Home Depot
    "LOW.US",  // Lowe's
    "TJX.US",  // TJX Companies
    "ROST.US", // Ross Stores
    "DG.US",   // Dollar General
    "DLTR.US", // Dollar Tree
    "NKE.US",  // Nike
    "LULU.US", // Lululemon
    "SBUX.US", // Starbucks
    "MCD.US",  // McDonald's
    "CMG.US",  // Chipotle
    "YUM.US",  // Yum! Brands
    "DPZ.US",  // Domino's Pizza
    "BKNG.US", // Booking Holdings
    "MAR.US",  // Marriott
    "HLT.US",  // Hilton
    "RCL.US",  // Royal Caribbean
    "LVS.US",  // Las Vegas Sands
    "WYNN.US", // Wynn Resorts
    // ═══════════════════════════════════════════════════
    // ── Consumer Staples ──
    // ═══════════════════════════════════════════════════
    "PG.US",   // Procter & Gamble
    "KO.US",   // Coca-Cola
    "PEP.US",  // PepsiCo
    "PM.US",   // Philip Morris
    "MO.US",   // Altria
    "MDLZ.US", // Mondelez
    "CL.US",   // Colgate-Palmolive
    "KMB.US",  // Kimberly-Clark
    "GIS.US",  // General Mills
    "K.US",    // Kellanova
    "HSY.US",  // Hershey
    "STZ.US",  // Constellation Brands
    "KHC.US",  // Kraft Heinz
    "SYY.US",  // Sysco
    "KDP.US",  // Keurig Dr Pepper
    // ═══════════════════════════════════════════════════
    // ── Energy ──
    // ═══════════════════════════════════════════════════
    "XOM.US", // Exxon Mobil
    "CVX.US", // Chevron
    "COP.US", // ConocoPhillips
    "SLB.US", // Schlumberger
    "EOG.US", // EOG Resources
    "MPC.US", // Marathon Petroleum
    "PSX.US", // Phillips 66
    "VLO.US", // Valero Energy
    "OXY.US", // Occidental Petroleum
    "HAL.US", // Halliburton
    "DVN.US", // Devon Energy
    // "PXD.US",   // Pioneer Natural Resources — acquired by Exxon, delisted
    "FANG.US", // Diamondback Energy
    "KMI.US",  // Kinder Morgan
    "WMB.US",  // Williams Companies
    "OKE.US",  // ONEOK
    "LNG.US",  // Cheniere Energy
    // ═══════════════════════════════════════════════════
    // ── Industrials ──
    // ═══════════════════════════════════════════════════
    "CAT.US",  // Caterpillar
    "DE.US",   // Deere & Company
    "BA.US",   // Boeing
    "RTX.US",  // RTX (Raytheon)
    "LMT.US",  // Lockheed Martin
    "GE.US",   // GE Aerospace
    "HON.US",  // Honeywell
    "UNP.US",  // Union Pacific
    "UPS.US",  // United Parcel Service
    "FDX.US",  // FedEx
    "MMM.US",  // 3M
    "GD.US",   // General Dynamics
    "NOC.US",  // Northrop Grumman
    "LHX.US",  // L3Harris
    "EMR.US",  // Emerson Electric
    "ETN.US",  // Eaton
    "ITW.US",  // Illinois Tool Works
    "ROK.US",  // Rockwell Automation
    "CARR.US", // Carrier Global
    "WM.US",   // Waste Management
    "RSG.US",  // Republic Services
    "CSX.US",  // CSX
    "NSC.US",  // Norfolk Southern
    "DAL.US",  // Delta Air Lines
    "UAL.US",  // United Airlines
    "AAL.US",  // American Airlines
    "LUV.US",  // Southwest Airlines
    // ═══════════════════════════════════════════════════
    // ── Utilities ──
    // ═══════════════════════════════════════════════════
    "NEE.US", // NextEra Energy
    "DUK.US", // Duke Energy
    "SO.US",  // Southern Company
    "D.US",   // Dominion Energy
    "AEP.US", // American Electric Power
    "SRE.US", // Sempra
    "EXC.US", // Exelon
    "XEL.US", // Xcel Energy
    "PCG.US", // PG&E
    "ED.US",  // Consolidated Edison
    "CEG.US", // Constellation Energy
    "VST.US", // Vistra
    // ═══════════════════════════════════════════════════
    // ── Real Estate ──
    // ═══════════════════════════════════════════════════
    "PLD.US",  // Prologis
    "AMT.US",  // American Tower
    "CCI.US",  // Crown Castle
    "EQIX.US", // Equinix
    "SPG.US",  // Simon Property Group
    "PSA.US",  // Public Storage
    "O.US",    // Realty Income
    "DLR.US",  // Digital Realty
    "WELL.US", // Welltower
    "AVB.US",  // AvalonBay
    // ═══════════════════════════════════════════════════
    // ── Materials ──
    // ═══════════════════════════════════════════════════
    "LIN.US", // Linde
    "SHW.US", // Sherwin-Williams
    "APD.US", // Air Products
    "ECL.US", // Ecolab
    "FCX.US", // Freeport-McMoRan
    "NEM.US", // Newmont
    "NUE.US", // Nucor
    "DOW.US", // Dow Inc
    "DD.US",  // DuPont
    "VMC.US", // Vulcan Materials
    "MLM.US", // Martin Marietta
    // ═══════════════════════════════════════════════════
    // ── Telecom & Media ──
    // ═══════════════════════════════════════════════════
    "T.US",     // AT&T
    "VZ.US",    // Verizon
    "TMUS.US",  // T-Mobile
    "CMCSA.US", // Comcast
    "DIS.US",   // Walt Disney
    "WBD.US",   // Warner Bros Discovery
    // "PARA.US",  // Paramount — merged with Skydance, delisted
    "CHTR.US", // Charter Communications
    "EA.US",   // Electronic Arts
    "TTWO.US", // Take-Two Interactive
    // "ATVI.US",  // Activision Blizzard — acquired by Microsoft, delisted
    // ═══════════════════════════════════════════════════
    // ── Crypto-related ──
    // ═══════════════════════════════════════════════════
    "MSTR.US", // MicroStrategy
    "MARA.US", // Marathon Digital
    "RIOT.US", // Riot Platforms
    "BITF.US", // Bitfarms
    "CLSK.US", // CleanSpark
    // ═══════════════════════════════════════════════════
    // ── Meme / Momentum / Popular ──
    // ═══════════════════════════════════════════════════
    "GME.US",  // GameStop
    "AMC.US",  // AMC Entertainment
    "BB.US",   // BlackBerry
    "CLOV.US", // Clover Health
    "IONQ.US", // IonQ (Quantum)
    "SMCI.US", // Super Micro Computer
    "AI.US",   // C3.ai
    "SOUN.US", // SoundHound AI
    // ═══════════════════════════════════════════════════
    // ── Additional S&P 500 / Large-cap ──
    // ═══════════════════════════════════════════════════
    "ACGL.US", // Arch Capital Group
    "ADP.US",  // Automatic Data Processing
    "ADSK.US", // Autodesk
    "AJG.US",  // Arthur J. Gallagher
    "ANET.US", // Arista Networks
    "ANSS.US", // ANSYS
    "AON.US",  // Aon
    "AZO.US",  // AutoZone
    "BDX.US",  // Becton Dickinson
    "BR.US",   // Broadridge Financial
    "CBOE.US", // Cboe Global Markets
    "CBRE.US", // CBRE Group
    "CPRT.US", // Copart
    "CTSH.US", // Cognizant
    "DFS.US",  // Discover Financial
    "EBAY.US", // eBay
    "ENPH.US", // Enphase Energy
    "EQR.US",  // Equity Residential
    "FAST.US", // Fastenal
    "FICO.US", // Fair Isaac Corporation
    "FTNT.US", // Fortinet
    "GEV.US",  // GE Vernova
    "GPN.US",  // Global Payments
    "GRMN.US", // Garmin
    "HPQ.US",  // HP Inc
    "IT.US",   // Gartner
    "KEYS.US", // Keysight Technologies
    "LDOS.US", // Leidos Holdings
    "MKTX.US", // MarketAxess
    "NDAQ.US", // Nasdaq Inc
    "ODFL.US", // Old Dominion Freight
    "ORLY.US", // O'Reilly Automotive
    "OTIS.US", // Otis Worldwide
    "PAYC.US", // Paycom Software
    "PCAR.US", // PACCAR
    "PHM.US",  // PulteGroup
    "PKG.US",  // Packaging Corp of America
    "POOL.US", // Pool Corporation
    "RMD.US",  // ResMed
    "SBAC.US", // SBA Communications
    "TRGP.US", // Targa Resources
    "TSCO.US", // Tractor Supply
    "TT.US",   // Trane Technologies
    "VRSK.US", // Verisk Analytics
    "WST.US",  // West Pharmaceutical Services
    "ZBRA.US", // Zebra Technologies
    "ZBH.US",  // Zimmer Biomet
    "CDW.US",  // CDW Corporation
    "CSGP.US", // CoStar Group
    "DECK.US", // Deckers Outdoor
    "FSLR.US", // First Solar
    "GWW.US",  // W.W. Grainger
    "HUBB.US", // Hubbell
    "IRM.US",  // Iron Mountain
    "KR.US",   // Kroger
    "LPLA.US", // LPL Financial
    // ═══════════════════════════════════════════════════
    // ── ETFs: Macro ──
    // ═══════════════════════════════════════════════════
    "SPY.US", // S&P 500 ETF
    "QQQ.US", // Nasdaq-100 ETF
    "IWM.US", // Russell 2000 ETF
    "DIA.US", // Dow Jones ETF
    "TLT.US", // 20+ Year Treasury Bond ETF
    "GLD.US", // Gold ETF
    "SLV.US", // Silver ETF
    "USO.US", // United States Oil Fund
    "UNG.US", // US Natural Gas Fund
    "VXX.US", // VIX Short-Term Futures
    "HYG.US", // High Yield Corporate Bond
    "LQD.US", // Investment Grade Corporate Bond
    "EEM.US", // Emerging Markets ETF
    "FXI.US", // China Large-Cap ETF
    "EWJ.US", // Japan ETF
    "EFA.US", // EAFE (Developed ex-US) ETF
    // ═══════════════════════════════════════════════════
    // ── ETFs: Sector ──
    // ═══════════════════════════════════════════════════
    "XLF.US",  // Financial Select Sector
    "XLK.US",  // Technology Select Sector
    "XLE.US",  // Energy Select Sector
    "XLV.US",  // Health Care Select Sector
    "XLI.US",  // Industrial Select Sector
    "XLP.US",  // Consumer Staples Select Sector
    "XLU.US",  // Utilities Select Sector
    "XLY.US",  // Consumer Discretionary Select Sector
    "XLB.US",  // Materials Select Sector
    "XLRE.US", // Real Estate Select Sector
    "XLC.US",  // Communication Services Select Sector
    // ═══════════════════════════════════════════════════
    // ── ETFs: Thematic ──
    // ═══════════════════════════════════════════════════
    "ARKK.US", // ARK Innovation ETF
    "SOXX.US", // iShares Semiconductor ETF
    "SMH.US",  // VanEck Semiconductor ETF
    "KWEB.US", // KraneShares China Internet ETF
    "XBI.US",  // SPDR Biotech ETF
    "HACK.US", // ETFMG Prime Cyber Security ETF
    "BOTZ.US", // Global X Robotics & AI ETF
    "TAN.US",  // Invesco Solar ETF
    "ICLN.US", // iShares Global Clean Energy
    "IBIT.US", // iShares Bitcoin Trust
    "BITO.US", // ProShares Bitcoin Strategy
];

/// A dual-listed stock that trades on both US and HK exchanges.
/// Used to detect cross-market arbitrage signals and propagate sentiment.
#[derive(Debug, Clone)]
pub struct CrossMarketPair {
    pub us_symbol: &'static str,
    pub hk_symbol: &'static str,
    pub name: &'static str,
}

/// All known US<->HK dual-listed pairs.
/// Each US ADR maps to its corresponding HK secondary listing.
pub const CROSS_MARKET_PAIRS: &[CrossMarketPair] = &[
    CrossMarketPair {
        us_symbol: "BABA.US",
        hk_symbol: "9988.HK",
        name: "Alibaba",
    },
    CrossMarketPair {
        us_symbol: "BIDU.US",
        hk_symbol: "9888.HK",
        name: "Baidu",
    },
    CrossMarketPair {
        us_symbol: "NIO.US",
        hk_symbol: "9866.HK",
        name: "NIO",
    },
    CrossMarketPair {
        us_symbol: "XPEV.US",
        hk_symbol: "9868.HK",
        name: "XPeng",
    },
    CrossMarketPair {
        us_symbol: "LI.US",
        hk_symbol: "2015.HK",
        name: "Li Auto",
    },
    CrossMarketPair {
        us_symbol: "JD.US",
        hk_symbol: "9618.HK",
        name: "JD.com",
    },
    CrossMarketPair {
        us_symbol: "TCOM.US",
        hk_symbol: "9961.HK",
        name: "Trip.com",
    },
    CrossMarketPair {
        us_symbol: "ZTO.US",
        hk_symbol: "2057.HK",
        name: "ZTO Express",
    },
    CrossMarketPair {
        us_symbol: "BILI.US",
        hk_symbol: "9626.HK",
        name: "Bilibili",
    },
];

/// Look up the HK symbol for a given US symbol.
pub fn hk_counterpart(us_symbol: &str) -> Option<&'static str> {
    CROSS_MARKET_PAIRS
        .iter()
        .find(|p| p.us_symbol == us_symbol)
        .map(|p| p.hk_symbol)
}

/// Look up the US symbol for a given HK symbol.
pub fn us_counterpart(hk_symbol: &str) -> Option<&'static str> {
    CROSS_MARKET_PAIRS
        .iter()
        .find(|p| p.hk_symbol == hk_symbol)
        .map(|p| p.us_symbol)
}

/// Map a US symbol to its sector ID. Derived from the watchlist section headers.
pub fn us_symbol_sector(symbol: &str) -> Option<&'static str> {
    crate::ontology::store::us_symbol_sector(symbol)
}

pub fn us_sector_names() -> &'static [(&'static str, &'static str)] {
    crate::ontology::store::us_sector_names()
}

/// Returns true if this symbol is part of a cross-market pair.
pub fn is_dual_listed(symbol: &str) -> bool {
    CROSS_MARKET_PAIRS
        .iter()
        .any(|p| p.us_symbol == symbol || p.hk_symbol == symbol)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watchlist_has_expected_count() {
        // Update this when adding/removing symbols
        assert!(
            US_WATCHLIST.len() >= 350,
            "expected >= 350 symbols, got {}",
            US_WATCHLIST.len()
        );
    }

    #[test]
    fn all_us_symbols_have_us_suffix() {
        for sym in US_WATCHLIST {
            assert!(sym.ends_with(".US"), "{sym} missing .US suffix");
        }
    }

    #[test]
    fn no_duplicate_symbols_in_watchlist() {
        let mut seen = std::collections::HashSet::new();
        for sym in US_WATCHLIST {
            assert!(seen.insert(sym), "duplicate symbol: {sym}");
        }
    }

    #[test]
    fn cross_market_us_symbols_in_watchlist() {
        for pair in CROSS_MARKET_PAIRS {
            assert!(
                US_WATCHLIST.contains(&pair.us_symbol),
                "{} not in US_WATCHLIST",
                pair.us_symbol
            );
        }
    }

    #[test]
    fn cross_market_hk_symbols_have_hk_suffix() {
        for pair in CROSS_MARKET_PAIRS {
            assert!(
                pair.hk_symbol.ends_with(".HK"),
                "{} missing .HK suffix",
                pair.hk_symbol
            );
        }
    }

    #[test]
    fn hk_counterpart_lookup() {
        assert_eq!(hk_counterpart("BABA.US"), Some("9988.HK"));
        assert_eq!(hk_counterpart("JD.US"), Some("9618.HK"));
        assert_eq!(hk_counterpart("TSLA.US"), None);
    }

    #[test]
    fn us_counterpart_lookup() {
        assert_eq!(us_counterpart("9988.HK"), Some("BABA.US"));
        assert_eq!(us_counterpart("9868.HK"), Some("XPEV.US"));
        assert_eq!(us_counterpart("700.HK"), None);
    }

    #[test]
    fn is_dual_listed_check() {
        assert!(is_dual_listed("BABA.US"));
        assert!(is_dual_listed("9988.HK"));
        assert!(!is_dual_listed("AAPL.US"));
        assert!(!is_dual_listed("700.HK"));
    }

    #[test]
    fn no_duplicate_cross_market_pairs() {
        let mut us_seen = std::collections::HashSet::new();
        let mut hk_seen = std::collections::HashSet::new();
        for pair in CROSS_MARKET_PAIRS {
            assert!(
                us_seen.insert(pair.us_symbol),
                "duplicate US: {}",
                pair.us_symbol
            );
            assert!(
                hk_seen.insert(pair.hk_symbol),
                "duplicate HK: {}",
                pair.hk_symbol
            );
        }
    }

    #[test]
    fn pair_count() {
        assert_eq!(CROSS_MARKET_PAIRS.len(), 9);
    }
}
