import { getActions, withGlobal } from '../../global';
import { selectTabState } from '../../global/selectors';
import { selectUser } from '../../global/selectors/users';
import { memo, useEffect, useRef } from '../../lib/teact/teact';
import { mountOpenclaw } from '../../openclaw/src/renderer/main';

import type { ApiChat } from '../../api/types/chats';
import type { ApiUser } from '../../api/types/users';
import type { OpenclawInstance } from '../../openclaw/src/renderer/main';

import './OpenClawColumn.scss';

type OwnProps = {
  isMobile?: boolean;
};

type StateProps = {
  isOpenclawModalOpen?: boolean;
  chatList: ApiChat[];
  contactList: ApiUser[];
};

const OpenClawColumn = ({
  isOpenclawModalOpen,
  chatList,
  contactList,
}: OwnProps & StateProps) => {
  const containerRef = useRef<HTMLDivElement>();
  const instanceRef = useRef<OpenclawInstance>();
  const { closeOpenclawModal } = getActions();

  // Mount once, unmount on cleanup
  useEffect(() => {
    if (!containerRef.current) {
      return undefined;
    }

    const instance = mountOpenclaw(containerRef.current, {
      onClose: closeOpenclawModal,
    });
    instanceRef.current = instance;

    return () => {
      instance.unmount();
      instanceRef.current = undefined;
    };
  }, [closeOpenclawModal]);

  // Push data updates without remounting
  useEffect(() => {
    instanceRef.current?.updateTelegramData({ chatList, contactList });
  }, [chatList, contactList]);

  return (
    <div id="OpenClawColumn" className={!isOpenclawModalOpen ? 'is-hidden' : undefined}>
      <div ref={containerRef} className="openclaw-root-container" />
    </div>
  );
};

export default memo(withGlobal<OwnProps>(
  (global): Complete<StateProps> => {
    const { isOpenclawModalOpen } = selectTabState(global);
    const activeChatIds = global.chats.listIds.active || [];
    const chatList = activeChatIds
      .map((chatId) => global.chats.byId[chatId])
      .filter(Boolean);
    const contactIds = global.contactList?.userIds || [];
    const contactList = contactIds
      .map((userId) => selectUser(global, userId))
      .filter(Boolean);

    return {
      isOpenclawModalOpen,
      chatList,
      contactList,
    };
  },
)(OpenClawColumn));
