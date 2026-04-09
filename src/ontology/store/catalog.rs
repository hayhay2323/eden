use super::*;

pub fn define_sectors() -> Vec<Sector> {
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

pub fn us_symbol_sector(symbol: &str) -> Option<&'static str> {
    match symbol {
        // ── Tech / Software / IT Services ──
        "AAPL.US" | "MSFT.US" | "GOOGL.US" | "GOOG.US" | "AMZN.US" | "META.US" | "NFLX.US"
        | "CRM.US" | "ORCL.US" | "ADBE.US" | "NOW.US" | "INTU.US" | "SHOP.US" | "SNOW.US"
        | "PLTR.US" | "UBER.US" | "ABNB.US" | "DASH.US" | "SQ.US" | "PYPL.US" | "COIN.US"
        | "DDOG.US" | "CRWD.US" | "ZS.US" | "NET.US" | "MDB.US" | "PANW.US" | "WDAY.US"
        | "TEAM.US" | "HUBS.US" | "VEEV.US" | "DKNG.US" | "ROKU.US" | "TTD.US" | "SNAP.US"
        | "PINS.US" | "SPOT.US" | "RBLX.US" | "U.US" | "ZM.US" | "DOCU.US" | "TWLO.US"
        | "OKTA.US" | "MNDY.US" | "PATH.US" | "APP.US" | "BILL.US" | "GTLB.US" | "IOT.US"
        | "ADP.US" | "ADSK.US" | "ANET.US" | "ANSS.US" | "CDW.US" | "CSGP.US" | "CTSH.US"
        | "FICO.US" | "FTNT.US" | "GPN.US" | "HPQ.US" | "IT.US" | "KEYS.US" | "PAYC.US"
        | "VRSK.US" | "ZBRA.US" | "BR.US" | "ACN.US" | "AKAM.US" | "CSCO.US" | "EPAM.US"
        | "FFIV.US" | "FISV.US" | "GDDY.US" | "GEN.US" | "IBM.US" | "JKHY.US" | "PTC.US"
        | "SNPS.US" | "CDNS.US" | "TRMB.US" | "TYL.US" | "VRSN.US" | "TECH.US" | "FDS.US"
        | "DELL.US" | "HPE.US" | "STX.US" | "WDC.US" | "XYZ.US" => Some("tech"),
        // ── Semiconductor ──
        "NVDA.US" | "AMD.US" | "AVGO.US" | "QCOM.US" | "INTC.US" | "MU.US" | "AMAT.US"
        | "LRCX.US" | "KLAC.US" | "TSM.US" | "TXN.US" | "ADI.US" | "MRVL.US" | "ON.US"
        | "NXPI.US" | "MCHP.US" | "SWKS.US" | "MPWR.US" | "ARM.US" | "ASML.US"
        | "GFS.US" | "GRMN.US" | "COHR.US" | "QRVO.US" | "TER.US" | "LITE.US" => {
            Some("semiconductor")
        }
        // ── China ADR ──
        "BABA.US" | "BIDU.US" | "NIO.US" | "XPEV.US" | "LI.US" | "PDD.US" | "JD.US" | "TCOM.US"
        | "ZTO.US" | "BILI.US" | "NTES.US" | "TME.US" | "WB.US" | "IQ.US" | "VNET.US"
        | "FUTU.US" | "TIGR.US" | "MNSO.US" | "TAL.US" | "EDU.US" | "HTHT.US" | "YMM.US"
        | "QFIN.US" | "LX.US" | "FINV.US" | "GDS.US" | "KC.US" | "ZLAB.US" | "BGNE.US"
        | "LEGN.US" => Some("china_adr"),
        // ── EV / Auto ──
        "TSLA.US" | "RIVN.US" | "LCID.US" | "GM.US" | "F.US" | "TM.US" | "STLA.US"
        | "APTV.US" | "CVNA.US" => Some("ev_auto"),
        // ── Finance / Insurance ──
        "JPM.US" | "GS.US" | "MS.US" | "BAC.US" | "WFC.US" | "C.US" | "USB.US" | "PNC.US"
        | "SCHW.US" | "BK.US" | "TFC.US" | "COF.US" | "AXP.US" | "BLK.US" | "ICE.US" | "CME.US"
        | "SPGI.US" | "MCO.US" | "MSCI.US" | "FIS.US" | "MA.US" | "V.US" | "HOOD.US"
        | "SOFI.US" | "PGR.US" | "TRV.US" | "ALL.US" | "MET.US" | "AIG.US" | "AFL.US"
        | "ACGL.US" | "AJG.US" | "AON.US" | "CBOE.US" | "DFS.US" | "MKTX.US" | "NDAQ.US"
        | "LPLA.US" | "APO.US" | "ARES.US" | "BX.US" | "BEN.US" | "BRO.US" | "CB.US"
        | "CFG.US" | "CINF.US" | "CPAY.US" | "EG.US" | "ERIE.US" | "FITB.US" | "GL.US"
        | "HBAN.US" | "HIG.US" | "IBKR.US" | "IVZ.US" | "KEY.US" | "KKR.US" | "L.US"
        | "MRSH.US" | "MTB.US" | "NTRS.US" | "PFG.US" | "PRU.US" | "RF.US" | "RJF.US"
        | "STT.US" | "SYF.US" | "WRB.US" => Some("finance"),
        // ── Healthcare / Pharma / Biotech ──
        "UNH.US" | "JNJ.US" | "LLY.US" | "ABBV.US" | "PFE.US" | "MRK.US" | "TMO.US" | "ABT.US"
        | "DHR.US" | "BMY.US" | "AMGN.US" | "GILD.US" | "VRTX.US" | "REGN.US" | "ISRG.US"
        | "SYK.US" | "BSX.US" | "MDT.US" | "ZTS.US" | "EW.US" | "A.US" | "DXCM.US" | "IDXX.US"
        | "MRNA.US" | "BIIB.US" | "ALNY.US" | "ILMN.US" | "CVS.US" | "CI.US" | "ELV.US"
        | "HCA.US" | "HUM.US" | "MCK.US" | "BDX.US" | "RMD.US" | "WST.US" | "ZBH.US"
        | "BAX.US" | "CAH.US" | "CNC.US" | "COR.US" | "CRL.US" | "DGX.US" | "DOC.US"
        | "DVA.US" | "GEHC.US" | "HOLX.US" | "HSIC.US" | "IEX.US" | "INCY.US" | "IQV.US"
        | "LH.US" | "MTD.US" | "PODD.US" | "RVTY.US" | "SOLV.US" | "STE.US" | "UHS.US"
        | "VTRS.US" => Some("healthcare"),
        // ── Consumer (Discretionary + Staples) ──
        "WMT.US" | "COST.US" | "TGT.US" | "HD.US" | "LOW.US" | "TJX.US" | "ROST.US" | "DG.US"
        | "DLTR.US" | "NKE.US" | "LULU.US" | "SBUX.US" | "MCD.US" | "CMG.US" | "YUM.US"
        | "DPZ.US" | "BKNG.US" | "MAR.US" | "HLT.US" | "RCL.US" | "LVS.US" | "WYNN.US"
        | "PG.US" | "KO.US" | "PEP.US" | "PM.US" | "MO.US" | "MDLZ.US" | "CL.US" | "KMB.US"
        | "GIS.US" | "K.US" | "HSY.US" | "STZ.US" | "KHC.US" | "SYY.US" | "KDP.US" | "AZO.US"
        | "DECK.US" | "EBAY.US" | "ORLY.US" | "POOL.US" | "TSCO.US" | "KR.US"
        | "BBY.US" | "CAG.US" | "CCL.US" | "CHD.US" | "CLX.US" | "CPB.US" | "DRI.US"
        | "EL.US" | "EXPE.US" | "GPC.US" | "HAS.US" | "HRL.US" | "HST.US" | "KVUE.US"
        | "LEN.US" | "DHI.US" | "LII.US" | "MGM.US" | "MKC.US" | "MNST.US" | "NCLH.US"
        | "PHM.US" | "RL.US" | "SJM.US" | "SNA.US" | "TAP.US" | "TPR.US" | "TSN.US"
        | "ULTA.US" | "VFC.US" | "WBA.US" => Some("consumer"),
        // ── Energy ──
        "XOM.US" | "CVX.US" | "COP.US" | "SLB.US" | "EOG.US" | "MPC.US" | "PSX.US" | "VLO.US"
        | "OXY.US" | "HAL.US" | "DVN.US" | "FANG.US" | "KMI.US" | "WMB.US" | "OKE.US"
        | "LNG.US" | "ENPH.US" | "FSLR.US" | "TRGP.US"
        | "APA.US" | "BKR.US" | "CF.US" | "CTRA.US" | "EQT.US" | "EXE.US" => Some("energy"),
        // ── Industrial ──
        "CAT.US" | "DE.US" | "BA.US" | "RTX.US" | "LMT.US" | "GE.US" | "HON.US" | "UNP.US"
        | "UPS.US" | "FDX.US" | "MMM.US" | "GD.US" | "NOC.US" | "LHX.US" | "EMR.US" | "ETN.US"
        | "ITW.US" | "ROK.US" | "CARR.US" | "WM.US" | "RSG.US" | "CSX.US" | "NSC.US" | "DAL.US"
        | "UAL.US" | "AAL.US" | "LUV.US" | "FAST.US" | "GEV.US" | "GWW.US" | "HUBB.US"
        | "LDOS.US" | "ODFL.US" | "OTIS.US" | "PCAR.US" | "TT.US"
        | "AXON.US" | "BLDR.US" | "CHRW.US" | "CMI.US" | "DOV.US" | "EME.US" | "EXPD.US"
        | "FIX.US" | "GNRC.US" | "HII.US" | "HWM.US" | "IR.US" | "J.US" | "JBHT.US"
        | "JBL.US" | "JCI.US" | "NDSN.US" | "PH.US" | "PNR.US" | "PWR.US" | "ROP.US"
        | "ROL.US" | "SWK.US" | "TDG.US" | "TDY.US" | "TEL.US" | "TFX.US" | "TXT.US"
        | "URI.US" | "WAB.US" | "XYL.US" => Some("industrial"),
        // ── Utility ──
        "NEE.US" | "DUK.US" | "SO.US" | "D.US" | "AEP.US" | "SRE.US" | "EXC.US" | "XEL.US"
        | "PCG.US" | "ED.US" | "CEG.US" | "VST.US" | "CPRT.US"
        | "AEE.US" | "AES.US" | "ATO.US" | "AWK.US" | "CMS.US" | "CNP.US" | "DTE.US"
        | "EIX.US" | "ES.US" | "ETR.US" | "EVRG.US" | "FE.US" | "LNT.US" | "NI.US"
        | "NRG.US" | "PEG.US" | "PNW.US" | "PPL.US" | "WEC.US" => Some("utility"),
        // ── Real Estate ──
        "PLD.US" | "AMT.US" | "CCI.US" | "EQIX.US" | "SPG.US" | "PSA.US" | "O.US" | "DLR.US"
        | "WELL.US" | "AVB.US" | "CBRE.US" | "EQR.US" | "IRM.US" | "SBAC.US"
        | "ARE.US" | "BXP.US" | "CPT.US" | "ESS.US" | "EXR.US" | "FRT.US" | "INVH.US"
        | "KIM.US" | "MAA.US" | "REG.US" | "UDR.US" | "VICI.US" => Some("real_estate"),
        // ── Materials ──
        "LIN.US" | "SHW.US" | "APD.US" | "ECL.US" | "FCX.US" | "NEM.US" | "NUE.US" | "DOW.US"
        | "DD.US" | "VMC.US" | "MLM.US" | "PKG.US"
        | "ALB.US" | "AVY.US" | "BALL.US" | "BG.US" | "CRH.US" | "CTVA.US"
        | "IP.US" | "MAS.US" | "MOS.US" | "PPG.US" | "STLD.US" | "SW.US" | "WRK.US"
        | "WY.US" | "ADM.US" | "AME.US" | "GLW.US" | "IFF.US" => Some("materials"),
        // ── Telecom / Media ──
        "T.US" | "VZ.US" | "TMUS.US" | "CMCSA.US" | "DIS.US" | "WBD.US" | "CHTR.US" | "EA.US"
        | "TTWO.US" | "FOX.US" | "FOXA.US" | "NWSA.US" | "NWS.US" | "OMC.US" | "CIEN.US"
        | "LYV.US" => Some("telecom_media"),
        "MSTR.US" | "MARA.US" | "RIOT.US" | "BITF.US" | "CLSK.US" => Some("crypto"),
        "SNDK.US" => Some("semiconductor"),
        "RKLB.US" | "ASTS.US" | "LUNR.US" => Some("industrial"),
        "RDDT.US" | "DUOL.US" | "SE.US" | "GRAB.US" => Some("tech"),
        "HIMS.US" => Some("healthcare"),
        "CELH.US" | "BIRK.US" => Some("consumer"),
        "AFRM.US" | "UPST.US" | "NU.US" => Some("finance"),
        "MELI.US" => Some("consumer"),
        "RGTI.US" | "QUBT.US" => Some("tech"),
        "GME.US" | "AMC.US" | "BB.US" | "CLOV.US" | "IONQ.US" | "SMCI.US" | "AI.US" | "SOUN.US"
        | "VRT.US" => Some("momentum"),
        // ── Misc S&P 500 (conglomerates, insurance, misc) ──
        "AIZ.US" | "ALLE.US" | "AMCR.US" | "AMP.US" | "AOS.US" | "CTAS.US"
        | "NTAP.US" | "NVR.US" | "RHI.US" | "SATS.US" | "TROW.US" | "WAT.US"
        | "APH.US" | "FTV.US" | "MSI.US" => Some("industrial"),
        "COO.US" | "EFX.US" | "ALGN.US" => Some("healthcare"),
        "LYB.US" => Some("materials"),
        "SPY.US" | "QQQ.US" | "IWM.US" | "DIA.US" | "TLT.US" | "GLD.US" | "SLV.US" | "USO.US"
        | "UNG.US" | "VXX.US" | "HYG.US" | "LQD.US" | "EEM.US" | "FXI.US" | "EWJ.US" | "EFA.US"
        | "XLF.US" | "XLK.US" | "XLE.US" | "XLV.US" | "XLI.US" | "XLP.US" | "XLU.US" | "XLY.US"
        | "XLB.US" | "XLRE.US" | "XLC.US" | "ARKK.US" | "SOXX.US" | "SMH.US" | "KWEB.US"
        | "XBI.US" | "HACK.US" | "BOTZ.US" | "TAN.US" | "ICLN.US" | "IBIT.US" | "BITO.US" => {
            Some("etf")
        }
        _ => Some("other"),
    }
}

