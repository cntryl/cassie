//! PostgreSQL 18.6 timezone abbreviation catalog (PostgreSQL license).
//! Data derived from src/timezone/tznames/{Default,Australia,India}.
//!
//! PostgreSQL Database Management System
//! (also known as Postgres, formerly known as Postgres95)
//!
//! Portions Copyright (c) 1996-2026, PostgreSQL Global Development Group
//!
//! Portions Copyright (c) 1994, The Regents of the University of California
//!
//! Permission to use, copy, modify, and distribute this software and its
//! documentation for any purpose, without fee, and without a written agreement
//! is hereby granted, provided that the above copyright notice and this
//! paragraph and the following two paragraphs appear in all copies.
//!
//! IN NO EVENT SHALL THE UNIVERSITY OF CALIFORNIA BE LIABLE TO ANY PARTY FOR
//! DIRECT, INDIRECT, SPECIAL, INCIDENTAL, OR CONSEQUENTIAL DAMAGES, INCLUDING
//! LOST PROFITS, ARISING OUT OF THE USE OF THIS SOFTWARE AND ITS
//! DOCUMENTATION, EVEN IF THE UNIVERSITY OF CALIFORNIA HAS BEEN ADVISED OF THE
//! POSSIBILITY OF SUCH DAMAGE.
//!
//! THE UNIVERSITY OF CALIFORNIA SPECIFICALLY DISCLAIMS ANY WARRANTIES,
//! INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY
//! AND FITNESS FOR A PARTICULAR PURPOSE.  THE SOFTWARE PROVIDED HEREUNDER IS
//! ON AN "AS IS" BASIS, AND THE UNIVERSITY OF CALIFORNIA HAS NO OBLIGATIONS TO
//! PROVIDE MAINTENANCE, SUPPORT, UPDATES, ENHANCEMENTS, OR MODIFICATIONS.
use super::super::zone::AbbreviationSet;

