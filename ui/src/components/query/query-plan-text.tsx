import { For } from "@askrjs/askr/control";
import type {
  QueryExplainPlan,
  QueryPlanAnalyze,
  QueryPlanFeature,
  QueryPlanNode,
} from "@/adapters";
import type { QueryExecutionResult } from "@/features/query/query-models";
import { QueryResultJson } from "./query-result-json";

export interface QueryPlanTextProps {
  result: QueryExecutionResult;
}

interface PlanEstimateBar {
  id: string;
  label: string;
  value: number;
  width: string;
}

function planText(result: QueryExecutionResult) {
  const value = result.rows[0]?.[0];

  return typeof value === "string" ? value : "";
}

function formatNumber(value: number) {
  return new Intl.NumberFormat("en-US", { maximumFractionDigits: 0 }).format(value);
}

function estimateBars(plan: QueryExplainPlan): PlanEstimateBar[] {
  const estimates = [
    { id: "scan_cost", label: "Scan cost", value: plan.estimates.scan_cost },
    { id: "index_cost", label: "Index cost", value: plan.estimates.index_cost },
    { id: "selected_cost", label: "Selected cost", value: plan.estimates.selected_cost },
    { id: "scan_rows", label: "Scan rows", value: plan.estimates.scan_rows },
    { id: "index_rows", label: "Index rows", value: plan.estimates.index_rows },
    { id: "estimated_rows", label: "Estimated rows", value: plan.summary.estimated_rows },
  ];
  const maxValue = Math.max(1, ...estimates.map((estimate) => estimate.value));

  return estimates.map((estimate) => ({
    ...estimate,
    width: `${Math.max(4, Math.round((estimate.value / maxValue) * 100))}%`,
  }));
}

function enabledFeatureCount(features: QueryPlanFeature[]) {
  return features.filter((feature) => feature.enabled).length;
}

function rawPlanBlock(rawText: string) {
  if (!rawText) {
    return null;
  }

  return (
    <pre class="cassie-query-plan-text" data-testid="query-plan-text">
      <code>{rawText}</code>
    </pre>
  );
}

function QueryPlanNodeView({ node }: { node: QueryPlanNode }) {
  return (
    <article
      class="cassie-query-plan-node"
      data-testid="query-plan-node"
      data-node-kind={node.kind}
      data-status={node.status}
    >
      <header class="cassie-query-plan-node-header">
        <span class="cassie-query-plan-node-kind">{node.kind}</span>
        <span class="cassie-query-plan-node-status">{node.status}</span>
      </header>
      <h3 class="cassie-query-plan-node-title">{node.label}</h3>
      <p class="cassie-query-plan-node-detail">{node.detail}</p>
      {node.badges.length > 0 ? (
        <div class="cassie-query-plan-badges">
          <For each={node.badges} by={(badge) => badge}>
            {(badge) => <span class="cassie-query-plan-badge">{badge}</span>}
          </For>
        </div>
      ) : null}
      <dl class="cassie-query-plan-metrics">
        <For each={node.metrics} by={(metric) => metric.label}>
          {(metric) => (
            <div class="cassie-query-plan-metric">
              <dt>{metric.label}</dt>
              <dd>
                {metric.value}
                {metric.unit ? ` ${metric.unit}` : ""}
              </dd>
            </div>
          )}
        </For>
      </dl>
    </article>
  );
}

function QueryPlanFeatureView({ feature }: { feature: QueryPlanFeature }) {
  return (
    <span
      class="cassie-query-plan-feature"
      data-enabled={feature.enabled ? "true" : "false"}
      data-intent={feature.intent}
      data-node-id={feature.node_id}
      title={feature.detail}
    >
      {feature.label}
    </span>
  );
}

