#[derive(Debug, Clone)]
pub struct CrossMarketPair {
    pub us_symbol: &'static str,
    pub hk_symbol: &'static str,
    pub name: &'static str,
}

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
    // Added 2026-04-07: more dual-listed China ADRs
    CrossMarketPair {
        us_symbol: "NTES.US",
        hk_symbol: "9999.HK",
        name: "NetEase",
    },
    // PDD not yet dual-listed in HK as of 2026-04, skipped
    CrossMarketPair {
        us_symbol: "BGNE.US",
        hk_symbol: "6160.HK",
        name: "BeiGene",
    },
    CrossMarketPair {
        us_symbol: "TME.US",
        hk_symbol: "1698.HK",
        name: "Tencent Music", // 1698.HK is the correct HK listing
    },
    CrossMarketPair {
        us_symbol: "EDU.US",
        hk_symbol: "9901.HK",
        name: "New Oriental",
    },
    CrossMarketPair {
        us_symbol: "MNSO.US",
        hk_symbol: "9896.HK",
        name: "MINISO",
    },
];
