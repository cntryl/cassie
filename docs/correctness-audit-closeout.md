# Correctness audit closeout

This record reconciles the filed findings and acceptance criteria of [round 1 (#292)](https://github.com/cntryl/cassie/issues/292) and [round 2 (#388)](https://github.com/cntryl/cassie/issues/388). GitHub state was read on 2026-10-04 against main `bbdebe7e85a6d8850ef1fdd54317078eb0fe7867`.

## Findings and verification scope

| Corpus | Findings | Closed at readback |
| --- | ---: | ---: |
| Round 1, #293–#387 | 95 | 95 |
| Round 2, #389–#511 | 123 | 123 |
| Held verification filings, #512–#570 | 59 | 59 |
| Total | 277 | 277 |

The [round 2 verification update](https://github.com/cntryl/cassie/issues/388#issuecomment-5756746448) records the disposition of all six contested findings and all 61 reviewer leads. The [same update on round 1](https://github.com/cntryl/cassie/issues/292#issuecomment-5756746260) links that work. Round 2's 22 dimensions cover the surface requested by round 1's follow-up criterion.

For 271 findings, the readback located a merged PR body reference and stored red/failing-before-fix evidence. Six other findings have explicit resolution records, listed below. This is a provenance verification and review of the systemic slices; it does not reconstruct and rerun all 277 historical pre-fix revisions. The linked records retain their original validation results, including failures and support limits.

The accompanying exact-path implementation addresses the remaining graph follow-up, [#735](https://github.com/cntryl/cassie/issues/735). Tracker closure follows required checks and merge. This closes the filed rounds; broader coverage narratives remain research leads rather than a certification that the engine has no other defects.

## Systemic delivery

| Root cause | Shared implementation and merged delivery | Regression owners |
| --- | --- | --- |
| Inconsistent identifier folding | [#745](https://github.com/cntryl/cassie/pull/745) establishes `src/sql/column_identifier.rs` across binding, execution, row schemas, indexes, search and protocol paths. | `tests/parser_types.rs`, `tests/sql_queries.rs`, `tests/storage_indexes.rs`, `tests/pgwire_core.rs`, `tests/pgwire_extended.rs` |
| Schema changes leave derived constraints or indexes stale | [#578](https://github.com/cntryl/cassie/pull/578) shares foreign-key binding and lifecycle guards; [#585](https://github.com/cntryl/cassie/pull/585) validates/backfills uniqueness; [#587](https://github.com/cntryl/cassie/pull/587) and [#602](https://github.com/cntryl/cassie/pull/602) durably clean reservations. Current owners include `schema_foreign_keys.rs`, `unique_constraint_publication.rs`, and `unique_constraint_cleanup.rs`. | `tests/sql_mutations.rs`, `tests/storage_indexes.rs` |
| Accelerated reads disagree with row scans; duplicate implementations drift | [#729](https://github.com/cntryl/cassie/pull/729) supplies 25 focused differential cases for 18 findings and declines unsafe acceleration. [#572](https://github.com/cntryl/cassie/pull/572) canonicalizes integer bounds; [#709](https://github.com/cntryl/cassie/pull/709) preserves signed-zero equivalence. Current owners include `scalar_index_constraints.rs` and `index_probe_canonicalization.rs`. | `tests/storage_indexes.rs`, `tests/domain_models.rs`, `tests/search.rs` |

## Explicit dispositions outside merged PR body references

| Finding | Resolution record |
| --- | --- |
| [#354](https://github.com/cntryl/cassie/issues/354) | [Current regression verification](https://github.com/cntryl/cassie/issues/354#issuecomment-5851212050); delivered by #585. |
| [#358](https://github.com/cntryl/cassie/issues/358) | [Shared FK binder verification](https://github.com/cntryl/cassie/issues/358#issuecomment-5898493248); delivered by #578. |
| [#380](https://github.com/cntryl/cassie/issues/380) | [Invalid finding disposition](https://github.com/cntryl/cassie/issues/380#issuecomment-5756401138); quoted COPY null behavior is a documented, test-pinned contract. |
| [#398](https://github.com/cntryl/cassie/issues/398) | [Cross-database sequence verification](https://github.com/cntryl/cassie/issues/398#issuecomment-5880357366); delivered by #592. |
| [#518](https://github.com/cntryl/cassie/issues/518) | The appended current-main verification in the issue body links #694/#711 and records literal/parameter equivalence, eight native timestamp-partition hits before/after restart, required validation and a reproducible probe. |
| [#534](https://github.com/cntryl/cassie/issues/534) | [Declared-spelling constraint verification](https://github.com/cntryl/cassie/issues/534#issuecomment-5854689712); delivered by #578. |

## Remaining operational work

[#8](https://github.com/cntryl/cassie/issues/8) still requires complete, repeated, comparable frozen-revision native Linux amd64/arm64 benchmark, recovery and endurance bundles, including variance, before operational thresholds can be promoted. [#29](https://github.com/cntryl/cassie/issues/29) depends on that evidence for its remaining local operational promotion. Its field contracts are already delivered. Audit closure does not change these evidence gates or Cassie-wide production readiness.

## Per-finding provenance

Every row below was closed at the readback. PR links identify recorded delivery or verification; the original issue/PR records remain authoritative for each finding's behavior and TDD evidence.

| Finding | Merged delivery or explicit disposition |
| --- | --- |
| [#293](https://github.com/cntryl/cassie/issues/293) | [#710](https://github.com/cntryl/cassie/pull/710) |
| [#294](https://github.com/cntryl/cassie/issues/294) | [#710](https://github.com/cntryl/cassie/pull/710) |
| [#295](https://github.com/cntryl/cassie/issues/295) | [#685](https://github.com/cntryl/cassie/pull/685) |
| [#296](https://github.com/cntryl/cassie/issues/296) | [#687](https://github.com/cntryl/cassie/pull/687) |
| [#297](https://github.com/cntryl/cassie/issues/297) | [#687](https://github.com/cntryl/cassie/pull/687) |
| [#298](https://github.com/cntryl/cassie/issues/298) | [#709](https://github.com/cntryl/cassie/pull/709) |
| [#299](https://github.com/cntryl/cassie/issues/299) | [#710](https://github.com/cntryl/cassie/pull/710) |
| [#300](https://github.com/cntryl/cassie/issues/300) | [#741](https://github.com/cntryl/cassie/pull/741) |
| [#301](https://github.com/cntryl/cassie/issues/301) | [#670](https://github.com/cntryl/cassie/pull/670) |
| [#302](https://github.com/cntryl/cassie/issues/302) | [#711](https://github.com/cntryl/cassie/pull/711) |
| [#303](https://github.com/cntryl/cassie/issues/303) | [#714](https://github.com/cntryl/cassie/pull/714) |
| [#304](https://github.com/cntryl/cassie/issues/304) | [#691](https://github.com/cntryl/cassie/pull/691) |
| [#305](https://github.com/cntryl/cassie/issues/305) | [#648](https://github.com/cntryl/cassie/pull/648) |
| [#306](https://github.com/cntryl/cassie/issues/306) | [#644](https://github.com/cntryl/cassie/pull/644) |
| [#307](https://github.com/cntryl/cassie/issues/307) | [#648](https://github.com/cntryl/cassie/pull/648) |
| [#308](https://github.com/cntryl/cassie/issues/308) | [#644](https://github.com/cntryl/cassie/pull/644) |
| [#309](https://github.com/cntryl/cassie/issues/309) | [#644](https://github.com/cntryl/cassie/pull/644) |
| [#310](https://github.com/cntryl/cassie/issues/310) | [#693](https://github.com/cntryl/cassie/pull/693) |
| [#311](https://github.com/cntryl/cassie/issues/311) | [#717](https://github.com/cntryl/cassie/pull/717) |
| [#312](https://github.com/cntryl/cassie/issues/312) | [#717](https://github.com/cntryl/cassie/pull/717) |
| [#313](https://github.com/cntryl/cassie/issues/313) | [#715](https://github.com/cntryl/cassie/pull/715) |
| [#314](https://github.com/cntryl/cassie/issues/314) | [#743](https://github.com/cntryl/cassie/pull/743) |
| [#315](https://github.com/cntryl/cassie/issues/315) | [#668](https://github.com/cntryl/cassie/pull/668) |
| [#316](https://github.com/cntryl/cassie/issues/316) | [#678](https://github.com/cntryl/cassie/pull/678) |
| [#317](https://github.com/cntryl/cassie/issues/317) | [#679](https://github.com/cntryl/cassie/pull/679), [#676](https://github.com/cntryl/cassie/pull/676) |
| [#318](https://github.com/cntryl/cassie/issues/318) | [#580](https://github.com/cntryl/cassie/pull/580) |
| [#319](https://github.com/cntryl/cassie/issues/319) | [#714](https://github.com/cntryl/cassie/pull/714) |
| [#320](https://github.com/cntryl/cassie/issues/320) | [#581](https://github.com/cntryl/cassie/pull/581) |
| [#321](https://github.com/cntryl/cassie/issues/321) | [#727](https://github.com/cntryl/cassie/pull/727) |
| [#322](https://github.com/cntryl/cassie/issues/322) | [#676](https://github.com/cntryl/cassie/pull/676) |
| [#323](https://github.com/cntryl/cassie/issues/323) | [#698](https://github.com/cntryl/cassie/pull/698), [#676](https://github.com/cntryl/cassie/pull/676) |
| [#324](https://github.com/cntryl/cassie/issues/324) | [#727](https://github.com/cntryl/cassie/pull/727) |
| [#325](https://github.com/cntryl/cassie/issues/325) | [#696](https://github.com/cntryl/cassie/pull/696) |
| [#326](https://github.com/cntryl/cassie/issues/326) | [#727](https://github.com/cntryl/cassie/pull/727) |
| [#327](https://github.com/cntryl/cassie/issues/327) | [#664](https://github.com/cntryl/cassie/pull/664) |
| [#328](https://github.com/cntryl/cassie/issues/328) | [#674](https://github.com/cntryl/cassie/pull/674), [#671](https://github.com/cntryl/cassie/pull/671), [#664](https://github.com/cntryl/cassie/pull/664) |
| [#329](https://github.com/cntryl/cassie/issues/329) | [#683](https://github.com/cntryl/cassie/pull/683) |
| [#330](https://github.com/cntryl/cassie/issues/330) | [#729](https://github.com/cntryl/cassie/pull/729) |
| [#331](https://github.com/cntryl/cassie/issues/331) | [#729](https://github.com/cntryl/cassie/pull/729) |
| [#332](https://github.com/cntryl/cassie/issues/332) | [#729](https://github.com/cntryl/cassie/pull/729) |
| [#333](https://github.com/cntryl/cassie/issues/333) | [#729](https://github.com/cntryl/cassie/pull/729) |
| [#334](https://github.com/cntryl/cassie/issues/334) | [#732](https://github.com/cntryl/cassie/pull/732) |
| [#335](https://github.com/cntryl/cassie/issues/335) | [#582](https://github.com/cntryl/cassie/pull/582) |
| [#336](https://github.com/cntryl/cassie/issues/336) | [#583](https://github.com/cntryl/cassie/pull/583) |
| [#337](https://github.com/cntryl/cassie/issues/337) | [#733](https://github.com/cntryl/cassie/pull/733) |
| [#338](https://github.com/cntryl/cassie/issues/338) | [#676](https://github.com/cntryl/cassie/pull/676) |
| [#339](https://github.com/cntryl/cassie/issues/339) | [#688](https://github.com/cntryl/cassie/pull/688), [#676](https://github.com/cntryl/cassie/pull/676), [#644](https://github.com/cntryl/cassie/pull/644) |
| [#340](https://github.com/cntryl/cassie/issues/340) | [#733](https://github.com/cntryl/cassie/pull/733) |
| [#341](https://github.com/cntryl/cassie/issues/341) | [#733](https://github.com/cntryl/cassie/pull/733), [#645](https://github.com/cntryl/cassie/pull/645) |
| [#342](https://github.com/cntryl/cassie/issues/342) | [#744](https://github.com/cntryl/cassie/pull/744), [#738](https://github.com/cntryl/cassie/pull/738) |
| [#343](https://github.com/cntryl/cassie/issues/343) | [#729](https://github.com/cntryl/cassie/pull/729) |
| [#344](https://github.com/cntryl/cassie/issues/344) | [#729](https://github.com/cntryl/cassie/pull/729) |
| [#345](https://github.com/cntryl/cassie/issues/345) | [#729](https://github.com/cntryl/cassie/pull/729) |
| [#346](https://github.com/cntryl/cassie/issues/346) | [#722](https://github.com/cntryl/cassie/pull/722) |
| [#347](https://github.com/cntryl/cassie/issues/347) | [#729](https://github.com/cntryl/cassie/pull/729) |
| [#348](https://github.com/cntryl/cassie/issues/348) | [#585](https://github.com/cntryl/cassie/pull/585), [#584](https://github.com/cntryl/cassie/pull/584) |
| [#349](https://github.com/cntryl/cassie/issues/349) | [#729](https://github.com/cntryl/cassie/pull/729) |
| [#350](https://github.com/cntryl/cassie/issues/350) | [#742](https://github.com/cntryl/cassie/pull/742), [#741](https://github.com/cntryl/cassie/pull/741) |
| [#351](https://github.com/cntryl/cassie/issues/351) | [#585](https://github.com/cntryl/cassie/pull/585) |
| [#352](https://github.com/cntryl/cassie/issues/352) | [#586](https://github.com/cntryl/cassie/pull/586) |
| [#353](https://github.com/cntryl/cassie/issues/353) | [#587](https://github.com/cntryl/cassie/pull/587) |
| [#354](https://github.com/cntryl/cassie/issues/354) | Explicit disposition above |
| [#355](https://github.com/cntryl/cassie/issues/355) | [#588](https://github.com/cntryl/cassie/pull/588) |
| [#356](https://github.com/cntryl/cassie/issues/356) | [#589](https://github.com/cntryl/cassie/pull/589) |
| [#357](https://github.com/cntryl/cassie/issues/357) | [#682](https://github.com/cntryl/cassie/pull/682) |
| [#358](https://github.com/cntryl/cassie/issues/358) | Explicit disposition above |
| [#359](https://github.com/cntryl/cassie/issues/359) | [#590](https://github.com/cntryl/cassie/pull/590), [#578](https://github.com/cntryl/cassie/pull/578) |
| [#360](https://github.com/cntryl/cassie/issues/360) | [#591](https://github.com/cntryl/cassie/pull/591), [#578](https://github.com/cntryl/cassie/pull/578) |
| [#361](https://github.com/cntryl/cassie/issues/361) | [#680](https://github.com/cntryl/cassie/pull/680) |
| [#362](https://github.com/cntryl/cassie/issues/362) | [#578](https://github.com/cntryl/cassie/pull/578) |
| [#363](https://github.com/cntryl/cassie/issues/363) | [#592](https://github.com/cntryl/cassie/pull/592) |
| [#364](https://github.com/cntryl/cassie/issues/364) | [#593](https://github.com/cntryl/cassie/pull/593) |
| [#365](https://github.com/cntryl/cassie/issues/365) | [#594](https://github.com/cntryl/cassie/pull/594) |
| [#366](https://github.com/cntryl/cassie/issues/366) | [#595](https://github.com/cntryl/cassie/pull/595) |
| [#367](https://github.com/cntryl/cassie/issues/367) | [#676](https://github.com/cntryl/cassie/pull/676), [#673](https://github.com/cntryl/cassie/pull/673) |
| [#368](https://github.com/cntryl/cassie/issues/368) | [#575](https://github.com/cntryl/cassie/pull/575) |
| [#369](https://github.com/cntryl/cassie/issues/369) | [#596](https://github.com/cntryl/cassie/pull/596) |
| [#370](https://github.com/cntryl/cassie/issues/370) | [#576](https://github.com/cntryl/cassie/pull/576) |
| [#371](https://github.com/cntryl/cassie/issues/371) | [#597](https://github.com/cntryl/cassie/pull/597) |
| [#372](https://github.com/cntryl/cassie/issues/372) | [#650](https://github.com/cntryl/cassie/pull/650), [#579](https://github.com/cntryl/cassie/pull/579) |
| [#373](https://github.com/cntryl/cassie/issues/373) | [#691](https://github.com/cntryl/cassie/pull/691) |
| [#374](https://github.com/cntryl/cassie/issues/374) | [#691](https://github.com/cntryl/cassie/pull/691) |
| [#375](https://github.com/cntryl/cassie/issues/375) | [#663](https://github.com/cntryl/cassie/pull/663), [#650](https://github.com/cntryl/cassie/pull/650) |
| [#376](https://github.com/cntryl/cassie/issues/376) | [#579](https://github.com/cntryl/cassie/pull/579) |
| [#377](https://github.com/cntryl/cassie/issues/377) | [#579](https://github.com/cntryl/cassie/pull/579) |
| [#378](https://github.com/cntryl/cassie/issues/378) | [#579](https://github.com/cntryl/cassie/pull/579) |
| [#379](https://github.com/cntryl/cassie/issues/379) | [#579](https://github.com/cntryl/cassie/pull/579) |
| [#380](https://github.com/cntryl/cassie/issues/380) | Explicit disposition above |
| [#381](https://github.com/cntryl/cassie/issues/381) | [#576](https://github.com/cntryl/cassie/pull/576) |
| [#382](https://github.com/cntryl/cassie/issues/382) | [#739](https://github.com/cntryl/cassie/pull/739), [#644](https://github.com/cntryl/cassie/pull/644) |
| [#383](https://github.com/cntryl/cassie/issues/383) | [#577](https://github.com/cntryl/cassie/pull/577) |
| [#384](https://github.com/cntryl/cassie/issues/384) | [#608](https://github.com/cntryl/cassie/pull/608) |
| [#385](https://github.com/cntryl/cassie/issues/385) | [#577](https://github.com/cntryl/cassie/pull/577) |
| [#386](https://github.com/cntryl/cassie/issues/386) | [#610](https://github.com/cntryl/cassie/pull/610), [#577](https://github.com/cntryl/cassie/pull/577) |
| [#387](https://github.com/cntryl/cassie/issues/387) | [#729](https://github.com/cntryl/cassie/pull/729) |
| [#389](https://github.com/cntryl/cassie/issues/389) | [#729](https://github.com/cntryl/cassie/pull/729) |
| [#390](https://github.com/cntryl/cassie/issues/390) | [#729](https://github.com/cntryl/cassie/pull/729) |
| [#391](https://github.com/cntryl/cassie/issues/391) | [#729](https://github.com/cntryl/cassie/pull/729) |
| [#392](https://github.com/cntryl/cassie/issues/392) | [#611](https://github.com/cntryl/cassie/pull/611) |
| [#393](https://github.com/cntryl/cassie/issues/393) | [#612](https://github.com/cntryl/cassie/pull/612) |
| [#394](https://github.com/cntryl/cassie/issues/394) | [#614](https://github.com/cntryl/cassie/pull/614) |
| [#395](https://github.com/cntryl/cassie/issues/395) | [#615](https://github.com/cntryl/cassie/pull/615), [#614](https://github.com/cntryl/cassie/pull/614) |
| [#396](https://github.com/cntryl/cassie/issues/396) | [#616](https://github.com/cntryl/cassie/pull/616) |
| [#397](https://github.com/cntryl/cassie/issues/397) | [#641](https://github.com/cntryl/cassie/pull/641), [#617](https://github.com/cntryl/cassie/pull/617) |
| [#398](https://github.com/cntryl/cassie/issues/398) | Explicit disposition above |
| [#399](https://github.com/cntryl/cassie/issues/399) | [#618](https://github.com/cntryl/cassie/pull/618) |
| [#400](https://github.com/cntryl/cassie/issues/400) | [#618](https://github.com/cntryl/cassie/pull/618) |
| [#401](https://github.com/cntryl/cassie/issues/401) | [#619](https://github.com/cntryl/cassie/pull/619) |
| [#402](https://github.com/cntryl/cassie/issues/402) | [#620](https://github.com/cntryl/cassie/pull/620) |
| [#403](https://github.com/cntryl/cassie/issues/403) | [#579](https://github.com/cntryl/cassie/pull/579) |
| [#404](https://github.com/cntryl/cassie/issues/404) | [#621](https://github.com/cntryl/cassie/pull/621) |
| [#405](https://github.com/cntryl/cassie/issues/405) | [#622](https://github.com/cntryl/cassie/pull/622) |
| [#406](https://github.com/cntryl/cassie/issues/406) | [#622](https://github.com/cntryl/cassie/pull/622) |
| [#407](https://github.com/cntryl/cassie/issues/407) | [#672](https://github.com/cntryl/cassie/pull/672) |
| [#408](https://github.com/cntryl/cassie/issues/408) | [#740](https://github.com/cntryl/cassie/pull/740), [#724](https://github.com/cntryl/cassie/pull/724) |
| [#409](https://github.com/cntryl/cassie/issues/409) | [#623](https://github.com/cntryl/cassie/pull/623) |
| [#410](https://github.com/cntryl/cassie/issues/410) | [#675](https://github.com/cntryl/cassie/pull/675), [#672](https://github.com/cntryl/cassie/pull/672) |
| [#411](https://github.com/cntryl/cassie/issues/411) | [#724](https://github.com/cntryl/cassie/pull/724) |
| [#412](https://github.com/cntryl/cassie/issues/412) | [#623](https://github.com/cntryl/cassie/pull/623) |
| [#413](https://github.com/cntryl/cassie/issues/413) | [#623](https://github.com/cntryl/cassie/pull/623) |
| [#414](https://github.com/cntryl/cassie/issues/414) | [#734](https://github.com/cntryl/cassie/pull/734) |
| [#415](https://github.com/cntryl/cassie/issues/415) | [#686](https://github.com/cntryl/cassie/pull/686) |
| [#416](https://github.com/cntryl/cassie/issues/416) | [#684](https://github.com/cntryl/cassie/pull/684) |
| [#417](https://github.com/cntryl/cassie/issues/417) | [#624](https://github.com/cntryl/cassie/pull/624) |
| [#418](https://github.com/cntryl/cassie/issues/418) | [#681](https://github.com/cntryl/cassie/pull/681), [#624](https://github.com/cntryl/cassie/pull/624) |
| [#419](https://github.com/cntryl/cassie/issues/419) | [#624](https://github.com/cntryl/cassie/pull/624) |
| [#420](https://github.com/cntryl/cassie/issues/420) | [#625](https://github.com/cntryl/cassie/pull/625) |
| [#421](https://github.com/cntryl/cassie/issues/421) | [#625](https://github.com/cntryl/cassie/pull/625) |
| [#422](https://github.com/cntryl/cassie/issues/422) | [#732](https://github.com/cntryl/cassie/pull/732), [#625](https://github.com/cntryl/cassie/pull/625) |
| [#423](https://github.com/cntryl/cassie/issues/423) | [#694](https://github.com/cntryl/cassie/pull/694), [#691](https://github.com/cntryl/cassie/pull/691), [#625](https://github.com/cntryl/cassie/pull/625) |
| [#424](https://github.com/cntryl/cassie/issues/424) | [#626](https://github.com/cntryl/cassie/pull/626) |
| [#425](https://github.com/cntryl/cassie/issues/425) | [#732](https://github.com/cntryl/cassie/pull/732) |
| [#426](https://github.com/cntryl/cassie/issues/426) | [#579](https://github.com/cntryl/cassie/pull/579) |
| [#427](https://github.com/cntryl/cassie/issues/427) | [#732](https://github.com/cntryl/cassie/pull/732) |
| [#428](https://github.com/cntryl/cassie/issues/428) | [#670](https://github.com/cntryl/cassie/pull/670) |
| [#429](https://github.com/cntryl/cassie/issues/429) | [#699](https://github.com/cntryl/cassie/pull/699) |
| [#430](https://github.com/cntryl/cassie/issues/430) | [#659](https://github.com/cntryl/cassie/pull/659) |
| [#431](https://github.com/cntryl/cassie/issues/431) | [#627](https://github.com/cntryl/cassie/pull/627) |
| [#432](https://github.com/cntryl/cassie/issues/432) | [#702](https://github.com/cntryl/cassie/pull/702) |
| [#433](https://github.com/cntryl/cassie/issues/433) | [#737](https://github.com/cntryl/cassie/pull/737) |
| [#434](https://github.com/cntryl/cassie/issues/434) | [#722](https://github.com/cntryl/cassie/pull/722) |
| [#435](https://github.com/cntryl/cassie/issues/435) | [#737](https://github.com/cntryl/cassie/pull/737) |
| [#436](https://github.com/cntryl/cassie/issues/436) | [#737](https://github.com/cntryl/cassie/pull/737) |
| [#437](https://github.com/cntryl/cassie/issues/437) | [#628](https://github.com/cntryl/cassie/pull/628) |
| [#438](https://github.com/cntryl/cassie/issues/438) | [#628](https://github.com/cntryl/cassie/pull/628) |
| [#439](https://github.com/cntryl/cassie/issues/439) | [#628](https://github.com/cntryl/cassie/pull/628) |
| [#440](https://github.com/cntryl/cassie/issues/440) | [#641](https://github.com/cntryl/cassie/pull/641) |
| [#441](https://github.com/cntryl/cassie/issues/441) | [#647](https://github.com/cntryl/cassie/pull/647), [#643](https://github.com/cntryl/cassie/pull/643) |
| [#442](https://github.com/cntryl/cassie/issues/442) | [#736](https://github.com/cntryl/cassie/pull/736) |
| [#443](https://github.com/cntryl/cassie/issues/443) | [#703](https://github.com/cntryl/cassie/pull/703), [#676](https://github.com/cntryl/cassie/pull/676) |
| [#444](https://github.com/cntryl/cassie/issues/444) | [#703](https://github.com/cntryl/cassie/pull/703), [#676](https://github.com/cntryl/cassie/pull/676) |
| [#445](https://github.com/cntryl/cassie/issues/445) | [#647](https://github.com/cntryl/cassie/pull/647), [#643](https://github.com/cntryl/cassie/pull/643) |
| [#446](https://github.com/cntryl/cassie/issues/446) | [#736](https://github.com/cntryl/cassie/pull/736) |
| [#447](https://github.com/cntryl/cassie/issues/447) | [#645](https://github.com/cntryl/cassie/pull/645), [#579](https://github.com/cntryl/cassie/pull/579) |
| [#448](https://github.com/cntryl/cassie/issues/448) | [#645](https://github.com/cntryl/cassie/pull/645), [#579](https://github.com/cntryl/cassie/pull/579) |
| [#449](https://github.com/cntryl/cassie/issues/449) | [#708](https://github.com/cntryl/cassie/pull/708) |
| [#450](https://github.com/cntryl/cassie/issues/450) | [#708](https://github.com/cntryl/cassie/pull/708) |
| [#451](https://github.com/cntryl/cassie/issues/451) | [#708](https://github.com/cntryl/cassie/pull/708) |
| [#452](https://github.com/cntryl/cassie/issues/452) | [#708](https://github.com/cntryl/cassie/pull/708), [#705](https://github.com/cntryl/cassie/pull/705), [#700](https://github.com/cntryl/cassie/pull/700) |
| [#453](https://github.com/cntryl/cassie/issues/453) | [#708](https://github.com/cntryl/cassie/pull/708), [#705](https://github.com/cntryl/cassie/pull/705), [#700](https://github.com/cntryl/cassie/pull/700) |
| [#454](https://github.com/cntryl/cassie/issues/454) | [#708](https://github.com/cntryl/cassie/pull/708) |
| [#455](https://github.com/cntryl/cassie/issues/455) | [#706](https://github.com/cntryl/cassie/pull/706) |
| [#456](https://github.com/cntryl/cassie/issues/456) | [#706](https://github.com/cntryl/cassie/pull/706) |
| [#457](https://github.com/cntryl/cassie/issues/457) | [#706](https://github.com/cntryl/cassie/pull/706) |
| [#458](https://github.com/cntryl/cassie/issues/458) | [#707](https://github.com/cntryl/cassie/pull/707), [#706](https://github.com/cntryl/cassie/pull/706) |
| [#459](https://github.com/cntryl/cassie/issues/459) | [#661](https://github.com/cntryl/cassie/pull/661), [#609](https://github.com/cntryl/cassie/pull/609) |
| [#460](https://github.com/cntryl/cassie/issues/460) | [#690](https://github.com/cntryl/cassie/pull/690) |
| [#461](https://github.com/cntryl/cassie/issues/461) | [#695](https://github.com/cntryl/cassie/pull/695) |
| [#462](https://github.com/cntryl/cassie/issues/462) | [#700](https://github.com/cntryl/cassie/pull/700) |
| [#463](https://github.com/cntryl/cassie/issues/463) | [#704](https://github.com/cntryl/cassie/pull/704) |
| [#464](https://github.com/cntryl/cassie/issues/464) | [#579](https://github.com/cntryl/cassie/pull/579) |
| [#465](https://github.com/cntryl/cassie/issues/465) | [#700](https://github.com/cntryl/cassie/pull/700) |
| [#466](https://github.com/cntryl/cassie/issues/466) | [#705](https://github.com/cntryl/cassie/pull/705) |
| [#467](https://github.com/cntryl/cassie/issues/467) | [#657](https://github.com/cntryl/cassie/pull/657) |
| [#468](https://github.com/cntryl/cassie/issues/468) | [#702](https://github.com/cntryl/cassie/pull/702) |
| [#469](https://github.com/cntryl/cassie/issues/469) | [#708](https://github.com/cntryl/cassie/pull/708), [#705](https://github.com/cntryl/cassie/pull/705) |
| [#470](https://github.com/cntryl/cassie/issues/470) | [#702](https://github.com/cntryl/cassie/pull/702) |
| [#471](https://github.com/cntryl/cassie/issues/471) | [#689](https://github.com/cntryl/cassie/pull/689) |
| [#472](https://github.com/cntryl/cassie/issues/472) | [#676](https://github.com/cntryl/cassie/pull/676) |
| [#473](https://github.com/cntryl/cassie/issues/473) | [#656](https://github.com/cntryl/cassie/pull/656) |
| [#474](https://github.com/cntryl/cassie/issues/474) | [#613](https://github.com/cntryl/cassie/pull/613) |
| [#475](https://github.com/cntryl/cassie/issues/475) | [#642](https://github.com/cntryl/cassie/pull/642) |
| [#476](https://github.com/cntryl/cassie/issues/476) | [#577](https://github.com/cntryl/cassie/pull/577) |
| [#477](https://github.com/cntryl/cassie/issues/477) | [#642](https://github.com/cntryl/cassie/pull/642) |
| [#478](https://github.com/cntryl/cassie/issues/478) | [#651](https://github.com/cntryl/cassie/pull/651) |
| [#479](https://github.com/cntryl/cassie/issues/479) | [#662](https://github.com/cntryl/cassie/pull/662) |
| [#480](https://github.com/cntryl/cassie/issues/480) | [#629](https://github.com/cntryl/cassie/pull/629) |
| [#481](https://github.com/cntryl/cassie/issues/481) | [#630](https://github.com/cntryl/cassie/pull/630), [#629](https://github.com/cntryl/cassie/pull/629) |
| [#482](https://github.com/cntryl/cassie/issues/482) | [#630](https://github.com/cntryl/cassie/pull/630) |
| [#483](https://github.com/cntryl/cassie/issues/483) | [#630](https://github.com/cntryl/cassie/pull/630) |
| [#484](https://github.com/cntryl/cassie/issues/484) | [#677](https://github.com/cntryl/cassie/pull/677) |
| [#485](https://github.com/cntryl/cassie/issues/485) | [#635](https://github.com/cntryl/cassie/pull/635), [#630](https://github.com/cntryl/cassie/pull/630) |
| [#486](https://github.com/cntryl/cassie/issues/486) | [#578](https://github.com/cntryl/cassie/pull/578) |
| [#487](https://github.com/cntryl/cassie/issues/487) | [#633](https://github.com/cntryl/cassie/pull/633), [#578](https://github.com/cntryl/cassie/pull/578) |
| [#488](https://github.com/cntryl/cassie/issues/488) | [#633](https://github.com/cntryl/cassie/pull/633), [#578](https://github.com/cntryl/cassie/pull/578) |
| [#489](https://github.com/cntryl/cassie/issues/489) | [#633](https://github.com/cntryl/cassie/pull/633), [#578](https://github.com/cntryl/cassie/pull/578) |
| [#490](https://github.com/cntryl/cassie/issues/490) | [#578](https://github.com/cntryl/cassie/pull/578) |
| [#491](https://github.com/cntryl/cassie/issues/491) | [#578](https://github.com/cntryl/cassie/pull/578) |
| [#492](https://github.com/cntryl/cassie/issues/492) | [#578](https://github.com/cntryl/cassie/pull/578) |
| [#493](https://github.com/cntryl/cassie/issues/493) | [#633](https://github.com/cntryl/cassie/pull/633), [#578](https://github.com/cntryl/cassie/pull/578) |
| [#494](https://github.com/cntryl/cassie/issues/494) | [#578](https://github.com/cntryl/cassie/pull/578) |
| [#495](https://github.com/cntryl/cassie/issues/495) | [#676](https://github.com/cntryl/cassie/pull/676) |
| [#496](https://github.com/cntryl/cassie/issues/496) | [#660](https://github.com/cntryl/cassie/pull/660) |
| [#497](https://github.com/cntryl/cassie/issues/497) | [#653](https://github.com/cntryl/cassie/pull/653), [#650](https://github.com/cntryl/cassie/pull/650) |
| [#498](https://github.com/cntryl/cassie/issues/498) | [#691](https://github.com/cntryl/cassie/pull/691) |
| [#499](https://github.com/cntryl/cassie/issues/499) | [#667](https://github.com/cntryl/cassie/pull/667) |
| [#500](https://github.com/cntryl/cassie/issues/500) | [#691](https://github.com/cntryl/cassie/pull/691) |
| [#501](https://github.com/cntryl/cassie/issues/501) | [#652](https://github.com/cntryl/cassie/pull/652) |
| [#502](https://github.com/cntryl/cassie/issues/502) | [#654](https://github.com/cntryl/cassie/pull/654) |
| [#503](https://github.com/cntryl/cassie/issues/503) | [#654](https://github.com/cntryl/cassie/pull/654) |
| [#504](https://github.com/cntryl/cassie/issues/504) | [#738](https://github.com/cntryl/cassie/pull/738) |
| [#505](https://github.com/cntryl/cassie/issues/505) | [#666](https://github.com/cntryl/cassie/pull/666) |
| [#506](https://github.com/cntryl/cassie/issues/506) | [#640](https://github.com/cntryl/cassie/pull/640) |
| [#507](https://github.com/cntryl/cassie/issues/507) | [#638](https://github.com/cntryl/cassie/pull/638) |
| [#508](https://github.com/cntryl/cassie/issues/508) | [#638](https://github.com/cntryl/cassie/pull/638) |
| [#509](https://github.com/cntryl/cassie/issues/509) | [#634](https://github.com/cntryl/cassie/pull/634) |
| [#510](https://github.com/cntryl/cassie/issues/510) | [#634](https://github.com/cntryl/cassie/pull/634) |
| [#511](https://github.com/cntryl/cassie/issues/511) | [#669](https://github.com/cntryl/cassie/pull/669) |
| [#512](https://github.com/cntryl/cassie/issues/512) | [#736](https://github.com/cntryl/cassie/pull/736) |
| [#513](https://github.com/cntryl/cassie/issues/513) | [#665](https://github.com/cntryl/cassie/pull/665) |
| [#514](https://github.com/cntryl/cassie/issues/514) | [#651](https://github.com/cntryl/cassie/pull/651) |
| [#515](https://github.com/cntryl/cassie/issues/515) | [#599](https://github.com/cntryl/cassie/pull/599), [#578](https://github.com/cntryl/cassie/pull/578) |
| [#516](https://github.com/cntryl/cassie/issues/516) | [#645](https://github.com/cntryl/cassie/pull/645), [#579](https://github.com/cntryl/cassie/pull/579) |
| [#517](https://github.com/cntryl/cassie/issues/517) | [#729](https://github.com/cntryl/cassie/pull/729) |
| [#518](https://github.com/cntryl/cassie/issues/518) | Explicit disposition above |
| [#519](https://github.com/cntryl/cassie/issues/519) | [#600](https://github.com/cntryl/cassie/pull/600) |
| [#520](https://github.com/cntryl/cassie/issues/520) | [#648](https://github.com/cntryl/cassie/pull/648) |
| [#521](https://github.com/cntryl/cassie/issues/521) | [#652](https://github.com/cntryl/cassie/pull/652) |
| [#522](https://github.com/cntryl/cassie/issues/522) | [#724](https://github.com/cntryl/cassie/pull/724) |
| [#523](https://github.com/cntryl/cassie/issues/523) | [#701](https://github.com/cntryl/cassie/pull/701) |
| [#524](https://github.com/cntryl/cassie/issues/524) | [#737](https://github.com/cntryl/cassie/pull/737) |
| [#525](https://github.com/cntryl/cassie/issues/525) | [#676](https://github.com/cntryl/cassie/pull/676) |
| [#526](https://github.com/cntryl/cassie/issues/526) | [#668](https://github.com/cntryl/cassie/pull/668) |
| [#527](https://github.com/cntryl/cassie/issues/527) | [#733](https://github.com/cntryl/cassie/pull/733) |
| [#528](https://github.com/cntryl/cassie/issues/528) | [#603](https://github.com/cntryl/cassie/pull/603), [#601](https://github.com/cntryl/cassie/pull/601) |
| [#529](https://github.com/cntryl/cassie/issues/529) | [#642](https://github.com/cntryl/cassie/pull/642) |
| [#530](https://github.com/cntryl/cassie/issues/530) | [#602](https://github.com/cntryl/cassie/pull/602) |
| [#531](https://github.com/cntryl/cassie/issues/531) | [#642](https://github.com/cntryl/cassie/pull/642) |
| [#532](https://github.com/cntryl/cassie/issues/532) | [#603](https://github.com/cntryl/cassie/pull/603) |
| [#533](https://github.com/cntryl/cassie/issues/533) | [#692](https://github.com/cntryl/cassie/pull/692), [#676](https://github.com/cntryl/cassie/pull/676) |
| [#534](https://github.com/cntryl/cassie/issues/534) | Explicit disposition above |
| [#535](https://github.com/cntryl/cassie/issues/535) | [#604](https://github.com/cntryl/cassie/pull/604) |
| [#536](https://github.com/cntryl/cassie/issues/536) | [#676](https://github.com/cntryl/cassie/pull/676) |
| [#537](https://github.com/cntryl/cassie/issues/537) | [#676](https://github.com/cntryl/cassie/pull/676) |
| [#538](https://github.com/cntryl/cassie/issues/538) | [#671](https://github.com/cntryl/cassie/pull/671) |
| [#539](https://github.com/cntryl/cassie/issues/539) | [#671](https://github.com/cntryl/cassie/pull/671) |
| [#540](https://github.com/cntryl/cassie/issues/540) | [#676](https://github.com/cntryl/cassie/pull/676) |
| [#541](https://github.com/cntryl/cassie/issues/541) | [#679](https://github.com/cntryl/cassie/pull/679), [#676](https://github.com/cntryl/cassie/pull/676) |
| [#542](https://github.com/cntryl/cassie/issues/542) | [#729](https://github.com/cntryl/cassie/pull/729) |
| [#543](https://github.com/cntryl/cassie/issues/543) | [#644](https://github.com/cntryl/cassie/pull/644) |
| [#544](https://github.com/cntryl/cassie/issues/544) | [#605](https://github.com/cntryl/cassie/pull/605) |
| [#545](https://github.com/cntryl/cassie/issues/545) | [#598](https://github.com/cntryl/cassie/pull/598) |
| [#546](https://github.com/cntryl/cassie/issues/546) | [#645](https://github.com/cntryl/cassie/pull/645), [#579](https://github.com/cntryl/cassie/pull/579) |
| [#547](https://github.com/cntryl/cassie/issues/547) | [#697](https://github.com/cntryl/cassie/pull/697), [#676](https://github.com/cntryl/cassie/pull/676) |
| [#548](https://github.com/cntryl/cassie/issues/548) | [#722](https://github.com/cntryl/cassie/pull/722) |
| [#549](https://github.com/cntryl/cassie/issues/549) | [#729](https://github.com/cntryl/cassie/pull/729) |
| [#550](https://github.com/cntryl/cassie/issues/550) | [#729](https://github.com/cntryl/cassie/pull/729) |
| [#551](https://github.com/cntryl/cassie/issues/551) | [#606](https://github.com/cntryl/cassie/pull/606) |
| [#552](https://github.com/cntryl/cassie/issues/552) | [#644](https://github.com/cntryl/cassie/pull/644) |
| [#553](https://github.com/cntryl/cassie/issues/553) | [#607](https://github.com/cntryl/cassie/pull/607) |
| [#554](https://github.com/cntryl/cassie/issues/554) | [#638](https://github.com/cntryl/cassie/pull/638) |
| [#555](https://github.com/cntryl/cassie/issues/555) | [#578](https://github.com/cntryl/cassie/pull/578) |
| [#556](https://github.com/cntryl/cassie/issues/556) | [#578](https://github.com/cntryl/cassie/pull/578) |
| [#557](https://github.com/cntryl/cassie/issues/557) | [#578](https://github.com/cntryl/cassie/pull/578) |
| [#558](https://github.com/cntryl/cassie/issues/558) | [#613](https://github.com/cntryl/cassie/pull/613) |
| [#559](https://github.com/cntryl/cassie/issues/559) | [#649](https://github.com/cntryl/cassie/pull/649) |
| [#560](https://github.com/cntryl/cassie/issues/560) | [#659](https://github.com/cntryl/cassie/pull/659), [#654](https://github.com/cntryl/cassie/pull/654) |
| [#561](https://github.com/cntryl/cassie/issues/561) | [#631](https://github.com/cntryl/cassie/pull/631) |
| [#562](https://github.com/cntryl/cassie/issues/562) | [#676](https://github.com/cntryl/cassie/pull/676) |
| [#563](https://github.com/cntryl/cassie/issues/563) | [#688](https://github.com/cntryl/cassie/pull/688), [#676](https://github.com/cntryl/cassie/pull/676), [#644](https://github.com/cntryl/cassie/pull/644) |
| [#564](https://github.com/cntryl/cassie/issues/564) | [#691](https://github.com/cntryl/cassie/pull/691) |
| [#565](https://github.com/cntryl/cassie/issues/565) | [#667](https://github.com/cntryl/cassie/pull/667) |
| [#566](https://github.com/cntryl/cassie/issues/566) | [#577](https://github.com/cntryl/cassie/pull/577) |
| [#567](https://github.com/cntryl/cassie/issues/567) | [#577](https://github.com/cntryl/cassie/pull/577) |
| [#568](https://github.com/cntryl/cassie/issues/568) | [#577](https://github.com/cntryl/cassie/pull/577) |
| [#569](https://github.com/cntryl/cassie/issues/569) | [#577](https://github.com/cntryl/cassie/pull/577) |
| [#570](https://github.com/cntryl/cassie/issues/570) | [#729](https://github.com/cntryl/cassie/pull/729) |
