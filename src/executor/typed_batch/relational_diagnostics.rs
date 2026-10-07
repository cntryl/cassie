//! Private completed-path diagnostics; test observations are confined to the calling thread.
#[cfg(test)]
std::thread_local! {
    static INPUT_HANDOFFS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static LAST_PATH: std::cell::Cell<Option<(&'static str, &'static str)>> = const { std::cell::Cell::new(None) };
}

pub(crate) fn publish(operator: &'static str, path: &'static str) {
    let state = if path == "scalar_cte_materialization" {
        "documented_scalar_boundary"
    } else {
        "accounted_blocking"
    };
    tracing::debug!(operator, path, state, "relational operator completed");
    #[cfg(test)]
    LAST_PATH.with(|last| last.set(Some((operator, path))));
}

#[cfg(test)]
pub(crate) fn last_path() -> Option<(&'static str, &'static str)> {
    LAST_PATH.with(std::cell::Cell::get)
}

#[cfg(test)]
pub(crate) fn input_handoff() {
    INPUT_HANDOFFS.with(|count| count.set(count.get() + 1));
}

#[cfg(test)]
pub(crate) fn input_handoffs() -> usize {
    INPUT_HANDOFFS.with(std::cell::Cell::get)
}
