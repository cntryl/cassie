use super::{
    BatchRow, Cassie, CassieSession, FunctionMeta, HashMap, LogicalPlan, QueryError,
    QueryExecutionControls, Value,
};

pub(super) fn try_execute(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    plan: &LogicalPlan,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    controls: &QueryExecutionControls,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    super::super::time_series_read::try_execute_time_series_read(
        cassie,
        session,
        plan,
        user_functions,
        params,
        controls,
    )
}
