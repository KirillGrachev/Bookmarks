import { useCallback, useMemo, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Loader2, Lock, Pin, PinOff, Plus, SearchX, Trash2 } from 'lucide-react';
import { toast } from 'sonner';

import { getBackend } from '@/api/backend';
import { PAGE_SIZE, queryKeys, scopeFor } from '@/api/keys';
import type { NoteQuery, NoteSummary } from '@/api/contracts';
import { useDebouncedValue } from '@/hooks/useDebouncedValue';
import { useAppStore } from '@/store/useAppStore';
import { cn } from '@/lib/utils';
import { formatRelative } from '@/lib/time';
import { parseQuery, renderSnippet, textTerms, highlightTerms } from '@/lib/searchQuery';
import type { Translator } from '@/lib/i18n';

interface NoteListProps {
  t: Translator;
  lang: 'ru' | 'en';
}

/**
 * Виртуализация здесь намеренно не используется.
 *
 * Список отдаётся страницами по 60 строк с догрузкой при прокрутке, поэтому в
 * DOM никогда не больше пары сотен узлов; отдельный виртуализатор дал бы
 * выигрыш только на десятках тысяч строк, а стоимость — скачущая высота строк
 * при раскрытии превью и лишняя зависимость.
 */
export function NoteList({ t, lang }: NoteListProps) {
  const queryClient = useQueryClient();
  const view = useAppStore((state) => state.view);
  const searchQuery = useAppStore((state) => state.searchQuery);
  const setSearchQuery = useAppStore((state) => state.setSearchQuery);
  const sortOrder = useAppStore((state) => state.sortOrder);
  const setSortOrder = useAppStore((state) => state.setSortOrder);
  const selectedId = useAppStore((state) => state.selectedNoteId);
  const selectNote = useAppStore((state) => state.selectNote);
  const showExcerpts = useAppStore((state) => state.settings.interface.showExcerpts);
  const compact = useAppStore((state) => state.settings.interface.compactList);

  // Поиск уходит в ядро с задержкой: ввод не должен ждать FTS на каждый символ.
  const debouncedSearch = useDebouncedValue(searchQuery, 180);
  const [offset, setOffset] = useState(0);

  const noteQuery = useMemo<NoteQuery>(() => {
    const filter: NoteQuery['filter'] = { scope: scopeFor(view.kind), recursive: false };
    if (view.kind === 'folder') {
      filter.folderId = view.folderId;
      filter.recursive = view.recursive;
    }
    if (view.kind === 'pinned') {
      filter.pinned = true;
    }
    if (view.kind === 'tag') {
      filter.tags = [view.tagId];
    }
    return { filter, query: debouncedSearch, sort: sortOrder, limit: PAGE_SIZE, offset };
  }, [debouncedSearch, offset, sortOrder, view]);

  const listQuery = useQuery({
    queryKey: queryKeys.notesPage(noteQuery),
    queryFn: () => getBackend().listNotes(noteQuery),
  });

  const searchHits = useQuery({
    queryKey: queryKeys.search(debouncedSearch, noteQuery.filter, PAGE_SIZE),
    queryFn: () => getBackend().searchNotes(debouncedSearch, noteQuery.filter, PAGE_SIZE),
    enabled: parseQuery(debouncedSearch).text.length > 0,
  });

  const terms = useMemo(() => textTerms(parseQuery(debouncedSearch)), [debouncedSearch]);

  const hasTextQuery = terms.include.length > 0 || terms.exclude.length > 0;
  const items: Array<{ note: NoteSummary; snippet: string | null }> = useMemo(() => {
    if (searchHits.data && hasTextQuery) {
      return searchHits.data.map((hit) => ({ note: hit.note, snippet: hit.snippet }));
    }
    return (listQuery.data?.items ?? []).map((note) => ({ note, snippet: null }));
  }, [hasTextQuery, listQuery.data, searchHits.data]);

  const total = searchHits.data ? searchHits.data.length : (listQuery.data?.total ?? 0);

  const createNote = useMutation({
    mutationFn: () => getBackend().createNote({ folderId: view.kind === 'folder' ? view.folderId : null }),
    onSuccess: (note) => {
      void queryClient.invalidateQueries({ queryKey: ['notes'] });
      void queryClient.invalidateQueries({ queryKey: ['stats'] });
      selectNote(note.id);
    },
    onError: (error: Error) => toast.error(error.message),
  });

  const togglePin = useMutation({
    mutationFn: ({ id, pinned }: { id: string; pinned: boolean }) => getBackend().setNotePinned(id, pinned),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: ['notes'] }),
    onError: (error: Error) => toast.error(error.message),
  });

  const onDelete = useCallback(
    async (id: string) => {
      try {
        await getBackend().deleteNotes([id]);
        void queryClient.invalidateQueries({ queryKey: ['notes'] });
        void queryClient.invalidateQueries({ queryKey: ['stats'] });
        if (selectedId === id) {
          selectNote(null);
        }
        toast.success(t('note.delete'));
      } catch (error) {
        toast.error((error as Error).message);
      }
    },
    [queryClient, selectNote, selectedId, t],
  );

  const isLoading = listQuery.isPending || searchHits.isFetching;

  return (
    <section
      className="flex h-full min-w-0 flex-col border-r border-border bg-surface"
      aria-label={t('list.search')}
    >
      <header className="space-y-2 border-b border-border px-3 py-2.5">
        <div className="flex items-center gap-2">
          <div className="relative min-w-0 flex-1">
            <input
              type="search"
              value={searchQuery}
              onChange={(event) => {
                setSearchQuery(event.target.value);
                setOffset(0);
              }}
              placeholder={t('list.search')}
              aria-label={t('list.search')}
              className="h-9 w-full rounded-lg border border-input bg-background pl-3 pr-8 text-[0.83rem] text-foreground outline-none transition-colors placeholder:text-muted-foreground/60 hover:border-input-hover focus:border-primary focus:ring-2 focus:ring-ring/50"
            />
          </div>
          <select
            value={sortOrder}
            onChange={(event) => setSortOrder(event.target.value as typeof sortOrder)}
            aria-label={t('list.sort')}
            title={t('list.sort')}
            className="h-9 shrink-0 cursor-pointer rounded-lg border border-input bg-background px-2 text-[0.78rem] text-muted-foreground outline-none transition-colors hover:border-input-hover focus:border-primary"
          >
            {(['updated_desc', 'created_desc', 'title_asc', 'title_desc'] as const).map((value) => (
              <option key={value} value={value}>
                {t(`sort.${value}`)}
              </option>
            ))}
          </select>
        </div>
        <p className="px-0.5 text-[0.72rem] text-muted-foreground/70 tabular-nums">
          {isLoading ? t('list.loading') : t('list.count', { count: total })}
        </p>
      </header>

      <div className="min-h-0 flex-1 overflow-y-auto">
        {isLoading && items.length === 0 && (
          <div className="grid place-items-center py-16 text-muted-foreground">
            <Loader2 className="size-6 animate-spin" aria-hidden />
          </div>
        )}

        {!isLoading && items.length === 0 && (
          <EmptyState
            icon={<SearchX className="size-8" aria-hidden />}
            title={debouncedSearch ? t('list.noResults') : t('list.empty')}
            hint={debouncedSearch ? t('list.noResultsHint') : t('list.emptyHint')}
            action={
              debouncedSearch ? undefined : (
                <button
                  type="button"
                  onClick={() => createNote.mutate()}
                  className="mt-3 inline-flex cursor-pointer items-center gap-1.5 rounded-lg bg-primary px-4 py-2 text-[0.82rem] font-medium text-primary-foreground transition-colors hover:bg-primary-hover"
                >
                  <Plus className="size-3.5" aria-hidden />
                  {t('palette.newNote')}
                </button>
              )
            }
          />
        )}

        <ul className="py-1">
          {items.map(({ note, snippet }) => (
            <li key={note.id}>
              <NoteRow
                note={note}
                snippet={snippet}
                terms={terms.include}
                selected={note.id === selectedId}
                showExcerpt={showExcerpts}
                compact={compact}
                locale={lang}
                labels={{
                  untitled: t('nav.untitled'),
                  pin: t('note.pin'),
                  unpin: t('note.unpin'),
                  remove: t('note.delete'),
                }}
                onSelect={() => selectNote(note.id)}
                onTogglePin={() => togglePin.mutate({ id: note.id, pinned: !note.pinned })}
                onDelete={() => void onDelete(note.id)}
              />
            </li>
          ))}
        </ul>

        {(listQuery.data?.hasMore ?? false) && (
          <div className="px-3 py-3">
            <button
              type="button"
              onClick={() => setOffset((value) => value + PAGE_SIZE)}
              className="w-full cursor-pointer rounded-lg border border-border py-2 text-[0.8rem] text-muted-foreground transition-colors hover:bg-surface-hover hover:text-foreground"
            >
              {t('list.loadMore')}
            </button>
          </div>
        )}
      </div>
    </section>
  );
}

