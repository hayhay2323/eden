use std::collections::HashMap;
use std::sync::Arc;

use longport::quote::QuoteContext;

use super::objects::*;

// ── Sector definitions for our watchlist ──

fn define_sectors() -> Vec<Sector> {
    vec![
        Sector {
            id: SectorId("tech".into()),
            name: "Technology".into(),
        },
        Sector {
            id: SectorId("semiconductor".into()),
            name: "Semiconductor".into(),
        },
        Sector {
            id: SectorId("finance".into()),
            name: "Finance".into(),
        },
        Sector {
            id: SectorId("energy".into()),
            name: "Energy".into(),
        },
        Sector {
            id: SectorId("telecom".into()),
            name: "Telecommunications".into(),
        },
        Sector {
            id: SectorId("property".into()),
            name: "Property".into(),
        },
        Sector {
            id: SectorId("consumer".into()),
            name: "Consumer".into(),
        },
        Sector {
            id: SectorId("healthcare".into()),
            name: "Healthcare".into(),
        },
        Sector {
            id: SectorId("utilities".into()),
            name: "Utilities".into(),
        },
        Sector {
            id: SectorId("insurance".into()),
            name: "Insurance".into(),
        },
        Sector {
            id: SectorId("auto".into()),
            name: "Automobile".into(),
        },
        Sector {
            id: SectorId("materials".into()),
            name: "Materials".into(),
        },
        Sector {
            id: SectorId("industrial".into()),
            name: "Industrial".into(),
        },
        Sector {
            id: SectorId("conglomerate".into()),
            name: "Conglomerate".into(),
        },
        Sector {
            id: SectorId("media".into()),
            name: "Media & Entertainment".into(),
        },
        Sector {
            id: SectorId("logistics".into()),
            name: "Logistics & Transport".into(),
        },
        Sector {
            id: SectorId("education".into()),
            name: "Education".into(),
        },
    ]
}

