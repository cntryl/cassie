//! Canonical PostgreSQL 18.6 encoding identifiers.
//!
//! Names and numeric ordering follow PostgreSQL `REL_18_6`:
//! <https://github.com/postgres/postgres/blob/REL_18_6/src/common/encnames.c>
//! <https://github.com/postgres/postgres/blob/REL_18_6/src/include/mb/pg_wchar.h>
//!
//! Lookup does not enable a different Cassie client or database encoding.

pub(super) fn encoding_to_char(encoding: i64) -> &'static str {
    usize::try_from(encoding)
        .ok()
        .and_then(|index| ENCODING_NAMES.get(index))
        .copied()
        .unwrap_or("")
}

const ENCODING_NAMES: &[&str] = &[
    "SQL_ASCII",
    "EUC_JP",
    "EUC_CN",
    "EUC_KR",
    "EUC_TW",
    "EUC_JIS_2004",
    "UTF8",
    "MULE_INTERNAL",
    "LATIN1",
    "LATIN2",
    "LATIN3",
    "LATIN4",
    "LATIN5",
    "LATIN6",
    "LATIN7",
    "LATIN8",
    "LATIN9",
    "LATIN10",
    "WIN1256",
    "WIN1258",
    "WIN866",
    "WIN874",
    "KOI8R",
    "WIN1251",
    "WIN1252",
    "ISO_8859_5",
    "ISO_8859_6",
    "ISO_8859_7",
    "ISO_8859_8",
    "WIN1250",
    "WIN1253",
    "WIN1254",
    "WIN1255",
    "WIN1257",
    "KOI8U",
    "SJIS",
    "BIG5",
    "GBK",
    "UHC",
    "GB18030",
    "JOHAB",
    "SHIFT_JIS_2004",
];
