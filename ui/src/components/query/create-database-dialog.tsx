import { state } from "@askrjs/askr";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogOverlay,
  DialogPortal,
  DialogTitle,
} from "@askrjs/ui";
import { Block, Button, Field, FieldError, Input, Label } from "@askrjs/themes/components";

import { apiv1 } from "@/adapters";
import { apiErrorMessage, unwrapResponse } from "@/shared/errors/api";

interface CreateDatabaseDialogProps {
  databaseNames: () => string[];
  onClose: () => void;
  onCreated: (name: string) => Promise<void>;
}

export function CreateDatabaseDialog({
  databaseNames,
  onClose,
  onCreated,
}: CreateDatabaseDialogProps) {
  const [error, setError] = state<string | null>(null);
  const [pending, setPending] = state(false);
  let nameInput: HTMLInputElement | null = null;

  function setNameInput(node: unknown) {
    nameInput = node instanceof HTMLInputElement ? node : null;
  }

  async function handleSubmit(event?: { preventDefault?: () => void }) {
    event?.preventDefault?.();
    const databaseName = nameInput?.value.trim() ?? "";
    if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(databaseName)) {
      setError("Use a letter or underscore first, followed by letters, numbers, or underscores.");
      return;
    }
    if (pending()) return;

    const names = databaseNames();
    const administrationDatabase = names.includes("postgres")
      ? "postgres"
      : (names[0] ?? "postgres");
    setError(null);
    setPending(true);
    try {
      unwrapResponse(
        await apiv1.createAdminQueryExecution({
          body: {
            database: administrationDatabase,
            sql: `CREATE DATABASE ${databaseName}`,
            operation_id: crypto.randomUUID(),
          },
        }),
        "Unable to create database",
      );
      await onCreated(databaseName);
      onClose();
    } catch (caught) {
      setError(apiErrorMessage(caught));
    } finally {
      setPending(false);
    }
  }

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogPortal>
        <DialogOverlay class="cassie-query-dialog-overlay" />
        <DialogContent class="cassie-query-dialog">
          <DialogTitle>Create database</DialogTitle>
          <DialogDescription>Create a database and open its first query tab.</DialogDescription>
          <Block as="form" direction="column" gap="lg" onSubmit={handleSubmit}>
            <Field>
              <Label for="create-database-name">Database name</Label>
              <Input
                id="create-database-name"
                name="database"
                autocomplete="off"
                autofocus
                required
                disabled={pending()}
                ref={setNameInput}
              />
            </Field>
            {error() ? <FieldError>{error()}</FieldError> : null}
            <Block direction="row" gap="md" justify="end">
              <DialogClose asChild>
                <Button type="button" variant="ghost" disabled={pending()} onPress={onClose}>
                  Cancel
                </Button>
              </DialogClose>
              <Button type="submit" variant="primary" disabled={pending()}>
                {pending() ? "Creating…" : "Create database"}
              </Button>
            </Block>
          </Block>
        </DialogContent>
      </DialogPortal>
    </Dialog>
  );
}
