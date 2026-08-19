import { For } from "@askrjs/askr/control";
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
import { TriangleAlertIcon } from "@askrjs/lucide";
import {
  Alert,
  Button,
  Field,
  Form,
  Input,
  Label,
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectItemText,
  SelectLabel,
  SelectPortal,
  SelectTrigger,
  SelectValue,
} from "@askrjs/themes/components";

interface NewQueryDialogProps {
  databases: () => string[];
  loading: boolean;
  error: string | null;
  suggestedName: string;
  onClose: () => void;
  onCreate: (database: string, name: string) => void;
}

export function NewQueryDialog({
  databases,
  loading,
  error,
  suggestedName,
  onClose,
  onCreate,
}: NewQueryDialogProps) {
  const [name, setName] = state("");
  const [database, setDatabase] = state("");

  function submit(event: Event) {
    event.preventDefault();
    if (database()) onCreate(database(), name());
  }

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogPortal>
        <DialogOverlay class="cassie-query-dialog-overlay" />
        <DialogContent class="cassie-query-dialog">
          <DialogTitle>New Query</DialogTitle>
          <DialogDescription>Name the query and select the database it will use.</DialogDescription>
          <Form class="cassie-query-dialog-form" onSubmit={submit}>
            <Field>
              <Label for="new-query-name">Query name</Label>
              <Input
                id="new-query-name"
                name="name"
                placeholder={suggestedName}
                value={name()}
                onInput={(event) => setName((event.target as HTMLInputElement).value)}
              />
            </Field>
            {loading ? <p>Loading databases…</p> : null}
            {error ? (
              <Alert
                title="Unable to load databases"
                variant="danger"
                description={error}
                icon={<TriangleAlertIcon size={16} />}
              />
            ) : null}
            <Field>
              <Label for="new-query-database">Database</Label>
              <Select value={database()} onValueChange={setDatabase} disabled={loading}>
                <SelectTrigger id="new-query-database" aria-label="Database">
                  <SelectValue placeholder="Select a database" />
                </SelectTrigger>
                <SelectPortal>
                  <SelectContent sideOffset={4}>
                    <SelectGroup>
                      <SelectLabel>Database</SelectLabel>
                      <For each={databases} by={(databaseName) => databaseName}>
                        {(databaseName) => (
                          <SelectItem value={databaseName}>
                            <SelectItemText>{databaseName}</SelectItemText>
                          </SelectItem>
                        )}
                      </For>
                    </SelectGroup>
                  </SelectContent>
                </SelectPortal>
              </Select>
            </Field>
            <div class="cassie-query-dialog-actions">
              <DialogClose asChild>
                <Button type="button" variant="ghost" onPress={onClose}>
                  Cancel
                </Button>
              </DialogClose>
              <Button type="submit" variant="primary" disabled={!database() || loading}>
                Create
              </Button>
            </div>
          </Form>
        </DialogContent>
      </DialogPortal>
    </Dialog>
  );
}