pub fn us_sector_names() -> &'static [(&'static str, &'static str)] {
    &[
        ("tech", "科技"),
        ("semiconductor", "半導體"),
        ("china_adr", "中概股"),
        ("ev_auto", "電動車"),
        ("finance", "金融"),
        ("healthcare", "醫療"),
        ("consumer", "消費"),
        ("energy", "能源"),
        ("industrial", "工業"),
        ("utility", "公用"),
        ("real_estate", "地產"),
        ("materials", "材料"),
        ("telecom_media", "電訊傳媒"),
        ("crypto", "加密"),
        ("momentum", "動量"),
        ("etf", "ETF"),
        ("other", "其他"),
    ]
}

pub fn canonical_sector_id(value: &str) -> Option<&'static str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    const HK_SECTOR_ALIASES: &[(&str, &str)] = &[
        ("tech", "Technology"),
        ("semiconductor", "Semiconductor"),
        ("finance", "Finance"),
        ("energy", "Energy"),
        ("telecom", "Telecommunications"),
        ("property", "Property"),
        ("consumer", "Consumer"),
        ("healthcare", "Healthcare"),
        ("utilities", "Utilities"),
        ("insurance", "Insurance"),
        ("auto", "Automobile"),
        ("materials", "Materials"),
        ("industrial", "Industrial"),
        ("conglomerate", "Conglomerate"),
        ("media", "Media & Entertainment"),
        ("logistics", "Logistics & Transport"),
        ("education", "Education"),
    ];

    for (sector_id, sector_label) in HK_SECTOR_ALIASES.iter().chain(us_sector_names().iter()) {
        if trimmed.eq_ignore_ascii_case(sector_id) || trimmed.eq_ignore_ascii_case(sector_label) {
            return Some(sector_id);
        }
        if trimmed == *sector_label {
            return Some(sector_id);
        }
    }

    None
}

