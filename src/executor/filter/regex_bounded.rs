//! Request-owned native regex backing with pre-admitted NFA and search state.
//!
//! Bounds are qualified against regex-automata 0.4.18. Parser/HIR and native
//! compiler auxiliary scratch are separate pending qualification, not measured
//! by the native NFA size limit. No process-RSS bound is claimed here.

use std::{cell::RefCell, mem::size_of};

use regex_automata::{
    nfa::thompson::{
        self,
        pikevm::{Cache, PikeVM},
        State,
    },
    util::{captures::Captures, primitives::StateID, syntax},
    Input, Match, MatchKind,
};

use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

use super::{check_interruption, invalid_regular_expression, resource_limit, QueryError};

const MAX_NFA_BYTES: usize = 10 * 1024 * 1024;
const MIN_NFA_BYTES: usize = 256;

pub(super) struct Regex {
    vm: PikeVM,
    cache: RefCell<Cache>,
    // These leases outlive both the immutable NFA and retained mutable cache.
    _compiled_memory: Option<QueryMemoryReservation>,
    _search_memory: Option<QueryMemoryReservation>,
}

impl Regex {
    pub(super) fn build(
        pattern: &str,
        syntax: syntax::Config,
        match_kind: Option<MatchKind>,
        controls: Option<&QueryExecutionControls>,
    ) -> Result<Self, QueryError> {
        let workspace = super::workspace::bytes(pattern, syntax, controls)?;
        let mut limit = if controls.is_some() {
            mul(pattern.len(), 64)?.clamp(MIN_NFA_BYTES, MAX_NFA_BYTES)
        } else {
            MAX_NFA_BYTES
        };
        loop {
            check_interruption(controls)?;
            let envelope = add(compilation_bytes(limit, pattern.len())?, workspace)?;
            let mut compiled_memory = reserve(controls, envelope)?;
            let built = PikeVM::builder()
                .syntax(syntax)
                .configure(
                    PikeVM::config().match_kind(match_kind.unwrap_or(MatchKind::LeftmostFirst)),
                )
                .thompson(thompson::Config::new().nfa_size_limit(Some(limit)))
                .build(pattern);
            check_interruption(controls)?;
            match built {
                Ok(vm) => {
                    let retained = retained_bytes(&vm, pattern.len())?;
                    if retained > envelope {
                        return Err(resource_limit(
                            "regular-expression native backing exceeded admission",
                        ));
                    }
                    if let Some(memory) = &mut compiled_memory {
                        memory.shrink_to(retained);
                    }
                    let (cache, search_memory) = admitted_cache(&vm, controls)?;
                    return Ok(Self {
                        vm,
                        cache,
                        _compiled_memory: compiled_memory,
                        _search_memory: search_memory,
                    });
                }
                Err(error) if error.size_limit().is_some() && limit < MAX_NFA_BYTES => {
                    // Drop the complete failed attempt before admitting a larger
                    // native NFA ceiling. The next loop checks cancellation first.
                    drop(compiled_memory);
                    limit = limit.saturating_mul(2).min(MAX_NFA_BYTES);
                }
                Err(error) if error.size_limit().is_some() => {
                    return Err(resource_limit(
                        "regular-expression compiled NFA size limit exceeded",
                    ));
                }
                Err(error) => return Err(invalid_regular_expression(&error.to_string())),
            }
        }
    }

    pub(super) fn is_match(&self, input: &str) -> bool {
        self.vm.is_match(&mut self.cache.borrow_mut(), input)
    }

    pub(super) fn find(&self, input: Input<'_>) -> Option<Match> {
        self.vm.find(&mut self.cache.borrow_mut(), input)
    }

    pub(super) fn create_captures(&self) -> Captures {
        // ReplacementWalk owns at most one such buffer per live wrapper. Its
        // slots were admitted with the search cache before either allocation.
        self.vm.create_captures()
    }

    pub(super) fn search_captures(&self, input: &Input<'_>, captures: &mut Captures) {
        self.vm
            .search(&mut self.cache.borrow_mut(), input, captures);
    }
}

