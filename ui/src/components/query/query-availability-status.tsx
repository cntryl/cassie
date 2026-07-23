import { DatabaseIcon, TriangleAlertIcon } from "@askrjs/lucide";
import { Button } from "@askrjs/themes/components";

interface QueryAvailabilityStatusProps {
  database: string;
  state: "available" | "checking" | "unavailable" | "discovery-error";
  onCreateDatabase: () => void;
}

export function QueryAvailabilityStatus({
  database,
  state,
  onCreateDatabase,
}: QueryAvailabilityStatusProps) {
  if (state === "available" || state === "checking") return null;

  const discoveryFailed = state === "discovery-error";
  return (
    <div class="cassie-query-availability-status" role="status">
      {discoveryFailed ? (
        <TriangleAlertIcon size={15} aria-hidden="true" />
      ) : (
        <DatabaseIcon size={15} aria-hidden="true" />
      )}
      <span>
        {discoveryFailed
          ? "Database discovery failed. Your SQL draft is still editable."
          : `${database} is not on this server. Your SQL draft is preserved.`}
      </span>
      {!discoveryFailed ? (
        <Button type="button" size="xs" variant="outline" onPress={onCreateDatabase}>
          Create database
        </Button>
      ) : null}
    </div>
  );
}
