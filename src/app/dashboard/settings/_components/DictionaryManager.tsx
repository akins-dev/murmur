/**
 * SOURCE OF TRUTH KEYWORDS: DictionaryManager, DictionaryEntry, MatchKind,
 *   createDictionaryEntry, updateDictionaryEntry, deleteDictionaryEntry
 * WHAT:  The custom dictionary: add a pattern and its replacement, change how it
 *        matches, enable, disable, delete.
 * WHY:   The pattern is not editable after creation — the backend's update takes
 *        replacement, match kind and enabled, and nothing else. That is correct:
 *        the pattern is the entry's identity, so changing it is a different
 *        entry, and offering a field that silently does nothing would be worse
 *        than not offering it. WORD is the default match kind because substring
 *        matching corrupts unrelated words, which is a bug the user attributes
 *        to the model rather than to their own rule.
 * WHERE: The vocabulary section of Settings.
 */

import { useCallback, useState, type FormEvent } from "react";
import { Plus, Trash2 } from "lucide-react";
import { commands, type DictionaryEntry, type MatchKind } from "@/lib/bindings";
import { unwrapCommand, useCommand } from "@/lib/ipc";
import { ErrorSurface, EmptyState } from "@/components/global";

const MATCH_LABEL: Readonly<Record<MatchKind, string>> = {
  WORD: "Whole word",
  WORD_CASE_SENSITIVE: "Whole word, case sensitive",
  SUBSTRING: "Anywhere",
};

/** Listed rather than derived from the map's keys, because deriving them needs
 *  a cast and a cast is exactly what the exhaustive Record above prevents. */
const MATCH_KINDS: readonly MatchKind[] = ["WORD", "WORD_CASE_SENSITIVE", "SUBSTRING"];
const FIELD_CLASS = "hairline h-8 min-w-0 rounded-input bg-sunken px-2 text-body text-text-primary";

export function DictionaryManager() {
  const entries = useCommand(commands.listDictionary, []);
  const [pattern, setPattern] = useState("");
  const [replacement, setReplacement] = useState("");

  const add = useCallback(
    (event: FormEvent) => {
      event.preventDefault();
      if (pattern.trim().length === 0 || replacement.trim().length === 0) return;
      void unwrapCommand(() =>
        commands.createDictionaryEntry({ pattern, replacement, match_kind: "WORD" }),
      ).then((result) => {
        if (result.status !== "ok") return;
        setPattern("");
        setReplacement("");
        entries.reload();
      });
    },
    [entries, pattern, replacement],
  );

  const update = useCallback(
    (entry: DictionaryEntry, changes: Partial<Pick<DictionaryEntry, "replacement" | "match_kind" | "enabled">>) => {
      void unwrapCommand(() =>
        commands.updateDictionaryEntry({
          id: entry.id,
          replacement: changes.replacement ?? entry.replacement,
          match_kind: changes.match_kind ?? entry.match_kind,
          enabled: changes.enabled ?? entry.enabled,
        }),
      ).then(entries.reload);
    },
    [entries],
  );

  const remove = useCallback(
    (entry: DictionaryEntry) => {
      void unwrapCommand(() => commands.deleteDictionaryEntry({ id: entry.id })).then(entries.reload);
    },
    [entries],
  );

  if (entries.error) return <ErrorSurface error={entries.error} onRetry={entries.reload} size="compact" />;

  return (
    <div className="flex flex-col gap-3">
      <form onSubmit={add} className="flex items-center gap-2">
        <input
          value={pattern}
          onChange={(event) => setPattern(event.target.value)}
          placeholder="Heard as"
          className={`${FIELD_CLASS} flex-1`}
        />
        <span className="text-label text-text-tertiary">→</span>
        <input
          value={replacement}
          onChange={(event) => setReplacement(event.target.value)}
          placeholder="Should be"
          className={`${FIELD_CLASS} flex-1`}
        />
        <button
          type="submit"
          aria-label="Add dictionary entry"
          className="hairline flex h-8 shrink-0 items-center gap-1 rounded-input bg-sunken px-3 text-body text-text-primary transition-colors hover:bg-sunken-strong"
        >
          <Plus className="size-4" />
          Add
        </button>
      </form>

      {entries.data && entries.data.length === 0 ? (
        <EmptyState
          size="compact"
          headline="No replacements yet"
          description="Add the words the model keeps getting wrong — names, jargon, product names."
        />
      ) : null}

      <ul className="flex flex-col">
        {(entries.data ?? []).map((entry) => (
          <li key={entry.id} className="hairline-b flex items-center gap-2 py-2 last:border-b-0">
            <input
              type="checkbox"
              checked={entry.enabled}
              aria-label={`Enable ${entry.pattern}`}
              onChange={(event) => update(entry, { enabled: event.target.checked })}
              className="accent-[var(--accent)]"
            />
            <span className="w-40 shrink-0 truncate text-body text-text-secondary">{entry.pattern}</span>
            <input
              defaultValue={entry.replacement}
              aria-label={`Replacement for ${entry.pattern}`}
              onBlur={(event) => {
                if (event.target.value !== entry.replacement) update(entry, { replacement: event.target.value });
              }}
              className={`${FIELD_CLASS} flex-1`}
            />
            <select
              value={entry.match_kind}
              aria-label={`Match kind for ${entry.pattern}`}
              onChange={(event) => {
                const kind = MATCH_KINDS.find((candidate) => candidate === event.target.value);
                if (kind) update(entry, { match_kind: kind });
              }}
              className={FIELD_CLASS}
            >
              {MATCH_KINDS.map((kind) => (
                <option key={kind} value={kind}>
                  {MATCH_LABEL[kind]}
                </option>
              ))}
            </select>
            <button
              type="button"
              aria-label={`Delete ${entry.pattern}`}
              onClick={() => remove(entry)}
              className="shrink-0 rounded-input p-1 text-text-secondary transition-colors hover:bg-sunken hover:text-danger"
            >
              <Trash2 className="size-4" />
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}