fn admitted_cache(
    vm: &PikeVM,
    controls: Option<&QueryExecutionControls>,
) -> Result<(RefCell<Cache>, Option<QueryMemoryReservation>), QueryError> {
    let memory = reserve(controls, search_bytes(vm)?)?;
    Ok((RefCell::new(vm.create_cache()), memory))
}

fn reserve(
    controls: Option<&QueryExecutionControls>,
    bytes: usize,
) -> Result<Option<QueryMemoryReservation>, QueryError> {
    controls
        .map(|controls| controls.reserve_query_memory(bytes).map_err(Into::into))
        .transpose()
}

fn compilation_bytes(limit: usize, pattern_bytes: usize) -> Result<usize, QueryError> {
    // The native limit counts intermediate NFA states and transition payloads,
    // not Vec capacity. Cover old/new capacity, simultaneous final states,
    // ID-remapping/empty-state directories and a single transition overshoot.
    // Group/name metadata is bounded separately by source-pattern bytes.
    add(add(mul(limit, 8)?, mul(pattern_bytes, 128)?)?, 4096)
}

fn retained_bytes(vm: &PikeVM, pattern_bytes: usize) -> Result<usize, QueryError> {
    // Native NFA memory_usage includes Arc inner, states, boxed transitions and
    // group metadata. Its length-based Vec counts receive capacity allowance;
    // named-capture hash directories receive a source-size allowance.
    add(
        add(
            mul(vm.get_nfa().memory_usage(), 2)?,
            mul(pattern_bytes, 128)?,
        )?,
        1024,
    )
}

fn search_bytes(vm: &PikeVM) -> Result<usize, QueryError> {
    let nfa = vm.get_nfa();
    let states = nfa.states().len();
    let slots = nfa.group_info().slot_len();
    // Each of two ActiveStates owns two SparseSet directories and one fixed
    // SlotTable (including its capture scratch row). Fresh Vec resize has a
    // minimum capacity of four elements. No directory grows during a search.
    let sparse = mul(mul(states.max(4), size_of::<StateID>())?, 4)?;
    let table_slots = add(mul(states, slots)?, slots.max(mul(nfa.pattern_len(), 2)?))?;
    let tables = mul(mul(table_slots.max(4), size_of::<usize>())?, 2)?;
    let captures = mul(slots.max(4), size_of::<usize>())?;
    // The epsilon closure visits each state once. Only unvisited outgoing
    // union branches and capture restores push frames, plus its initial root.
    // A private FollowEpsilon frame fits eight machine words in the qualified
    // dependency. Four times peak depth covers geometric capacity and old/new
    // allocation overlap. Keep this charge even after the native stack clears.
    let frames = nfa.states().iter().try_fold(1_usize, |frames, state| {
        let pushed = match state {
            State::Union { alternates } => alternates.len().saturating_sub(1),
            State::BinaryUnion { .. } | State::Capture { .. } => 1,
            _ => 0,
        };
        add(frames, pushed)
    })?;
    let stack = mul(mul(frames.max(4), 4)?, mul(8, size_of::<usize>())?)?;
    add(add(sparse, tables)?, add(captures, stack)?)
}

fn add(left: usize, right: usize) -> Result<usize, QueryError> {
    left.checked_add(right)
        .ok_or_else(|| resource_limit("regular-expression admission size overflow"))
}

fn mul(left: usize, right: usize) -> Result<usize, QueryError> {
    left.checked_mul(right)
        .ok_or_else(|| resource_limit("regular-expression admission size overflow"))
}

#[cfg(test)]
mod tests {
    use super::{admitted_cache, compilation_bytes, search_bytes, Regex, MIN_NFA_BYTES};
    use crate::{
        app::CassieError,
        config::CassieRuntimeLimits,
        executor::filter::QueryError,
        runtime::{QueryCancellationHandle, QueryExecutionControls},
    };
    use regex_automata::{nfa::thompson::pikevm::PikeVM, util::syntax, Input};