pub fn symbol_sector(symbol: &str) -> Option<SectorId> {
    const TECH: &[&str] = &[
        "700.HK", "9988.HK", "3690.HK", "9618.HK", "1810.HK", "9888.HK", "268.HK", "9999.HK",
        "9698.HK", "1024.HK", "772.HK", "780.HK", "3888.HK", "9626.HK", "6618.HK", "241.HK",
        "9898.HK", "6060.HK", "2013.HK", "1797.HK", "992.HK", "909.HK", "2018.HK", "2382.HK",
        "285.HK", "6690.HK", "1691.HK", "2038.HK", "669.HK", "1833.HK", "6677.HK", "1119.HK",
        "9969.HK", "3738.HK", "2096.HK", "6855.HK", "3709.HK", "1137.HK", "6098.HK", "302.HK",
        "522.HK", "1357.HK", "9961.HK", "1478.HK", "354.HK", "9901.HK", "9911.HK", "6058.HK",
        "763.HK", "552.HK", "1686.HK", "303.HK", "179.HK", "2513.HK", "100.HK",
    ];
    const SEMICONDUCTOR: &[&str] = &[
        "981.HK", "1347.HK", "2518.HK", "1385.HK", "6869.HK", "6082.HK", "3896.HK", "6809.HK",
        "600.HK",
    ];
    const FINANCE: &[&str] = &[
        "5.HK", "388.HK", "1398.HK", "3988.HK", "939.HK", "1288.HK", "2388.HK", "11.HK", "3328.HK",
        "1658.HK", "6881.HK", "6030.HK", "3908.HK", "6886.HK", "3968.HK", "1988.HK", "998.HK",
        "1963.HK", "6818.HK", "2066.HK", "6837.HK", "1776.HK", "1359.HK", "6199.HK", "2799.HK",
        "3618.HK", "1916.HK", "2611.HK", "3698.HK", "1578.HK", "2016.HK", "6196.HK", "1461.HK",
        "2356.HK", "440.HK", "23.HK", "1111.HK", "6178.HK", "1336.HK", "3958.HK", "1375.HK",
        "3903.HK", "412.HK", "2858.HK", "6099.HK", "2888.HK",
    ];
    const ENERGY: &[&str] = &[
        "883.HK", "857.HK", "386.HK", "1088.HK", "2688.HK", "384.HK", "1193.HK", "135.HK",
        "1171.HK", "3983.HK", "1600.HK", "467.HK", "2883.HK", "3899.HK", "1083.HK", "8270.HK",
    ];
    const TELECOM: &[&str] = &["941.HK", "762.HK", "728.HK", "6823.HK", "215.HK"];
    const PROPERTY: &[&str] = &[
        "16.HK", "1109.HK", "688.HK", "1113.HK", "17.HK", "12.HK", "101.HK", "823.HK", "1997.HK",
        "960.HK", "3383.HK", "884.HK", "2202.HK", "1030.HK", "123.HK", "119.HK", "3900.HK",
        "2777.HK", "81.HK", "754.HK", "2669.HK", "1918.HK", "813.HK", "2007.HK", "83.HK", "14.HK",
        "1972.HK", "778.HK", "87001.HK", "405.HK", "1908.HK", "6158.HK", "9979.HK", "3377.HK",
        "1638.HK", "272.HK", "2868.HK", "1238.HK", "2778.HK", "435.HK", "808.HK",
    ];
    const CONSUMER: &[&str] = &[
        "1929.HK", "2020.HK", "6862.HK", "9633.HK", "2319.HK", "291.HK", "168.HK", "322.HK",
        "151.HK", "2331.HK", "9987.HK", "220.HK", "6186.HK", "1044.HK", "3799.HK", "6969.HK",
        "9922.HK", "1458.HK", "6808.HK", "3331.HK", "1910.HK", "2255.HK", "9992.HK", "6993.HK",
        "9995.HK", "3998.HK", "9660.HK", "6110.HK", "116.HK", "590.HK", "3319.HK", "1579.HK",
        "9869.HK", "336.HK", "345.HK", "1361.HK", "6049.HK", "1212.HK", "9688.HK", "1733.HK",
        "69.HK", "551.HK",
    ];
    const HEALTHCARE: &[&str] = &[
        "2269.HK", "1177.HK", "2359.HK", "1093.HK", "6160.HK", "2616.HK", "3692.HK", "1801.HK",
        "2196.HK", "6185.HK", "2171.HK", "1513.HK", "6127.HK", "570.HK", "867.HK", "6622.HK",
        "1681.HK", "2607.HK", "3320.HK", "2142.HK", "6996.HK", "1066.HK", "1302.HK", "3613.HK",
        "2186.HK", "1530.HK", "9926.HK", "1877.HK", "1548.HK", "2126.HK", "6616.HK", "1539.HK",
        "6699.HK",
    ];
    const UTILITIES: &[&str] = &[
        "2.HK", "3.HK", "6.HK", "836.HK", "1038.HK", "902.HK", "1071.HK", "816.HK", "1816.HK",
        "1868.HK", "579.HK", "956.HK", "371.HK", "270.HK", "855.HK", "2380.HK", "1798.HK",
        "1799.HK", "968.HK", "2208.HK",
    ];
    const INSURANCE: &[&str] = &[
        "2318.HK", "1299.HK", "2628.HK", "2601.HK", "966.HK", "1339.HK", "1508.HK",
    ];
    const AUTO: &[&str] = &[
        "9868.HK", "2015.HK", "1211.HK", "175.HK", "2333.HK", "9863.HK", "2238.HK", "1114.HK",
        "6978.HK", "1958.HK", "2039.HK", "1268.HK", "489.HK", "2488.HK",
    ];
    const MATERIALS: &[&str] = &[
        "2259.HK", "2899.HK", "914.HK", "2600.HK", "358.HK", "3323.HK", "1818.HK", "3993.HK",
        "1138.HK", "691.HK", "1208.HK", "2009.HK", "323.HK", "347.HK", "1787.HK", "6865.HK",
        "3606.HK", "546.HK", "1164.HK", "189.HK",
    ];
    const INDUSTRIAL: &[&str] = &[
        "1186.HK", "390.HK", "1766.HK", "1800.HK", "3311.HK", "1072.HK", "2727.HK", "1157.HK",
        "3339.HK", "3898.HK", "696.HK", "1880.HK", "586.HK", "1888.HK", "1052.HK", "107.HK",
        "548.HK", "995.HK", "177.HK", "576.HK", "1882.HK", "1618.HK", "1133.HK", "2357.HK",
    ];
    const CONGLOMERATE: &[&str] = &[
        "1.HK", "19.HK", "4.HK", "267.HK", "27.HK", "10.HK", "66.HK", "293.HK", "683.HK", "659.HK",
        "20.HK", "880.HK", "1128.HK", "2282.HK", "6883.HK", "1928.HK", "142.HK", "242.HK",
        "493.HK",
    ];
    const MEDIA: &[&str] = &["1060.HK", "2400.HK", "799.HK", "777.HK", "484.HK"];
    const LOGISTICS: &[&str] = &[
        "2057.HK", "2618.HK", "6139.HK", "316.HK", "144.HK", "1199.HK", "1919.HK", "1308.HK",
        "636.HK", "2343.HK", "598.HK", "2866.HK", "3378.HK", "152.HK", "694.HK", "753.HK",
        "670.HK", "1055.HK",
    ];
    const EDUCATION: &[&str] = &["1765.HK", "839.HK", "2001.HK", "1317.HK"];

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
