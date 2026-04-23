import { withGlobal } from '../../global';
import { selectTabState } from '../../global/selectors';
import { useVtn } from '../../hooks/animations/useVtn';
import { memo, useEffect, useRef } from '../../lib/teact/teact';
import { mountOpenclaw } from '../../openclaw/src/renderer/main';
import { IS_TAURI } from '../../util/browser/globalEnvironment';
import { IS_MAC_OS } from '../../util/browser/windowEnvironment';

import './OpenClawColumn.scss';

type OwnProps = {
  isMobile?: boolean;
};

type StateProps = {
  isOpenclawModalOpen?: boolean;
};

const OpenClawColumn = ({
  isOpenclawModalOpen,
}: OwnProps & StateProps) => {
  const { createVtnStyle } = useVtn();
  const containerRef = useRef<HTMLDivElement>();

  useEffect(() => {
    if (!containerRef.current) {
      return undefined;
    }

    return mountOpenclaw(containerRef.current);
  }, []);

  return (
    <div id="OpenClawColumn">
      <div ref={containerRef} className="openclaw-root-container" />
    </div>
  );
};

export default memo(withGlobal<OwnProps>(
  (global): Complete<StateProps> => {
    const { isOpenclawModalOpen } = selectTabState(global);

    return {
      isOpenclawModalOpen,
    };
  },
)(OpenClawColumn));
