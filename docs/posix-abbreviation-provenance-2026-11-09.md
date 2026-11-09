# Scoped POSIX timezone abbreviation contract

For #815, PostgreSQL 18.6 treats equal standard and daylight abbreviations in a
POSIX rule as the first, standard designation. Under `AAA0AAA-1,M3.2.0,M11.1.0`
it resolves `AAA` to UTC in both January and July. With `EEE5EEE4,...`,
`EEE-5EEE-6,...`, and `EEE5:30EEE4:30,...`, it resolves EEE as UTC−05,
UTC+05, and UTC−05:30 respectively. Explicit `+` and `-` and ±23:59 were
also accepted. PostgreSQL accepted 3-, 10-, and 16-letter names, signed zero,
omitted DST offsets/rules, and both one- and two-digit minutes; lowercase `cst`
selected the session's UTC designation over global CST. It rejected a POSIX
rule with nonzero seconds at SET with 22023, while `:00` seconds were accepted.
The oracle was stopped. Pinned Jiff requires two-digit minutes/seconds and a
DST rule suffix, so the other PG-accepted spellings do not enter this path.

Jev selected an allocation-free, narrowly recognized SQL path with provenance
captured at the actual POSIX fallback branch (`target/assembly-review/temporal-
posix-provenance-contract-*`). A marker is installed under the same lock as the
exact immutable statement session and is invalidated on replacement. Only a
POSIX fallback object, an unquoted identical uppercase ASCII standard and DST
name (3–16 letters), and a standard `+/-HH[:MM[:00]]` offset within 23:59
are recognized, with two-digit minute/second components and a DST rule suffix;
the Jiff POSIX parser has already validated the full rule. The
input abbreviation must match that name case-insensitively. The POSIX sign is
reversed to obtain seconds east. All other zone origins, malformed, quoted,
nonmatching, oversized and otherwise unrecognized forms retain the legacy
resolver. For a matching grammar whose provenance later mismatches, cancellation
or deadline may win while acquiring the origin lock before the legacy fallback. The size condition is only a
fast-path eligibility test, not an accepted-input cap. Only typed timestamp and
timestamptz `timezone` overloads are in this increment.

The allocation-free parser checks lengths before scanning at most 255 borrowed
rule bytes and a 16-byte name. Unmatched forms introduce no interruption check;
a matching grammar checks query controls while acquiring the provenance lock.
The parser does not recreate a Jiff object. Provenance is meaningful only while
the control slot still owns that exact session. The slot adds `size_of::<TemporalSlot>() - size_of::<Option<Arc<TemporalSession>>>()`
bytes, including padding, to the existing controls allocation outside the query
tracker; no new heap owner is introduced. No claim is made about normal timestamp text, arbitrary IANA/TZif
classification, no-controls callers, other overloads or #815 completion.
Authored Cassie probes are unexecuted pending the consolidated build/test gate.

A further PG18.6/source probe found that matching names alone are insufficient:
`AAA0AAA-1,M3.2.0/2,M3.2.0/3` has no transitions, and PostgreSQL marks the
standard designation unused. It reports 22023 for AAA and falls back to the
global offset for an analogous CST rule. Jev therefore selected a narrower
fully parsed subset (`target/assembly-review/temporal-posix-transition-contract-*`):
both transition rules must be plain `Mmonth.week.weekday` without explicit
times, and their months must differ by 2 through 10. Both offsets are limited
to 23:59 with zero seconds. Validate both offsets and all rule fields; unsupported rules use the legacy resolver. Since omitted times
are 02:00, the shortest season and complement are at least weeks, while the
maximum accepted offset difference is under 48 hours. PostgreSQL's 400-year
`tzparse` loop therefore emits transitions and retains the standard type.
Southern order, minimum separation, offset extremes and DST seconds were also
probed against PostgreSQL 18.6. Some extreme non-minute DST settings were
rejected depending on the currently active offset, so the fast path excludes
them; the oracle was stopped. This proof and its
Cassie probes are source-only and unexecuted.
