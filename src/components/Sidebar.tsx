import { useMemo, useState } from 'react';
import {
  Archive,
  ChevronDown,
  ChevronRight,
  Folder as FolderIcon,
  Hash,
  LayoutList,
  Pin,
  Plus,
  Settings as SettingsIcon,
  Trash2,
} from 'lucide-react';
import { useQuery } from '@tanstack/react-query';

import { getBackend } from '@/api/backend';
import { queryKeys } from '@/api/keys';
import type { FolderNode } from '@/api/contracts';
import { IconButton } from '@/components/ui/IconButton';
import { cn, pluralRu } from '@/lib/utils';
import type { Translator } from '@/lib/i18n';
import type { ListView } from '@/store/useAppStore';
import { useAppStore } from '@/store/useAppStore';

interface SidebarProps {
  t: Translator;
  lang: 'ru' | 'en';
  onNewFolder: () => void;
  onOpenSettings: () => void;
}

/**
 * Левая панель: разделы, дерево папок, теги.
 *
 * Счётчики берутся из одного запроса статистики, а не пересчитываются на
 * каждую строку: иначе при тысяче заметок панель перерисовывалась бы заметно
 * дольше самого списка.
 */
export function Sidebar({ t, lang, onNewFolder, onOpenSettings }: SidebarProps) {
  const view = useAppStore((state) => state.view);
  const setView = useAppStore((state) => state.setView);
  const showSidebar = useAppStore((state) => state.settings.interface.showSidebar);
  const sidebarWidth = useAppStore((state) => state.settings.interface.sidebarWidth);

  const foldersQuery = useQuery({
    queryKey: queryKeys.folders(),
    queryFn: () => getBackend().folderTree(),
  });
  const tagsQuery = useQuery({ queryKey: queryKeys.tags(), queryFn: () => getBackend().listTags() });
  const statsQuery = useQuery({ queryKey: queryKeys.stats(), queryFn: () => getBackend().libraryStats() });

  const stats = statsQuery.data;

  const sections = useMemo(
    () => [
      { id: 'all', label: t('nav.all'), icon: LayoutList, count: stats?.notes },
      { id: 'pinned', label: t('nav.pinned'), icon: Pin, count: undefined },
      { id: 'archive', label: t('nav.archive'), icon: Archive, count: stats?.archived },
      { id: 'trash', label: t('nav.trash'), icon: Trash2, count: stats?.trashed },
    ],
    [stats, t],
  );

  if (!showSidebar) {
    return null;
  }

  return (
    <aside
      className="flex h-full shrink-0 flex-col border-r border-border bg-surface"
      style={{ width: sidebarWidth }}
      aria-label={t('nav.folders')}
    >
      <nav className="flex-1 overflow-y-auto px-2 py-3">
        <ul className="space-y-0.5">
          {sections.map((section) => (
            <li key={section.id}>
              <NavRow
                active={view.kind === section.id}
                icon={<section.icon className="size-4" aria-hidden />}
                label={section.label}
                count={section.count}
                onClick={() => setView({ kind: section.id } as ListView)}
              />
            </li>
          ))}
        </ul>

        <SectionHeader
          label={t('nav.folders')}
          action={<IconButton icon={Plus} label={t('folder.new')} size="sm" onClick={onNewFolder} />}
        />
        <ul className="space-y-0.5">
          {(foldersQuery.data ?? []).map((node) => (
            <FolderRow key={node.folder.id} node={node} view={view} onSelect={setView} />
          ))}
          {(foldersQuery.data ?? []).length === 0 && (
            <li className="px-3 py-1.5 text-[0.78rem] text-muted-foreground/70">{t('nav.empty')}</li>
          )}
        </ul>

        <SectionHeader label={t('nav.tags')} />
        <ul className="space-y-0.5">
          {(tagsQuery.data ?? []).slice(0, 40).map((tag) => (
            <li key={tag.id}>
              <NavRow
                active={view.kind === 'tag' && view.tagId === tag.id}
                icon={
                  tag.color ? (
                    <span className={cn('color-dot', `dot-${tag.color}`)} aria-hidden />
                  ) : (
                    <Hash className="size-4" aria-hidden />
                  )
                }
                label={tag.name}
                count={tag.noteCount}
                onClick={() => setView({ kind: 'tag', tagId: tag.id })}
              />
            </li>
          ))}
          {(tagsQuery.data ?? []).length === 0 && (
            <li className="px-3 py-1.5 text-[0.78rem] text-muted-foreground/70">{t('nav.empty')}</li>
          )}
        </ul>
      </nav>

      <footer className="border-t border-border px-3 py-2.5">
        <button
          type="button"
          onClick={onOpenSettings}
          className="flex w-full cursor-pointer items-center gap-2 rounded-lg px-2 py-1.5 text-[0.82rem] text-muted-foreground transition-colors hover:bg-surface-hover hover:text-foreground"
        >
          <SettingsIcon className="size-4" aria-hidden />
          {t('nav.settings')}
        </button>
        {stats && (
          <p className="mt-1 px-2 text-[0.72rem] text-muted-foreground/60">
            {lang === 'ru' ? pluralRu(stats.notes, 'заметка', 'заметки', 'заметок') : `${stats.notes} notes`}
          </p>
        )}
      </footer>
    </aside>
  );
}

