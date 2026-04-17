/** @type {import('tailwindcss').Config} */
module.exports = {
  important: '.openclaw-root-container',
  content: [
    "./src/openclaw/**/*.{js,ts,jsx,tsx,md,html}",
  ],
  theme: {
    extend: {
      // Base theme extensions
    },
  },
  plugins: [
    require('@tailwindcss/typography'),
    require('./src/openclaw/src/renderer/theme/tailwind/plugin.cjs'),
  ],
}
