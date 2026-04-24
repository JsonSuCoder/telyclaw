import React, { useEffect, useState } from 'react';
import Modal from '../common/Modal';
import ClockIcon from '../icons/ClockIcon';
import PuzzleIcon from '../icons/PuzzleIcon';
import ConnectorIcon from '../icons/ConnectorIcon';
import Cog6ToothIcon from '../icons/Cog6ToothIcon';
import XMarkIcon from '../icons/XMarkIcon';
import { i18nService } from '../../services/i18n';
import EllipsisHorizontalIcon from '../icons/EllipsisHorizontalIcon';

type HeaderMenuView = 'scheduledTasks' | 'skills' | 'mcp' | 'agents';

interface HeaderMenuPopoverProps {
  activeView: HeaderMenuView | 'cowork';
  onShowScheduledTasks: () => void;
  onShowSkills: () => void;
  onShowMcp: () => void;
  onShowAgents: () => void;
  onShowSettings: () => void;
}

const HeaderMenuPopover: React.FC<HeaderMenuPopoverProps> = ({
  activeView,
  onShowScheduledTasks,
  onShowSkills,
  onShowMcp,
  onShowAgents,
  onShowSettings,
}) => {
  const [isOpen, setIsOpen] = useState(false);

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

  const handleSelect = (callback: () => void) => {
    callback();
    setIsOpen(false);
  };

  const menuItemClassName = (view: HeaderMenuView) => `w-full inline-flex items-center gap-2 rounded-lg px-2.5 py-2 text-sm font-medium transition-colors ${activeView === view
    ? 'bg-primary/10 text-primary hover:bg-primary/20'
    : 'text-secondary hover:text-foreground hover:bg-surface-raised'
    }`;

  return (
    <div className="relative">
      <button
        type="button"
        onClick={handleToggleOpen}
        className="p-1 text-secondary hover:text-foreground rounded-md hover:bg-surface-raised"
      >
        <EllipsisHorizontalIcon className="h-5 w-5" />
      </button>
      {isOpen && (
        <Modal
          isOpen={isOpen}
          onClose={() => setIsOpen(false)}
          overlayClassName="fixed inset-0 z-50"
          className="absolute right-4 top-14 w-56 rounded-xl border border-border bg-surface shadow-popover popover-enter overflow-hidden p-2"
        >
          <div className="space-y-1">
            <button
              type="button"
              onClick={() => handleSelect(onShowScheduledTasks)}
              className={menuItemClassName('scheduledTasks')}
            >
              <ClockIcon className="h-4 w-4" />
              {i18nService.t('scheduledTasks')}
            </button>
            <button
              type="button"
              onClick={() => handleSelect(onShowSkills)}
              className={menuItemClassName('skills')}
            >
              <PuzzleIcon className="h-4 w-4" />
              {i18nService.t('skills')}
            </button>
            <button
              type="button"
              onClick={() => handleSelect(onShowMcp)}
              className={menuItemClassName('mcp')}
            >
              <ConnectorIcon className="h-4 w-4" />
              {i18nService.t('mcpServers')}
            </button>
            <button
              type="button"
              onClick={() => handleSelect(onShowAgents)}
              className={menuItemClassName('agents')}
            >
              <PuzzleIcon className="h-4 w-4" />
              {i18nService.t('myAgents')}
            </button>
            <button
              type="button"
              onClick={() => handleSelect(onShowSettings)}
              className="w-full inline-flex items-center gap-2 rounded-lg px-2.5 py-2 text-sm font-medium text-secondary hover:text-foreground hover:bg-surface-raised transition-colors"
            >
              <Cog6ToothIcon className="h-4 w-4" />
              {i18nService.t('openSettings')}
            </button>
          </div>
        </Modal>
      )}
    </div>
  );
};

interface HeaderViewModalProps {
  children: React.ReactNode;
  isOpen: boolean;
  onClose: () => void;
}

export const HeaderViewModal: React.FC<HeaderViewModalProps> = ({ children, isOpen, onClose }) => {
  return (
    <Modal
      isOpen={isOpen}
      onClose={onClose}
      overlayClassName="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
      className="relative h-[min(90vh,56rem)] w-[min(96vw,72rem)] overflow-hidden rounded-2xl border border-border bg-surface shadow-2xl"
    >
      <button
        type="button"
        onClick={onClose}
        className="absolute right-3 top-3 z-10 inline-flex h-8 w-8 items-center justify-center rounded-lg text-secondary transition-colors hover:bg-surface-raised hover:text-foreground"
      >
        <XMarkIcon className="h-5 w-5" />
      </button>
      <div className="flex h-full min-h-0 flex-col bg-background">
        {children}
      </div>
    </Modal>
  );
};

export default HeaderMenuPopover;
