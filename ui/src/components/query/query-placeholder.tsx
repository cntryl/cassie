import { EmptyState } from "@askrjs/themes/components";

export interface QueryPlaceholderProps {
  title: string;
  description: string;
}

export function QueryPlaceholder({ title, description }: QueryPlaceholderProps) {
  return (
    <EmptyState
      class="cassie-query-results-placeholder"
      title={title}
      titleAs="h3"
      description={description}
      aria-label={title}
    />
  );
}
