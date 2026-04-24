import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTelegramData } from '../../contexts/TelegramDataContext';

import type { ApiChat } from '../../../../../api/types/chats';
import type { ApiUser } from '../../../../../api/types/users';

export type MentionTrigger = '@' | '#';

export interface MentionItem {
  id: string;
  label: string;
  secondary?: string;
  trigger: MentionTrigger;
}

interface MentionPopoverProps {
  trigger: MentionTrigger | null;
  query: string;
  anchorRef: React.RefObject<HTMLTextAreaElement | null>;
  cursorPosition: number;
  onSelect: (item: MentionItem) => void;
  onClose: () => void;
}

const MAX_RESULTS = 8;

function userToMentionItem(user: ApiUser): MentionItem {
  const name = [user.firstName, user.lastName].filter(Boolean).join(' ');
  const username = user.usernames?.[0]?.username;
  return {
    id: user.id,
    label: name || username || user.id,
    secondary: username ? `@${username}` : undefined,
    trigger: '@',
  };
}

function chatToMentionItem(chat: ApiChat): MentionItem {
  const username = chat.usernames?.[0]?.username;
  return {
    id: chat.id,
    label: chat.title,
    secondary: username ? `@${username}` : undefined,
    trigger: '#',
  };
}

const MentionPopover: React.FC<MentionPopoverProps> = ({
  trigger,
  query,
  anchorRef,
  cursorPosition: _cursorPosition,
  onSelect,
  onClose,
}) => {
  const { chatList, contactList } = useTelegramData();
  const popoverRef = useRef<HTMLDivElement>(null);
  const [activeIndex, setActiveIndex] = useState(0);

  const items = useMemo(() => {
    if (!trigger) return [];
    const q = query.toLowerCase();

    if (trigger === '@') {
      const list = (contactList || []).map(userToMentionItem);
      if (!q) return list.slice(0, MAX_RESULTS);
      return list
        .filter(
          (item) =>
            item.label.toLowerCase().includes(q) ||
            (item.secondary && item.secondary.toLowerCase().includes(q)),
        )
        .slice(0, MAX_RESULTS);
    }

    if (trigger === '#') {
      const list = (chatList || []).map(chatToMentionItem);
      if (!q) return list.slice(0, MAX_RESULTS);
      return list
        .filter(
          (item) =>
            item.label.toLowerCase().includes(q) ||
            (item.secondary && item.secondary.toLowerCase().includes(q)),
        )
        .slice(0, MAX_RESULTS);
    }

    return [];
  }, [trigger, query, chatList, contactList]);

  // Reset active index when items change
  useEffect(() => {
    setActiveIndex(0);
  }, [items]);

  // Close on click outside
  useEffect(() => {
    if (!trigger) return;
    const handleClickOutside = (e: MouseEvent) => {
      if (popoverRef.current && !popoverRef.current.contains(e.target as Node)) {
        onClose();
      }
    };
    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, [trigger, onClose]);

  // Keyboard navigation
  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      if (!trigger || items.length === 0) return;

      if (e.key === 'ArrowDown') {
        e.preventDefault();
        setActiveIndex((prev) => (prev + 1) % items.length);
      } else if (e.key === 'ArrowUp') {
        e.preventDefault();
        setActiveIndex((prev) => (prev - 1 + items.length) % items.length);
      } else if (e.key === 'Enter' || e.key === 'Tab') {
        e.preventDefault();
        e.stopPropagation();
        onSelect(items[activeIndex]);
      } else if (e.key === 'Escape') {
        e.preventDefault();
        onClose();
      }
    },
    [trigger, items, activeIndex, onSelect, onClose],
  );

  useEffect(() => {
    if (!trigger) return;
    // Use capture phase to intercept before textarea's own handler
    document.addEventListener('keydown', handleKeyDown, true);
    return () => document.removeEventListener('keydown', handleKeyDown, true);
  }, [trigger, handleKeyDown]);

  if (!trigger || items.length === 0) return null;

  // Calculate position above the textarea cursor
  const getPopoverStyle = (): React.CSSProperties => {
    const textarea = anchorRef.current;
    if (!textarea) return { display: 'none' };

    const rect = textarea.getBoundingClientRect();
    // Position above the textarea, aligned to the left
    return {
      position: 'fixed',
      bottom: window.innerHeight - rect.top + 4,
      left: rect.left,
      minWidth: '220px',
      maxWidth: '320px',
      zIndex: 50,
    };
  };

  return (
    <div
      ref={popoverRef}
      style={getPopoverStyle()}
      className="rounded-lg border border-border bg-background shadow-lg overflow-hidden"
    >
      <div className="px-2 py-1.5 text-xs text-secondary border-b border-border">
        {trigger === '@' ? '联系人' : '聊天'}
      </div>
      <div className="max-h-[240px] overflow-y-auto py-1">
        {items.map((item, index) => (
          <button
            key={item.id}
            type="button"
            className={`w-full text-left px-3 py-1.5 text-sm flex flex-col gap-0.5 transition-colors ${
              index === activeIndex
                ? 'bg-primary/10 text-foreground'
                : 'text-foreground hover:bg-surface-raised'
            }`}
            onMouseEnter={() => setActiveIndex(index)}
            onClick={() => onSelect(item)}
          >
            <span className="truncate">{item.label}</span>
            {item.secondary && (
              <span className="text-xs text-secondary truncate">{item.secondary}</span>
            )}
          </button>
        ))}
      </div>
    </div>
  );
};

export default MentionPopover;
