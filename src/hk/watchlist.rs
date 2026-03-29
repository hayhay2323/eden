pub const WATCHLIST: &[&str] = &[
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
