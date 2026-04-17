import type { FC } from '../../lib/teact/teact';
import { memo, useEffect, useRef } from '../../lib/teact/teact';
import { getActions, withGlobal } from '../../global';

import { selectTabState } from '../../global/selectors';

import Modal from '../ui/Modal';
import { mountOpenclaw } from '../../openclaw/src/renderer/main';

import './OpenclawModal.scss';

type OwnProps = {
};

type StateProps = {
  isOpen?: boolean;
};

const OpenclawModal: FC<OwnProps & StateProps> = ({ isOpen }) => {
  const { closeOpenclawModal } = getActions();
  const containerRef = useRef<HTMLDivElement>();

  useEffect(() => {
    if (!isOpen || !containerRef.current) {
      return undefined;
    }

    const unmount = mountOpenclaw(containerRef.current);
    return unmount;
  }, [isOpen]);

  return (
    <Modal
      className="OpenclawModal"
      isOpen={isOpen}
      onClose={closeOpenclawModal}
      title="Openclaw"
      hasCloseButton
      isSlim
    >
      <div ref={containerRef} className="openclaw-root-container" />
    </Modal>
  );
};

export default memo(withGlobal<OwnProps>(
  (global): StateProps => {
    const tabState = selectTabState(global);
    return {
      isOpen: tabState.isOpenclawModalOpen,
    };
  },
)(OpenclawModal));

