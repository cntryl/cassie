import {
  Toast,
  ToastClose,
  ToastDescription,
  ToastHost,
  ToastTitle,
  ToastViewport,
} from "@askrjs/themes/components";

export type QueryValidationToastVariant = "success" | "warning" | "danger";

export interface QueryValidationToastData {
  variant: QueryValidationToastVariant;
  title: string;
  description: string;
}

export interface QueryValidationToastProps {
  toast: QueryValidationToastData | null;
  onDismiss: () => void;
}

export function QueryValidationToast({ toast, onDismiss }: QueryValidationToastProps) {
  return (
    <ToastHost>
      <ToastViewport />
      {toast ? (
        <Toast id="query-validation-toast" open variant={toast.variant}>
          <ToastTitle>{toast.title}</ToastTitle>
          <ToastDescription>{toast.description}</ToastDescription>
          <ToastClose aria-label="Dismiss notification" onPress={onDismiss}>
            Dismiss
          </ToastClose>
        </Toast>
      ) : null}
    </ToastHost>
  );
}