#[derive(Clone, Copy, Debug)]
pub(crate) enum ZoneRule {
    Fixed(i32),
    Dynamic(&'static str),
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ZoneAbbreviation {
    pub(crate) name: &'static str,
    pub(crate) rule: ZoneRule,
}

pub(super) fn find(chars: &[char], set: AbbreviationSet) -> Option<(ZoneAbbreviation, usize)> {
    let text: String = chars
        .iter()
        .take(16)
        .take_while(|c| c.is_ascii_alphabetic())
        .map(char::to_ascii_lowercase)
        .collect();
    let overrides = match set {
        AbbreviationSet::Default => &[][..],
        AbbreviationSet::Australia => AUSTRALIA,
        AbbreviationSet::India => INDIA,
    };
    DEFAULT
        .iter()
        .chain(overrides)
        .filter(|(name, _)| text.starts_with(*name))
        .max_by_key(|(name, _)| name.len())
        .map(|(name, rule)| (ZoneAbbreviation { name, rule: *rule }, name.len()))
}

pub(super) fn find_exact(name: &str, set: AbbreviationSet) -> Option<ZoneAbbreviation> {
    if name.len() > 6 || !name.bytes().all(|byte| byte.is_ascii_alphabetic()) {
        return None;
    }
    let overrides = match set {
        AbbreviationSet::Default => &[][..],
        AbbreviationSet::Australia => AUSTRALIA,
        AbbreviationSet::India => INDIA,
    };
    overrides
        .iter()
        .chain(DEFAULT)
        .find(|(abbreviation, _)| abbreviation.eq_ignore_ascii_case(name))
        .map(|(name, rule)| ZoneAbbreviation { name, rule: *rule })
}

const DEFAULT: &[(&str, ZoneRule)] = &[
    ("eat", ZoneRule::Fixed(10800)),
    ("sast", ZoneRule::Fixed(7200)),
    ("wat", ZoneRule::Fixed(3600)),
    ("act", ZoneRule::Fixed(-18000)),
    ("akdt", ZoneRule::Fixed(-28800)),
    ("akst", ZoneRule::Fixed(-32400)),
    ("art", ZoneRule::Dynamic("America/Argentina/Buenos_Aires")),
    ("arst", ZoneRule::Dynamic("America/Argentina/Buenos_Aires")),
    ("bot", ZoneRule::Fixed(-14400)),
    ("bra", ZoneRule::Fixed(-10800)),
    ("brst", ZoneRule::Fixed(-7200)),
    ("brt", ZoneRule::Fixed(-10800)),
    ("cot", ZoneRule::Fixed(-18000)),
    ("cdt", ZoneRule::Fixed(-18000)),
    ("clst", ZoneRule::Fixed(-10800)),
    ("clt", ZoneRule::Dynamic("America/Santiago")),
    ("cst", ZoneRule::Fixed(-21600)),
    ("edt", ZoneRule::Fixed(-14400)),
    ("egst", ZoneRule::Fixed(0)),
    ("egt", ZoneRule::Fixed(-3600)),
    ("est", ZoneRule::Fixed(-18000)),
    ("fnt", ZoneRule::Fixed(-7200)),
    ("fnst", ZoneRule::Fixed(-3600)),
    ("gft", ZoneRule::Fixed(-10800)),
    ("gyt", ZoneRule::Dynamic("America/Guyana")),
    ("mdt", ZoneRule::Fixed(-21600)),
    ("mst", ZoneRule::Fixed(-25200)),
    ("ndt", ZoneRule::Fixed(-9000)),
    ("nft", ZoneRule::Fixed(-12600)),
    ("nst", ZoneRule::Fixed(-12600)),
    ("pet", ZoneRule::Fixed(-18000)),
    ("pdt", ZoneRule::Fixed(-25200)),
    ("pmdt", ZoneRule::Fixed(-7200)),
    ("pmst", ZoneRule::Fixed(-10800)),
    ("pst", ZoneRule::Fixed(-28800)),
    ("pyst", ZoneRule::Fixed(-10800)),
    ("pyt", ZoneRule::Dynamic("America/Asuncion")),
    ("uyst", ZoneRule::Fixed(-7200)),
    ("uyt", ZoneRule::Fixed(-10800)),
    ("vet", ZoneRule::Dynamic("America/Caracas")),
    ("wgst", ZoneRule::Fixed(-7200)),
    ("wgt", ZoneRule::Fixed(-10800)),
    ("davt", ZoneRule::Dynamic("Antarctica/Davis")),
    ("ddut", ZoneRule::Fixed(36000)),
    ("mawt", ZoneRule::Dynamic("Antarctica/Mawson")),
    ("aft", ZoneRule::Fixed(16200)),
    ("almt", ZoneRule::Fixed(21600)),
    ("almst", ZoneRule::Fixed(25200)),
    ("amst", ZoneRule::Dynamic("Asia/Yerevan")),
    ("amt", ZoneRule::Fixed(-14400)),
    ("anast", ZoneRule::Dynamic("Asia/Anadyr")),
    ("anat", ZoneRule::Dynamic("Asia/Anadyr")),
    ("azst", ZoneRule::Dynamic("Asia/Baku")),
    ("azt", ZoneRule::Dynamic("Asia/Baku")),
    ("bdt", ZoneRule::Fixed(21600)),
    ("bnt", ZoneRule::Fixed(28800)),
    ("bort", ZoneRule::Fixed(28800)),
    ("btt", ZoneRule::Fixed(21600)),
    ("cct", ZoneRule::Fixed(28800)),
    ("gest", ZoneRule::Dynamic("Asia/Tbilisi")),
    ("get", ZoneRule::Dynamic("Asia/Tbilisi")),
    ("hkt", ZoneRule::Fixed(28800)),
    ("ict", ZoneRule::Fixed(25200)),
    ("idt", ZoneRule::Fixed(10800)),
    ("irkst", ZoneRule::Dynamic("Asia/Irkutsk")),
    ("irkt", ZoneRule::Dynamic("Asia/Irkutsk")),
    ("irt", ZoneRule::Fixed(12600)),
    ("ist", ZoneRule::Fixed(7200)),
    ("jayt", ZoneRule::Fixed(32400)),
    ("jst", ZoneRule::Fixed(32400)),
    ("kdt", ZoneRule::Fixed(36000)),
    ("kgst", ZoneRule::Fixed(21600)),
    ("kgt", ZoneRule::Dynamic("Asia/Bishkek")),
    ("krast", ZoneRule::Dynamic("Asia/Krasnoyarsk")),
    ("krat", ZoneRule::Dynamic("Asia/Krasnoyarsk")),
    ("kst", ZoneRule::Fixed(32400)),
    ("lkt", ZoneRule::Dynamic("Asia/Colombo")),
    ("magst", ZoneRule::Dynamic("Asia/Magadan")),
    ("magt", ZoneRule::Dynamic("Asia/Magadan")),
    ("mmt", ZoneRule::Fixed(23400)),
    ("myt", ZoneRule::Fixed(28800)),
    ("novst", ZoneRule::Dynamic("Asia/Novosibirsk")),
    ("novt", ZoneRule::Dynamic("Asia/Novosibirsk")),
    ("npt", ZoneRule::Fixed(20700)),
    ("omsst", ZoneRule::Dynamic("Asia/Omsk")),
    ("omst", ZoneRule::Dynamic("Asia/Omsk")),
    ("petst", ZoneRule::Dynamic("Asia/Kamchatka")),
    ("pett", ZoneRule::Dynamic("Asia/Kamchatka")),
    ("pht", ZoneRule::Fixed(28800)),
    ("pkt", ZoneRule::Fixed(18000)),
    ("pkst", ZoneRule::Fixed(21600)),
    ("sgt", ZoneRule::Dynamic("Asia/Singapore")),
    ("tjt", ZoneRule::Fixed(18000)),
    ("tmt", ZoneRule::Dynamic("Asia/Ashgabat")),
    ("ulast", ZoneRule::Fixed(32400)),
    ("ulat", ZoneRule::Dynamic("Asia/Ulaanbaatar")),
    ("uzst", ZoneRule::Fixed(21600)),
    ("uzt", ZoneRule::Fixed(18000)),
    ("vlast", ZoneRule::Dynamic("Asia/Vladivostok")),
    ("vlat", ZoneRule::Dynamic("Asia/Vladivostok")),
    ("xjt", ZoneRule::Fixed(21600)),
    ("yakst", ZoneRule::Dynamic("Asia/Yakutsk")),
    ("yakt", ZoneRule::Dynamic("Asia/Yakutsk")),
    ("yekst", ZoneRule::Fixed(21600)),
    ("yekt", ZoneRule::Dynamic("Asia/Yekaterinburg")),
    ("adt", ZoneRule::Fixed(-10800)),
    ("ast", ZoneRule::Fixed(-14400)),
    ("azost", ZoneRule::Fixed(0)),
    ("azot", ZoneRule::Fixed(-3600)),
    ("fkst", ZoneRule::Dynamic("Atlantic/Stanley")),
    ("fkt", ZoneRule::Dynamic("Atlantic/Stanley")),
    ("acsst", ZoneRule::Fixed(37800)),
    ("acdt", ZoneRule::Fixed(37800)),
    ("acst", ZoneRule::Fixed(34200)),
    ("acwst", ZoneRule::Fixed(31500)),
    ("aesst", ZoneRule::Fixed(39600)),
    ("aedt", ZoneRule::Fixed(39600)),
    ("aest", ZoneRule::Fixed(36000)),
    ("awsst", ZoneRule::Fixed(32400)),
    ("awst", ZoneRule::Fixed(28800)),
    ("cadt", ZoneRule::Fixed(37800)),
    ("cast", ZoneRule::Fixed(34200)),
    ("lhdt", ZoneRule::Dynamic("Australia/Lord_Howe")),
    ("lhst", ZoneRule::Fixed(37800)),
    ("ligt", ZoneRule::Fixed(36000)),
    ("nzt", ZoneRule::Fixed(43200)),
    ("sadt", ZoneRule::Fixed(37800)),
    ("wadt", ZoneRule::Fixed(28800)),
    ("wast", ZoneRule::Fixed(25200)),
    ("wdt", ZoneRule::Fixed(32400)),
    ("gmt", ZoneRule::Fixed(0)),
    ("uct", ZoneRule::Fixed(0)),
    ("ut", ZoneRule::Fixed(0)),
    ("utc", ZoneRule::Fixed(0)),
    ("z", ZoneRule::Fixed(0)),
    ("zulu", ZoneRule::Fixed(0)),
    ("bst", ZoneRule::Fixed(3600)),
    ("bdst", ZoneRule::Fixed(7200)),
    ("cest", ZoneRule::Fixed(7200)),
    ("cet", ZoneRule::Fixed(3600)),
    ("cetdst", ZoneRule::Fixed(7200)),
    ("eest", ZoneRule::Fixed(10800)),
    ("eet", ZoneRule::Fixed(7200)),
    ("eetdst", ZoneRule::Fixed(10800)),
    ("fet", ZoneRule::Fixed(10800)),
    ("mest", ZoneRule::Fixed(7200)),
    ("mesz", ZoneRule::Fixed(7200)),
    ("met", ZoneRule::Fixed(3600)),
    ("metdst", ZoneRule::Fixed(7200)),
    ("mez", ZoneRule::Fixed(3600)),
    ("msd", ZoneRule::Fixed(14400)),
    ("msk", ZoneRule::Dynamic("Europe/Moscow")),
    ("volt", ZoneRule::Dynamic("Europe/Volgograd")),
    ("wet", ZoneRule::Fixed(0)),
    ("wetdst", ZoneRule::Fixed(3600)),
    ("cxt", ZoneRule::Fixed(25200)),
    ("iot", ZoneRule::Dynamic("Indian/Chagos")),
    ("mut", ZoneRule::Fixed(14400)),
    ("must", ZoneRule::Fixed(18000)),
    ("mvt", ZoneRule::Fixed(18000)),
    ("ret", ZoneRule::Fixed(14400)),
    ("sct", ZoneRule::Fixed(14400)),
    ("tft", ZoneRule::Fixed(18000)),
    ("chadt", ZoneRule::Fixed(49500)),
    ("chast", ZoneRule::Fixed(45900)),
    ("chut", ZoneRule::Fixed(36000)),
    ("ckt", ZoneRule::Dynamic("Pacific/Rarotonga")),
    ("easst", ZoneRule::Dynamic("Pacific/Easter")),
    ("east", ZoneRule::Dynamic("Pacific/Easter")),
    ("fjst", ZoneRule::Fixed(46800)),
    ("fjt", ZoneRule::Fixed(43200)),
    ("galt", ZoneRule::Fixed(-21600)),
    ("gamt", ZoneRule::Fixed(-32400)),
    ("gilt", ZoneRule::Fixed(43200)),
    ("hst", ZoneRule::Fixed(-36000)),
    ("kost", ZoneRule::Dynamic("Pacific/Kosrae")),
    ("lint", ZoneRule::Dynamic("Pacific/Kiritimati")),
    ("mart", ZoneRule::Fixed(-34200)),
    ("mht", ZoneRule::Fixed(43200)),
    ("mpt", ZoneRule::Fixed(36000)),
    ("nut", ZoneRule::Dynamic("Pacific/Niue")),
    ("nzdt", ZoneRule::Fixed(46800)),
    ("nzst", ZoneRule::Fixed(43200)),
    ("pgt", ZoneRule::Fixed(36000)),
    ("pont", ZoneRule::Fixed(39600)),
    ("pwt", ZoneRule::Fixed(32400)),
    ("taht", ZoneRule::Fixed(-36000)),
    ("tkt", ZoneRule::Dynamic("Pacific/Fakaofo")),
    ("tot", ZoneRule::Fixed(46800)),
    ("trut", ZoneRule::Fixed(36000)),
    ("tvt", ZoneRule::Fixed(43200)),
    ("vut", ZoneRule::Fixed(39600)),
    ("wakt", ZoneRule::Fixed(43200)),
    ("wft", ZoneRule::Fixed(43200)),
    ("yapt", ZoneRule::Fixed(36000)),
];

const AUSTRALIA: &[(&str, ZoneRule)] = &[
    ("cst", ZoneRule::Fixed(34200)),
    ("east", ZoneRule::Fixed(36000)),
    ("est", ZoneRule::Fixed(36000)),
    ("sast", ZoneRule::Fixed(34200)),
    ("sat", ZoneRule::Fixed(34200)),
    ("wst", ZoneRule::Fixed(28800)),
];

const INDIA: &[(&str, ZoneRule)] = &[("ist", ZoneRule::Fixed(19800))];