function SectionHeader({ label, action }: { label: string; action?: React.ReactNode }) {
  return (
    <div className="mt-5 mb-1 flex items-center justify-between px-3">
      <span className="text-[0.68rem] font-semibold tracking-[0.08em] text-muted-foreground/70 uppercase">
        {label}
      </span>
      {action}
    </div>
  );
}

function NavRow({
  active,
  icon,
  label,
  count,
  onClick,
  depth = 0,
}: {
  active: boolean;
  icon: React.ReactNode;
  label: string;
  count?: number;
  onClick: () => void;
  depth?: number;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-current={active ? 'true' : undefined}
      style={{ paddingLeft: 8 + depth * 14 }}
      className={cn(
        // Подсветка меняет только цвет фона, не начертание: иначе текст
        // «прыгает» на полпикселя при каждом выборе раздела.
        'flex w-full cursor-pointer items-center gap-2 rounded-lg py-1.5 pr-2 text-left text-[0.83rem]',
        'text-muted-foreground transition-colors duration-150 outline-none',
        'hover:bg-surface-hover hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring',
        active && 'bg-primary/10 text-primary hover:bg-primary/15 hover:text-primary',
      )}
    >
      <span className="shrink-0">{icon}</span>
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {typeof count === 'number' && count > 0 && (
        <span className="shrink-0 text-[0.72rem] tabular-nums opacity-60">{count}</span>
      )}
    </button>
  );
}

function FolderRow({
  node,
  view,
  onSelect,
  depth = 0,
}: {
  node: FolderNode;
  view: ListView;
  onSelect: (view: ListView) => void;
  depth?: number;
}) {
  const [expanded, setExpanded] = useState(depth < 1);
  const active = view.kind === 'folder' && view.folderId === node.folder.id;
  const hasChildren = node.children.length > 0;

  return (
    <li>
      <div className="flex items-center">
        {hasChildren ? (
          <IconButton
            icon={expanded ? ChevronDown : ChevronRight}
            label={expanded ? 'Свернуть' : 'Развернуть'}
            size="sm"
            className="size-6"
            onClick={() => setExpanded((value) => !value)}
          />
        ) : (
          <span className="inline-block w-6 shrink-0" aria-hidden />
        )}
        <div className="min-w-0 flex-1">
          <NavRow
            active={active}
            depth={depth}
            icon={
              node.folder.color ? (
                <span className={cn('color-dot', `dot-${node.folder.color}`)} aria-hidden />
              ) : (
                <FolderIcon className="size-4" aria-hidden />
              )
            }
            label={node.folder.name}
            count={node.folder.noteCount}
            onClick={() => onSelect({ kind: 'folder', folderId: node.folder.id, recursive: false })}
          />
        </div>
      </div>
      {expanded && hasChildren && (
        <ul className="space-y-0.5">
          {node.children.map((child) => (
            <FolderRow key={child.folder.id} node={child} view={view} onSelect={onSelect} depth={depth + 1} />
          ))}
        </ul>
      )}
    </li>
  );
}
