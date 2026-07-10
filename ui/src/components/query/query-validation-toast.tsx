import { Alert } from "@askrjs/themes/components";

export type QueryValidationToastVariant = "success" | "warning" | "danger";

export interface QueryValidationToastData {
  variant: QueryValidationToastVariant;
  title: string;
  description: string;
}

export interface QueryValidationToastProps {
  toast: QueryValidationToastData | null;
  onDismiss: () => void;
  onPause: () => void;
  onResume: () => void;
}

// A thin positioned wrapper around <Alert> rather than askr-themes' <Toast>
// primitive: mounting <Toast> anywhere alongside an askr <Portal> (as this
// page already has, for the schema tree) throws the render scheduler into an
// infinite update loop — reproduced in isolation down to <Toast open={false}>
// plus a sibling/ancestor <Portal>, independent of any app logic. Filed
// upstream; swap back once fixed. <Alert> is a separate, unaffected
// component that already renders variant-colored title/description/dismiss
// markup, so it's reused here instead of hand-rolling the same thing again.
export function QueryValidationToast({
  toast,
  onDismiss,
  onPause,
  onResume,
}: QueryValidationToastProps) {
  return (
    <div
      class="cassie-query-validation-toast"
      data-testid="query-validation-toast"
      data-variant={toast?.variant}
      hidden={toast === null}
      onMouseEnter={onPause}
      onMouseLeave={onResume}
      onFocusIn={onPause}
      onFocusOut={onResume}
    >
      {toast ? (
        <Alert
          variant={toast.variant}
          title={toast.title}
          description={toast.description}
          dismissLabel="Dismiss notification"
          onDismiss={onDismiss}
        />
      ) : null}
    </div>
  );
}
