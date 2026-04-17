import React from 'react';
import { Provider } from 'react-redux';
import { store } from './store';
import App from './App';
import './index.css';

const OpenclawAppWrapper: React.FC = () => {
  return (
    <Provider store={store}>
      <App />
    </Provider>
  );
};

export default OpenclawAppWrapper;
