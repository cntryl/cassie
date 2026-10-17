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

// Alias data follows the same pinned encnames.c table as ENCODING_NAMES.
pub(super) fn char_to_encoding(input: &str) -> i64 {
    // The SQL function takes NAME: apply its 63-byte UTF-8 boundary before
    // native encnames lookup. NAME catalog/wire identity remains separate.
    let mut end = input.len().min(63);
    while !input.is_char_boundary(end) {
        end -= 1;
    }
    let input = &input[..end];
    // The pinned Darwin oracle preserves non-ASCII name bytes in its key;
    // none can match the ASCII-only alias table.
    if !input.is_ascii() {
        return -1;
    }
    let mut normalized = [0_u8; 64];
    let mut length = 0;
    for byte in input.bytes().filter(u8::is_ascii_alphanumeric) {
        normalized[length] = byte.to_ascii_lowercase();
        length += 1;
    }
    let key = &normalized[..length];
    ENCODING_ALIASES
        .binary_search_by(|(name, _)| name.as_bytes().cmp(key))
        .map_or(-1, |index| ENCODING_ALIASES[index].1)
}
const ENCODING_ALIASES: &[(&str, i64)] = &[
    ("abc", 19),
    ("alt", 20),
    ("big5", 36),
    ("euccn", 2),
    ("eucjis2004", 5),
    ("eucjp", 1),
    ("euckr", 3),
    ("euctw", 4),
    ("gb18030", 39),
    ("gbk", 37),
    ("iso88591", 8),
    ("iso885910", 13),
    ("iso885913", 14),
    ("iso885914", 15),
    ("iso885915", 16),
    ("iso885916", 17),
    ("iso88592", 9),
    ("iso88593", 10),
    ("iso88594", 11),
    ("iso88595", 25),
    ("iso88596", 26),
    ("iso88597", 27),
    ("iso88598", 28),
    ("iso88599", 12),
    ("johab", 40),
    ("koi8", 22),
    ("koi8r", 22),
    ("koi8u", 34),
    ("latin1", 8),
    ("latin10", 17),
    ("latin2", 9),
    ("latin3", 10),
    ("latin4", 11),
    ("latin5", 12),
    ("latin6", 13),
    ("latin7", 14),
    ("latin8", 15),
    ("latin9", 16),
    ("mskanji", 35),
    ("muleinternal", 7),
    ("shiftjis", 35),
    ("shiftjis2004", 41),
    ("sjis", 35),
    ("sqlascii", 0),
    ("tcvn", 19),
    ("tcvn5712", 19),
    ("uhc", 38),
    ("unicode", 6),
    ("utf8", 6),
    ("vscii", 19),
    ("win", 23),
    ("win1250", 29),
    ("win1251", 23),
    ("win1252", 24),
    ("win1253", 30),
    ("win1254", 31),
    ("win1255", 32),
    ("win1256", 18),
    ("win1257", 33),
    ("win1258", 19),
    ("win866", 20),
    ("win874", 21),
    ("win932", 35),
    ("win936", 37),
    ("win949", 38),
    ("win950", 36),
    ("windows1250", 29),
    ("windows1251", 23),
    ("windows1252", 24),
    ("windows1253", 30),
    ("windows1254", 31),
    ("windows1255", 32),
    ("windows1256", 18),
    ("windows1257", 33),
    ("windows1258", 19),
    ("windows866", 20),
    ("windows874", 21),
    ("windows932", 35),
    ("windows936", 37),
    ("windows949", 38),
    ("windows950", 36),
];
