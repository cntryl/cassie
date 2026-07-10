import { Alert } from "@askrjs/themes/components";

import type { QueryStatus } from "@/features/query/query-models";

export interface QueryExecutionBannerProps {
  status: QueryStatus;
  isBusy: boolean;
  errorMessage: string | null;
}

export function QueryExecutionBanner({ status, isBusy, errorMessage }: QueryExecutionBannerProps) {
  const banner = (() => {
    if (errorMessage !== null) {
      return {
        variant: "danger" as const,
        title: "Query action failed",
        description: errorMessage,
      };
    }

    if (isBusy) {
      return {
        variant: "info" as const,
        title: "Query action",
        description:
          status === "running"
            ? "Running query..."
            : status === "explaining"
              ? "Generating explain plan..."
              : status === "validating"
                ? "Validating SQL..."
                : "Working on query operation...",
      };
    }

    return null;
  })();

  if (!banner) {
    return null;
  }

  return <Alert variant={banner.variant} title={banner.title} description={banner.description} />;
}