    fn controls(bytes: usize) -> QueryExecutionControls {
        QueryExecutionControls::from_limits(
            &CassieRuntimeLimits {
                query_memory_budget_bytes: bytes,
                ..CassieRuntimeLimits::default()
            },
            std::time::Instant::now(),
        )
    }

    #[test]
    fn should_preserve_native_meta_match_capture_results() {
        // Arrange
        let patterns = [
            "a|aa",
            "(a|aa)",
            "(a+?)(a*)",
            "(?:|a)",
            "(a?)*",
            "(?P<letter>é|δ)",
            "[a-z&&[^aeiou]]",
            "^a.$",
        ];
        let inputs = ["", "aaa", "éδ", "a\nb"];
        let modes = [
            syntax::Config::new().dot_matches_new_line(true),
            syntax::Config::new()
                .multi_line(true)
                .case_insensitive(true),
        ];
        let controls = controls(1024 * 1024);
        let mut observations = Vec::new();

        // Act
        for pattern in patterns {
            for mode in modes {
                for kind in [
                    regex_automata::MatchKind::LeftmostFirst,
                    regex_automata::MatchKind::All,
                ] {
                    let previous = regex_automata::meta::Regex::builder()
                        .syntax(mode)
                        .configure(regex_automata::meta::Regex::config().match_kind(kind))
                        .build(pattern)
                        .expect("previous engine fixture");
                    let admitted = Regex::build(pattern, mode, Some(kind), Some(&controls))
                        .expect("admitted current engine");
                    for input in inputs {
                        for start in [0, input.len()] {
                            for anchored in
                                [regex_automata::Anchored::No, regex_automata::Anchored::Yes]
                            {
                                let input = Input::new(input)
                                    .range(start..input.len())
                                    .anchored(anchored);
                                let old_match = previous.find(input.clone());
                                let new_match = admitted.find(input.clone());
                                let mut old_captures = previous.create_captures();
                                let mut new_captures = admitted.create_captures();
                                previous.search_captures(&input, &mut old_captures);
                                admitted.search_captures(&input, &mut new_captures);
                                observations.push((
                                    old_match,
                                    new_match,
                                    old_captures.iter().collect::<Vec<_>>(),
                                    new_captures.iter().collect::<Vec<_>>(),
                                ));
                            }
                        }
                    }
                }
            }
        }

        // Assert
        for (old_match, new_match, old_captures, new_captures) in observations {
            assert_eq!(new_match, old_match);
            assert_eq!(new_captures, old_captures);
        }
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_admit_regex_compilation_exactly_at_its_reserved_boundary() {
        // Arrange
        let bytes = compilation_bytes(MIN_NFA_BYTES, 1).expect("native backing envelope");
        let below = controls(bytes - 1);
        let exact = controls(bytes);

        // Act
        let rejected = Regex::build("x", syntax::Config::new(), None, Some(&below));
        let admitted = Regex::build("x", syntax::Config::new(), None, Some(&exact));

        // Assert
        assert!(matches!(
            rejected,
            Err(QueryError::Cassie(CassieError::ResourceLimit(_)))
        ));
        assert_eq!(below.current_query_memory_bytes(), 0);
        assert!(admitted.expect("exact admission succeeds").is_match("x"));
        assert_eq!(exact.current_query_memory_bytes(), 0);
        assert_eq!(exact.peak_query_memory_bytes(), bytes);
    }

    #[test]
    fn should_admit_regex_search_cache_before_native_allocation() {
        // Arrange
        let vm = PikeVM::new("(a|aa)").expect("independent native fixture");
        let bytes = search_bytes(&vm).expect("search backing envelope");
        let below = controls(bytes - 1);
        let exact = controls(bytes);

        // Act
        let rejected = admitted_cache(&vm, Some(&below));
        let admitted = admitted_cache(&vm, Some(&exact));

        // Assert
        assert!(matches!(
            rejected,
            Err(QueryError::Cassie(CassieError::ResourceLimit(_)))
        ));
        assert_eq!(below.current_query_memory_bytes(), 0);
        assert_eq!(exact.current_query_memory_bytes(), bytes);
        drop(admitted.expect("exact cache backing is admitted"));
        assert_eq!(exact.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_keep_cleared_regex_search_backing_charged_until_drop() {
        // Arrange
        let controls = controls(128 * 1024);
        let regex = Regex::build("((a|aa)*)", syntax::Config::new(), None, Some(&controls))
            .expect("admitted regex");
        let retained = controls.current_query_memory_bytes();
        let mut captures = regex.create_captures();

        // Act
        regex.search_captures(&Input::new("aaaa"), &mut captures);
        let matched = captures.get_match();
        let missing = regex.find(Input::new("z").anchored(regex_automata::Anchored::Yes));
        let after_clear = controls.current_query_memory_bytes();
        drop(captures);
        drop(regex);

        // Assert
        assert!(matched.is_some());
        assert!(missing.is_some_and(|matched| matched.is_empty()));
        assert!(retained > 0);
        assert_eq!(after_clear, retained);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_release_failed_native_regex_attempt_before_retry_admission() {
        // Arrange
        let workspace = super::super::workspace::bytes(r"\w", syntax::Config::new(), None)
            .expect("Unicode workspace");
        let first_envelope =
            compilation_bytes(MIN_NFA_BYTES, 2).expect("first native attempt") + workspace;
        let controls = controls(first_envelope);

        // Act
        let result = Regex::build(r"\w", syntax::Config::new(), None, Some(&controls));

        // Assert
        assert!(matches!(
            result,
            Err(QueryError::Cassie(CassieError::ResourceLimit(_)))
        ));
        assert_eq!(controls.peak_query_memory_bytes(), first_envelope);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_admit_unicode_regex_after_releasing_smaller_native_attempts() {
        // Arrange
        let controls = controls(4 * 1024 * 1024);
        let workspace = super::super::workspace::bytes(r"\w", syntax::Config::new(), None)
            .expect("Unicode workspace");
        let first_envelope =
            compilation_bytes(MIN_NFA_BYTES, 2).expect("first native attempt") + workspace;

        // Act
        let regex = Regex::build(r"\w", syntax::Config::new(), None, Some(&controls))
            .expect("larger native regex is admitted");
        let matched = regex.is_match("猫");
        let peak = controls.peak_query_memory_bytes();
        let native_ceiling = (peak - workspace - 2 * 128 - 4096) / 8;
        drop(regex);

        // Assert
        assert!(matched);
        assert!(peak > first_envelope);
        assert!(native_ceiling.is_power_of_two());
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_release_regex_admission_after_malformed_patterns() {
        // Arrange
        let controls = controls(128 * 1024);

        // Act
        let result = Regex::build("[", syntax::Config::new(), None, Some(&controls));

        // Assert
        assert!(
            matches!(result, Err(QueryError::Cassie(CassieError::Diagnostic(ref diagnostic))) if diagnostic.sql_state == "2201B")
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_isolate_regex_admission_between_requests() {
        // Arrange
        let first_controls = controls(128 * 1024);
        let second_controls = controls(128 * 1024);
        let first = Regex::build("a", syntax::Config::new(), None, Some(&first_controls))
            .expect("first request");
        let first_retained = first_controls.current_query_memory_bytes();

        // Act
        let second = Regex::build("b", syntax::Config::new(), None, Some(&second_controls))
            .expect("second request");
        drop(first);
        let second_retained = second_controls.current_query_memory_bytes();
        drop(second);

        // Assert
        assert!(first_retained > 0);
        assert_eq!(first_controls.current_query_memory_bytes(), 0);
        assert!(second_retained > 0);
        assert_eq!(second_controls.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_skip_native_regex_allocation_for_cancelled_requests() {
        // Arrange
        let cancellation = QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            std::time::Instant::now(),
            cancellation.clone(),
        );
        cancellation.cancel();

        // Act
        let result = Regex::build("x", syntax::Config::new(), None, Some(&controls));

        // Assert
        assert!(matches!(
            result,
            Err(QueryError::Cassie(CassieError::QueryCancelled))
        ));
        assert_eq!(controls.peak_query_memory_bytes(), 0);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
