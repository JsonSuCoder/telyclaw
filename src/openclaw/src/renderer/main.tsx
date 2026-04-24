import React from 'react';
import ReactDOM from 'react-dom/client';
import OpenclawAppWrapper from './OpenclawAppWrapper';

export function mountOpenclaw(container: HTMLElement, props?: any) {
  const root = ReactDOM.createRoot(container);
  root.render(
    <React.StrictMode>
      <OpenclawAppWrapper {...props} />
    </React.StrictMode>
  );
  return () => {
    setTimeout(() => {
      root.unmount();
    }, 0);
  };
}


