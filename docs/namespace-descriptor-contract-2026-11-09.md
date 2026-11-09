# Namespace catalog descriptor prerequisite

For #817/#843, PostgreSQL 18.6 describes `pg_namespace.oid` as OID (26) and
`nspname` as NAME (19). Cassie currently exposes these fields as INT (23) and
TEXT (25). Jev selected correcting those two descriptors now
(`target/assembly-review/namespace-descriptor-contract-*`), with focused
text/binary, empty/nonempty, join and namespace identity/scope probes.

This changes the wire descriptors observed by existing clients. It does not
change stored namespace IDs, rename/restart behavior, row values, or database
scope. `nspowner` and `nspacl` remain existing experimental carriers and are not
claimed to represent PostgreSQL owner or ACL semantics. The broader #817 and
extension lifecycle in #843 remain incomplete. The PostgreSQL oracle was
stopped; newly authored Cassie probes have not been executed.
