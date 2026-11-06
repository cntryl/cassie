# Formatted timezone abbreviations under #815

`TZ` in a timestamp/date format resolves the longest recognized prefix from
the active PostgreSQL 18.6 `timezone_abbreviations` set. Fixed abbreviations
use that set's signed offset; dynamic abbreviations resolve against their
catalogued IANA zone at the parsed local date, including timezone transitions.
The selected sets are Default, Australia and India. Numeric `TZ` follows the
`OF` displacement rules. A later recognized `TZ` supersedes earlier numeric
fields; later numeric fields again take precedence. Unknown alphabetic inputs
return `22007`. `to_date` validates the abbreviation without needing its offset.

PostgreSQL 18.6 confirmed EST, EDT, India IST, Australia EST, unknown JUNK,
numeric `TZ -00:30`, both mixed-field orders, EST followed by trailing text,
and historical MSK/YAKT. In the 2011 Moscow spring gap and 2014 fall overlap,
the MSK result follows the parsed date and the abbreviation-specific offset.
Oracle receipts are under ignored `target/scalar-oracle/timezone-abbrev*.json`.

Jev selected the private session-aware metadata path with the complete fixed
and dynamic abbreviation catalog, preserving public `Parsed` and `parse`
signatures, with probability 0.91. Its 0.93 gap on treating historical MSK as
fixed was confirmed by native probes and must be corrected. The session's
active abbreviation set selects the name; the session's own timezone is not
used to resolve a catalogued abbreviation. This is a runtime semantics repair,
without a new persistent representation, storage layer or Midge capability.

Regressions were authored before production edits and remain unexecuted until
the assembled gate. No pre-fix RED or post-fix passing result is claimed.

The catalog was transcribed from PostgreSQL tag `REL_18_6`, files
[`Default`](https://github.com/postgres/postgres/blob/REL_18_6/src/timezone/tznames/Default),
[`Australia`](https://github.com/postgres/postgres/blob/REL_18_6/src/timezone/tznames/Australia),
and [`India`](https://github.com/postgres/postgres/blob/REL_18_6/src/timezone/tznames/India).
The default contains 145 fixed and 50 dynamic entries; the overrides contain
six and one fixed entries respectively. All 36 distinct dynamic zone names
appear in the pinned `jiff-tzdb 0.1.9` bundled name table, a source check only;
runtime lookup remains unexecuted. An unavailable bundled zone returns `22007`
instead of substituting a fabricated fixed offset. Dynamic lookup uses the
existing Jiff representable-range normalization, which repeats the Gregorian
calendar for timestamps beyond Jiff's supported years; those extreme-year
semantics remain subject to the existing zone implementation's bounds.
The public `format::parse` retains its signature and resolves dynamic offsets
using the default catalog; it returns a timestamp-range error if those date
fields cannot be represented as a timestamp. SQL `to_date` validates a name
without imposing that timestamp bound.

The PostgreSQL catalog is used under the [PostgreSQL License](https://www.postgresql.org/about/licence/).
