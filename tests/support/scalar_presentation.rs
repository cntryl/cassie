//! Fixed PostgreSQL 18.6 output witnesses; no Cassie formatter builds expected bytes.

pub struct FloatCase {
    pub bits: u64,
    pub text: &'static str,
}

pub const FLOATS: &[FloatCase] = &[
    FloatCase {
        bits: 0x0000_0000_0000_0000,
        text: "0",
    },
    FloatCase {
        bits: 0x0000_0000_0000_0001,
        text: "5e-324",
    },
    FloatCase {
        bits: 0x000f_ffff_ffff_ffff,
        text: "2.225073858507201e-308",
    },
    FloatCase {
        bits: 0x0010_0000_0000_0000,
        text: "2.2250738585072014e-308",
    },
    FloatCase {
        bits: 0x0010_0000_0000_0001,
        text: "2.225073858507202e-308",
    },
    FloatCase {
        bits: 0x0060_0000_0000_0000,
        text: "7.120236347223045e-307",
    },
    FloatCase {
        bits: 0x0100_0000_0000_0000,
        text: "7.291122019556398e-304",
    },
    FloatCase {
        bits: 0x0420_0000_0000_0000,
        text: "8.209073602596753e-289",
    },
    FloatCase {
        bits: 0x0660_0000_0000_0000,
        text: "5.641232424577593e-278",
    },
    FloatCase {
        bits: 0x0d70_0000_0000_0000,
        text: "5.858190679279809e-244",
    },
    FloatCase {
        bits: 0x0e80_0000_0000_0000,
        text: "7.678447687145631e-239",
    },
    FloatCase {
        bits: 0x0eb0_0000_0000_0000,
        text: "6.142758149716505e-238",
    },
    FloatCase {
        bits: 0x0f50_0000_0000_0000,
        text: "6.290184345309701e-235",
    },
    FloatCase {
        bits: 0x13e0_0000_0000_0000,
        text: "5.940911144672375e-213",
    },
    FloatCase {
        bits: 0x1480_0000_0000_0000,
        text: "6.083493012144512e-210",
    },
    FloatCase {
        bits: 0x1690_0000_0000_0000,
        text: "5.225680706521042e-200",
    },
    FloatCase {
        bits: 0x1730_0000_0000_0000,
        text: "5.351097043477547e-197",
    },
    FloatCase {
        bits: 0x1da0_0000_0000_0000,
        text: "5.426657103235053e-166",
    },
    FloatCase {
        bits: 0x2020_0000_0000_0000,
        text: "5.966672584960166e-154",
    },
    FloatCase {
        bits: 0x20f0_0000_0000_0000,
        text: "4.887898181599368e-150",
    },
    FloatCase {
        bits: 0x2160_0000_0000_0000,
        text: "6.256509672447191e-148",
    },
    FloatCase {
        bits: 0x2800_0000_0000_0000,
        text: "5.075883674631299e-116",
    },
    FloatCase {
        bits: 0x2910_0000_0000_0000,
        text: "6.653062250012736e-111",
    },
    FloatCase {
        bits: 0x2d70_0000_0000_0000,
        text: "7.854549544476363e-90",
    },
    FloatCase {
        bits: 0x3730_0000_0000_0000,
        text: "7.174648137343064e-43",
    },
    FloatCase {
        bits: 0x39e0_0000_0000_0000,
        text: "6.310887241768095e-30",
    },
    FloatCase {
        bits: 0x3b20_0000_0000_0000,
        text: "6.617444900424222e-24",
    },
    FloatCase {
        bits: 0x3d30_0000_0000_0000,
        text: "5.684341886080802e-14",
    },
    FloatCase {
        bits: 0x3e60_0000_0000_0000,
        text: "2.9802322387695312e-08",
    },
    FloatCase {
        bits: 0x3e70_0000_0000_0000,
        text: "5.960464477539063e-08",
    },
    FloatCase {
        bits: 0x3ee4_f8b5_88e3_68f0,
        text: "9.999999999999999e-06",
    },
    FloatCase {
        bits: 0x3ee4_f8b5_88e3_68f1,
        text: "1e-05",
    },
    FloatCase {
        bits: 0x3ee4_f8b5_88e3_68f2,
        text: "1.0000000000000003e-05",
    },
    FloatCase {
        bits: 0x3f1a_36e2_eb1c_432c,
        text: "9.999999999999999e-05",
    },
    FloatCase {
        bits: 0x3f1a_36e2_eb1c_432d,
        text: "0.0001",
    },
    FloatCase {
        bits: 0x3f1a_36e2_eb1c_432e,
        text: "0.00010000000000000002",
    },
    FloatCase {
        bits: 0x3fb9_9999_9999_9999,
        text: "0.09999999999999999",
    },
    FloatCase {
        bits: 0x3fb9_9999_9999_999a,
        text: "0.1",
    },
    FloatCase {
        bits: 0x3fb9_9999_9999_999b,
        text: "0.10000000000000002",
    },
    FloatCase {
        bits: 0x3ff3_ffff_ffff_ffff,
        text: "1.2499999999999998",
    },
    FloatCase {
        bits: 0x3ff4_0000_0000_0000,
        text: "1.25",
    },
    FloatCase {
        bits: 0x3ff4_0000_0000_0001,
        text: "1.2500000000000002",
    },
    FloatCase {
        bits: 0x42d6_bcc4_1e8f_ffff,
        text: "99999999999999.98",
    },
    FloatCase {
        bits: 0x42d6_bcc4_1e90_0000,
        text: "100000000000000",
    },
    FloatCase {
        bits: 0x42d6_bcc4_1e90_0001,
        text: "100000000000000.02",
    },
    FloatCase {
        bits: 0x4300_0000_0000_0002,
        text: "562949953421312.2",
    },
    FloatCase {
        bits: 0x4301_ed2f_becd_3d5a,
        text: "630732990556075.2",
    },
    FloatCase {
        bits: 0x430c_6bf5_2633_ffff,
        text: "999999999999999.9",
    },
    FloatCase {
        bits: 0x430c_6bf5_2634_0000,
        text: "1e+15",
    },
    FloatCase {
        bits: 0x430c_6bf5_2634_0001,
        text: "1.0000000000000001e+15",
    },
    FloatCase {
        bits: 0x4310_0000_0000_0001,
        text: "1.1258999068426242e+15",
    },
    FloatCase {
        bits: 0x4312_fdcb_e503_2709,
        text: "1.3364004357923222e+15",
    },
    FloatCase {
        bits: 0x4314_f4fb_f084_7ee1,
        text: "1.4747156108492082e+15",
    },
    FloatCase {
        bits: 0x431b_b76d_3feb_73c5,
        text: "1.9503760557252972e+15",
    },
    FloatCase {
        bits: 0x431f_ffff_ffff_fffd,
        text: "2.2517998136852472e+15",
    },
    FloatCase {
        bits: 0x4350_0000_0000_0002,
        text: "1.8014398509481992e+16",
    },
    FloatCase {
        bits: 0x435d_9240_588c_b5d6,
        text: "3.3294317543151448e+16",
    },
    FloatCase {
        bits: 0x4360_0000_0000_0002,
        text: "3.6028797018963984e+16",
    },
    FloatCase {
        bits: 0x436c_2d13_9a89_a130,
        text: "6.3446892545902976e+16",
    },
    FloatCase {
        bits: 0x4415_af1d_78b5_8c3f,
        text: "9.999999999999998e+19",
    },
    FloatCase {
        bits: 0x4415_af1d_78b5_8c40,
        text: "1e+20",
    },
    FloatCase {
        bits: 0x4415_af1d_78b5_8c41,
        text: "1.0000000000000002e+20",
    },
    FloatCase {
        bits: 0x441a_c53a_7e04_bcd8,
        text: "1.2345678901234565e+20",
    },
    FloatCase {
        bits: 0x441a_c53a_7e04_bcd9,
        text: "1.2345678901234567e+20",
    },
    FloatCase {
        bits: 0x441a_c53a_7e04_bcda,
        text: "1.2345678901234568e+20",
    },
    FloatCase {
        bits: 0x4580_0000_0000_0000,
        text: "6.189700196426902e+26",
    },
    FloatCase {
        bits: 0x4790_0000_0000_0000,
        text: "5.316911983139664e+36",
    },
    FloatCase {
        bits: 0x4830_0000_0000_0000,
        text: "5.444517870735016e+39",
    },
    FloatCase {
        bits: 0x4ab0_0000_0000_0000,
        text: "5.986310706507379e+51",
    },
    FloatCase {
        bits: 0x4b50_0000_0000_0000,
        text: "6.129982163463556e+54",
    },
    FloatCase {
        bits: 0x5120_0000_0000_0000,
        text: "6.070840288205404e+82",
    },
    FloatCase {
        bits: 0x5300_0000_0000_0000,
        text: "6.518515124270356e+91",
    },
    FloatCase {
        bits: 0x5580_0000_0000_0000,
        text: "7.167183174968974e+103",
    },
    FloatCase {
        bits: 0x5790_0000_0000_0000,
        text: "6.156563468186638e+113",
    },
    FloatCase {
        bits: 0x58d0_0000_0000_0000,
        text: "6.455624695217272e+119",
    },
    FloatCase {
        bits: 0x5940_0000_0000_0000,
        text: "8.263199609878108e+121",
    },
    FloatCase {
        bits: 0x5e00_0000_0000_0000,
        text: "6.243497100631985e+144",
    },
    FloatCase {
        bits: 0x6150_0000_0000_0000,
        text: "5.623642243178996e+160",
    },
    FloatCase {
        bits: 0x61f0_0000_0000_0000,
        text: "5.758609657015292e+163",
    },
    FloatCase {
        bits: 0x6290_0000_0000_0000,
        text: "5.896816288783659e+166",
    },
    FloatCase {
        bits: 0x63d0_0000_0000_0000,
        text: "6.183260036827614e+172",
    },
    FloatCase {
        bits: 0x6510_0000_0000_0000,
        text: "6.483618076376552e+178",
    },
    FloatCase {
        bits: 0x6c50_0000_0000_0000,
        text: "5.386379163185535e+213",
    },
    FloatCase {
        bits: 0x7220_0000_0000_0000,
        text: "5.334411546303884e+241",
    },
    FloatCase {
        bits: 0x75e0_0000_0000_0000,
        text: "6.150157786156811e+259",
    },
    FloatCase {
        bits: 0x77f0_0000_0000_0000,
        text: "5.282945311356653e+269",
    },
    FloatCase {
        bits: 0x7cf0_0000_0000_0000,
        text: "6.386688990511104e+293",
    },
    FloatCase {
        bits: 0x7fef_ffff_ffff_ffff,
        text: "1.7976931348623157e+308",
    },
    FloatCase {
        bits: 0x8000_0000_0000_0000,
        text: "-0",
    },
    FloatCase {
        bits: 0x800f_ffff_ffff_ffff,
        text: "-2.225073858507201e-308",
    },
    FloatCase {
        bits: 0x8010_0000_0000_0000,
        text: "-2.2250738585072014e-308",
    },
    FloatCase {
        bits: 0x8010_0000_0000_0001,
        text: "-2.225073858507202e-308",
    },
    FloatCase {
        bits: 0x8060_0000_0000_0000,
        text: "-7.120236347223045e-307",
    },
    FloatCase {
        bits: 0x8100_0000_0000_0000,
        text: "-7.291122019556398e-304",
    },
    FloatCase {
        bits: 0x8420_0000_0000_0000,
        text: "-8.209073602596753e-289",
    },
    FloatCase {
        bits: 0x8660_0000_0000_0000,
        text: "-5.641232424577593e-278",
    },
    FloatCase {
        bits: 0x8d70_0000_0000_0000,
        text: "-5.858190679279809e-244",
    },
    FloatCase {
        bits: 0x8e80_0000_0000_0000,
        text: "-7.678447687145631e-239",
    },
    FloatCase {
        bits: 0x8eb0_0000_0000_0000,
        text: "-6.142758149716505e-238",
    },
    FloatCase {
        bits: 0x8f50_0000_0000_0000,
        text: "-6.290184345309701e-235",
    },
    FloatCase {
        bits: 0x93e0_0000_0000_0000,
        text: "-5.940911144672375e-213",
    },
    FloatCase {
        bits: 0x9480_0000_0000_0000,
        text: "-6.083493012144512e-210",
    },
    FloatCase {
        bits: 0x9690_0000_0000_0000,
        text: "-5.225680706521042e-200",
    },
    FloatCase {
        bits: 0x9730_0000_0000_0000,
        text: "-5.351097043477547e-197",
    },
    FloatCase {
        bits: 0x9da0_0000_0000_0000,
        text: "-5.426657103235053e-166",
    },
    FloatCase {
        bits: 0xa020_0000_0000_0000,
        text: "-5.966672584960166e-154",
    },
    FloatCase {
        bits: 0xa0f0_0000_0000_0000,
        text: "-4.887898181599368e-150",
    },
    FloatCase {
        bits: 0xa160_0000_0000_0000,
        text: "-6.256509672447191e-148",
    },
    FloatCase {
        bits: 0xa800_0000_0000_0000,
        text: "-5.075883674631299e-116",
    },
    FloatCase {
        bits: 0xa910_0000_0000_0000,
        text: "-6.653062250012736e-111",
    },
    FloatCase {
        bits: 0xad70_0000_0000_0000,
        text: "-7.854549544476363e-90",
    },
    FloatCase {
        bits: 0xb730_0000_0000_0000,
        text: "-7.174648137343064e-43",
    },
    FloatCase {
        bits: 0xb9e0_0000_0000_0000,
        text: "-6.310887241768095e-30",
    },
    FloatCase {
        bits: 0xbb20_0000_0000_0000,
        text: "-6.617444900424222e-24",
    },
    FloatCase {
        bits: 0xbd30_0000_0000_0000,
        text: "-5.684341886080802e-14",
    },
    FloatCase {
        bits: 0xbe60_0000_0000_0000,
        text: "-2.9802322387695312e-08",
    },
    FloatCase {
        bits: 0xbe70_0000_0000_0000,
        text: "-5.960464477539063e-08",
    },
    FloatCase {
        bits: 0xbee4_f8b5_88e3_68f0,
        text: "-9.999999999999999e-06",
    },
    FloatCase {
        bits: 0xbee4_f8b5_88e3_68f1,
        text: "-1e-05",
    },
    FloatCase {
        bits: 0xbee4_f8b5_88e3_68f2,
        text: "-1.0000000000000003e-05",
    },
    FloatCase {
        bits: 0xbf1a_36e2_eb1c_432c,
        text: "-9.999999999999999e-05",
    },
    FloatCase {
        bits: 0xbf1a_36e2_eb1c_432d,
        text: "-0.0001",
    },
    FloatCase {
        bits: 0xbf1a_36e2_eb1c_432e,
        text: "-0.00010000000000000002",
    },
    FloatCase {
        bits: 0xbfb9_9999_9999_9999,
        text: "-0.09999999999999999",
    },
    FloatCase {
        bits: 0xbfb9_9999_9999_999a,
        text: "-0.1",
    },
    FloatCase {
        bits: 0xbfb9_9999_9999_999b,
        text: "-0.10000000000000002",
    },
    FloatCase {
        bits: 0xbff3_ffff_ffff_ffff,
        text: "-1.2499999999999998",
    },
    FloatCase {
        bits: 0xbff4_0000_0000_0000,
        text: "-1.25",
    },
    FloatCase {
        bits: 0xbff4_0000_0000_0001,
        text: "-1.2500000000000002",
    },
    FloatCase {
        bits: 0xc2d6_bcc4_1e8f_ffff,
        text: "-99999999999999.98",
    },
    FloatCase {
        bits: 0xc2d6_bcc4_1e90_0000,
        text: "-100000000000000",
    },
    FloatCase {
        bits: 0xc2d6_bcc4_1e90_0001,
        text: "-100000000000000.02",
    },
    FloatCase {
        bits: 0xc300_0000_0000_0002,
        text: "-562949953421312.2",
    },
    FloatCase {
        bits: 0xc301_ed2f_becd_3d5a,
        text: "-630732990556075.2",
    },
    FloatCase {
        bits: 0xc30c_6bf5_2633_ffff,
        text: "-999999999999999.9",
    },
    FloatCase {
        bits: 0xc30c_6bf5_2634_0000,
        text: "-1e+15",
    },
    FloatCase {
        bits: 0xc30c_6bf5_2634_0001,
        text: "-1.0000000000000001e+15",
    },
    FloatCase {
        bits: 0xc310_0000_0000_0001,
        text: "-1.1258999068426242e+15",
    },
    FloatCase {
        bits: 0xc312_fdcb_e503_2709,
        text: "-1.3364004357923222e+15",
    },
    FloatCase {
        bits: 0xc314_f4fb_f084_7ee1,
        text: "-1.4747156108492082e+15",
    },
    FloatCase {
        bits: 0xc31b_b76d_3feb_73c5,
        text: "-1.9503760557252972e+15",
    },
    FloatCase {
        bits: 0xc31f_ffff_ffff_fffd,
        text: "-2.2517998136852472e+15",
    },
    FloatCase {
        bits: 0xc350_0000_0000_0002,
        text: "-1.8014398509481992e+16",
    },
    FloatCase {
        bits: 0xc35d_9240_588c_b5d6,
        text: "-3.3294317543151448e+16",
    },
    FloatCase {
        bits: 0xc360_0000_0000_0002,
        text: "-3.6028797018963984e+16",
    },
    FloatCase {
        bits: 0xc36c_2d13_9a89_a130,
        text: "-6.3446892545902976e+16",
    },
    FloatCase {
        bits: 0xc415_af1d_78b5_8c3f,
        text: "-9.999999999999998e+19",
    },
    FloatCase {
        bits: 0xc415_af1d_78b5_8c40,
        text: "-1e+20",
    },
    FloatCase {
        bits: 0xc415_af1d_78b5_8c41,
        text: "-1.0000000000000002e+20",
    },
    FloatCase {
        bits: 0xc41a_c53a_7e04_bcd8,
        text: "-1.2345678901234565e+20",
    },
    FloatCase {
        bits: 0xc41a_c53a_7e04_bcd9,
        text: "-1.2345678901234567e+20",
    },
    FloatCase {
        bits: 0xc41a_c53a_7e04_bcda,
        text: "-1.2345678901234568e+20",
    },
    FloatCase {
        bits: 0xc580_0000_0000_0000,
        text: "-6.189700196426902e+26",
    },
    FloatCase {
        bits: 0xc790_0000_0000_0000,
        text: "-5.316911983139664e+36",
    },
    FloatCase {
        bits: 0xc830_0000_0000_0000,
        text: "-5.444517870735016e+39",
    },
    FloatCase {
        bits: 0xcab0_0000_0000_0000,
        text: "-5.986310706507379e+51",
    },
    FloatCase {
        bits: 0xcb50_0000_0000_0000,
        text: "-6.129982163463556e+54",
    },
    FloatCase {
        bits: 0xd120_0000_0000_0000,
        text: "-6.070840288205404e+82",
    },
    FloatCase {
        bits: 0xd300_0000_0000_0000,
        text: "-6.518515124270356e+91",
    },
    FloatCase {
        bits: 0xd580_0000_0000_0000,
        text: "-7.167183174968974e+103",
    },
    FloatCase {
        bits: 0xd790_0000_0000_0000,
        text: "-6.156563468186638e+113",
    },
    FloatCase {
        bits: 0xd8d0_0000_0000_0000,
        text: "-6.455624695217272e+119",
    },
    FloatCase {
        bits: 0xd940_0000_0000_0000,
        text: "-8.263199609878108e+121",
    },
    FloatCase {
        bits: 0xde00_0000_0000_0000,
        text: "-6.243497100631985e+144",
    },
    FloatCase {
        bits: 0xe150_0000_0000_0000,
        text: "-5.623642243178996e+160",
    },
    FloatCase {
        bits: 0xe1f0_0000_0000_0000,
        text: "-5.758609657015292e+163",
    },
    FloatCase {
        bits: 0xe290_0000_0000_0000,
        text: "-5.896816288783659e+166",
    },
    FloatCase {
        bits: 0xe3d0_0000_0000_0000,
        text: "-6.183260036827614e+172",
    },
    FloatCase {
        bits: 0xe510_0000_0000_0000,
        text: "-6.483618076376552e+178",
    },
    FloatCase {
        bits: 0xec50_0000_0000_0000,
        text: "-5.386379163185535e+213",
    },
    FloatCase {
        bits: 0xf220_0000_0000_0000,
        text: "-5.334411546303884e+241",
    },
    FloatCase {
        bits: 0xf5e0_0000_0000_0000,
        text: "-6.150157786156811e+259",
    },
    FloatCase {
        bits: 0xf7f0_0000_0000_0000,
        text: "-5.282945311356653e+269",
    },
    FloatCase {
        bits: 0xfcf0_0000_0000_0000,
        text: "-6.386688990511104e+293",
    },
    FloatCase {
        bits: 0xffef_ffff_ffff_ffff,
        text: "-1.7976931348623157e+308",
    },
    FloatCase {
        bits: 0x7ff0_0000_0000_0000,
        text: "Infinity",
    },
    FloatCase {
        bits: 0xfff0_0000_0000_0000,
        text: "-Infinity",
    },
    FloatCase {
        bits: 0x7ff8_0000_0000_0000,
        text: "NaN",
    },
];