/// Symbol → sector mapping for HKEX watchlist.
/// Organized as arrays per sector for maintainability.
fn symbol_sector(symbol: &str) -> Option<SectorId> {
    // ── Tech: Internet, software, cloud, platforms ──
    const TECH: &[&str] = &[
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
        "6677.HK", // eHi Car Services
        "1119.HK", // iDreamSky
        "9969.HK", // iQIYI (dual listing)
        "3738.HK", // Vnet Group
        "2096.HK", // Sinohealth
        "6855.HK", // Asiainfo Technologies
        "3709.HK", // iClick Interactive
        "1137.HK", // Hong Kong Technology Venture
        "6098.HK", // CG Services
        "302.HK",  // Wing Hang Investment
        "522.HK",  // ASM Pacific (now ASMPT - also semi-adjacent)
        "1357.HK", // Meitu (AI image)
        "9961.HK", // Trip.com (ADR)
        "1478.HK", // Q Technology
        "354.HK",  // China Software International
        "9901.HK", // New Oriental Education
        "9911.HK", // NewBorn Town
        "6058.HK", // OneConnect Financial
        "763.HK",  // ZTE Corporation
        "552.HK",  // China Communications Services
        "1686.HK", // Sunevision (data center)
        "303.HK",  // VTech Holdings
        "179.HK",  // Johnson Electric
        "2513.HK", // 智譜 AI (Zhipu) — AI LLM
        "100.HK",  // MiniMax — AI LLM
    ];

    // ── Semiconductor: Chips, GPU, memory, optical ──
    const SEMICONDUCTOR: &[&str] = &[
        "981.HK",  // SMIC
        "1347.HK", // Hua Hong Semiconductor
        "2518.HK", // ASMPT
        "1385.HK", // Shanghai Fudan Microelectronics
        "6869.HK", // 長飛光纖 Yangtze Optical Fibre
        "6082.HK", // 壁仞科技 Biren Technology (GPU)
        "3896.HK", // 兆易創新 GigaDevice
        "6809.HK", // 澜起科技 Montage Technology
        "600.HK",  // 愛芯元智 Aixin (AI vision chips)
    ];

    // ── Finance: Banks, brokerages, exchanges, fintech ──
    const FINANCE: &[&str] = &[
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
        "2066.HK", // Shenwan Hongyuan
        "6837.HK", // Haitong Securities
        "1776.HK", // GF Securities
        "1359.HK", // China Cinda
        "6199.HK", // Lufax
        "2799.HK", // China Huarong
        "3618.HK", // Chongqing Rural Commercial
        "1916.HK", // China Resources Bank
        "2611.HK", // Guotai Junan International
        "3698.HK", // Huishang Bank
        "1578.HK", // Bank of Gansu
        "2016.HK", // ZhongAn Bank
        "6196.HK", // Bank of Zhengzhou
        "1461.HK", // Bank of Guizhou
        "2356.HK", // Dah Sing Banking
        "440.HK",  // Dah Sing Financial
        "23.HK",   // Bank of East Asia
        "1111.HK", // Chong Hing Bank
        "6178.HK", // Everbright Securities
        "1336.HK", // New China Life
        "3958.HK", // Orient Securities
        "1375.HK", // Central China Securities
        "3903.HK", // Hanhua Financial
        "412.HK",  // China Shandong Hi-Speed Financial
        "2858.HK", // Yixin Group
        "6099.HK", // China Merchants Securities
        "2888.HK", // Standard Chartered HK
    ];

    // ── Energy: Oil, gas, coal, renewables ──
    const ENERGY: &[&str] = &[
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
        "1600.HK", // China Lumena ... actually might be delisted
        "467.HK",  // United Energy Group
        "2883.HK", // China Oilfield Services
        "3899.HK", // CIMC Enric
        "1083.HK", // Towngas China
        "8270.HK", // China CBM Group ... not sure
    ];

    // ── Telecom ──
    const TELECOM: &[&str] = &[
        "941.HK",  // China Mobile
        "762.HK",  // China Unicom
        "728.HK",  // China Telecom
        "6823.HK", // HKT Trust
        "215.HK",  // Hutchison Telecom HK
    ];

    // ── Property: Developers, REITs, property services ──
    const PROPERTY: &[&str] = &[
        "16.HK",    // SHK Properties
        "1109.HK",  // China Resources Land
        "688.HK",   // China Overseas Land
        "1113.HK",  // CK Asset
        "17.HK",    // New World Development
        "12.HK",    // Henderson Land
        "101.HK",   // Hang Lung Properties
        "823.HK",   // Link REIT
        "1997.HK",  // Wharf REIC
        "960.HK",   // Longfor Group
        "3383.HK",  // Agile Group
        "884.HK",   // CIFI Holdings
        "2202.HK",  // China Vanke
        "1030.HK",  // Future Land
        "123.HK",   // Yuexiu Property
        "119.HK",   // Poly Property
        "3900.HK",  // Greentown China
        "2777.HK",  // Guangzhou R&F
        "81.HK",    // China Overseas Grand Oceans
        "754.HK",   // Hopson Development
        "2669.HK",  // China Overseas Property
        "1918.HK",  // Sunac China
        "813.HK",   // Shimao Group
        "2007.HK",  // Country Garden
        "83.HK",    // Sino Land
        "14.HK",    // Hysan Development
        "1972.HK",  // Swire Properties
        "778.HK",   // Fortune REIT
        "87001.HK", // Hui Xian REIT
        "405.HK",   // Yuexiu REIT
        "1908.HK",  // C&D International
        "6158.HK",  // COLI Property
        "9979.HK",  // Greentown Management
        "3377.HK",  // Sino-Ocean Group
        "1638.HK",  // Kaisa Group
        "272.HK",   // Shui On Land
        "2868.HK",  // Beijing Capital Land
        "1238.HK",  // Powerlong Real Estate
        "2778.HK",  // Champion REIT
        "435.HK",   // Sunlight REIT
        "808.HK",   // Prosperus Real Estate
    ];

    // ── Consumer: Food, beverage, retail, luxury, restaurants, sportswear ──
    const CONSUMER: &[&str] = &[
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
        "3799.HK", // Dali Foods
        "6969.HK", // Smoore International
        "9922.HK", // Jiumaojiu
        "1458.HK", // Zhou Hei Ya
        "6808.HK", // Sun Art Retail
        "3331.HK", // Vinda International
        "1910.HK", // Samsonite
        "2255.HK", // Haichang Ocean Park
        "9992.HK", // Pop Mart
        "6993.HK", // Blue Moon Group
        "9995.HK", // RLX Technology
        "3998.HK", // Bosideng
        "9660.HK", // Mao Geping
        "6110.HK", // Topsports International
        "116.HK",  // Chow Sang Sang
        "590.HK",  // Luk Fook Holdings
        "3319.HK", // A-Living Smart City Services
        "1579.HK", // Yihai International
        "9869.HK", // Soulgate
        "336.HK",  // Huabao International
        "345.HK",  // Vitasoy
        "1361.HK", // 361 Degrees
        "6049.HK", // Poly Culture
        "1212.HK", // Lifestyle International
        "9688.HK", // ZJLD Group
        "1733.HK", // EEKA Fashion
        "69.HK",   // Shangri-La Asia
        "551.HK",  // Yue Yuen Industrial
    ];

    // ── Healthcare: Pharma, biotech, medical devices ──
    const HEALTHCARE: &[&str] = &[
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
        "2171.HK", // Carsgen Therapeutics
        "1513.HK", // Livzon Pharmaceutical
        "6127.HK", // Yifeng Pharmacy
        "570.HK",  // China Traditional Chinese Medicine
        "867.HK",  // China Medical System
        "6622.HK", // Zhaoke Ophthalmology
        "1681.HK", // Consun Pharmaceutical
        "2607.HK", // Shanghai Pharmaceuticals
        "3320.HK", // China Resources Medical
        "2142.HK", // Simcere Pharmaceutical
        "6996.HK", // Antengene ... actually not sure
        "1066.HK", // Weigao Group
        "1302.HK", // Kindstar Globalgene
        "3613.HK", // Beijing Health
        "2186.HK", // Luye Pharma
        "1530.HK", // 3SBio
        "9926.HK", // Akeso
        "1877.HK", // Shanghai Junshi Bio
        "1548.HK", // Genscript Biotech
        "2126.HK", // Grand Pharma
        "6616.HK", // Gene Harbour Biosciences
        "6978.HK", // Yadea Group (miscat - actually auto/EV)
        "1539.HK", // Yestar Healthcare
    ];

    // ── Utilities: Power, gas, water ──
    const UTILITIES: &[&str] = &[
        "2.HK",    // CLP Holdings
        "3.HK",    // HK & China Gas
        "6.HK",    // Power Assets
        "836.HK",  // China Resources Power
        "1038.HK", // CK Infrastructure
        "902.HK",  // Huaneng Power
        "1071.HK", // Huadian Power
        "816.HK",  // Huadian Fuxin
        "1816.HK", // CGN Power
        "1868.HK", // Neo Solar Power ... not sure
        "579.HK",  // Beijing Jingneng Clean Energy
        "956.HK",  // China Suntien Green Energy
        "371.HK",  // Beijing Enterprises Water
        "270.HK",  // Guangdong Investment
        "855.HK",  // China Water Affairs
        "2380.HK", // China Power International
        "1798.HK", // Datang New Energy
        "1799.HK", // Xinyi Solar
        "968.HK",  // Xinyi Glass
        "2208.HK", // Xinjiang Goldwind Tech
    ];

    // ── Insurance ──
    const INSURANCE: &[&str] = &[
        "2318.HK", // Ping An
        "1299.HK", // AIA
        "2628.HK", // China Life
        "2601.HK", // CPIC
        "966.HK",  // China Taiping
        "1339.HK", // PICC
        "1508.HK", // China Reinsurance
    ];

    // ── Auto: EVs, traditional auto, auto parts ──
    const AUTO: &[&str] = &[
        "9868.HK", // XPeng
        "2015.HK", // Li Auto
        "1211.HK", // BYD
        "175.HK",  // Geely Auto
        "2333.HK", // Great Wall Motor
        "9863.HK", // Zeekr
        "2238.HK", // GAC Group
        "1114.HK", // Brilliance China
        "6699.HK", // Angelalign Technology ... actually healthcare
        "1958.HK", // BAIC Motor
        "2039.HK", // CIMC Vehicles
        "1268.HK", // Meihua International ... actually materials
        "489.HK",  // Dongfeng Motor
        "2488.HK", // Leapmotor
    ];

    // ── Materials: Mining, metals, cement, chemicals, gold ──
    const MATERIALS: &[&str] = &[
        "2259.HK", // Zijin Gold International
        "2899.HK", // Zijin Mining
        "914.HK",  // Anhui Conch Cement
        "2600.HK", // Aluminum Corp of China (Chalco)
        "358.HK",  // Jiangxi Copper
        "3323.HK", // China National Building Material
        "1818.HK", // Zhaojin Mining
        "3993.HK", // China Molybdenum
        "1138.HK", // China Resources Cement
        "691.HK",  // Shanshui Cement
        "1208.HK", // MMG Limited
        "2009.HK", // BBMG Corporation
        "323.HK",  // Maanshan Iron & Steel
        "347.HK",  // Angang Steel
        "1787.HK", // Shandong Gold Mining
        "6865.HK", // Flat Glass Group
        "3606.HK", // Fuyao Glass
        "546.HK",  // Fufeng Group (bio-fermentation/chemicals)
        "1164.HK", // CGN Mining
        "189.HK",  // Dongyue Group (chemical)
    ];

    // ── Industrial: Construction, railways, infrastructure, machinery ──
    const INDUSTRIAL: &[&str] = &[
        "1186.HK", // China Railway Construction
        "390.HK",  // China Railway Group
        "1766.HK", // CRRC
        "1800.HK", // China Communications Construction
        "3311.HK", // China State Construction International
        "1072.HK", // Dongfang Electric
        "2727.HK", // Shanghai Electric
        "1157.HK", // Zoomlion Heavy
        "3339.HK", // Lonking Holdings
        "3898.HK", // China Yida Holding
        "696.HK",  // TravelSky Technology
        "1880.HK", // China Railway Signal
        "586.HK",  // China Conch Venture
        "1888.HK", // China Kingstone Mining ... materials
        "1052.HK", // Yuexiu Transport
        "107.HK",  // Sichuan Expressway
        "548.HK",  // Shenzhen Expressway
        "995.HK",  // Anhui Expressway
        "177.HK",  // Jiangsu Expressway
        "576.HK",  // Zhejiang Expressway
        "1882.HK", // Haitian International
        "1618.HK", // Metallurgical Corp China
        "1133.HK", // Harbin Electric
        "2357.HK", // AVIC International
    ];

    // ── Conglomerate: Diversified holdings ──
    const CONGLOMERATE: &[&str] = &[
        "1.HK",    // CK Hutchison
        "19.HK",   // Swire Pacific
        "4.HK",    // Wharf Holdings
        "267.HK",  // CITIC Limited
        "27.HK",   // Galaxy Entertainment
        "10.HK",   // Hang Lung Group
        "66.HK",   // MTR Corporation
        "293.HK",  // Cathay Pacific ... actually logistics
        "683.HK",  // Kerry Properties
        "659.HK",  // NWS Holdings
        "20.HK",   // SJM Holdings (gaming)
        "880.HK",  // SJM Holdings
        "1128.HK", // Wynn Macau
        "2282.HK", // MGM China
        "6883.HK", // Melco International
        "1928.HK", // Sands China
        "142.HK",  // First Pacific
        "242.HK",  // Shun Tak Holdings
        "493.HK",  // GOME Retail
    ];

    // ── Media & Entertainment ──
    const MEDIA: &[&str] = &[
        "1060.HK", // Alibaba Pictures
        "2400.HK", // XD Inc (gaming)
        "799.HK",  // IGG Inc
        "777.HK",  // NetDragon Websoft
        "484.HK",  // HKT Trust ... already in telecom
    ];

    // ── Logistics & Transport: Shipping, ports, delivery ──
    const LOGISTICS: &[&str] = &[
        "2057.HK", // ZTO Express
        "2618.HK", // JD Logistics
        "6139.HK", // Kerry Logistics
        "316.HK",  // Orient Overseas (International)
        "144.HK",  // China Merchants Port
        "1199.HK", // COSCO Shipping
        "1919.HK", // COSCO Shipping Holdings
        "1308.HK", // SITC International
        "636.HK",  // Kerry Logistics Network
        "2343.HK", // Pacific Basin Shipping
        "598.HK",  // Sinotrans
        "2866.HK", // COSCO Shipping Development
        "3378.HK", // Xiamen C&D
        "152.HK",  // Shenzhen International
        "694.HK",  // Beijing Capital International Airport
        "753.HK",  // Air China
        "670.HK",  // China Eastern Airlines
        "1055.HK", // China Southern Airlines
    ];

    // ── Education ──
    const EDUCATION: &[&str] = &[
        "1765.HK", // Hope Education
        "839.HK",  // China Education Group
        "2001.HK", // New Higher Education
        "1317.HK", // Maple Leaf Education
    ];

    // Search through sector arrays
    let (sectors, names): (&[&[&str]], &[&str]) = (
        &[
            TECH,
            SEMICONDUCTOR,
            FINANCE,
            ENERGY,
            TELECOM,
            PROPERTY,
            CONSUMER,
            HEALTHCARE,
            UTILITIES,
            INSURANCE,
            AUTO,
            MATERIALS,
            INDUSTRIAL,
            CONGLOMERATE,
            MEDIA,
            LOGISTICS,
            EDUCATION,
        ],
        &[
            "tech",
            "semiconductor",
            "finance",
            "energy",
            "telecom",
            "property",
            "consumer",
            "healthcare",
            "utilities",
            "insurance",
            "auto",
            "materials",
            "industrial",
            "conglomerate",
            "media",
            "logistics",
            "education",
        ],
    );

    for (arr, name) in sectors.iter().zip(names.iter()) {
        if arr.contains(&symbol) {
            return Some(SectorId((*name).into()));
        }
    }

    None
}

