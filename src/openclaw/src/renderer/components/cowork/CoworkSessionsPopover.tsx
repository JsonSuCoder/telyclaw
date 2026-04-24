import React, { useEffect, useRef, useState } from 'react';
import { useSelector } from 'react-redux';
import ClockIcon from '../icons/ClockIcon';
import CoworkSessionList from './CoworkSessionList';
import { RootState } from '../../store';
import {
  selectCurrentSessionId,
  selectCoworkSessions,
} from '../../store/selectors/coworkSelectors';

interface CoworkSessionsPopoverProps {
  onSelectSession: (sessionId: string) => void;
  onDeleteSession: (sessionId: string) => void;
  onTogglePin: (sessionId: string, pinned: boolean) => void;
  onRenameSession: (sessionId: string, title: string) => void;
}

const CoworkSessionsPopover: React.FC<CoworkSessionsPopoverProps> = ({
  onSelectSession,
  onDeleteSession,
  onTogglePin,
  onRenameSession,
}) => {
  const [isOpen, setIsOpen] = useState(false);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const popoverRef = useRef<HTMLDivElement>(null);
  const currentSessionId = useSelector(selectCurrentSessionId);
  const sessions = useSelector((state: RootState) => selectCoworkSessions(state));

  useEffect(() => {
    if (!isOpen) return;

    const handleClickOutside = (event: MouseEvent) => {
      const target = event.target as Node;
      const isInsidePopover = popoverRef.current?.contains(target);
      const isInsideAnchor = buttonRef.current?.contains(target);

      if (!isInsidePopover && !isInsideAnchor) {
        setIsOpen(false);
      }
    };

    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, [isOpen]);

  useEffect(() => {
    if (!isOpen) return;

    const handleEscape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        setIsOpen(false);
      }
    };

    document.addEventListener('keydown', handleEscape);
    return () => document.removeEventListener('keydown', handleEscape);
  }, [isOpen]);

  const handleToggleOpen = () => {
    setIsOpen((prev) => !prev);
  };

  const handleSelectSession = (sessionId: string) => {
    onSelectSession(sessionId);
    setIsOpen(false);
  };

  return (
    <div className="relative">
      <button
        type="button"
        onClick={handleToggleOpen}
        className="p-1 text-secondary hover:text-foreground rounded-md hover:bg-surface-raised"
      >
        <ClockIcon className="h-5 w-5" />
      </button>
      {isOpen && (
        <div
          ref={popoverRef}
          className="absolute right-0 top-full z-50 mt-2 w-80 rounded-xl border border-border bg-surface shadow-popover popover-enter overflow-hidden"
        >
          <div className="max-h-[28rem] overflow-y-auto p-2">
            <CoworkSessionList
              sessions={sessions}
              currentSessionId={currentSessionId}
              isBatchMode={false}
              selectedIds={new Set<string>()}
              showBatchOption={false}
              onSelectSession={handleSelectSession}
              onDeleteSession={onDeleteSession}
              onTogglePin={onTogglePin}
              onRenameSession={onRenameSession}
              onToggleSelection={() => undefined}
              onEnterBatchMode={() => undefined}
            />
          </div>
        </div>
      )}
    </div>
  );
};

export default CoworkSessionsPopover;