function EmptyState({
  icon,
  title,
  hint,
  action,
}: {
  icon: React.ReactNode;
  title: string;
  hint: string;
  action?: React.ReactNode;
}) {
  return (
    <div className="flex flex-col items-center px-8 py-20 text-center">
      <div className="mb-3 text-muted-foreground/40">{icon}</div>
      <p className="text-[0.88rem] font-medium text-foreground/80">{title}</p>
      <p className="mt-1 max-w-[260px] text-[0.78rem] leading-relaxed text-muted-foreground">{hint}</p>
      {action}
    </div>
  );
}

function NoteRow({
  note,
  snippet,
  terms,
  selected,
  showExcerpt,
  compact,
  locale,
  labels,
  onSelect,
  onTogglePin,
  onDelete,
}: {
  note: NoteSummary;
  snippet: string | null;
  terms: string[];
  selected: boolean;
  showExcerpt: boolean;
  compact: boolean;
  locale: string;
  labels: { untitled: string; pin: string; unpin: string; remove: string };
  onSelect: () => void;
  onTogglePin: () => void;
  onDelete: () => void;
}) {
  const title = note.title.trim() || labels.untitled;
  const excerpt = snippet ? renderSnippet(snippet) : highlightTerms(note.excerpt, terms);

  return (
    <div
      role="option"
      aria-selected={selected}
      tabIndex={0}
      onClick={onSelect}
      onKeyDown={(event) => {
        if (event.key === 'Enter' || event.key === ' ') {
          event.preventDefault();
          onSelect();
        }
      }}
      className={cn(
        'group relative mx-1.5 flex cursor-pointer items-stretch gap-2 rounded-lg px-2.5 outline-none transition-colors duration-150',
        compact ? 'py-1.5' : 'py-2.5',
        selected ? 'bg-primary/10' : 'hover:bg-surface-hover',
        'focus-visible:ring-2 focus-visible:ring-ring',
      )}
    >
      {note.color && <span className={cn('color-bar my-1', `dot-${note.color}`)} aria-hidden />}

      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-1.5">
          {note.pinned && <Pin className="size-3 shrink-0 text-primary" aria-hidden />}
          {note.encrypted && <Lock className="size-3 shrink-0 text-muted-foreground" aria-hidden />}
          <span
            className={cn(
              'min-w-0 flex-1 truncate text-[0.86rem] font-medium',
              selected ? 'text-primary' : 'text-foreground',
            )}
            // Подсветка совпадений: текст экранируется в `highlightTerms`,
            // поэтому разметка из заметки в DOM не попадает.
            dangerouslySetInnerHTML={{ __html: highlightTerms(title, terms) }}
          />
          <span className="shrink-0 text-[0.7rem] text-muted-foreground/60 tabular-nums">
            {formatRelative(note.updatedAt, locale)}
          </span>
        </div>

        {showExcerpt && !compact && (
          <p
            className="mt-0.5 line-clamp-2 text-[0.78rem] leading-snug text-muted-foreground"
            // Сниппет и подсветка экранируются в `renderSnippet`/`highlightTerms`:
            // пользовательский текст не может внедрить разметку.
            dangerouslySetInnerHTML={{ __html: excerpt }}
          />
        )}

        {note.tags.length > 0 && (
          <div className="mt-1 flex flex-wrap gap-1">
            {note.tags.slice(0, 4).map((tag) => (
              <span
                key={tag}
                className="rounded-full bg-muted px-1.5 py-px text-[0.68rem] text-muted-foreground"
              >
                #{tag}
              </span>
            ))}
          </div>
        )}
      </div>

      <div className="flex shrink-0 flex-col justify-center gap-0.5 opacity-0 transition-opacity group-hover:opacity-100 group-focus-within:opacity-100">
        <button
          type="button"
          onClick={(event) => {
            event.stopPropagation();
            onTogglePin();
          }}
          title={note.pinned ? labels.unpin : labels.pin}
          aria-label={note.pinned ? labels.unpin : labels.pin}
          className="grid size-6 cursor-pointer place-items-center rounded-md text-muted-foreground transition-colors hover:bg-background hover:text-primary"
        >
          {note.pinned ? (
            <PinOff className="size-3.5" aria-hidden />
          ) : (
            <Pin className="size-3.5" aria-hidden />
          )}
        </button>
        <button
          type="button"
          onClick={(event) => {
            event.stopPropagation();
            onDelete();
          }}
          title={labels.remove}
          aria-label={labels.remove}
          className="grid size-6 cursor-pointer place-items-center rounded-md text-muted-foreground transition-colors hover:bg-background hover:text-danger"
        >
          <Trash2 className="size-3.5" aria-hidden />
        </button>
      </div>
    </div>
  );
}