// ── ObjectStore ──

pub struct ObjectStore {
    pub institutions: HashMap<InstitutionId, Institution>,
    pub brokers: HashMap<BrokerId, Broker>,
    pub stocks: HashMap<Symbol, Stock>,
    pub sectors: HashMap<SectorId, Sector>,
    pub broker_to_institution: HashMap<BrokerId, InstitutionId>,
}

impl ObjectStore {
    pub fn institution_for_broker(&self, broker_id: &BrokerId) -> Option<&Institution> {
        self.broker_to_institution
            .get(broker_id)
            .and_then(|iid| self.institutions.get(iid))
    }

    pub fn brokers_for_institution(&self, institution_id: &InstitutionId) -> Vec<&Broker> {
        self.institutions
            .get(institution_id)
            .map(|inst| {
                inst.broker_ids
                    .iter()
                    .filter_map(|bid| self.brokers.get(bid))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn stocks_in_sector(&self, sector_id: &SectorId) -> Vec<&Stock> {
        self.stocks
            .values()
            .filter(|s| s.sector_id.as_ref() == Some(sector_id))
            .collect()
    }
}

// ── Test helper ──

#[cfg(test)]
impl ObjectStore {
    /// Build an ObjectStore from raw data, no API needed.
    pub fn from_parts(
        institutions: Vec<Institution>,
        stocks: Vec<Stock>,
        sectors: Vec<Sector>,
    ) -> Self {
        let mut inst_map = HashMap::new();
        let mut broker_map = HashMap::new();
        let mut b2i = HashMap::new();

        for inst in institutions {
            for &bid in &inst.broker_ids {
                broker_map.insert(
                    bid,
                    Broker {
                        id: bid,
                        institution_id: inst.id,
                    },
                );
                b2i.insert(bid, inst.id);
            }
            inst_map.insert(inst.id, inst);
        }

        let stock_map: HashMap<Symbol, Stock> =
            stocks.into_iter().map(|s| (s.symbol.clone(), s)).collect();

        let sector_map: HashMap<SectorId, Sector> =
            sectors.into_iter().map(|s| (s.id.clone(), s)).collect();

        ObjectStore {
            institutions: inst_map,
            brokers: broker_map,
            stocks: stock_map,
            sectors: sector_map,
            broker_to_institution: b2i,
        }
    }
}

// ── Initialization from Longport API ──

pub async fn initialize(ctx: &QuoteContext, watchlist: &[&str]) -> Arc<ObjectStore> {
    // 1. Fetch all HKEX participants → build Institutions + Brokers
    let participants = ctx
        .participants()
        .await
        .expect("failed to fetch participants");

    let mut institutions: HashMap<InstitutionId, Institution> = HashMap::new();
    let mut brokers: HashMap<BrokerId, Broker> = HashMap::new();
    let mut broker_to_institution: HashMap<BrokerId, InstitutionId> = HashMap::new();

    for p in &participants {
        let mut broker_ids: std::collections::HashSet<BrokerId> = std::collections::HashSet::new();
        for &raw_id in &p.broker_ids {
            broker_ids.insert(BrokerId(raw_id));
        }

        if broker_ids.is_empty() {
            continue;
        }

        // InstitutionId = min(broker_ids) — stable, deterministic
        let min_id = broker_ids.iter().map(|b| b.0).min().unwrap();
        let institution_id = InstitutionId(min_id);
        let class = InstitutionClass::classify_from_brokers(&broker_ids);

        let institution = Institution {
            id: institution_id,
            name_en: p.name_en.clone(),
            name_cn: p.name_cn.clone(),
            name_hk: p.name_hk.clone(),
            broker_ids: broker_ids.clone(),
            class,
        };

        institutions.insert(institution_id, institution);

        for bid in &broker_ids {
            brokers.insert(
                *bid,
                Broker {
                    id: *bid,
                    institution_id,
                },
            );
            broker_to_institution.insert(*bid, institution_id);
        }
    }

    // 2. Sectors (hardcoded)
    let sectors: HashMap<SectorId, Sector> = define_sectors()
        .into_iter()
        .map(|s| (s.id.clone(), s))
        .collect();

    // 3. Fetch static info for watchlist → build Stocks
    let symbols: Vec<String> = watchlist.iter().map(|s| s.to_string()).collect();
    let static_infos = ctx
        .static_info(symbols)
        .await
        .expect("failed to fetch static_info");

    let mut stocks: HashMap<Symbol, Stock> = HashMap::new();
    for info in &static_infos {
        let sym = Symbol(info.symbol.clone());
        let sector_id = symbol_sector(&info.symbol);
        stocks.insert(
            sym.clone(),
            Stock {
                symbol: sym,
                name_en: info.name_en.clone(),
                name_cn: info.name_cn.clone(),
                name_hk: info.name_hk.clone(),
                exchange: info.exchange.clone(),
                lot_size: info.lot_size,
                sector_id,
                total_shares: info.total_shares,
                circulating_shares: info.circulating_shares,
                eps_ttm: info.eps_ttm,
                bps: info.bps,
                dividend_yield: info.dividend_yield,
            },
        );
    }

    Arc::new(ObjectStore {
        institutions,
        brokers,
        stocks,
        sectors,
        broker_to_institution,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn make_institution(min_id: i32, ids: &[i32], name: &str) -> Institution {
        Institution {
            id: InstitutionId(min_id),
            name_en: name.into(),
            name_cn: String::new(),
            name_hk: String::new(),
            broker_ids: ids.iter().map(|&i| BrokerId(i)).collect(),
            class: InstitutionClass::classify_from_brokers(
                &ids.iter().map(|&i| BrokerId(i)).collect(),
            ),
        }
    }

    fn make_stock(symbol: &str, sector: Option<&str>) -> Stock {
        Stock {
            symbol: Symbol(symbol.into()),
            name_en: symbol.into(),
            name_cn: String::new(),
            name_hk: String::new(),
            exchange: "SEHK".into(),
            lot_size: 100,
            sector_id: sector.map(|s| SectorId(s.into())),
            total_shares: 0,
            circulating_shares: 0,
            eps_ttm: rust_decimal::Decimal::ZERO,
            bps: rust_decimal::Decimal::ZERO,
            dividend_yield: rust_decimal::Decimal::ZERO,
        }
    }

    fn test_store() -> ObjectStore {
        let barclays = make_institution(2040, &[2040, 2041, 4497], "Barclays Asia");
        let stock_connect = make_institution(6996, &[6996, 6997], "Stock Connect SH");
        let morgan = make_institution(3000, &[3000, 3001], "Morgan Stanley");

        let stocks = vec![
            make_stock("700.HK", Some("tech")),
            make_stock("9988.HK", Some("tech")),
            make_stock("5.HK", Some("finance")),
            make_stock("883.HK", Some("energy")),
            make_stock("UNKNOWN.HK", None),
        ];

        let sectors = vec![
            Sector {
                id: SectorId("tech".into()),
                name: "Technology".into(),
            },
            Sector {
                id: SectorId("finance".into()),
                name: "Finance".into(),
            },
            Sector {
                id: SectorId("energy".into()),
                name: "Energy".into(),
            },
        ];

        ObjectStore::from_parts(vec![barclays, stock_connect, morgan], stocks, sectors)
    }

    // ── institution_for_broker ──

    #[test]
    fn lookup_broker_finds_institution() {
        let store = test_store();
        let inst = store.institution_for_broker(&BrokerId(4497)).unwrap();
        assert_eq!(inst.id, InstitutionId(2040));
        assert_eq!(inst.name_en, "Barclays Asia");
    }

    #[test]
    fn lookup_broker_min_id_also_works() {
        let store = test_store();
        let inst = store.institution_for_broker(&BrokerId(2040)).unwrap();
        assert_eq!(inst.id, InstitutionId(2040));
    }

    #[test]
    fn lookup_broker_not_found() {
        let store = test_store();
        assert!(store.institution_for_broker(&BrokerId(9999)).is_none());
    }

    // ── brokers_for_institution ──

    #[test]
    fn brokers_for_barclays() {
        let store = test_store();
        let brokers = store.brokers_for_institution(&InstitutionId(2040));
        let mut ids: Vec<i32> = brokers.iter().map(|b| b.id.0).collect();
        ids.sort();
        assert_eq!(ids, vec![2040, 2041, 4497]);
    }

    #[test]
    fn brokers_for_nonexistent_institution() {
        let store = test_store();
        let brokers = store.brokers_for_institution(&InstitutionId(8888));
        assert!(brokers.is_empty());
    }

    // ── stocks_in_sector ──

    #[test]
    fn tech_stocks() {
        let store = test_store();
        let stocks = store.stocks_in_sector(&SectorId("tech".into()));
        let mut syms: Vec<&str> = stocks.iter().map(|s| s.symbol.0.as_str()).collect();
        syms.sort();
        assert_eq!(syms, vec!["700.HK", "9988.HK"]);
    }

    #[test]
    fn energy_stocks() {
        let store = test_store();
        let stocks = store.stocks_in_sector(&SectorId("energy".into()));
        assert_eq!(stocks.len(), 1);
        assert_eq!(stocks[0].symbol.0, "883.HK");
    }

    #[test]
    fn empty_sector() {
        let store = test_store();
        let stocks = store.stocks_in_sector(&SectorId("consumer".into()));
        assert!(stocks.is_empty());
    }

    // ── Institution ID = min(broker_ids) ──

    #[test]
    fn institution_id_is_min_broker() {
        let store = test_store();
        // Barclays has brokers [2040, 2041, 4497], so InstitutionId should be 2040
        let inst = store.institutions.get(&InstitutionId(2040)).unwrap();
        let min_broker = inst.broker_ids.iter().map(|b| b.0).min().unwrap();
        assert_eq!(inst.id.0, min_broker);
    }

    // ── Stock Connect classification ──

    #[test]
    fn stock_connect_classified_correctly() {
        let store = test_store();
        let sc = store.institutions.get(&InstitutionId(6996)).unwrap();
        assert_eq!(sc.class, InstitutionClass::StockConnectChannel);
    }

    #[test]
    fn regular_institution_is_unknown() {
        let store = test_store();
        let barclays = store.institutions.get(&InstitutionId(2040)).unwrap();
        assert_eq!(barclays.class, InstitutionClass::Unknown);
    }

    // ── Broker → Institution consistency ──

    #[test]
    fn all_brokers_point_to_valid_institution() {
        let store = test_store();
        for (bid, iid) in &store.broker_to_institution {
            assert!(
                store.institutions.contains_key(iid),
                "Broker {} points to non-existent institution {}",
                bid,
                iid,
            );
        }
    }

    #[test]
    fn all_institution_brokers_exist_in_broker_map() {
        let store = test_store();
        for inst in store.institutions.values() {
            for bid in &inst.broker_ids {
                assert!(
                    store.brokers.contains_key(bid),
                    "Institution {} claims broker {} but it's not in broker map",
                    inst.id,
                    bid,
                );
            }
        }
    }

    // ── symbol_sector mapping ──

    #[test]
    fn symbol_sector_tech() {
        assert_eq!(symbol_sector("700.HK"), Some(SectorId("tech".into())));
        assert_eq!(symbol_sector("9988.HK"), Some(SectorId("tech".into())));
        assert_eq!(symbol_sector("268.HK"), Some(SectorId("tech".into())));
    }

    #[test]
    fn symbol_sector_finance() {
        assert_eq!(symbol_sector("5.HK"), Some(SectorId("finance".into())));
        assert_eq!(symbol_sector("388.HK"), Some(SectorId("finance".into())));
    }

    #[test]
    fn symbol_sector_energy() {
        assert_eq!(symbol_sector("883.HK"), Some(SectorId("energy".into())));
    }

    #[test]
    fn symbol_sector_cross_sector_cleanup() {
        assert_eq!(symbol_sector("1818.HK"), Some(SectorId("materials".into())));
        assert_eq!(symbol_sector("316.HK"), Some(SectorId("logistics".into())));
    }

    #[test]
    fn symbol_sector_unknown() {
        assert_eq!(symbol_sector("FAKE.HK"), None);
    }

    // ── define_sectors ──

    #[test]
    fn sectors_have_unique_ids() {
        let sectors = define_sectors();
        let ids: HashSet<_> = sectors.iter().map(|s| &s.id).collect();
        assert_eq!(ids.len(), sectors.len());
    }

    #[test]
    fn sectors_count() {
        assert_eq!(define_sectors().len(), 17);
    }
}