pub struct TemporalCase {
    pub input: &'static str,
    pub text: &'static str,
    pub micros: i64,
}

pub const TIMESTAMPS: &[TemporalCase] = &[
    TemporalCase {
        input: "2000-01-01 00:00:00",
        text: "2000-01-01 00:00:00",
        micros: 0,
    },
    TemporalCase {
        input: "1999-12-31 23:59:59.999999",
        text: "1999-12-31 23:59:59.999999",
        micros: -1,
    },
    TemporalCase {
        input: "2000-01-01 00:00:00.000001",
        text: "2000-01-01 00:00:00.000001",
        micros: 1,
    },
    TemporalCase {
        input: "2024-02-29 12:34:56.120000",
        text: "2024-02-29 12:34:56.12",
        micros: 762_525_296_120_000,
    },
    TemporalCase {
        input: "2024-02-29 12:34:56.000001",
        text: "2024-02-29 12:34:56.000001",
        micros: 762_525_296_000_001,
    },
    TemporalCase {
        input: "2024-02-29 12:34:56.100000",
        text: "2024-02-29 12:34:56.1",
        micros: 762_525_296_100_000,
    },
    TemporalCase {
        input: "2024-02-29 12:34:56.123456",
        text: "2024-02-29 12:34:56.123456",
        micros: 762_525_296_123_456,
    },
    TemporalCase {
        input: "2024-02-29 12:34:56.999999",
        text: "2024-02-29 12:34:56.999999",
        micros: 762_525_296_999_999,
    },
    TemporalCase {
        input: "1900-01-01 00:00:00",
        text: "1900-01-01 00:00:00",
        micros: -3_155_673_600_000_000,
    },
    TemporalCase {
        input: "2100-01-01 00:00:00",
        text: "2100-01-01 00:00:00",
        micros: 3_155_760_000_000_000,
    },
    TemporalCase {
        input: "0001-01-01 00:00:00",
        text: "0001-01-01 00:00:00",
        micros: -63_082_281_600_000_000,
    },
    TemporalCase {
        input: "9999-12-31 23:59:59.999999",
        text: "9999-12-31 23:59:59.999999",
        micros: 252_455_615_999_999_999,
    },
];

pub const TIMES: &[TemporalCase] = &[
    TemporalCase {
        input: "00:00:00",
        text: "00:00:00",
        micros: 0,
    },
    TemporalCase {
        input: "23:59:59.999999",
        text: "23:59:59.999999",
        micros: 86_399_999_999,
    },
    TemporalCase {
        input: "12:34:56.120000",
        text: "12:34:56.12",
        micros: 45_296_120_000,
    },
    TemporalCase {
        input: "12:34:56.100000",
        text: "12:34:56.1",
        micros: 45_296_100_000,
    },
    TemporalCase {
        input: "12:34:56.000001",
        text: "12:34:56.000001",
        micros: 45_296_000_001,
    },
    TemporalCase {
        input: "12:34:56.123456",
        text: "12:34:56.123456",
        micros: 45_296_123_456,
    },
    TemporalCase {
        input: "12:34:56.999999",
        text: "12:34:56.999999",
        micros: 45_296_999_999,
    },
    TemporalCase {
        input: "12:34:50.000000",
        text: "12:34:50",
        micros: 45_290_000_000,
    },
];