function QueryPlanAnalyzeView({ analyze }: { analyze: QueryPlanAnalyze }) {
  return (
    <section class="cassie-query-plan-analyze" aria-label="Analyze">
      <div class="cassie-query-plan-analyze-summary">
        <span>{formatNumber(analyze.actual_rows)} rows</span>
        <span>{formatNumber(Number(analyze.actual_ms))} ms</span>
        <span>{formatNumber(analyze.diagnostics.storage_reads_delta)} reads</span>
      </div>
      {analyze.operator_actuals.length > 0 ? (
        <div class="cassie-query-plan-actuals">
          <For each={analyze.operator_actuals} by={(actual) => actual.operator}>
            {(actual) => (
              <div class="cassie-query-plan-actual">
                <span>{actual.operator}</span>
                <span>{formatNumber(actual.rows_out)} rows</span>
                <span>{formatNumber(Number(actual.elapsed_ms))} ms</span>
              </div>
            )}
          </For>
        </div>
      ) : null}
    </section>
  );
}

function QueryPlanVisual({ plan, rawText }: { plan: QueryExplainPlan; rawText: string }) {
  return (
    <div class="cassie-query-plan-visual" data-testid="query-plan-visual">
      <section class="cassie-query-plan-summary" aria-label="Plan summary">
        <div class="cassie-query-plan-summary-main">
          <p class="cassie-query-plan-title">{plan.summary.collection}</p>
          <p class="cassie-query-plan-subtitle">
            {plan.summary.root_operator} / {plan.summary.access_path}
          </p>
        </div>
        <div class="cassie-query-plan-score">
          <span>{formatNumber(plan.summary.selected_cost)}</span>
          <small>cost</small>
        </div>
      </section>

      <div class="cassie-query-plan-attributes" aria-label="Plan attributes">
        <For each={plan.attributes} by={(attribute) => attribute.label}>
          {(attribute) => (
            <span class="cassie-query-plan-attribute" data-intent={attribute.intent}>
              <strong>{attribute.label}</strong>
              <span>{attribute.value}</span>
            </span>
          )}
        </For>
      </div>

      <section class="cassie-query-plan-pipeline" aria-label="Plan operators">
        <For each={plan.nodes} by={(node) => node.id}>
          {(node) => <QueryPlanNodeView node={node} />}
        </For>
      </section>

      <section class="cassie-query-plan-estimates" aria-label="Plan estimates">
        <For each={estimateBars(plan)} by={(estimate) => estimate.id}>
          {(estimate) => (
            <div class="cassie-query-plan-estimate">
              <div class="cassie-query-plan-estimate-label">
                <span>{estimate.label}</span>
                <strong>{formatNumber(estimate.value)}</strong>
              </div>
              <span class="cassie-query-plan-estimate-track" aria-hidden="true">
                <span
                  class="cassie-query-plan-estimate-bar"
                  style={{ inlineSize: estimate.width }}
                />
              </span>
            </div>
          )}
        </For>
      </section>

      <section class="cassie-query-plan-features" aria-label="Plan features">
        <header class="cassie-query-plan-section-header">
          <span>Features</span>
          <strong>
            {enabledFeatureCount(plan.features)} / {plan.features.length}
          </strong>
        </header>
        <div class="cassie-query-plan-feature-grid">
          <For each={plan.features} by={(feature) => feature.id}>
            {(feature) => <QueryPlanFeatureView feature={feature} />}
          </For>
        </div>
      </section>

      <section class="cassie-query-plan-diagnostics" aria-label="Plan diagnostics">
        <span>{plan.diagnostics.access_path_reason}</span>
        <span>{plan.diagnostics.pagination_strategy}</span>
        <span>{plan.diagnostics.early_stop}</span>
        <span>{plan.diagnostics.projection_freshness}</span>
      </section>

      {plan.analyze ? <QueryPlanAnalyzeView analyze={plan.analyze} /> : null}

      <section class="cassie-query-plan-raw" aria-label="Raw plan">
        {rawPlanBlock(rawText)}
      </section>
    </div>
  );
}

export function QueryPlanText({ result }: QueryPlanTextProps) {
  const isSinglePlanColumn =
    result.columns.length === 1 && result.rows.length === 1 && result.rows[0].length === 1;
  const rawText = planText(result);

  if (result.plan) {
    return <QueryPlanVisual plan={result.plan} rawText={rawText} />;
  }

  if (!isSinglePlanColumn) {
    return <QueryResultJson result={result} />;
  }

  return rawPlanBlock(rawText);
}
