import autoprefixer from 'autoprefixer';
import tailwindcss from 'tailwindcss';
import postcssImport from 'postcss-import';
import type { Config } from 'postcss-load-config';

import removeGlobalPlugin from './dev/postcss-remove-global.ts';

const config: Config = {
  plugins: [
    postcssImport(),
    tailwindcss(),
    removeGlobalPlugin(),
    autoprefixer(),
  ],
};

export default config;
