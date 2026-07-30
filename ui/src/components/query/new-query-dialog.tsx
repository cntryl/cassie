import { For } from "@askrjs/askr/control";
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
  draft: { name: string; database: string };
  databases: () => string[];
  loading: boolean;
  error: string | null;
  suggestedName: string;
  onClose: () => void;
  onCreate: (database: string, name: string) => void;
}

export function NewQueryDialog({
  draft,
  databases,
  loading,
  error,
  suggestedName,
  onClose,
  onCreate,
}: NewQueryDialogProps) {
  let createButton: HTMLButtonElement | null = null;

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogPortal>
        <DialogOverlay class="cassie-query-dialog-overlay" />
        <DialogContent class="cassie-query-dialog">
          <DialogTitle>New Query</DialogTitle>
          <DialogDescription>Name the query and select the database it will use.</DialogDescription>
          <Field>
            <Label for="new-query-name">Query name</Label>
            <Input
              id="new-query-name"
              placeholder={suggestedName}
              onInput={(event) => {
                draft.name = (event.target as HTMLInputElement).value;
              }}
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
            <Select
              onValueChange={(value) => {
                draft.database = value;
                createButton?.setAttribute("aria-disabled", value ? "false" : "true");
                if (value) createButton?.removeAttribute("data-disabled");
                else createButton?.setAttribute("data-disabled", "true");
              }}
            >
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
            <Button
              type="button"
              variant="primary"
              aria-disabled="true"
              data-disabled="true"
              ref={(node: HTMLButtonElement | null) => {
                createButton = node;
              }}
              onPress={() => {
                if (draft.database) {
                  onCreate(draft.database, draft.name);
                }
              }}
            >
              Create
            </Button>
          </div>
        </DialogContent>
      </DialogPortal>
    </Dialog>
  );
}
