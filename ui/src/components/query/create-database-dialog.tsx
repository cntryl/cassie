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
import { Block, Button, Field, FieldError, Form, Input, Label } from "@askrjs/themes/components";

import { createDatabaseMutation } from "@/features/query/query-actions";
import { apiErrorMessage } from "@/shared/errors/api";

interface CreateDatabaseDialogProps {
  onClose: () => void;
  onCreated: (name: string) => Promise<void>;
}

export function CreateDatabaseDialog({ onClose, onCreated }: CreateDatabaseDialogProps) {
  const createDatabase = createDatabaseMutation();
  const [name, setName] = state("");
  const [error, setError] = state<string | null>(null);

  async function handleSubmit(event?: { preventDefault?: () => void }) {
    event?.preventDefault?.();
    const databaseName = name().trim();
    if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(databaseName)) {
      setError("Use a letter or underscore first, followed by letters, numbers, or underscores.");
      return;
    }
    if (createDatabase.pending) return;

    setError(null);
    try {
      const database = await createDatabase.execute(databaseName);
      await onCreated(database.name);
      onClose();
    } catch (caught) {
      setError(apiErrorMessage(caught));
    }
  }

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogPortal>
        <DialogOverlay class="cassie-query-dialog-overlay" />
        <DialogContent class="cassie-query-dialog">
          <DialogTitle>Create database</DialogTitle>
          <DialogDescription>Create a database and open its first query tab.</DialogDescription>
          <Form class="cassie-query-dialog-form" onSubmit={handleSubmit}>
            <Field invalid={error() !== null}>
              <Label for="create-database-name">Database name</Label>
              <Input
                id="create-database-name"
                name="database"
                autocomplete="off"
                required
                disabled={createDatabase.pending}
                value={name()}
                onInput={(event) => setName((event.target as HTMLInputElement).value)}
              />
              {error() ? <FieldError>{error()}</FieldError> : null}
            </Field>
            <Block direction="row" gap="md" justify="end">
              <DialogClose asChild>
                <Button
                  type="button"
                  variant="ghost"
                  disabled={createDatabase.pending}
                  onPress={onClose}
                >
                  Cancel
                </Button>
              </DialogClose>
              <Button type="submit" variant="primary" disabled={createDatabase.pending}>
                {createDatabase.pending ? "Creating…" : "Create database"}
              </Button>
            </Block>
          </Form>
        </DialogContent>
      </DialogPortal>
    </Dialog>
  );
}
